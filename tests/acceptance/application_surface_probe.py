#!/usr/bin/env python3
"""Opt-in owned raised-surface GUI acceptance through standard DTW stdio MCP.

  python3 tests/acceptance/application_surface_probe.py --dtw SIGNED_DTW --output NEW_DIRECTORY

Builds and starts one owned AppKit fixture, opens one non-layer-zero window,
checks ordinary + raised window inventory, captures the raised surface, and
clicks its pad exactly once with independent received-event evidence. Closing
only that owned surface must retire its original reference without losing the
ordinary window or app state. A stale click is attempted once and must refuse
before dispatch. There are no input retries or global-idle/pointer invariants;
unrelated user keyboard, mouse and other-app activity does not fail this probe.
Target conflicts and all original receipts remain visible in the output.
This proves native GUI/MCP behavior, not a live-model, remote-service, or browser
exact-tab capability. Coordinates come from the fixture's independent geometry.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import sys
import time
import traceback
import uuid

from native_input_probe import check_cleanup, close_mcp
from standard_agent_probe import Fixture, Mcp, require, save_png, successful, wait_until, write_json


def image_point(surface, shot):
    bounds, point = surface['bounds'], surface['pad_screen_point']
    width, height = shot['image']['width'], shot['image']['height']
    require(len(bounds) == 4 and len(point) == 2 and all(
        type(value) in (int, float) and math.isfinite(value) for value in bounds + point),
        'Invalid fixture surface geometry')
    require(bounds[2] > 0 and bounds[3] > 0 and width > 0 and height > 0, 'Empty image/surface')
    x, y = (point[0]-bounds[0])/bounds[2]*width, (point[1]-bounds[1])/bounds[3]*height
    require(0 < x < width and 0 < y < height, 'Owned click pad lies outside the exact image')
    return {'x': x, 'y': y}


def surface_events(fixture, after_sequence):
    return [json.loads(line) for line in (fixture.root/'events.jsonl').read_bytes().splitlines(keepends=True)
            if line.endswith(b'\n') and json.loads(line)['sequence'] > after_sequence]


def verify_click(before, after, events):
    target = before['application_surface']['id']
    first, last = before['application_surface']['pad'], after['application_surface']['pad']
    require(after['application_surface']['id'] == target, 'Surface identity changed during click')
    require(last == {**first, 'left_down': first['left_down']+1, 'left_up': first['left_up']+1},
            {'wrong_surface_effect': last, 'before': first})
    received = [event for event in events if event['kind'].startswith('surface_')]
    require([event['kind'] for event in received] == ['surface_left_down', 'surface_left_up'],
            {'surface_event_sequence': received})
    require(all(event['window_number'] == target for event in received), 'Click reached another surface')
    require(after['pad'] == before['pad'], 'Raised click reached the ordinary window pad')
    return received


def verify_stale_refusal(record):
    require(record.get('error') is not None and record.get('status') == 'completed', record)
    require(record.get('context_reset') is False and record.get('state_reset') is False,
            'Stale window reset the app owner')
    script = record.get('script', {})
    require(script.get('state_committed') is False and script.get('failure', {}).get('code') == 'target_changed',
            'Stale surface failed for a reason other than target disappearance')
    steps = script.get('steps', [])
    require(len(steps) == 1 and steps[0].get('operation') == 'click', 'Unexpected stale action sequence')
    step = steps[0]
    require(step.get('error', {}).get('code') == 'target_changed', 'Stale click error lost target identity')
    receipt = step.get('receipt')
    if receipt is not None:
        require(receipt.get('outcome', {}).get('dispatch') == 'not_dispatched', 'Stale click may have dispatched')
        provider = receipt.get('provider_outcome')
        require(provider is None or provider.get('dispatch') == 'not_dispatched', 'Stale native click may have dispatched')
    require(not script.get('shutdown_error'), 'Stale click caused cleanup failure')


def run(mcp, fixture, output, evidence):
    evidence['opened'] = fixture.command('application_surface_open')
    def ready():
        state = fixture.state()
        surface = state.get('application_surface', {})
        return state if surface.get('visible') and surface.get('window_server_present') else None
    before = wait_until(ready)
    evidence['ready'] = before
    require(type(before['application_surface']['window_server_layer']) is int
            and before['application_surface']['window_server_layer'] != 0, 'Fixture is not a raised WindowServer surface')
    require(before['windows'][0]['visible'], 'Ordinary window is missing')
    title = before['application_surface']['title']
    primary = before['windows'][0]['title']
    marker = uuid.uuid4().hex

    def execute(case, source):
        record, response = mcp.execute(fixture.app_id, case, source)
        evidence[case] = {'record': record, 'fixture': fixture.state()}
        return successful(record), response

    inventory, _ = execute('surface-inventory', f"""
