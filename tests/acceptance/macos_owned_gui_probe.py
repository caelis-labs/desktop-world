#!/usr/bin/env python3
"""Run candidate DTW against a fresh owned AppKit and WKWebView fixture on macOS.

Build DTW with `cargo build --manifest-path cmd/dtw/Cargo.toml`, then pass the
resulting executable with --dtw. Use a fresh --output directory for each run.
Only the disposable fixture bundle is targeted. The report preserves all
responses, fixture event deltas and cleanup outcomes, including failed cases.
"""
import argparse
import json
from pathlib import Path
import plistlib
import subprocess
import sys
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

    def build(self):
        self.executable.parent.mkdir(parents=True)
        metadata = {
            'CFBundleIdentifier': 'dev.caelis.dtw.owned-gui.' + uuid.uuid4().hex,
            'CFBundleExecutable': self.executable.name, 'CFBundleName': 'DTW Owned GUI',
            'CFBundlePackageType': 'APPL', 'CFBundleVersion': '1', 'NSPrincipalClass': 'NSApplication',
        }
        with (self.bundle / 'Contents/Info.plist').open('wb') as stream:
            plistlib.dump(metadata, stream)
        with (self.root / 'build.log').open('wb') as log:
            subprocess.run(['xcrun', '--sdk', 'macosx', 'swiftc', '-module-cache-path',
                            str(self.root / 'swift-cache'), str(SOURCE), '-o', str(self.executable)],
                           stdout=log, stderr=subprocess.STDOUT, timeout=120, check=True)

    def start(self):
        with (self.root / 'native.log').open('wb') as log:
            self.child = subprocess.Popen([str(self.executable), str(self.root)], stdout=log, stderr=subprocess.STDOUT)
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

    def execute(self, source, *, dispatched=0, error=None):
        step = {'source': source, 'expected_dispatches': dispatched, 'expected_error': error,
                'before': self.fixture.state()}
        self.entry['steps'].append(step)
        data, response = self.client.execute(self.fixture.bundle, source, allow_error=True)
        step['response'] = data
        step['is_error'] = response.get('isError') is True
        # Leave a window for callbacks delivered after transport settlement.
        # Popover/menu tracking in the context fixture ends after 450ms.
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


def pointer_buttons(probe):
    focus = probe.execute('await app.focus({});', dispatched=1)
    require(focus['after']['active'], focus)
    capture = probe.client.capture(probe.fixture.bundle, 'pointer-pad')
    probe.entry['capture'] = capture
    x, y, width, height = capture['view']['image']['bounds']
    px, py, pw, ph = probe.fixture.state()['pad_bounds']
    point = {'x': (px + pw / 2 - x) / width, 'y': (py + ph / 2 - y) / height, 'unit': 'fraction'}
    require(0 < point['x'] < 1 and 0 < point['y'] < 1, {'point': point, 'image': capture['view']['image']})
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


CASES = (locator_clicks, native_setter_and_wait, web_setter, target_scroll, target_scroll_refusals,
         invoke_menu, select_option, select_option_refusals, select_web_option_refusal, show_menu,
         pointer_buttons, wait_for_new_window)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True, help='Candidate DTW executable')
    parser.add_argument('--output', type=Path, required=True, help='Fresh evidence directory')
    parser.add_argument('--case', action='append', choices=[case.__name__ for case in CASES],
                        help='Run only the named case(s) in a fresh fixture')
    args = parser.parse_args()
    require(sys.platform == 'darwin', 'This fixture requires macOS')
    binary = args.dtw.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    fixture = Fixture(output / 'fixture')
    report = {'artifact': artifact_identity(binary), 'binary': str(binary), 'fixture': str(fixture.bundle),
              'cases': [], 'passed': False}
    client = None
    try:
        fixture.build()
        report['ready'] = fixture.start()
        client = AppClient(binary, output / 'mcp')
        probe = Probe(fixture, client, report)

        def focus_owned_fixture():
            step = probe.execute('await app.focus({});', dispatched=1)
            require(step['after']['active'], step)
            step = probe.execute('await app.look({full:true});')
            require([window['title'] for window in step['response']['view']['windows']]
                    == ['DTW Owned GUI Acceptance'], step)

        if probe.case('owned_fixture_ready', focus_owned_fixture):
            for case in CASES:
                if args.case is None or case.__name__ in args.case:
                    probe.case(case.__name__, lambda: case(probe))
    except Exception as error:
        report['error'] = str(error)
    finally:
        if client:
            try:
                result = client.close()
                report['transport_cleanup'] = result
                require(result.get('returncode') == 0 and not result.get('forced')
                        and result.get('native_workers_reaped') is True, result)
            except Exception as error:
                report['transport_cleanup_error'] = str(error)
        try:
            result = fixture.close()
            report['fixture_cleanup'] = result
            require(not result['started'] or (result['returncode'] == 0 and not result['forced']), result)
        except Exception as error:
            report['fixture_cleanup_error'] = str(error)
        report['passed'] = bool(report['cases']) and all(case['passed'] for case in report['cases']) and not any(
            key in report for key in ('error', 'transport_cleanup_error', 'fixture_cleanup_error'))
        (output / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(encode({'passed': report['passed'], 'report': str(output / 'report.json'), 'error': report.get('error')}))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
