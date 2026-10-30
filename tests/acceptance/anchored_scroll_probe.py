#!/usr/bin/env python3
"""Opt-in native AppKit / owned Chrome anchored-scroll acceptance.

Only real MCP scroll actions drive the fixtures. Every action runs once; original
execution IDs, receipts and independent wheel/offset journals survive failures.
No user profile, CDP, synthetic wheel event or desktop-idle assumption is used.
The native fixture intentionally maps each delivered wheel line to 16 points;
Chrome uses its ordinary overflow scrolling. This is not an external-Agent test.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import shutil
import select as io_select
import subprocess
import sys
import traceback
import uuid

from background_input_probe import command, front, fresh_end, observe_desktop
from native_input_probe import check_cleanup, close_mcp
from standard_agent_probe import Fixture, Mcp, process_table, require, save_png, stop_owned, successful, wait_until, write_json


REGIONS = ('left', 'right', 'nested')
APP = 'com.google.Chrome'


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def require_session(snapshot):
    require(snapshot.get('screen_locked') is None or type(snapshot['screen_locked']) is bool, 'Malformed session snapshot')
    if snapshot.get('screen_locked') is True:
        raise AssertionError('gui_precondition_locked: unlock the interactive session before GUI acceptance; not a product regression')
    # An absent field is unknown, not evidence that the screen is unlocked.
    return snapshot


def session(observer_binary):
    result = subprocess.run([str(observer_binary), '--session-precondition'], text=True,
                            capture_output=True, timeout=5, check=True)
    return json.loads(result.stdout)


def open_mcp(binary, root, clients):
    root.mkdir()
    mcp = Mcp(binary, root); clients.append(mcp)
    mcp.rpc('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                          'clientInfo': {'name': 'anchored-scroll-probe', 'version': '1'}})
    mcp.notify('notifications/initialized')
    return mcp


def execute(mcp, app, case, source, proof, *, success=True):
    proof.update(dispatch_attempts=1, source=source)
    try:
        record, response = mcp.execute(app, case, source)
    finally:
        frame = getattr(mcp, 'exec_frames', {}).get((app, case))
        if frame is not None: proof['rpc_id'] = frame['id']
        identity = getattr(mcp, 'executions', {}).get((app, case))
        if identity is not None: proof['execution_id'] = identity
    proof.update(record=record, execution_id=record['execution_id'], rpc_id=mcp.exec_frames[(app, case)]['id'])
    require(not record.get('script', {}).get('shutdown_error'), 'Scroll execution left a cleanup failure')
    return (successful(record) if success else record), response


def select(title):
    return 'const windows=(await app.windows()).windows.filter(w=>w.title===' + json.dumps(title) + ');' + \
        "if(windows.length!==1)throw Error('Owned scroll window is not unique');state.w=windows[0].window_ref;"


def semantic_source(title, region):
    name = region.capitalize() + ' anchor'
    return select(title) + 'const name=' + json.dumps(name) + ';' + r'''
let page=await app.observe({window_ref:state.w,role:'static_text',text:name,limit:5});
const pages=[];
while(true) {
  pages.push({elements:page.elements,coverage:page.coverage});
  const matches=page.elements.filter(e=>[e.name,e.value,e.label].includes(name));
  if(matches.length>1)throw Error('Owned anchor is ambiguous');
  if(matches.length===1) {
    state.anchor={window_ref:state.w,observation_ref:page.observation_ref,element_ref:matches[0].element_ref};
    return {anchor:state.anchor,element:matches[0],pages};
  }
  if(!page.next || pages.length>=12)throw Error('Owned non-pressable anchor not found '+JSON.stringify(pages));
  page=await app.observe({cursor:page.next});
}
'''


def point_in(rect, point, margin=0):
    x, y, width, height = rect
    return x+margin <= point[0] < x+width-margin and y+margin <= point[1] < y+height-margin


def map_pixel(window, image, point):
    values = list(window) + [image['width'], image['height']] + list(point)
    require(all(type(v) in (int, float) and math.isfinite(v) for v in values), 'Invalid coordinate geometry')
    x, y, width, height = window
    require(width > 0 and height > 0 and image['width'] > 0 and image['height'] > 0
            and point_in(window, point), 'Point is outside the captured owned window')
    return {'x': round((point[0]-x)*image['width']/width),
            'y': round((point[1]-y)*image['height']/height)}


def center(rect):
    return [rect[0]+rect[2]/2, rect[1]+rect[3]/2]


def scroll_outcome(record):
    steps = [s for s in record.get('script', {}).get('steps', []) if s.get('operation') == 'scroll']
    require(len(steps) == 1 and steps[0].get('receipt'), 'Missing unique scroll receipt')
    receipt = steps[0]['receipt']
    return receipt.get('provider_outcome') or receipt['outcome']


def require_native_effect(before, after, region, delta, point, anchor_rect=None):
    events = [row for row in after['events'] if row['sequence'] > before['events_count']]
    require(events, 'No native wheel event reached the owned fixture')
    require(all(row['region'] == region and row['window_id'] == before['windows'][0]['id'] for row in events),
            {'wheel_reached_other_region': events})
    require(abs(sum(row['delta_y'] for row in events)+delta) < 0.01,
            {'truncated_or_replayed_wheel_lines': events, 'requested_delta_y': delta})
    require(all(point_in(anchor_rect, row['screen_point']) if anchor_rect else math.dist(row['screen_point'], point) <= 2 for row in events),
            {'wheel_point_does_not_match_anchor': events, 'expected_point': point})
    require(abs(after['offsets'][region]-before['offsets'][region]-delta*16) < 0.01,
            {'target_region_did_not_move_full_amount': after['offsets']})
    require(all(after['offsets'][name] == before['offsets'][name] for name in REGIONS if name != region),
            {'neighbor_region_moved': after['offsets']})
    return events


def require_stale(record, before, after):
    require(record.get('status') == 'completed' and record.get('error'),
            'Stale-anchor evidence requires a terminal execution with an error')
    outcome = scroll_outcome(record)
    require(outcome.get('dispatch') == 'not_dispatched', 'Stale anchor was not rejected before input')
    require(outcome.get('error', {}).get('code') == 'stale_observation', outcome)
    require(before['events_count'] == after['events_count'] and before['offsets'] == after['offsets'],
            'Stale semantic anchor caused a native wheel or offset change')


def capture(mcp, app, title, case, proof):
    shot, response = execute(mcp, app, case, select(title) +
        'state.shot=await app.capture({window_ref:state.w});return state.shot;', proof)
    folder = mcp.output/case; folder.mkdir()
    proof['png'] = save_png(folder, response, shot)
    return shot


def native_case(mcp, target, desktop, mode, region, delta, name, evidence):
    proof = evidence.setdefault(name, {})
    command(target, 'reset'); proof['setup_foreground'] = front(desktop)
    proof['before'] = before = target.state(); desk_before = fresh_end(desktop)
    preparation = proof.setdefault('preparation', {})
    if mode == 'pixel':
        capture(mcp, target.app_id, before['windows'][0]['title'], name+'-capture', preparation)
        point = center(before['anchors'][region])
        arguments = {'delta_y': delta, **map_pixel(before['window_bounds'], preparation['png'], point)}
        source = 'return await app.scroll({window_ref:state.w,observation_ref:state.shot.observation_ref,...' + json.dumps(arguments) + '});'
    elif mode == 'semantic':
        anchor, _ = execute(mcp, target.app_id, name+'-observe',
                            semantic_source(before['windows'][0]['title'], region), preparation)
        require('press' not in anchor['element'].get('actions', []), 'Anchor unexpectedly relies on press capability')
        point = center(before['anchors'][region])
        source = 'return await app.scroll({...state.anchor,delta_y:' + str(delta) + '});'
    else:
        point = center(before['window_bounds'])
        require(point_in(before['regions'][region], point), 'Window center is not in the expected native region')
        source = select(before['windows'][0]['title']) + 'return await app.scroll({window_ref:state.w,delta_y:' + str(delta) + '});'
    proof['expected_point'] = point
    require(before['window_bounds'] == target.state()['window_bounds'], 'Fixture moved during preparation')
    execute(mcp, target.app_id, name, source, proof.setdefault('action', {}))
    expected = before['offsets'][region]+delta*16
    proof['after'] = after = wait_until(lambda: (s if abs(s['offsets'][region]-expected) < .01 else None)
                                         if (s := target.state()) else None, timeout=5)
    # A fresh publication catches later wheel ticks; never stop at an intermediate prefix.
    proof['after'] = after = fresh_end(target)
    proof['events'] = require_native_effect(before, after, region, delta, point, before['anchors'][region] if mode == 'semantic' else None)
    proof['point_assertion'] = 'inside independent named label rect' if mode == 'semantic' else 'within 2 points of exact pixel/default anchor'
    proof['delivery'] = scroll_outcome(proof['action']['record']).get('delivery')
    require(proof['delivery'] and proof['delivery']['route'] == 'background', 'Expected actual native background route')
    observe_desktop(desktop, desk_before, fresh_end(desktop), proof.setdefault('desktop', {}), forbidden_pids=(target.child.pid,))
    if mode == 'semantic' and delta > 10:
        require(after['anchors'][region] != before['anchors'][region], 'Own scroll did not move the observed AX row')
        require(len(proof['events']) >= 3, 'Large scroll did not exercise multiple native wheel posts')
        proof['semantic_row_moved_after_first_wheel'] = True
    proof['passed'] = True


def native_stale(mcp, target, desktop, evidence):
    proof = evidence.setdefault('native_stale_replacement', {})
    command(target, 'reset'); proof['setup_foreground'] = front(desktop)
    execute(mcp, target.app_id, 'stale-observe', semantic_source(target.state()['windows'][0]['title'], 'left'),
            proof.setdefault('preparation', {}))
    proof['replacement'] = command(target, 'replace-left')
    proof['before'] = before = target.state()
    record, _ = execute(mcp, target.app_id, 'stale-scroll',
        'return await app.scroll({...state.anchor,delta_y:8});', proof.setdefault('action', {}), success=False)
    proof['after'] = after = fresh_end(target)
    require_stale(record, before, after); proof['passed'] = True


def web_state(path):
    return json.loads(path.read_text())


def web_snapshot(path):
    rows = [e for e in web_state(path)['events'] if e['type'] == 'snapshot']
    return max(rows, key=lambda row: row['sequence']) if rows else None


def browser_point(snapshot, region):
    window = snapshot['window']; css = center(snapshot['regions'][region])
    # Normal Chrome frame edges are symmetric; actual trusted DOM wheel points
    # independently check this mapping, so a bad assumption cannot pass.
    local = [(window['width']-window['inner_width'])/2+css[0], window['height']-window['inner_height']+css[1]]
    return {'css': css, 'screen': [window['x']+local[0], window['y']+local[1]],
            'window': [window['x'], window['y'], window['width'], window['height']]}


def require_browser_effect(before, after, events, region, point, anchor_rect=None):
    wheels = [row for row in events if row['type'] == 'wheel' and row['sequence'] > before['sequence']]
    require(wheels and all(row.get('trusted') is True and row.get('region') == region for row in wheels),
            {'untrusted_or_wrong_region_wheel': wheels})
    require(all(row.get('delta_mode') == 0 and row['delta_y'] > 0 for row in wheels),
            'Chrome wheel units differ from the declared pixel-offset oracle; retain the sample')
    require(all(point_in(anchor_rect, row['client_point']) if anchor_rect else math.dist(row['client_point'], point) <= 2 for row in wheels),
            {'browser_wheel_point_does_not_match_anchor': wheels, 'expected_client_point': point})
    expected = sum(row['delta_y'] for row in wheels)
    require(abs(after['offsets'][region]-before['offsets'][region]-expected) <= 1,
            {'browser_target_did_not_move_full_received_delta': after['offsets'], 'expected': expected})
    require(all(after['offsets'][name] == before['offsets'][name] for name in REGIONS if name != region),
            {'browser_neighbor_region_moved': after['offsets']})
    return wheels


def browser_case(mcp, desktop, path, title, pid, mode, region, name, evidence):
    proof = evidence.setdefault(name, {})
    proof['setup_foreground'] = front(desktop); desk_before = fresh_end(desktop)
    proof['before'] = before = web_snapshot(path)
    prep = proof.setdefault('preparation', {})
    if mode == 'pixel':
        capture(mcp, APP, title, name+'-capture', prep)
        point = browser_point(before, region)
        coords = map_pixel(point['window'], prep['png'], point['screen'])
        source = 'return await app.scroll({window_ref:state.w,observation_ref:state.shot.observation_ref,delta_y:8,...' + json.dumps(coords) + '});'
        css = point['css']; proof['coordinate_mapping'] = point
    elif mode == 'semantic':
        anchor, _ = execute(mcp, APP, name+'-observe', semantic_source(title, region), prep)
        require('press' not in anchor['element'].get('actions', []), 'Browser anchor unexpectedly requires press')
        css = center(before['anchors'][region]); source = 'return await app.scroll({...state.anchor,delta_y:8});'
    else:
        window = before['window']
        css = [window['width']/2-(window['width']-window['inner_width'])/2,
               window['height']/2-(window['height']-window['inner_height'])]
        require(point_in(before['regions'][region], css), 'Default Chrome center is not in expected region')
        source = select(title)+'return await app.scroll({window_ref:state.w,delta_y:8});'
    proof['expected_client_point'] = css
    execute(mcp, APP, name, source, proof.setdefault('action', {}))
    wait_until(lambda: (s if s['offsets'][region] > before['offsets'][region] else None)
               if (s := web_snapshot(path)) else None, timeout=6)
    # Observe two later page publications; smooth scrolling is disabled in this
    # owned profile. No wheel or action is retried while waiting for readback.
    first = web_snapshot(path)['sequence']
    wait_until(lambda: (s if s['sequence'] >= first+2 else None) if (s := web_snapshot(path)) else None, timeout=3)
    all_events = web_state(path)['events']
    proof['after'] = after = max((row for row in all_events if row['type'] == 'snapshot'), key=lambda row: row['sequence'])
    proof['events'] = require_browser_effect(before, after, all_events, region, css, before['anchors'][region] if mode == 'semantic' else None)
    proof['point_assertion'] = 'inside independent named label rect' if mode == 'semantic' else 'within 2 CSS pixels of exact pixel/default anchor'
    proof['full_amount_scope'] = 'Exact pixel offset equals sum of trusted delivered DOM wheel deltas; native fixture separately verifies every requested line'
    proof['delivery'] = scroll_outcome(proof['action']['record']).get('delivery')
    require(proof['delivery'] and proof['delivery']['route'] == 'background', 'Expected actual Chrome background route')
    observe_desktop(desktop, desk_before, fresh_end(desktop), proof.setdefault('desktop', {}), forbidden_pids=(pid,))
    proof['passed'] = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True); parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', choices=('all', 'native', 'chrome', 'stale'), default='all')
    parser.add_argument('--chrome', type=Path, default=Path('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'))
    args = parser.parse_args(); require(sys.platform == 'darwin', 'Requires macOS GUI')
    repo = Path(__file__).resolve().parents[2]; source = repo/'tests/fixtures/anchored-scroll/main.swift'
    html = source.with_name('index.html'); server_source = repo/'tests/fixtures/browser-workbench/server.py'
    output = args.output.resolve(); output.mkdir(parents=True, exist_ok=False); binary = args.dtw.resolve(strict=True)
    observer_binary = output/'scroll-fixture'
    subprocess.run(['xcrun', 'swiftc', '-warnings-as-errors', '-module-cache-path', str(output/'swift-cache'),
                    str(source), '-o', str(observer_binary)], check=True, timeout=90)
    evidence = {'kind': 'owned-anchored-scroll-standard-MCP', 'case': args.case, 'binary_sha256': sha(binary),
        'fixture_sources': {str(p.relative_to(repo)): sha(p) for p in (source, html, server_source)},
        'live_llm_host': False, 'release_artifact': False, 'global_desktop_idle_required': False, 'replay': False,
        'foreground_scroll_real_gui': {'status': 'not_established', 'reason': 'No reliable ordinary fixture preflight decline; foreground point preservation is a separate native source control'},
        'limitations': ['Owned local layouts, not arbitrary-app or external-Agent acceptance',
                       'Coordinates come from independent fixture layout; not model vision',
                       '20ms foreground/pointer observations cannot attribute ambient user actions or exclude shorter excursions']}
    fixtures, clients, logs = [], [], []; chrome = server = None; failure = None
    profile = output/'chrome-profile'
    try:
        evidence['session_precondition'] = session(observer_binary); require_session(evidence['session_precondition'])
        desktop = Fixture.prepare(output/'desktop', 'dev.dtw.scroll.desktop.'+uuid.uuid4().hex,
                                  repo/'tests/fixtures/native-input/main.swift')
        fixtures.append(desktop); write_json(desktop.root/'fixture.json', {'background_probe': True, 'x': 1050})
        evidence['desktop_initial'] = desktop.start()
        if args.case in ('all', 'native', 'stale'):
            target = Fixture.prepare(output/'native', 'dev.dtw.scroll.target.'+uuid.uuid4().hex, source, compiled=observer_binary)
            fixtures.append(target); evidence['native_initial'] = target.start()
            evidence['native_session_precondition'] = session(observer_binary); require_session(evidence['native_session_precondition'])
            mcp = open_mcp(binary, output/'mcp-native', clients)
            if args.case in ('all', 'native'):
                for mode, region, delta, name in (
                    ('pixel', 'left', 8, 'native_pixel_left'), ('pixel', 'nested', 8, 'native_pixel_nested'),
                    ('semantic', 'right', 8, 'native_semantic_right'), ('semantic', 'nested', 8, 'native_semantic_nested'),
                    ('default', 'left', 8, 'native_default_center'), ('semantic', 'left', 25, 'native_semantic_content_moves')):
                    evidence.setdefault(name, {})['session_precondition'] = session(observer_binary)
                    require_session(evidence[name]['session_precondition'])
                    native_case(mcp, target, desktop, mode, region, delta, name, evidence)
            if args.case in ('all', 'stale'):
                evidence.setdefault('native_stale_replacement', {})['session_precondition'] = session(observer_binary)
                require_session(evidence['native_stale_replacement']['session_precondition'])
                native_stale(mcp, target, desktop, evidence)
        if args.case in ('all', 'chrome'):
            evidence['chrome_session_precondition'] = session(observer_binary); require_session(evidence['chrome_session_precondition'])
            case = 'anchor-'+uuid.uuid4().hex[:10]; title = 'DTW Scroll '+case
            page = output/'web-fixture'; page.mkdir(); shutil.copy2(server_source, page/'server.py'); shutil.copy2(html, page/'index.html')
            web = output/'web'; web.mkdir(); path = web/'state.json'
            server_log = (output/'server.log').open('w'); logs.append(server_log)
            server = subprocess.Popen([sys.executable, str(page/'server.py'), '--output', str(web), '--case', case],
                                      stdout=subprocess.PIPE, stderr=server_log, text=True)
            ready, _, _ = io_select.select([server.stdout], [], [], 5)
            require(ready, 'Owned loopback server did not publish its address within five seconds')
            address = json.loads(server.stdout.readline()); evidence['server'] = {'pid': server.pid, **address}
            profile.mkdir(); chrome_log = (output/'chrome.log').open('w'); logs.append(chrome_log)
            chrome_args = [str(args.chrome.resolve(strict=True)), '--user-data-dir='+str(profile), '--no-first-run',
                '--no-default-browser-check', '--disable-background-mode', '--disable-smooth-scrolling',
                '--window-size=1000,750', '--window-position=60,80', '--new-window', address['url']]
            chrome = subprocess.Popen(chrome_args, stdout=chrome_log, stderr=subprocess.STDOUT)
            evidence['chrome'] = {'pid': chrome.pid, 'argv': chrome_args, 'ownership': 'Exact child and unique disposable profile'}
            evidence['page_initial'] = wait_until(lambda: web_snapshot(path) if path.exists() else None, timeout=20)
            require(chrome.poll() is None, 'Owned Chrome exited or forwarded startup elsewhere')
            mcp = open_mcp(binary, output/'mcp-chrome', clients)
            for mode, region, name in (('pixel', 'left', 'chrome_pixel_left'), ('semantic', 'nested', 'chrome_semantic_nested'),
                                       ('pixel', 'nested', 'chrome_pixel_nested'), ('semantic', 'right', 'chrome_semantic_right'),
                                       ('default', 'left', 'chrome_default_center')):
                evidence.setdefault(name, {})['session_precondition'] = session(observer_binary)
                require_session(evidence[name]['session_precondition'])
                browser_case(mcp, desktop, path, title, chrome.pid, mode, region, name, evidence)
    except BaseException as error:
        failure = {'error': repr(error), 'traceback': traceback.format_exc()}
        if 'gui_precondition_locked:' in str(error): failure['kind'] = 'gui_precondition_locked'
    finally:
        cleanup = evidence.setdefault('cleanup', [])
        def attempt(action):
            nonlocal failure
            try: cleanup.append(action())
            except BaseException as error:
                cleanup.append({'error': repr(error)}); failure = failure or {'error': 'Owned cleanup failed'}
        for mcp in reversed(clients):
            def close_client(mcp=mcp):
                result = close_mcp(mcp); check_cleanup(mcp, result); return result
            attempt(close_client)
        if chrome is not None:
            def close_chrome():
                if chrome.poll() is None: chrome.terminate()
                result = stop_owned(chrome)
                require(not result['forced'] and result['returncode'] in (0, -15), result)
                marker = '--user-data-dir='+str(profile)
                wait_until(lambda: not any(marker in cmd for _, cmd in process_table().values()), timeout=5)
                result['profile_processes_reaped'] = True; return result
            attempt(close_chrome)
        if server is not None:
            def close_server():
                (output/'web/stop').write_text('stop\n'); result = stop_owned(server)
                require(not result['forced'] and result['returncode'] == 0, result); return result
            attempt(close_server)
        for fixture in reversed(fixtures):
            def close_fixture(fixture=fixture):
                result = fixture.close(); check_cleanup(fixture, result); return result
            attempt(close_fixture)
        for stream in logs: stream.close()
        if failure is not None:
            try: evidence['session_at_failure'] = session(observer_binary)
            except BaseException as error: evidence['session_at_failure'] = {'unavailable': repr(error)}
        evidence.update(passed=failure is None, failure=failure)
        write_json(output/'evidence.json', evidence)
    print(json.dumps({'passed': evidence['passed'], 'evidence': str(output/'evidence.json'), 'failure': failure}))
    return 0 if evidence['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