const inventory=await app.windows();
const surfaces=inventory.windows.filter(w=>w.title==={json.dumps(title)});
const ordinary=inventory.windows.filter(w=>w.title==={json.dumps(primary)});
if(surfaces.length!==1||ordinary.length!==1) throw new Error('Owned surface inventory is not unique');
state.surface=surfaces[0].window_ref;state.ordinary=ordinary[0].window_ref;
state.surfaceMarker={json.dumps(marker)};
return inventory;
""")
    require(inventory['coverage']['complete_within_scope'] is True, 'Surface inventory is incomplete')
    require(inventory['coverage']['scope'] == 'same_process_application_surfaces', 'Unexpected surface coverage')
    require('non_default_layers' not in inventory['coverage']['excludes'], 'Raised surfaces remain excluded')
    shot, response = execute('surface-capture',
        'state.surfaceShot=await app.capture({window_ref:state.surface});return state.surfaceShot;')
    folder = output/'surface-capture'; folder.mkdir()
    evidence['surface-capture']['png'] = save_png(folder, response, shot)
    point = image_point(before['application_surface'], shot)
    evidence['surface-capture']['point'] = point
    before_click = fixture.state()
    evidence['before_click'] = before_click
    execute('surface-click', f"""
return await app.click({{window_ref:state.surface,observation_ref:state.surfaceShot.observation_ref,
    x:{point['x']},y:{point['y']}}});
""")
    def clicked():
        state = fixture.state()
        return state if state['application_surface']['pad']['left_up'] > before_click['application_surface']['pad']['left_up'] else None
    after_click = wait_until(clicked)
    evidence['click_effect'] = {'state': after_click}
    evidence['click_effect']['events'] = verify_click(before_click, after_click,
        surface_events(fixture, before_click['events_count']))
    # A new capture is the original live binding for the later stale-ref test.
    # It is not a retry of the successful click.
    shot, response = execute('before-close-capture',
        'state.surfaceShot=await app.capture({window_ref:state.surface});return state.surfaceShot;')
    folder = output/'before-close-capture'; folder.mkdir()
    evidence['before-close-capture']['png'] = save_png(folder, response, shot)
    evidence['close_command'] = fixture.command('application_surface_close')
    surface_id = before['application_surface']['id']
    def closed():
        state = fixture.state()
        return state if surface_id in state['closed_window_ids'] and not state['application_surface']['visible'] else None
    evidence['closed'] = wait_until(closed)
    close_events = [event for event in surface_events(fixture, after_click['events_count'])
                    if event['kind'] == 'window_closed' and event['window_number'] == surface_id]
    evidence['close_events'] = close_events
    require(len(close_events) == 1, 'Expected one independent surface close notification')
    execute('after-close-inventory', f"""
const inventory=await app.windows();
if(inventory.windows.some(w=>w.window_ref===state.surface||w.title==={json.dumps(title)}))
  throw new Error('Closed raised surface remains live');
if(!inventory.windows.some(w=>w.window_ref===state.ordinary)) throw new Error('Ordinary window ref changed');
if(state.surfaceMarker!=={json.dumps(marker)}) throw new Error('App state was reset');
return inventory;
""")
    record, _ = mcp.execute(fixture.app_id, 'stale-surface-click', f"""
return await app.click({{window_ref:state.surface,observation_ref:state.surfaceShot.observation_ref,
    x:{point['x']},y:{point['y']}}});
""")
    evidence['stale-surface-click'] = {'record': record, 'fixture': fixture.state()}
    verify_stale_refusal(record)
    final = fixture.state()
    require(final['application_surface']['pad'] == after_click['application_surface']['pad'], 'Stale click added an effect')
    require(final['pad'] == before_click['pad'], 'Stale click reached the surviving ordinary window')
    evidence['final'] = final


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(sys.platform == 'darwin', 'Requires a macOS GUI desktop')
    output = args.output.absolute(); output.mkdir(mode=0o700)
    binary = args.dtw.resolve(strict=True)
    source = Path(__file__).resolve().parents[1]/'fixtures/native-input/main.swift'
    evidence = {'kind': 'owned-application-surface-standard-MCP', 'live_llm_host': False,
                'global_desktop_idle_required': False, 'input_retries': 0,
                'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                'fixture_source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                'started_unix_ms': int(time.time()*1000)}
    fixture, mcp, failure = None, None, None
    try:
        fixture = Fixture.prepare(output/'target', 'dev.dtw.surface.'+uuid.uuid4().hex, source)
        write_json(fixture.root/'fixture.json', {'background_probe': True,
            'surface_title': 'DTW Surface '+uuid.uuid4().hex[:12]})
        evidence['before'] = fixture.start()
        directory = output/'mcp'; directory.mkdir()
        mcp = Mcp(binary, directory)
        mcp.rpc('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                              'clientInfo': {'name': 'application-surface-probe', 'version': '1'}})
        mcp.notify('notifications/initialized')
        run(mcp, fixture, output, evidence)
    except BaseException as error:
        failure = {'type': type(error).__name__, 'message': str(error)}
        (output/'failure.txt').write_text(traceback.format_exc())
    finally:
        evidence['cleanup'] = []
        for name, resource in [('mcp', mcp), ('fixture', fixture)]:
            if resource is None: continue
            row = {'resource': name}; evidence['cleanup'].append(row)
            try:
                result = close_mcp(resource) if isinstance(resource, Mcp) else resource.close()
                row.update(result); check_cleanup(resource, result)
            except BaseException as error:
                row['error'] = {'type': type(error).__name__, 'message': str(error)}
                failure = failure or {'type': 'CleanupFailure', 'message': str(error)}
        evidence.update(passed=failure is None, failure=failure, finished_unix_ms=int(time.time()*1000))
        write_json(output/'evidence.json', evidence)
    print(json.dumps({'passed': evidence['passed'], 'evidence': str(output/'evidence.json'), 'failure': failure}, ensure_ascii=False))
    return 0 if evidence['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
