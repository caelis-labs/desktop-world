#!/usr/bin/env python3
"""Opt-in Chrome background-input acceptance through ordinary stdio MCP.

Run on macOS with a signed DTW candidate and an owned disposable Chrome profile:
  python3 tests/acceptance/background_browser_probe.py --dtw /path/to/dtw --output /tmp/new-run

The reused 240-record workbench runs in a new, disposable Chrome profile. An
owned AppKit window initially covers the coordinate target. Background and
occlusion coverage are recorded separately from the functional action result.
No CDP, browser automation, user profile, or action replay is used. The page's
native events and persisted business data determine functional success
independently of DTW dispatch receipts. Focus/pointer journals are observations,
not attribution of unrelated user input to DTW. Coordinates
come from owned-page geometry: this tests native delivery, not model vision or
external-Agent usability. Pointer sampling cannot exclude excursions <20 ms.
"""
import argparse
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import traceback
import uuid

from background_input_probe import front, fresh_end, observe_desktop
from browser_workbench_probe import SupervisorMcp, checked_windows, file_sha256
from native_input_probe import check_cleanup, close_mcp, input_is_released
from standard_agent_probe import Fixture, process_table, require, save_png, stop_owned, successful, wait_until, write_json

APP = 'com.google.Chrome'
SKU = 'DTW-0088'
EXPECTED = '后台复核：已核对 🐦 café'

# Startup instrumentation observes the owned business page. The only fixture
# behavior change is the textarea's static autofocus attribute, added below.
# Events are genuine DOM events; nothing here synthesizes input or saves data.
INSTRUMENTATION = r'''
<script>
(() => {
  let sequence = 0;
  const send = (type, details) => event('probe_' + type, {probe_seq: ++sequence, ...details});
  const control = target => target?.id || (target?.matches?.('#edit-form button[type=submit]') ? 'save' : target?.tagName || '');
  const rect = selector => { const e=document.querySelector(selector); if(!e) return null;
    const r=e.getBoundingClientRect(); return {x:r.x,y:r.y,width:r.width,height:r.height}; };
  const snapshot = () => send('snapshot', {
    active:control(document.activeElement), note:byId('note').value, search:byId('search').value,
    dialog_open:byId('editor').open, rendered_rows:byId('rows').children.length,
    scroll_y:scrollY, document_height:document.documentElement.scrollHeight,
    window:{x:screenX,y:screenY,width:outerWidth,height:outerHeight,
      inner_width:innerWidth,inner_height:innerHeight,device_pixel_ratio:devicePixelRatio},
    rects:{save:rect('#edit-form button[type=submit]'),note:rect('#note')}
  });
  for (const name of ['keydown','keyup','input','pointerdown','pointerup','click','wheel']) {
    document.addEventListener(name, e => {
      send(name, {control:control(e.target),trusted:e.isTrusted,key:e.key,code:e.code,
        meta:e.metaKey,ctrl:e.ctrlKey,alt:e.altKey,shift:e.shiftKey,
        client_x:e.clientX,client_y:e.clientY,delta_x:e.deltaX,delta_y:e.deltaY,
        value:typeof e.target?.value==='string'?e.target.value:null});
      if(name==='input') queueMicrotask(snapshot);
    }, {capture:true,passive:true});
  }
  document.addEventListener('scroll', snapshot, {capture:true,passive:true});
  setInterval(snapshot, 250);
  snapshot();
})();
</script>
'''

FIND = r'''
async function find(role,text) {
  let p=await app.observe({window_ref:state.w,role,text,limit:5}), pages=0;
  while(true) {
    pages++;
    const matches=p.elements.filter(e=>e.name===text);
    if(matches.length===1) return {ref:{window_ref:state.w,observation_ref:p.observation_ref,
      element_ref:matches[0].element_ref},pages,coverage:p.coverage};
    if(matches.length>1) throw Error('Ambiguous owned control '+text);
    if(!p.next || pages>=50) throw Error('Missing owned control '+text+' '+JSON.stringify(p.coverage));
    p=await app.observe({cursor:p.next});
  }
}
'''


def web_state(path):
    return json.loads(path.read_text())


