#!/usr/bin/env python3
"""Run candidate DTW against a fresh owned AppKit and WKWebView fixture on macOS.

Build DTW with `cargo build --manifest-path cmd/dtw/Cargo.toml`, then pass the
resulting executable with --dtw. Use a fresh --output directory for each run.
Only the disposable fixture bundle is targeted. The report preserves all
responses, fixture event deltas and cleanup outcomes, including failed cases.

Focused cue proof (quiet unlocked desktop, Accessibility and Screen Recording):
  cargo build --release --manifest-path cmd/dtw/Cargo.toml
  python3 tests/acceptance/macos_owned_gui_probe.py --case semantic_target_cues \
      --dtw cmd/dtw/target/release/dtw --output /tmp/dtw-semantic-cue-UNIQUE
Use --case addressed_middle with a different fresh --output for the background
middle-click proof. It launches a second owned foreground bundle at OUTPUT/decoy/Owned GUI.app.
Both cases build OUTPUT/fixture/Owned GUI.app/Contents/MacOS/OwnedGUI and
OUTPUT/cursor-overlay-observer. Park the physical cursor away from the centered
fixture before starting and do not move it during the run.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from functools import partial
import json
import math
import os
from pathlib import Path
import shutil
import plistlib
import subprocess
import sys
from threading import Event
import time
import uuid

from app_client import AppClient, artifact_identity, require


SOURCE = Path(__file__).resolve().parents[1] / 'fixtures/owned-gui/main.swift'
NOTE = 'Updated note 中文 😀'
WEB_MEMO = 'Web receipt café 中文 😀'
MENUS = {'menu_open', 'menu_closed'}


def encode(value):
    return json.dumps(value, ensure_ascii=False)


def locator(role, name):
    return f'app.getByRole({encode(role)},{{name:{encode(name)},exact:true}})'


def observed(text, action, role=None):
    query = {'text': text}
    if role is not None:
        query['role'] = role
    return f'''const view = await app.look({encode(query)});
const candidates = view.elements.filter(e => [e.name,e.label,e.value].includes({encode(text)}));
if (candidates.length !== 1) throw Error('Expected unique observed ID: ' + JSON.stringify(view.elements));
const target = candidates[0].id;
print(JSON.stringify(candidates[0]));
{action}'''


class Fixture:
    def __init__(self, root):
        self.root = root
        self.bundle = root / 'Owned GUI.app'
        self.executable = self.bundle / 'Contents/MacOS/OwnedGUI'
        self.state_file = root / 'state.json'
        self.journal_file = root / 'events.jsonl'
        self.child = None

    def build(self, *, compiled=None):
        self.executable.parent.mkdir(parents=True)
        metadata = {
            'CFBundleIdentifier': 'dev.caelis.dtw.owned-gui.' + uuid.uuid4().hex,
            'CFBundleExecutable': self.executable.name, 'CFBundleName': 'DTW Owned GUI',
            'CFBundlePackageType': 'APPL', 'CFBundleVersion': '1', 'NSPrincipalClass': 'NSApplication',
        }
        with (self.bundle / 'Contents/Info.plist').open('wb') as stream:
            plistlib.dump(metadata, stream)
        if compiled:
            shutil.copy2(compiled, self.executable)
            return
        with (self.root / 'build.log').open('wb') as log:
            subprocess.run(['xcrun', '--sdk', 'macosx', 'swiftc', '-module-cache-path',
                            str(self.root / 'swift-cache'), str(SOURCE), '-o', str(self.executable)],
                           stdout=log, stderr=subprocess.STDOUT, timeout=120, check=True)

    def start(self, *, oracle=None):
        args = [str(self.executable), str(self.root)]
        if oracle:
            args.append(oracle)
        with (self.root / 'native.log').open('wb') as log:
            self.child = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            require(self.child.poll() is None, f'Fixture exited during startup: {self.child.returncode}')
            if self.state_file.is_file():
                state = self.state()
                if state['ready']:
                    return state
            time.sleep(.05)
        raise TimeoutError('Owned AppKit window or WebKit receipt form did not report ready')

    def state(self):
        state = json.loads(self.state_file.read_text(encoding='utf-8'))
        require(state['pid'] == self.child.pid, state)
        return state

    def events(self, before, after):
        rows = self.journal_file.read_text(encoding='utf-8').splitlines()
        require(len(rows) >= after['sequence'], 'Fixture journal is missing a published event')
        return [json.loads(row) for row in rows[before['sequence']:after['sequence']]]

    def close(self):
        if self.child is None:
            return {'started': False}
        forced = []
        if self.child.poll() is None:
            (self.root / 'quit').touch()
        try:
            self.child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            forced.append('terminate')
            self.child.terminate()
            try:
                self.child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                forced.append('kill')
                self.child.kill()
                self.child.wait(timeout=2)
        return {'started': True, 'forced': forced, 'returncode': self.child.returncode}


class Probe:
    def __init__(self, fixture, client, report):
        self.fixture = fixture
        self.client = client
        self.report = report
        self.entry = None
        self.execution_ids = set()

    def execute(self, source, *, dispatched=0, error=None, observe=None):
        step = {'source': source, 'expected_dispatches': dispatched, 'expected_error': error,
                'before': self.fixture.state()}
        self.entry['steps'].append(step)
        step['rpc_started_uptime'] = time.monotonic()
        data, response = self.client.execute(self.fixture.bundle, source, allow_error=True)
        step['rpc_finished_uptime'] = time.monotonic()
        step['response'] = data
        step['is_error'] = response.get('isError') is True
        try:
            if observe:
                observe(step)
        finally:
            # Allow callbacks after settlement; context-menu tracking ends after 450ms.
            time.sleep(.65 if 'app.showMenu(' in source else .25)
            step['after'] = self.fixture.state()
            step['events'] = self.fixture.events(step['before'], step['after'])
        identity = data.get('execution_id')
        require(isinstance(identity, str) and identity not in self.execution_ids, data)
        self.execution_ids.add(identity)
        require(not (data.get('shutdown_error') or data.get('shutdown_errors')), data)
        effects = data.get('effects', {}).get('dispatch', {})
        require(effects.get('dispatched', 0) == dispatched and effects.get('unknown', 0) == 0, data)
        require(step['is_error'] == (error is not None), step)
        if error is None:
            require(data.get('status') == 'completed', data)
        else:
            require(data.get('error', {}).get('code') == error, data)
        return step

    def case(self, name, run):
        self.entry = {'name': name, 'steps': [], 'passed': False}
        self.report['cases'].append(self.entry)
        try:
            run()
            self.entry['passed'] = True
        except Exception as error:
            self.entry['error'] = str(error)
        print(encode({'case': name, 'passed': self.entry['passed']}), flush=True)
        return self.entry['passed']


def event_rows(step, *, ignoring=()):
    return [{k: v for k, v in row.items() if k not in ('uptime', 'sequence')}
            for row in step['events'] if row['kind'] not in ignoring]


def expect_events(step, *expected, ignoring=()):
    rows = event_rows(step, ignoring=ignoring)
    require(rows == list(expected), {'expected': list(expected), 'observed': rows})


def expect_wait(step, state, *, name=None, value=None, enabled=None):
    result = step['response']['result']
    require(result['state'] == state, result)
    if state == 'detached':
        require('element' not in result, result)
        return
    element = result['element']
    require(element['name'] == name, element)
    if value is not None:
        require(element['value'] == value, element)
    if enabled is not None:
        require(element['enabled'] is enabled, element)


def locator_clicks(probe):
    step = probe.execute(f'await {locator("button", "Save")}.click();', dispatched=1)
    expect_events(step, {'kind': 'button', 'title': 'Save', 'tag': 1})
    step = probe.execute(f'await {locator("button", "Save As")}.click();', dispatched=1)
    expect_events(step, {'kind': 'button', 'title': 'Save As', 'tag': 2})
    for name, code in [('Same caption', 'selector_ambiguous'), ('savE', 'selector_not_found'),
                       ('Sav', 'selector_not_found')]:
        expect_events(probe.execute(f'await {locator("button", name)}.click();', error=code))
    expect_events(probe.execute("await app.getByRole('button',{name:'Save'}).click();", error='selector_ambiguous'))


def native_setter_and_wait(probe):
    field = locator('text_field', 'Native note')
    gate = locator('button', 'Release gate')
    step = probe.execute(f"return await {field}.waitFor({{state:'disabled',timeout:1000}});")
    expect_events(step)
    expect_wait(step, 'disabled', name='Native note', enabled=False)
    expect_events(probe.execute(f"await {field}.waitFor({{state:'enabled',timeout:100}});", error='wait_timeout'))
    expect_events(probe.execute("await " + locator('button', 'Same caption') +
                                ".waitFor({state:'attached',timeout:100});", error='selector_ambiguous'))
    step = probe.execute(f"await {locator('button', 'Schedule enable')}.click();"
                         f"return await {field}.waitFor({{state:'enabled',timeout:4000}});", dispatched=1)
    expect_events(step, {'kind': 'gate_scheduled'}, {'kind': 'gate_enabled'})
    expect_wait(step, 'enabled', name='Native note', enabled=True)
    require(step['before']['note_enabled'] is False and step['after']['note_enabled'] is True, step)
    step = probe.execute(f'await {field}.setValue({encode(NOTE)});', dispatched=1)
    expect_events(step)
    require(step['after']['note'] == NOTE and step['after']['note_archive'] == 'neighbor', step)
    step = probe.execute(f'return await {field}.waitFor({{value:{encode(NOTE)},timeout:1000}});')
    expect_events(step)
    expect_wait(step, 'attached', name='Native note', value=NOTE)
    expect_events(probe.execute(f"await {field}.waitFor({{value:'Updated',timeout:100}});", error='wait_timeout'))
    step = probe.execute(f"await {locator('button', 'Commit native note')}.click();", dispatched=1)
    expect_events(step, {'kind': 'native_commit', 'value': NOTE, 'neighbor': 'neighbor'})
    step = probe.execute(f"return await {gate}.waitFor({{state:'attached',timeout:1000}});")
    expect_events(step)
    expect_wait(step, 'attached', name='Release gate')
    expect_events(probe.execute(f'await {gate}.click();', dispatched=1), {'kind': 'gate_removed'})
    step = probe.execute(f"return await {gate}.waitFor({{state:'detached',timeout:1000}});")
    expect_events(step)
    expect_wait(step, 'detached')
    require(step['after']['gate_attached'] is False, step)


def web_setter(probe):
    memo = locator('text_field', 'Web memo')
    expect_events(probe.execute(f'await {memo}.setValue({encode(WEB_MEMO)});', dispatched=1))
    step = probe.execute(f'return await {memo}.waitFor({{value:{encode(WEB_MEMO)},timeout:1000}});')
    expect_events(step)
    expect_wait(step, 'attached', name='Web memo', value=WEB_MEMO)
    step = probe.execute(f"await {locator('button', 'Commit web memo')}.click();"
                         "await app.wait({text:'Saved web memo',timeout_ms:4000});", dispatched=1)
    expect_events(step, {'kind': 'web_commit', 'value': WEB_MEMO, 'neighbor': 'web neighbor'})


def target_scroll(probe):
    source = observed('Left anchor', 'return await app.scroll({target,delta_y:3});', 'static_text')
    step = probe.execute(source, dispatched=1)
    before = step['before']['offsets']
    after = step['after']['offsets']
    expect_events(step, {'kind': 'wheel', 'pane': 'Left', 'delta_y': -3,
                         'before': before['Left'], 'after': before['Left'] + 48})
    require(after == {'Left': before['Left'] + 48, 'Right': before['Right']}, step)


def target_scroll_refusals(probe):
    for invalid, code in [
        ("await app.scroll({target:'w999.e999',delta_y:3});", 'target_changed'),
        (observed('Left anchor', 'await app.scroll({target,x:10,y:10,delta_y:3});', 'static_text'), 'invalid_arguments'),
    ]:
        step = probe.execute(invalid, error=code)
        expect_events(step)
        require(step['after']['offsets'] == step['before']['offsets'], step)


def invoke_menu(probe):
    for leaf, code in [('record', 'menu_path_not_found'), ('Twin Record', 'menu_path_ambiguous'),
                       ('Disabled Record', 'menu_item_disabled')]:
        path = ['Probe Actions', 'Primary Branch', leaf]
        step = probe.execute(f'await app.invokeMenu({{path:{encode(path)}}});', error=code)
        expect_events(step, ignoring=MENUS)
        require(step['after']['menus_tracking'] == [], step)
    step = probe.execute("return await app.invokeMenu({path:['Probe Actions','Primary Branch','Record']});",
                         dispatched=1)
    expect_events(step, {'kind': 'menu_action', 'title': 'Record', 'tag': 11}, ignoring=MENUS)
    require(step['after']['menus_tracking'] == [], step)


def popup_option(title):
    return observed('Department choices', f'return await app.selectOption({{target,option:{encode(title)}}});',
                    'pop_up_button')


def select_option(probe):
    step = probe.execute(popup_option('Finance'), dispatched=1)
    expect_events(step, {'kind': 'menu_open', 'title': 'Owned Options'},
                  {'kind': 'menu_closed', 'title': 'Owned Options'},
                  {'kind': 'popup_selection', 'title': 'Finance', 'index': 1})
    require(step['after']['popup'] == 'Finance' and step['after']['menus_tracking'] == [], step)


def select_option_refusals(probe):
    for title, code in [('finance', 'option_not_found'), ('Duplicated', 'option_ambiguous'), ('Locked', 'option_disabled')]:
        step = probe.execute(popup_option(title), error=code)
        expect_events(step, {'kind': 'menu_open', 'title': 'Owned Options'},
                      {'kind': 'menu_closed', 'title': 'Owned Options'})
        require(step['after']['popup'] == step['before']['popup'] and step['after']['menus_tracking'] == [], step)


def select_web_option_refusal(probe):
    source = observed('Web department', "await app.selectOption({target,option:'Finance'});")
    expect_events(probe.execute(source, error='unsupported_capability'))


def show_menu(probe):
    source = observed('Save As', 'await app.showMenu({target});', 'button')
    expect_events(probe.execute(source, error='unsupported_capability'))
    source = observed('Context owner', 'return await app.showMenu({target});', 'button')
    step = probe.execute(source, dispatched=1)
    expect_events(step, {'kind': 'context_request'}, {'kind': 'menu_open', 'title': 'Owned Context'},
                  {'kind': 'menu_closed', 'title': 'Owned Context'})
    require(step['after']['menus_tracking'] == [], step)


def capture_pad_point(probe):
    capture = probe.client.capture(probe.fixture.bundle, 'pointer-pad')
    probe.entry['capture'] = capture
    x, y, width, height = capture['view']['image']['bounds']
    px, py, pw, ph = probe.fixture.state()['pad_bounds']
    center = (px + pw / 2, py + ph / 2)
    point = {'x': (center[0] - x) / width, 'y': (center[1] - y) / height, 'unit': 'fraction'}
    require(0 < point['x'] < 1 and 0 < point['y'] < 1, {'point': point, 'image': capture['view']['image']})
    return point, center


def pointer_buttons(probe):
    focus = probe.execute('await app.focus({});', dispatched=1)
    require(focus['after']['active'], focus)
    point, _ = capture_pad_point(probe)
    clicks = [
        (dict(point, button='middle'), 2, None),
        (dict(point, button='middle', modifiers=['cmd', 'shift', 'option', 'ctrl']), None, 'unsupported_capability'),
        (point, 0, None),
        (dict(point, button='middle', modifiers=['cmd', 'cmd']), None, 'invalid_arguments'),
    ]
    observed_clicks = [(probe.execute(f'return await app.click({encode(options)});',
                                     dispatched=int(error is None), error=error), button)
                       for options, button, error in clicks]
    for step, button in observed_clicks:
        if button is None:
            expect_events(step)
        else:
            expect_events(step, *({'kind': 'pointer', 'phase': phase, 'button': button, 'flags': 0, 'count': 1}
                                  for phase in ('down', 'up')))
        for state in (step['before'], step['after']):
            require(all(state[key] == 0 for key in ('mouse_buttons', 'keyboard_modifiers',
                                                    'hid_modifiers', 'session_modifiers')), state)


def wait_for_new_window(probe):
    click = probe.execute(f'await {locator("button", "Schedule document")}.click();', dispatched=1)
    expect_events(click, {'kind': 'document_scheduled'})
    timeout = probe.execute('await app.waitForNewWindow({timeout_ms:350});', error='wait_timeout')
    expect_events(timeout)
    require(timeout['after']['second_document_id'] == 0, timeout)
    (probe.fixture.root / 'release-document').touch()
    title = 'DTW Owned Second Document'
    continued = probe.execute('return (await app.waitForNewWindow({timeout_ms:5000})).windows[0].title;')
    require(continued['response']['result'] == title, continued)
    exhausted = probe.execute('await app.waitForNewWindow({timeout_ms:350});', error='input_required')
    expect_events(exhausted)
    document = exhausted['after']
    require(document['second_document_id'] > 0 and document['pid'] == click['before']['pid'], document)
    events = probe.fixture.events(click['before'], document)
    probe.entry['fixture_events'] = events
    expect_events({'events': events}, {'kind': 'document_scheduled'},
                  {'kind': 'document_opened', 'title': title, 'window_id': document['second_document_id'],
                   'pid': document['pid']})


@contextmanager
def desktop_watch(observer, label, identity_path=None):
    from standard_agent_probe import wait_until

    stop = observer.root / (label + '.stop')
    samples = observer.root / (label + '-samples.jsonl')
    args = [str(observer.binary), '--watch', str(stop)]
    if identity_path:
        args.append(str(identity_path))
    with samples.open('wb') as log:
        child = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT)
    try:
        wait_until(lambda: samples.stat().st_size or child.poll() is not None, timeout=5)
        require(child.poll() is None, f'Observer failed: {samples}')
        yield samples
    finally:
        stop.touch()
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=3)
        require(child.returncode == 0, f'Observer failed: {samples}')


def read_samples(path):
    # The watcher may be writing its last line while the action is in flight.
    return [json.loads(line) for line in path.read_text().split('\n')[:-1]]


@contextmanager
def arm_reticle_capture(observer, label, identity_path, *, wait_seconds):
    from standard_agent_probe import wait_until

    png = observer.root / (label + '-reticle.png')
    ready = observer.root / (label + '-capture.ready')
    log_path = observer.root / (label + '-capture.jsonl')
    args = [str(observer.binary), '--capture-when-visible', str(identity_path), str(ready), str(png),
            str(wait_seconds)]
    with log_path.open('wb') as log:
        child = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT)
    try:
        wait_until(lambda: ready.exists() or child.poll() is not None, timeout=5)
        require(ready.exists(), log_path.read_text())
        yield child, log_path
    finally:
        if child.poll() is None:
            child.kill()
        child.wait(timeout=3)


def witnessed_action(probe, observer, label, source, *, identity_path=None, front_pid=None, observe=None, **options):
    with desktop_watch(observer, label, identity_path) as samples:
        with samples.open() as stream:
            ready = json.loads(stream.readline())
        # Semantic actions preserve whichever app is frontmost; do not take
        # focus back if setup moved the fixture to a different display. The
        # addressed-middle control specifically requires its owned decoy.
        expected_front = ready['front_pid'] if front_pid is None else front_pid
        require(expected_front > 0 and ready['front_pid'] == expected_front and not ready['middle_down'], ready)
        started = time.monotonic()
        step = probe.execute(source, observe=partial(observe, samples) if observe else None, **options)
        finished = time.monotonic()
        # The last sample must postdate the fixture state read.
        time.sleep(.06)
    rows = read_samples(samples)
    gaps = [b['uptime'] - a['uptime'] for a, b in zip(rows, rows[1:])]
    step['watch'] = {'path': str(samples), 'maximum_sample_gap': max(gaps, default=None)}
    require(gaps and max(gaps) <= .2 and rows[0]['uptime'] <= started <= finished <= rows[-1]['uptime'],
            step['watch'])
    require(all(row['cursor'] == rows[0]['cursor'] for row in rows), {'cursor_moved': rows})
    require(all(row['front_pid'] == expected_front and not row['middle_down'] for row in rows),
            {'foreground_or_middle_button_changed': rows})
    return step, rows


def addressed_middle(probe, observer, trace, decoy):
    from standard_agent_probe import wait_until

    point, center = capture_pad_point(probe)
    target = probe.fixture.state()
    decoy.build(compiled=probe.fixture.executable)
    decoy.start(oracle='--addressed-middle-decoy')
    wait_until(lambda: decoy.state()['active'] and not probe.fixture.state()['active'])
    decoy_before = decoy.state()
    proof = probe.entry['decoy'] = {'before': decoy_before, 'bundle': str(decoy.bundle)}
    require(decoy_before['pid'] != target['pid'] and decoy_before['window_id'] != target['window_id'], proof)
    x, y, w, h = target['pad_bounds']
    dx, dy, dw, dh = decoy_before['window_bounds']
    require(x + w <= dx or dx + dw <= x or y + h <= dy or dy + dh <= y,
            {'decoy_covers_pad': {'pad': target['pad_bounds'], 'decoy': decoy_before['window_bounds']}})
    start = trace.stat().st_size if trace.exists() else 0
    try:
        step, rows = witnessed_action(probe, observer, 'addressed-middle',
                                       f'await app.click({encode(dict(point, button="middle"))});',
                                       dispatched=1, front_pid=decoy_before['pid'])
    finally:
        proof['after'] = decoy.state()
        proof['events'] = decoy.events(decoy_before, proof['after'])
    require(proof['after']['active'] and proof['after']['window_id'] == decoy_before['window_id']
            and proof['after']['window_bounds'] == decoy_before['window_bounds'] and not proof['events'], proof)
    require(all(not state['active'] and state['pad_bounds'] == target['pad_bounds']
                for state in (step['before'], step['after'])), step)
    cx, cy = rows[0]['cursor']
    require(not (x - 10 <= cx <= x + w + 10 and y - 10 <= cy <= y + h + 10),
            'Park the physical cursor away from the owned pad')
    window = step['before']['window_id']
    require(window > 0 and step['after']['window_id'] == window, step)
    events = event_rows(step)
    require(len(events) == 2, events)
    for event, phase, event_type in zip(events, ('down', 'up'), (25, 26)):  # AppKit otherMouseDown/Up
        actual = event.pop('point')
        require(math.dist(center, actual) <= 1, {'expected': center, 'event_point': actual})
        require(event == {'kind': 'pointer', 'phase': phase, 'event_type': event_type, 'button': 2,
                          'flags': 0, 'count': 1, 'window_id': window}, event)
    require(all(state[key] == 0 for state in (step['before'], step['after'])
                for key in ('mouse_buttons', 'keyboard_modifiers', 'hid_modifiers', 'session_modifiers')), step)
    with trace.open() as stream:
        stream.seek(start)
        lines = stream.read().splitlines()
    routes = [json.loads(line.removeprefix('dtw_input ')) for line in lines if line.startswith('dtw_input ')]
    step['input_route_trace'] = routes
    clicks = [row for row in routes if row['operation'] == 'click']
    require(len(clicks) == 1 and (clicks[0]['window'], clicks[0]['route'], clicks[0]['reason'])
            == (window, 'addressed', 'pid_window_event'), routes)


def marker_point(snapshot):
    bounds = snapshot['windows'][0]['bounds']
    dx, dy = snapshot['capture']['reticle']['ink_center']
    return (bounds['x'] + dx, bounds['y'] + dy)


def check_cue_capture(probe, observer, identity, baseline, step):
    from cursor_overlay_probe import require_equal_roi, visible

    before = observer.snapshot(identity)
    proof = step['app_capture_isolation'] = {'cue_before': before}
    if not visible(before):
        proof['gap'] = 'reticle_expired_before_app_capture'
        return
    capture = probe.client.capture(probe.fixture.bundle, 'app-with-reticle')
    after = observer.snapshot(identity)
    proof.update(app_capture=capture, cue_after=after)
    if not visible(after) or before['windows'] != after['windows']:
        proof['gap'] = 'reticle_not_stably_visible_across_app_capture'
        return
    image = baseline['view']['image']
    actual = capture['view']['image']
    for key in ('width', 'height', 'bounds'):
        require(image[key] == actual[key], {'baseline': image, 'live': actual})
    x, y, w, h = image['bounds']
    sx, sy = image['width'] / w, image['height'] / h
    cx, cy = step['drawn_point']
    # Compare only the reticle footprint over Save, not the entire helper frame.
    left, right = math.floor((cx - 12 - x) * sx), math.ceil((cx + 12 - x) * sx)
    top, bottom = math.floor((cy - 12 - y) * sy), math.ceil((cy + 12 - y) * sy)
    output = probe.client.output
    result = subprocess.run([str(observer.binary), '--compare-roi', str(output / 'app-before-reticle.png'),
                             str(output / 'app-with-reticle.png'), *map(str, (left, top, right - left, bottom - top))],
                            text=True, capture_output=True, timeout=5, check=True)
    proof['comparison'] = json.loads(result.stdout)
    require_equal_roi(proof['comparison'])


def semantic_target_cues(probe, observer, binary):
    from cursor_overlay_probe import ChildWatch, helper, owned_helpers, visible
    from standard_agent_probe import process_table, wait_until

    transport = probe.client.transport
    identity = None
    (probe.fixture.root / 'prepare-cue-layout').touch()
    wait_until(lambda: probe.fixture.state()['cue_layout'])
    layout = probe.entry['cue_layout'] = probe.fixture.state()['cue_layout']
    require(layout['clear'], f'No unobstructed owned fixture location for both positive cue targets: {layout}')
    baseline_capture = probe.client.capture(probe.fixture.bundle, 'app-before-reticle')
    probe.entry['app_capture_baseline'] = baseline_capture

    def action(label, source, *, target=None, error=None):
        nonlocal identity
        from cursor_overlay_probe import rendering_proof, sha

        identity_path = observer.root / (label + '-identity.json')

        def publish_identity(owned):
            temporary = identity_path.with_suffix('.tmp')
            temporary.write_text(encode(owned))
            temporary.replace(identity_path)
            return owned

        if identity:
            baseline = observer.hidden(identity)
            require(baseline['identity_matches'], 'Helper exited before the action')
            publish_identity(identity)
        else:
            require(not owned_helpers(process_table(), transport.child.pid, binary), 'Unexpected preexisting helper')

        if target:
            # Keep both observers armed for the full RPC deadline plus startup and evidence collection.
            response_timeout = transport.RESPONSE_TIMEOUT
            stop_discovery = Event()
            with arm_reticle_capture(observer, label, identity_path, wait_seconds=response_timeout + 8) as (
                    capture, log_path), ThreadPoolExecutor(max_workers=1) as workers:
                discovery = None if identity else workers.submit(
                    lambda: publish_identity(helper(transport, binary, observer, timeout=response_timeout + 5,
                                                    stop=stop_discovery)))

                def observe(samples, step):
                    nonlocal identity
                    if discovery:
                        try:
                            identity = discovery.result(timeout=2)
                        except TimeoutError as error:
                            raise AssertionError('No authenticated overlay helper observed after the action receipt') from error
                    step['cue_identity'] = identity
                    capture.wait(timeout=8)
                    cue = json.loads(log_path.read_text())
                    step['cue'] = cue
                    require(capture.returncode == 0 and rendering_proof(cue) and cue['capture']['reticle']['verified'], cue)
                    cue['capture']['sha256'] = sha(cue['capture']['path'])
                    cue['capture']['finished_before_receipt'] = (
                        cue['capture']['finished_uptime'] <= step['rpc_finished_uptime'])
                    require(step['rpc_started_uptime'] <= cue['uptime'] <= cue['capture']['started_uptime']
                            <= cue['capture']['finished_uptime'], cue)
                    x, y, w, h = step['before']['cue_bounds'][target]
                    expected = (x + w / 2, y + h / 2)
                    actual = marker_point(cue)
                    step['expected_point'], step['drawn_point'] = expected, actual
                    require(math.dist(expected, actual) <= 1, {'expected': expected, 'drawn': actual})
                    if target == 'Save':
                        check_cue_capture(probe, observer, identity, baseline_capture, step)
                    wait_until(lambda: any(row.get('identity_matches') and not visible(row)
                                           and row['uptime'] > cue['capture']['finished_uptime']
                                           for row in read_samples(samples)), timeout=2)

                try:
                    step, rows = witnessed_action(probe, observer, label, source, identity_path=identity_path,
                                                  dispatched=1, observe=observe)
                finally:
                    stop_discovery.set()
            require(math.dist(rows[0]['cursor'], step['drawn_point']) > 12,
                    'Park the physical cursor away from the fixture targets')
            known = [row for row in rows if 'identity_matches' in row]
            require(known and all(row['identity_matches'] for row in known), known)
            shown = [row for row in known if visible(row)]
            require(len(shown) >= 2 and all(row['windows'] == step['cue']['windows'] for row in shown), shown)
            require(shown[-1]['uptime'] - shown[0]['uptime'] >= .2, shown)
            expired = next((row for row in known if row['uptime'] > shown[-1]['uptime']), None)
            step['cue_expired'] = expired
            require(expired and expired['uptime'] - shown[0]['uptime'] < 1.6,
                    {'first_visible': shown[0], 'expired': expired})
            require(all(visible(row) == (shown[0]['uptime'] <= row['uptime'] <= shown[-1]['uptime'])
                        for row in known), known)
        else:
            children = ChildWatch(transport, binary)
            try:
                step, rows = witnessed_action(probe, observer, label, source, identity_path=identity_path,
                                              error=error, observe=lambda _samples, _step: time.sleep(.3))
            finally:
                proof = children.finish()
            step['helper_sampling'] = proof
            require(proof['coverage'] == 'established' and all(
                row['helpers'] == [identity['pid']] for row in proof['samples']), proof)
            require(all(row['identity_matches'] and not visible(row) for row in rows), rows)
            require(helper(transport, binary, observer) == identity, 'Rejection changed helper identity')
        return step

    step = action('save', f'await {locator("button", "Save")}.click();', target='Save')
    expect_events(step, {'kind': 'button', 'title': 'Save', 'tag': 1})
    step = action('set-note', f'await {locator("text_field", "Native note")}.setValue({encode(NOTE)});',
                  target='Native note')
    expect_events(step)
    require(step['before']['note'] == 'initial' and step['after']['note'] == NOTE
            and step['before']['note_archive'] == step['after']['note_archive'] == 'neighbor', step)

    step = action('locator-miss', f'await {locator("button", "Missing owned control")}.click();',
                  error='selector_not_found')
    expect_events(step)
    gate = probe.execute(observed('Release gate', 'return target;', 'button'))['response']['result']
    require(isinstance(gate, str) and gate, gate)
    (probe.fixture.root / 'detach-gate').touch()
    wait_until(lambda: not probe.fixture.state()['gate_attached'])
    expect_events(action('stale-target', f'await app.click({{target:{encode(gate)}}});', error='target_changed'))


CASES = (locator_clicks, native_setter_and_wait, web_setter, target_scroll, target_scroll_refusals,
         invoke_menu, select_option, select_option_refusals, select_web_option_refusal, show_menu,
         pointer_buttons, wait_for_new_window)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True, help='Candidate DTW executable')
    parser.add_argument('--output', type=Path, required=True, help='Fresh evidence directory')
    focused_cases = {'semantic_target_cues': '--semantic-cue-oracle', 'addressed_middle': '--addressed-middle-oracle'}
    parser.add_argument('--case', action='append', choices=[case.__name__ for case in CASES] + list(focused_cases),
                        help='Run only the named case(s) in a fresh fixture')
    args = parser.parse_args()
    focused = next((name for name in (args.case or []) if name in focused_cases), None)
    if focused and args.case != [focused]:
        parser.error(f'{focused} needs a fresh fixture; select it alone')
    require(sys.platform == 'darwin', 'This fixture requires macOS')
    binary = args.dtw.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    fixture = Fixture(output / 'fixture')
    decoy = Fixture(output / 'decoy') if focused == 'addressed_middle' else None
    report = {'artifact': artifact_identity(binary), 'binary': str(binary), 'fixture': str(fixture.bundle),
              'cases': [], 'passed': False}
    client = None
    observer = None
    try:
        if focused:
            from cursor_overlay_probe import Observer

            observer_source = SOURCE.parents[1] / 'cursor-overlay-observer.swift'
            observer_binary = output / 'cursor-overlay-observer'
            with (output / 'observer-build.log').open('wb') as log:
                subprocess.run(['xcrun', '--sdk', 'macosx', 'swiftc', '-parse-as-library',
                                '-module-cache-path', str(output / 'swift-cache'), str(observer_source),
                                '-o', str(observer_binary)], stdout=log, stderr=subprocess.STDOUT,
                               timeout=120, check=True)
            observer = Observer(observer_binary, output)
        fixture.build()
        report['ready'] = fixture.start(oracle=focused_cases.get(focused))
        trace = output / 'input-route.log'
        trace_env = {'DTW_INPUT_TRACE': '1', 'DTW_INPUT_TRACE_FILE': str(trace)} if focused == 'addressed_middle' else {}
        previous = {key: os.environ.get(key) for key in trace_env}
        try:
            os.environ.update(trace_env)
            client = AppClient(binary, output / 'mcp')
        finally:
            for key, value in previous.items():
                if value is None:
                    del os.environ[key]
                else:
                    os.environ[key] = value
        probe = Probe(fixture, client, report)

        def focus_owned_fixture():
            step = probe.execute('await app.focus({});', dispatched=1)
            require(step['after']['active'], step)
            step = probe.execute('await app.look({full:true});')
            require([window['title'] for window in step['response']['view']['windows']]
                    == ['DTW Owned GUI Acceptance'], step)

        if probe.case('owned_fixture_ready', focus_owned_fixture):
            if focused == 'semantic_target_cues':
                probe.case(focused, lambda: semantic_target_cues(probe, observer, binary))
            elif focused == 'addressed_middle':
                probe.case(focused, lambda: addressed_middle(probe, observer, trace, decoy))
            else:
                for case in CASES:
                    if args.case is None or case.__name__ in args.case:
                        probe.case(case.__name__, lambda: case(probe))
    except Exception as error:
        report['error'] = str(error)
    finally:
        if client:
            try:
                try:
                    if observer:
                        from cursor_overlay_probe import owned_helpers
                        from standard_agent_probe import process_table, wait_until

                        helpers = owned_helpers(process_table(), client.transport.child.pid, binary)
                        identity = getattr(client.transport, 'probe_overlay_identity', None)
                        if identity:
                            helpers[identity['pid']] = identity['command']
                finally:
                    result = client.close()
                if observer:
                    def helpers_gone():
                        table = process_table()
                        return all(table.get(pid, (None, None))[1] != command for pid, command in helpers.items())

                    wait_until(helpers_gone, timeout=3)
                    if identity:
                        last = observer.snapshot(identity)
                        require(not last['identity_matches'] and not last['windows'], last)
                        result['overlay_final_snapshot'] = last
                    result['overlay_helpers_reaped'] = list(helpers)
                report['transport_cleanup'] = result
                require(result.get('returncode') == 0 and not result.get('forced')
                        and result.get('native_workers_reaped') is True, result)
            except Exception as error:
                report['transport_cleanup_error'] = str(error)
        for name, owned in (('decoy', decoy), ('fixture', fixture)):
            if owned is None:
                continue
            try:
                result = owned.close()
                report[f'{name}_cleanup'] = result
                require(not result['started'] or (result['returncode'] == 0 and not result['forced']), result)
            except Exception as error:
                report[f'{name}_cleanup_error'] = str(error)
        report['passed'] = bool(report['cases']) and all(case['passed'] for case in report['cases']) and not any(
            key == 'error' or key.endswith('_cleanup_error') for key in report)
        (output / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(encode({'passed': report['passed'], 'report': str(output / 'report.json'), 'error': report.get('error')}))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