def latest_snapshot(state):
    rows = [e for e in state['events'] if e.get('type') == 'probe_snapshot']
    return max(rows, key=lambda e: e['probe_seq']) if rows else None


def snapshot_when(path, predicate, *, after=0, timeout=6):
    def ready():
        snap = latest_snapshot(web_state(path))
        return snap if snap and snap['probe_seq'] > after and predicate(snap) else None
    return wait_until(ready, timeout=timeout)


def events_after(state, baseline):
    return [e for e in state['events'] if e.get('probe_seq', 0) > baseline]


def completed_keyboard_sample(state, baseline, expected):
    """Require the final snapshot and trusted release from one journal read.

    The expected text is also an intermediate prefix of expected + sentinel;
    text equality alone must not end observation before Backspace is delivered.
    """
    page = latest_snapshot(state)
    events = events_after(state, baseline)
    if not page or page['probe_seq'] <= baseline or page.get('note') != expected or not page.get('dialog_open'):
        return None
    keys = [e for e in events if e.get('control') == 'note' and e.get('trusted')]
    if not any(e['type'] == 'probe_keydown' and e.get('meta') and e.get('key', '').lower() == 'a' for e in keys):
        return None
    if not any(e['type'] == 'probe_input' for e in keys):
        return None
    pair = [[e for e in keys if e['type'] == kind and e.get('key') == 'Backspace']
            for kind in ('probe_keydown', 'probe_keyup')]
    if any(len(events) != 1 for events in pair):
        return None
    if not baseline < pair[0][0]['probe_seq'] < pair[1][0]['probe_seq'] < page['probe_seq']:
        return None
    return {'page': page, 'events': events}


def covered_point(snapshot, image, observer_bounds):
    """Map CSS geometry to window image pixels; never infer successful input."""
    window, rect = snapshot['window'], snapshot['rects']['save']
    values = list(window.values()) + list(rect.values()) + list(observer_bounds)
    require(all(type(v) in (int, float) and math.isfinite(v) for v in values), 'Nonfinite geometry')
    require(rect['width'] > 0 and rect['height'] > 0 and window['width'] > 0 and window['height'] > 0,
            'Missing visible coordinate target')
    require(0 < window['inner_width'] <= window['width'] and 0 < window['inner_height'] <= window['height'],
            'Invalid browser viewport dimensions')
    client = [rect['x'] + rect['width']/2, rect['y'] + rect['height']/2]
    # Normal Chrome windows have symmetric horizontal frame edges and no lower
    # toolbar. Native DOM click coordinates below independently check this map.
    local = [(window['width'] - window['inner_width'])/2 + client[0],
             window['height'] - window['inner_height'] + client[1]]
    require(0 < local[0] < window['width'] and 0 < local[1] < window['height'], 'Target is outside Chrome')
    screen = [window['x'] + local[0], window['y'] + local[1]]
    x, y, width, height = observer_bounds
    observer_covers = x + 4 < screen[0] < x + width - 4 and y + 4 < screen[1] < y + height - 4
    return {'x': round(local[0] * image['width'] / window['width']),
            'y': round(local[1] * image['height'] / window['height']),
            'client': client, 'screen': screen, 'browser_geometry': window,
            'observer_bounds': observer_bounds, 'observer_geometry_covers_point': observer_covers,
            'target_rect': rect,
            'coordinate_source': 'owned page read-only CSS geometry scaled to MCP image dimensions'}


def execute(client, name, source, proof):
    started = time.monotonic()
    record, response = client.execute(APP, name, source)
    proof.update(record=record, elapsed_seconds=time.monotonic()-started)
    require(record.get('app') == APP, 'MCP returned a different application identity')
    require(record.get('context_reset') is False and record.get('state_reset') is False,
            'Owned window identity continuity was lost; do not replay')
    return successful(record), response


def readiness_source():
    # Keep completed pages even if a later read is temporarily unavailable.
    # This script has no write/focus operations and changes no app state.
    return 'const name='+json.dumps('Edit '+SKU)+';\n'+r"""
const pages=[],started=Date.now();
try {
  let page=await app.observe({window_ref:state.w,role:'button',text:name,limit:5});
  while(true) {
    pages.push({elements:page.elements,coverage:page.coverage,
      observation_ref:page.observation_ref,next:page.next??null,elapsed_ms:Date.now()-started});
    if(!page.next) return {pages,read_error:null};
    if(pages.length>=50) return {pages,read_error:{code:'readiness_page_limit',message:'Read probe exceeded 50 pages'}};
    page=await app.observe({cursor:page.next});
  }
} catch(error) {
  return {pages,read_error:{code:typeof error?.code==='string'?error.code:null,message:String(error)}};
}
"""


def await_deep_control(client, output, proof):
    """Bounded fresh reads before any setup write; never retry the setup script."""
    schedule = (0, 0.25, 1.0, 2.25)
    started = time.monotonic()
    proof.update(attempts=[], scheduled_offsets_seconds=list(schedule), launch_window_seconds=5,
                 readonly=True, ready=False,
                 readiness_trace_enabled=os.environ.get('DTW_AX_READINESS_TRACE') == '1',
                 timing_scope='Only new read-probe launches stop at five seconds; an in-flight '
                              'call retains the DTW deadline. One owned-window diagnostic capture is separate.')
    for index, offset in enumerate(schedule):
        delay = started+offset-time.monotonic()
        if delay > 0: time.sleep(delay)
        now = time.monotonic()
        if now-started >= 5:
            proof['stopped_reason'] = 'launch_window_elapsed'
            break
        attempt = {'number':index+1, 'scheduled_offset_seconds':offset, 'started_offset_seconds':now-started}
        proof['attempts'].append(attempt)
        value, _ = execute(client, f'ax-ready-read-{index+1}', readiness_source(), attempt)
        require(not attempt['record'].get('script', {}).get('shutdown_error'),
                'Read-only AX probe left a cleanup failure')
        require(isinstance(value, dict) and isinstance(value.get('pages'), list), 'Malformed AX read result')
        pages, error = value['pages'], value.get('read_error')
        attempt.update(pages=pages, read_error=error)
        require(error is None or isinstance(error, dict) and error.get('code') == 'semantic_not_ready',
                {'unexpected_ax_read_error':error})
        require(len(pages) <= 50, 'Read probe returned too many pages')
        elements, references = [], []
        for page in pages:
            require(isinstance(page, dict) and isinstance(page.get('elements'), list)
                    and isinstance(page.get('coverage'), dict)
                    and isinstance(page.get('observation_ref'), str), 'Malformed AX observation page')
            elements.extend(page['elements']); references.append(page['observation_ref'])
        attempt['observation_refs'] = references
        matches = [element for element in elements if element.get('name') == 'Edit '+SKU]
        require(len(matches) <= 1, 'Ambiguous owned deep control in read-only readiness probe')
        if error is None and matches:
            proof.update(ready=True, matched_attempt=index+1, elapsed_seconds=time.monotonic()-started)
            attempt['status'] = 'target_observed'
            return
        # Nonempty but differently named results indicate a selector assumption,
        # not an empty/cold tree. Retain them and fail without another attempt.
        require(error is not None or not elements, 'AX read returned controls but no exact owned deep name')
        attempt['status'] = 'semantic_not_ready' if error else 'empty'
        if 'first_empty_capture' not in proof:
            capture = proof.setdefault('first_empty_capture', {'after_attempt':index+1})
            image, response = execute(client, 'ax-ready-first-empty-capture',
                                      'return await app.capture({window_ref:state.w});', capture)
            directory = output/'readiness-capture'; directory.mkdir()
            capture['png'] = save_png(directory, response, image)
    proof.update(elapsed_seconds=time.monotonic()-started,
                 stopped_reason=proof.get('stopped_reason', 'attempt_limit'))
    raise AssertionError('Owned deep AX control stayed unavailable within the bounded read-only probes; no setup writes ran')


def desktop_end(observer, before, proof, target_pid):
    after = fresh_end(observer)
    desktop = proof.setdefault('desktop', {})
    observe_desktop(observer, before, after, desktop)
    rows = [before, after] + desktop['samples'] + desktop['activations']
    target_front = [row for row in rows if row['frontmost_pid'] == target_pid]
    desktop['background_coverage'] = {
        'status': 'not_established' if target_front or desktop['sampling_coverage']['status'] != 'complete' else 'observed',
        'sampling_coverage': desktop['sampling_coverage'],
        'target_pid': target_pid, 'target_foreground_observations': target_front,
        'basis': 'owned Chrome PID absent from sampled foreground and activation journal',
        'attribution': 'Foreground changes may come from the user; observations alone do not identify their cause'}
    desktop['observer_input_observations'] = {
        'text_changed': after['text'] != before['text'], 'pad_changed': after['pad'] != before['pad'],
        'global_input_released_at_end': input_is_released(after),
        'attribution': 'User input is permitted; observer/global changes do not prove DTW misdelivery'}
    return after


def occlusion_coverage(point, desktop, observer_pid):
    """Report sampled coverage without failing valid input after unrelated moves."""
    x, y, width, height = desktop['after']['window_bounds']
    sx, sy = point['screen']
    after_covers = x + 4 < sx < x + width - 4 and y + 4 < sy < y + height - 4
    rows = [desktop['before'], desktop['after']] + desktop['samples'] + desktop['activations']
    observer_front = all(row['frontmost_pid'] == observer_pid for row in rows)
    continuous = desktop['sampling_coverage']['status'] == 'complete'
    observed = point['observer_geometry_covers_point'] and after_covers and observer_front and continuous
    return {'status': 'observed_at_boundaries' if observed else 'not_established',
            'observer_geometry_covers_before': point['observer_geometry_covers_point'],
            'observer_geometry_covers_after': after_covers,
            'observer_foreground_through_samples': observer_front,
            'sampling_coverage': desktop['sampling_coverage'],
            'limitations': 'Geometry is checked at action boundaries; this does not prove continuous occlusion'}


def action_deliveries(record):
    return [(step['receipt'].get('provider_outcome') or step['receipt']['outcome']).get('delivery')
            for step in record.get('script', {}).get('steps', []) if step.get('receipt')]


def require_delivery(record, routes):
    values = action_deliveries(record)
    require(len(values) == len(routes) and all(isinstance(value, dict) for value in values),
            'Native delivery evidence missing')
    require([value.get('route') for value in values] == routes, {'unexpected_delivery':values,'expected':routes})


def require_rejection(record):
    steps = record.get('script', {}).get('steps', [])
    require(len(steps) == 1, 'Strict rejection ran additional host actions')
    outcome = steps[0]['receipt']['outcome']
    require(outcome['dispatch'] == 'not_dispatched' and outcome['error']['code'] == 'foreground_required',
            'Strict action was not refused before dispatch')
    require(outcome.get('delivery', {}).get('foreground_borrowed') is False
            and outcome['delivery']['route'] is None and bool(outcome['delivery'].get('reason')),
            'Strict rejection lacks its route-selection reason')


def run_cases(client, observer, path, output, evidence, target_pid):
    # Query a control deep in the original 100-row AX table before filtering.
    await_deep_control(client, output, evidence.setdefault('ax_readiness', {}))
    setup = evidence.setdefault('setup', {})
    value, _ = execute(client, 'complex-ax-discovery', FIND + f"""
const far=await find('button','Edit {SKU}');
const search=await find('text_field','Search inventory');
await app.setValue({{...search.ref,value:{json.dumps(SKU)}}});
const edit=await find('button','Edit {SKU}');await app.press(edit.ref);
return {{deep_button:{{pages:far.pages,coverage:far.coverage}},search_pages:search.pages,edit_pages:edit.pages}};
""", setup)
    setup['discovery'] = value
    setup['page'] = snapshot_when(path, lambda s: s['dialog_open'] and s['active'] == 'note' and s['search'] == SKU)
    setup['foreground'] = front(observer)
    start = fresh_end(observer)
    setup['desktop_before_actions'] = start
    setup['global_input_released_before_actions'] = input_is_released(start)
    require(type(target_pid) is int and target_pid > 0, 'Missing exact owned Chrome PID')
    setup['target'] = {'app': APP, 'owned_pid': target_pid,
                       'window_ref': evidence['owned_window']['window_ref'],
                       'title': evidence['owned_window']['title']}

    seed = evidence.setdefault('background_plain_keyboard', {})
    baseline = latest_snapshot(web_state(path))['probe_seq']
    execute(client, 'background-plain-text', "return await app.typeText({window_ref:state.w,text:'seed-to-replace'});", seed)
    seed['page'] = snapshot_when(path, lambda s: s['note'] == 'seed-to-replace' and s['dialog_open'], after=baseline)
    desktop_end(observer, start, seed, target_pid)

    require_delivery(seed['record'], ['background'])
    if evidence['input_policy'] == 'background_only':
        strict = evidence.setdefault('strict_rejections', [])
        for name, action in [
            ('shortcut', "app.keypress({window_ref:state.w,keys:['META','A']})"),
            ('supplementary_unicode', "app.typeText({window_ref:state.w,text:'blocked 🐦'})"),
            ('focus', "app.focus({window_ref:state.w})"),
        ]:
            proof = {'operation': name}; strict.append(proof)
            before = fresh_end(observer)
            baseline = latest_snapshot(web_state(path))['probe_seq']
            record, response = client.execute(APP, 'strict-reject-'+name, 'return await '+action+';')
            proof.update(record=record, response=response)
            require_rejection(record)
            # Require a later observer tick and inspect the independent journal,
            # not the route receipt, for unintended input into this owned page.
            proof['page'] = snapshot_when(path, lambda page: page['note'] == 'seed-to-replace'
                and page['dialog_open'], after=baseline+1)
            proof['events'] = events_after(web_state(path), baseline)
            require(not any(event['type'] in ('probe_keydown','probe_keyup','probe_input',
                'probe_pointerdown','probe_pointerup','probe_click','probe_wheel')
                for event in proof['events']), 'Strict rejection still sent input to the owned page')
            desktop_end(observer, before, proof, target_pid)
        keyboard = evidence.setdefault('semantic_unicode_replacement', {})
        start = fresh_end(observer)
        execute(client, 'strict-semantic-replacement', FIND +
            "const note=await find('text_area','Handoff note');return await app.setValue({...note.ref,value:"
            + json.dumps(EXPECTED) + '});', keyboard)
        require_delivery(keyboard['record'], ['semantic'])
        keyboard['page'] = snapshot_when(path, lambda page: page['note'] == EXPECTED and page['dialog_open'])
        desktop_end(observer, start, keyboard, target_pid)
    else:
        keyboard = evidence.setdefault('automatic_keyboard', {'routing':'Cmd+printable and supplementary Unicode select foreground before input'})
        start = fresh_end(observer)
        baseline = latest_snapshot(web_state(path))['probe_seq']
        execute(client, 'automatic-unicode-and-shortcuts', f"""
    await app.keypress({{window_ref:state.w,keys:['META','A']}});
    await app.typeText({{window_ref:state.w,text:{json.dumps(EXPECTED + 'Z')}}});
    return await app.keypress({{window_ref:state.w,keys:['BACKSPACE']}});
    """, keyboard)
        def completed_keyboard():
            state = web_state(path)
            keyboard['last_observed_page'] = latest_snapshot(state)
            keyboard['events'] = events_after(state, baseline)
            return completed_keyboard_sample(state, baseline, EXPECTED)
        keyboard.update(wait_until(completed_keyboard, timeout=6))
        desktop_end(observer, start, keyboard, target_pid)
        require_delivery(keyboard['record'], ['foreground','foreground','background'])
        for returned in action_deliveries(keyboard['record'])[:2]:
            if returned['foreground_borrowed']:
                require(returned.get('focus_return') in ('retained','changed_externally'),
                        'Automatic fallback lost its foreground-retention evidence')
        require(fresh_end(observer)['frontmost_pid'] == target_pid,
                'Automatic fallback unexpectedly restored the previous app')
        keyboard['target_foreground_retention_verified'] = True
        # Cover the browser again explicitly for the independent occluded-click case.
        keyboard['observer_front_for_next_case'] = front(observer)

    click = evidence.setdefault('covered_coordinate_click', {})
    capture, response = execute(client, 'capture-covered-browser',
        'state.shot=await app.capture({window_ref:state.w});return state.shot;', click.setdefault('capture', {}))
    capture_dir = output/'capture'; capture_dir.mkdir()
    click['image'] = save_png(capture_dir, response, capture)
    page = snapshot_when(path, lambda s: s['note'] == EXPECTED and s['dialog_open'], after=baseline)
    before_click = fresh_end(observer)
    point = covered_point(page, capture['image'], before_click['window_bounds'])
    click['point'] = point
    baseline = latest_snapshot(web_state(path))['probe_seq']
    saves_before = len(web_state(path)['saves'])
    execute(client, 'click-covered-save', f"""
return await app.click({{window_ref:state.w,observation_ref:state.shot.observation_ref,
  x:{point['x']},y:{point['y']},button:'left',count:1}});
""", click)
    def saved():
        state = web_state(path)
        return state if len(state['saves']) > saves_before else None
    saved_state = wait_until(saved, timeout=6)
    require_delivery(click['record'], ['background'])
    click['saved'] = saved_state['saves'][saves_before:]
    require(len(click['saved']) == 1 and click['saved'][0]['sku'] == SKU
            and click['saved'][0]['note'] == EXPECTED, 'Save was missing, incorrect, or repeated')
    click['page'] = snapshot_when(path, lambda s: not s['dialog_open'], after=baseline)
    click['events'] = events_after(web_state(path), baseline)
    clicks = [e for e in click['events'] if e['type'] == 'probe_click' and e.get('control') == 'save']
    require(len(clicks) == 1 and clicks[0].get('trusted'), 'Missing or duplicate trusted Save click')
    require(abs(clicks[0]['client_x'] - point['client'][0]) <= 3
            and abs(clicks[0]['client_y'] - point['client'][1]) <= 3, 'Native click missed the intended DOM point')
    desktop_end(observer, before_click, click, target_pid)
    click['occlusion_coverage'] = occlusion_coverage(point, click['desktop'], observer.child.pid)

    scroll = evidence.setdefault('background_default_scroll', {})
    execute(client, 'restore-long-table', FIND + "const s=await find('text_field','Search inventory');"
            "return await app.setValue({...s.ref,value:''});", scroll.setdefault('setup', {}))
    initial = snapshot_when(path, lambda s: s['rendered_rows'] == 100 and not s['dialog_open'])
    require(initial['document_height'] > initial['window']['inner_height'] + 100, 'The page is not scrollable')
    baseline = initial['probe_seq']
    scroll['before'] = initial
    before_scroll = fresh_end(observer)
    execute(client, 'background-default-scroll',
            'return await app.scroll({window_ref:state.w,delta_y:8});', scroll)
    require_delivery(scroll['record'], ['background'])
    scroll['after'] = snapshot_when(path, lambda s: s['scroll_y'] > initial['scroll_y'], after=baseline)
    scroll['events'] = events_after(web_state(path), baseline)
    require(any(e['type'] == 'probe_wheel' and e.get('trusted') and e.get('delta_y', 0) > 0
                for e in scroll['events']), 'No trusted downward DOM wheel event')
    desktop_end(observer, before_scroll, scroll, target_pid)
    final_state = web_state(path)
    require(len(final_state['saves']) == saves_before + 1, 'Later input repeated the save')
    evidence['independent_final_save'] = final_state['saves'][-1]
    evidence['background_coverage'] = {name: case['desktop']['background_coverage']['status']
        for name, case in [('plain_keyboard', seed), ('semantic_replacement' if evidence['input_policy'] == 'background_only' else 'keyboard_with_fallback', keyboard), ('click', click), ('scroll', scroll)]}
    evidence['occlusion_coverage'] = click['occlusion_coverage']['status']
    evidence['all_cases_passed'] = True  # Functional effects; coverage is recorded separately.


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--input-policy', choices=['auto','background-only'], default='auto')
    parser.add_argument('--chrome', type=Path, default=Path('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'))
    args = parser.parse_args()
    require(sys.platform == 'darwin', 'This probe requires macOS')
    repo = Path(__file__).resolve().parents[2]
    binary, chrome_path = args.dtw.resolve(strict=True), args.chrome.resolve(strict=True)
    output = args.output.resolve(); output.mkdir(parents=True, exist_ok=False)
    source = repo/'tests/fixtures/native-input/main.swift'
    web_source = repo/'tests/fixtures/browser-workbench'
    # Chrome may abbreviate long titles in WindowServer; retain a short exact unique title.
    case = 'bg-' + uuid.uuid4().hex[:12]
    profile = output/'chrome-profile'
    evidence = {'kind':'owned-Chrome-background-input-standard-MCP','live_llm_host':False,
                'release_artifact':False,'input_policy':args.input_policy.replace('-','_'),'app':APP,'case':case,'binary_path':str(binary),
                'passed_scope':'functional actions and owned cleanup; inspect background/occlusion coverage separately',
                'binary_sha256':file_sha256(binary),'chrome_path':str(chrome_path),
                'profile':str(profile),'started_unix_ms':int(time.time()*1000),
                'fixture_sources':{str(p.relative_to(repo)):file_sha256(p)
                    for p in (source,web_source/'server.py',web_source/'index.html')},
                'limitations':['Owned local business page, not arbitrary-web compatibility or external-Agent acceptance',
                               'Read-only page geometry supplies coordinates; this does not assess model vision',
                               'Foreground/pointer sampling is 20 ms; shorter excursions are not excluded',
                               'Pointer/foreground changes alone cannot establish DTW causality',
                               'Functional success does not imply background or occlusion coverage']}
    observer = client = chrome = server = None
    logs = []
    failure = None
    try:
        observer = Fixture.prepare(output/'foreground', 'dev.dtw.backgroundbrowser.'+uuid.uuid4().hex, source)
        write_json(observer.root/'fixture.json', {'background_probe':True,'x':380,'title':'DTW Browser Background Observer'})
        evidence['observer_initial'] = observer.start()
        evidence['global_input_released_before_setup'] = input_is_released(evidence['observer_initial'])
        page_dir = output/'web-fixture'; page_dir.mkdir()
        shutil.copy2(web_source/'server.py', page_dir/'server.py')
        html = (web_source/'index.html').read_text()
        require(html.count('<textarea id="note">') == 1 and html.count('</html>') == 1, 'Workbench markup changed')
        html = html.replace('<textarea id="note">', '<textarea id="note" autofocus>')
        (page_dir/'index.html').write_text(html.replace('</html>', INSTRUMENTATION+'</html>'))
        evidence['instrumented_html_sha256'] = file_sha256(page_dir/'index.html')
        (output/'web').mkdir()
        server_log = (output/'server.log').open('w'); logs.append(server_log)
        server = subprocess.Popen([sys.executable,str(page_dir/'server.py'),'--output',str(output/'web'),'--case',case],
                                  stdout=subprocess.PIPE,stderr=server_log,text=True)
        address = json.loads(server.stdout.readline())
        evidence['server'] = {'pid':server.pid,**address}
        profile.mkdir()
        x, y, _, _ = evidence['observer_initial']['window_bounds']
        chrome_args = [str(chrome_path),'--user-data-dir='+str(profile),'--no-first-run','--no-default-browser-check',
                       '--disable-background-mode','--window-size=1000,750',
                       f'--window-position={max(30,int(x-150))},{max(30,int(y-200))}','--new-window',address['url']]
        chrome_log = (output/'chrome.log').open('w'); logs.append(chrome_log)
        chrome = subprocess.Popen(chrome_args,stdout=chrome_log,stderr=subprocess.STDOUT)
        evidence['chrome'] = {'pid':chrome.pid,'argv':chrome_args,'ownership':'exact Popen child with fresh unique user-data-dir'}
        path = output/'web/state.json'
        evidence['page_initial'] = snapshot_when(path, lambda s: s['rendered_rows'] == 100, timeout=20)
        require(chrome.poll() is None, 'Owned Chrome exited or forwarded startup elsewhere')
        require(len(web_state(path)['items']) == 240, 'Unexpected workbench dataset')
        (output/'mcp').mkdir()
        # Retain the owner before initialization so a failed handshake is reaped.
        client = SupervisorMcp(binary, output/'mcp', mcp_args=('--input-policy',args.input_policy))
        client.rpc('initialize', {'protocolVersion':'2025-06-18','capabilities':{},
                                 'clientInfo':{'name':'background-browser-probe','version':'1'}})
        client.notify('notifications/initialized')
        evidence['service_discovery'] = {}
        for tool in ('apps','api'):
            response, _ = client.tool(tool, {})
            evidence['service_discovery'][tool] = response['service']
            require(response['service']['input_policy'] == evidence['input_policy'],
                    'Service discovery did not report the actual launch policy')
        inventory_record, _ = client.execute(APP, 'owned-window-inventory', 'return await app.windows();')
        evidence['window_inventory'] = inventory_record
        windows = checked_windows(inventory_record, APP)
        owned = [w for w in windows if w['title'] == 'DTW Workbench '+case]
        require(len(owned) == 1, 'Owned Chrome window missing or not unique; never select a user window')
        evidence['owned_window'] = owned[0]
        execute(client, 'select-owned-window', 'state.w='+json.dumps(owned[0]['window_ref'])+';return state.w;',
                evidence.setdefault('selected_window', {}))
        run_cases(client, observer, path, output, evidence, chrome.pid)
    except BaseException as error:
        failure = error
        evidence['failure'] = {'type':type(error).__name__,'message':str(error),'traceback':traceback.format_exc()}
    finally:
        # Keep raw independent evidence even when the first input fails. Never
        # retry it, release keys manually, or close any unowned Chrome window.
        path = output/'web/state.json'
        if path.exists():
            evidence['web_state_sha256_before_cleanup'] = file_sha256(path)
        cleanup = evidence.setdefault('cleanup', {})
        def attempt(name, action):
            nonlocal failure
            try: action()
            except BaseException as error:
                cleanup[name] = {'error':str(error),'traceback':traceback.format_exc()}
                if failure is None: failure = error
        if client is not None:
            def close_client():
                result = close_mcp(client); cleanup['mcp'] = result; check_cleanup(client,result)
            attempt('mcp',close_client)
        if chrome is not None:
            def close_chrome():
                result = {'pid':chrome.pid,'signal':'SIGTERM exact owned Popen child only','sent':False}
                cleanup['chrome'] = result
                if chrome.poll() is None: chrome.terminate(); result['sent'] = True
                result.update(stop_owned(chrome))
                require(not result.get('forced') and result.get('returncode') in (0,-15), result)
                marker = '--user-data-dir='+str(profile)
                def gone():
                    result['remaining_profile_processes'] = {pid:command for pid,(_,command) in process_table().items() if marker in command}
                    return not result['remaining_profile_processes']
                wait_until(gone,timeout=5)
            attempt('chrome',close_chrome)
        if server is not None:
            def close_server():
                (output/'web/stop').write_text('stop\n')
                cleanup['server'] = stop_owned(server)
                require(not cleanup['server']['forced'] and cleanup['server']['returncode'] == 0, cleanup['server'])
                server.stdout.close()
            attempt('server',close_server)
        if observer is not None:
            def close_observer():
                try:
                    if observer.child is not None and observer.child.poll() is None:
                        cleanup['observer_final_observation'] = fresh_end(observer)
                finally:
                    cleanup['observer'] = observer.close()
                    check_cleanup(observer,cleanup['observer'])
            attempt('observer',close_observer)
        for log in logs: log.close()
        evidence['passed'] = failure is None and evidence.get('all_cases_passed') is True
        evidence['finished_unix_ms'] = int(time.time()*1000)
        evidence['binary_unchanged'] = file_sha256(binary) == evidence['binary_sha256']
        if not evidence['binary_unchanged']: evidence['passed'] = False
        write_json(output/'evidence.json',evidence)
    print(json.dumps({'passed':evidence['passed'],'passed_scope':evidence['passed_scope'],
                      'background_coverage':evidence.get('background_coverage'),
                      'occlusion_coverage':evidence.get('occlusion_coverage'),
                      'evidence':str(output/'evidence.json'),'failure':str(failure) if failure else None}))
    return 0 if evidence['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
