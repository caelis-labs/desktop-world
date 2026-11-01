#!/usr/bin/env python3
"""Opt-in modern Notepad shortcut regression using only DTW app input.

Opens one uniquely named owned text file, proves that its active document owns
the keyboard target, then compares bare S, Shift+A, Ctrl+A and Ctrl+S effects.
Each shortcut is issued once per independent case. Disk reads are independent
save oracles; a failed save is never replayed to turn the case into a pass.
Only the owned tab is closed through File > Close tab; existing tabs/processes
are never terminated. Use an exclusive GUI slot and python -X utf8.
"""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import time
import traceback
import uuid

from cursor_probe import Mcp, require, wait, write


def canonical(text):
    return text.replace('\r\n', '\n').replace('\r', '\n')


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def file_state(path):
    raw = path.read_bytes()
    return {'sha256': hashlib.sha256(raw).hexdigest(), 'bytes': len(raw),
            'text': canonical(raw.decode('utf-8-sig')), 'mtime_ns': path.stat().st_mtime_ns}


def disk_matches(path, expected, timeout=3):
    """Wait only for the already-issued save; never dispatch another action."""
    until = time.monotonic() + timeout
    latest = None
    while time.monotonic() < until:
        try:
            latest = file_state(path)
            if latest['text'] == expected:
                return latest
        except (OSError, UnicodeError):
            pass
        time.sleep(.025)
    return latest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--notepad', type=Path, required=True,
                        help='Actual installed notepad.exe, including a WindowsApps path if applicable')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--owned-file', type=Path,
                        help='Reuse an already opened pristine DTW-shortcut-<marker>.txt from a stopped setup attempt; no launcher/focus workaround is hidden')
    parser.add_argument('--protect-file', action='append', type=Path, default=[],
                        help='Existing acceptance document to hash before/after; never opened or modified by this probe')
    parser.add_argument('--file-label', default='File')
    parser.add_argument('--save-label', default='Save')
    parser.add_argument('--close-tab-label', default='Close tab')
    parser.add_argument('--trace-input-queues', action='store_true',
                        help='Independent read-only Win32 focus/thread/global-marker sampling; never attaches or supplies input')
    args = parser.parse_args()
    require(os.name == 'nt' and sys.flags.utf8_mode, 'Use Windows: python -X utf8')
    binary, notepad, out = args.dtw.resolve(strict=True), args.notepad.resolve(strict=True), args.output.resolve()
    require(not out.exists(), 'Use a new evidence directory')
    out.mkdir(parents=True)
    marker = uuid.uuid4().hex[:12]
    if args.owned_file:
        document = args.owned_file.resolve(strict=True)
        match = re.fullmatch(r'DTW-shortcut-([0-9a-f]{12})\.txt', document.name)
        require(match is not None, 'Reuse only a uniquely named owned probe document')
        marker = match.group(1)
    else:
        document = out / ('DTW-shortcut-' + marker + '.txt')
    seed = 'Owned DTW Notepad shortcut probe ' + marker + '\n'
    if args.owned_file:
        require(file_state(document)['text'] == seed, 'Reused setup document is not pristine; use a new owned file')
    else:
        document.write_text(seed, encoding='utf-8', newline='')
    protected = {str(path.resolve(strict=True)): sha(path.resolve(strict=True)) for path in args.protect_file}
    summary = {'passed': False, 'binary_sha256': sha(binary), 'notepad': str(notepad),
               'owned_document': str(document), 'protected_before': protected, 'cases': [], 'cleanup': []}
    client = launcher = observer = None
    app = window_ref = None
    baseline_names = []
    try:
        if args.trace_input_queues:
            from input_queue_observer import QueueObserver
            observer = QueueObserver(document.name, out / 'input-queues.jsonl')
            observer.start()
        client = Mcp(binary, out / 'mcp', env=dict(os.environ, DTW_AX_READINESS_TRACE='1'))
        before = client.tool('apps', {})
        write(out / 'apps-before.json', before)
        # The packaged Notepad launcher may hand off to an existing process.
        # No termination rights or cleanup assumption are derived from this PID.
        if not args.owned_file:
            launcher = subprocess.Popen([str(notepad), str(document)])
            summary['launcher_pid'] = launcher.pid
        else:
            summary['reused_pristine_owned_file'] = True
        def app_ready():
            catalog = client.tool('apps', {})
            found = [row for row in catalog.get('apps', []) if row['app'].casefold() == str(notepad).casefold()]
            require(len(found) <= 1, 'Ambiguous Notepad app identity')
            return found[0]['app'] if found else None
        app = wait(app_ready, 20)
        def owned_window():
            windows, _ = client.execute(app, 'discover-owned-window', 'return (await app.windows()).windows;')
            matches = [row for row in windows if document.name in row['title']]
            require(len(matches) <= 1, 'Owned filename appears in multiple windows')
            return matches[0]['window_ref'] if matches else None
        window_ref = wait(owned_window, 20)
        wr, filename = json.dumps(window_ref), json.dumps(document.name)
        # Every mutating test call rechecks the active window title. The exact
        # marker also proves the active document before its first keyboard input.
        guard = f'''const window_ref={wr};
const windows=(await app.windows()).windows;
if (!windows.some(w=>w.window_ref===window_ref && w.title.includes({filename})))
  throw Error('Owned Notepad tab is not the active target; no mutation issued');'''
        read = '''const snapshotDoc=await app.find({window_ref,selector:{role:'document'},timeout_ms:4000});
const text=await app.readText({window_ref,observation_ref:snapshotDoc.observation_ref,element_ref:snapshotDoc.element_ref,offset:0,limit:4000});
if(text.next_offset!==null) throw Error('Owned document exceeds probe text budget');
const tabs=await app.observe({window_ref,role:'tab_item',limit:100});
return {text:text.text,tabs:tabs.elements.map(e=>e.name)};'''
        def execute(label, body, read_after=True):
            return client.execute(app, label, guard + '\n' + body + ('\n' + read if read_after else ''))[0]
        initial = execute('initial-owned-document', '')
        require(canonical(initial['text']) == seed, 'Launched document text differs from the owned seed')
        baseline_names = [name for name in initial['tabs'] if document.name not in name]
        write(out / 'initial-document.json', initial)
        focus_text = 'FOCUS-' + marker
        focused = execute('prove-owned-editor-focus', f'''await app.focus({{window_ref}});
await app.typeText({{window_ref,text:{json.dumps(focus_text)}}});''')
        require(focus_text in canonical(focused['text']), 'Keyboard focus in the owned editor is unproven; stop before shortcut tests')

        def prepare(label, value):
            result = execute(label + '-prepare', f'''const doc=await app.find({{window_ref,selector:{{role:'document'}},timeout_ms:4000}});
await app.setValue({{window_ref,observation_ref:doc.observation_ref,element_ref:doc.element_ref,value:{json.dumps(value)}}});
await app.keypress({{window_ref,keys:['END']}});
await app.typeText({{window_ref,text:'|'}});''')
            # Prove editor focus and caret placement for every independent case;
            # a setter alone does not promise to focus the editable surface.
            prepared = value + '|'
            require(canonical(result['text']) == prepared, 'Owned editor focus/caret preparation is unproven')
            return prepared

        def menu(label, item):
            source = f'''let page=await app.observe({{window_ref,role:'menu_item',limit:100}});
const items=[...page.elements];
while(page.next) {{ page=await app.observe({{window_ref,cursor:page.next}}); items.push(...page.elements); }}
const targets=items.filter(e=>e.name==={json.dumps(item)} && e.actions.includes('press'));
if(targets.length>1) throw Error('Owned menu target is ambiguous');
if(targets.length===0) return {{pending:true,items:items.map(e=>({{name:e.name,actions:e.actions}}))}};
const receipt=await app.press({{window_ref,observation_ref:page.observation_ref,element_ref:targets[0].element_ref}});
return {{dispatch:receipt.outcome.dispatch}};'''
            # Each distinct observed entry is pressed at most once. Modern
            # Notepad's MSAA File item may prepare the XAML File button without
            # opening its popup. Read-only polling separates these transitions;
            # the selected Save/Close action is never replayed.
            for index, role in enumerate(['menu_item', 'button']):
                execute(label + '-open-' + role,
                        f'await app.press({{window_ref,selector:{{role:{json.dumps(role)},text:{json.dumps(args.file_label)}}},timeout_ms:4000}});return {{opened:true}};',
                        read_after=False)
                until = time.monotonic() + (.25 if index == 0 else 4)
                sample = None
                while time.monotonic() < until:
                    sample = execute(label + '-readiness-' + role, source, read_after=False)
                    if not sample.get('pending'):
                        return sample
                    time.sleep(.025)
            raise AssertionError('Owned menu target did not become ready: ' + json.dumps(sample))

        def set_document(label, value):
            result = execute(label + '-set-document', f'''const doc=await app.find({{window_ref,selector:{{role:'document'}},timeout_ms:4000}});
await app.setValue({{window_ref,observation_ref:doc.observation_ref,element_ref:doc.element_ref,value:{json.dumps(value)}}});''')
            require(canonical(result['text']) == value, 'Pure native setter preparation differs')

        # Reproduce the two real host paths before other modifier controls can
        # affect input state: DSH typed CJK+emoji, read it, then ENTER+CTRL+S;
        # AGY used the native setter and CONTROL+S. Aliases are separate cases.
        for mode, alias in [('typeText', 'CTRL'), ('setValue', 'CONTROL'),
                            ('typeText', 'CONTROL'), ('setValue', 'CTRL')]:
            name = 'cjk-' + mode.lower() + '-' + alias.lower()
            payload = 'DSH 插件已通过：中文🙂'
            prefix = 'Owned DTW Notepad shortcut probe ' + marker + '\n'
            expected = prefix + payload
            set_document(name, prefix if mode == 'typeText' else expected)
            if mode == 'typeText':
                # Match the live DSH call boundary: positioning, typeText,
                # independent native read, then ENTER+save in the next exec.
                execute(name + '-end', "await app.keypress({window_ref,keys:['CTRL','END']});")
                typed = execute(name + '-type', f'await app.typeText({{window_ref,text:{json.dumps(payload)}}});')
                require(canonical(typed['text']) == expected, 'CJK+emoji typeText preparation differs')
                execute(name + '-native-read', '')
                expected += '\n'
            before_disk = file_state(document)
            command = ("const enter=await app.keypress({window_ref,keys:['ENTER']});\n" if mode == 'typeText' else '')
            command += f'''const save=await app.keypress({{window_ref,keys:[{json.dumps(alias)},'S']}});
const tabs=await app.observe({{window_ref,role:'tab_item',limit:100}});
return {{save:save.outcome.dispatch,tabs:tabs.elements.map(e=>e.name)}};'''
            receipt = execute(name + '-save', command, read_after=False)
            actual = execute(name + '-after-read', '')
            after_disk = disk_matches(document, expected)
            case = {'case': name, 'mode': mode, 'keys': [alias, 'S'], 'save_receipt': receipt,
                    'preceding_enter_same_exec': mode == 'typeText', 'expected_text': expected,
                    'observed': actual, 'disk_before': before_disk, 'disk_after': after_disk,
                    'passed': canonical(actual['text']) == expected and after_disk is not None and after_disk['text'] == expected}
            summary['cases'].append(case)
            write(out / (name + '-case.json'), case)

        # Bare letter and Shift controls distinguish virtual-key delivery from
        # missing modifier recognition. Each case resets its owned document.
        for name, keys, suffix in [('bare-s', ['S'], 's'), ('shift-a', ['SHIFT', 'A'], 'A')]:
            prepared = 'CASE-' + name + '-' + marker
            prepared = prepare(name, prepared)
            actual = execute(name, f'''await app.keypress({{window_ref,keys:['END']}});
await app.keypress({{window_ref,keys:{json.dumps(keys)}}});''')
            expected = prepared + suffix
            case = {'case': name, 'keys': keys, 'expected_text': expected,
                    'observed': actual, 'passed': canonical(actual['text']) == expected}
            summary['cases'].append(case)
            write(out / (name + '-case.json'), case)

        prepared = 'SELECT-ALL-' + marker
        replacement = 'REPLACED-' + marker
        prepared = prepare('ctrl-a', prepared)
        selected = execute('ctrl-a', f'''await app.keypress({{window_ref,keys:['CTRL','A']}});
await app.typeText({{window_ref,text:{json.dumps(replacement)}}});''')
        case = {'case': 'ctrl-a', 'keys': ['CTRL', 'A'], 'expected_text': replacement,
                'observed': selected, 'passed': canonical(selected['text']) == replacement}
        summary['cases'].append(case)
        write(out / 'ctrl-a-case.json', case)

        for name, preceding_enter in [('ctrl-s-alone', False), ('enter-then-ctrl-s', True)]:
            prepared = 'SAVE-' + name + '-' + marker + ' 中文🙂'
            prepared = prepare(name, prepared)
            # End places the caret without depending on Ctrl+End. This line has
            # no indentation or markup whose Enter handling changes the oracle.
            execute(name + '-caret', "await app.keypress({window_ref,keys:['END']});")
            before_disk = file_state(document)
            source = ("const enter=await app.keypress({window_ref,keys:['ENTER']});\n" if preceding_enter else '')
            source += "const save=await app.keypress({window_ref,keys:['CTRL','S']});"
            actual = execute(name, source)
            expected = prepared + ('\n' if preceding_enter else '')
            after_disk = disk_matches(document, expected)
            case = {'case': name, 'keys': ['CTRL', 'S'], 'preceding_enter_same_exec': preceding_enter,
                    'expected_text': expected, 'observed': actual, 'disk_before': before_disk, 'disk_after': after_disk,
                    'passed': canonical(actual['text']) == expected and after_disk is not None and after_disk['text'] == expected}
            summary['cases'].append(case)
            write(out / (name + '-case.json'), case)
        # Cleanup is explicitly separate: its menu save never changes a failed
        # shortcut case or replaces the original result/independent disk oracle.
        summary['passed'] = all(case['passed'] for case in summary['cases'])
        summary['final_before_cleanup'] = execute('final-owned-readback', '')
        saved = menu('cleanup-owned-menu-save', args.save_label)
        require(saved['dispatch'] == 'dispatched', 'Cleanup save was not dispatched')
        current = canonical(summary['final_before_cleanup']['text'])
        require(disk_matches(document, current) is not None and file_state(document)['text'] == current,
                'Owned cleanup menu save did not settle; leave tab for review')
        closed = menu('close-owned-tab', args.close_tab_label)
        require(closed['dispatch'] == 'dispatched', 'Owned tab close was not dispatched')
        def closed_readback():
            current, _ = client.execute(app, 'owned-tab-closure-readback', f'''const windows=(await app.windows()).windows;
const remaining=[];
for(const window of windows) {{
  let page=await app.observe({{window_ref:window.window_ref,role:'tab_item',limit:100}});
  for(;;) {{
    remaining.push(...page.elements.filter(e=>e.name.includes({filename})).map(e=>e.name));
    if(!page.next) break;
    page=await app.observe({{window_ref:window.window_ref,cursor:page.next}});
  }}
}}
return {{remaining}};''')
            return current if not current['remaining'] else None
        wait(closed_readback, 5)
        summary['cleanup'].append({'owned_tab_close_dispatched': True, 'owned_tab_absent': True})
        summary['final_document'] = file_state(document)
    except BaseException:
        summary['failure'] = traceback.format_exc()
        summary['passed'] = False
        summary['cleanup'].append({'owned_tab_may_remain': str(document)})
    finally:
        if observer:
            try:
                write(out / 'input-queues-final.json', observer.sample())
                observer.close()
            except BaseException:
                summary['observer_error'] = traceback.format_exc()
                summary['passed'] = False
        if client:
            try:
                summary['cleanup'].append({'mcp': client.close()})
            except BaseException:
                summary['passed'] = False
                summary['cleanup'].append({'mcp_error': traceback.format_exc()})
        if launcher:
            summary['cleanup'].append({'launcher_pid': launcher.pid, 'exit_code_if_exited': launcher.poll(),
                                       'terminated': False})
        try:
            after = {path: sha(Path(path)) for path in protected}
            summary['protected_after'] = after
            require(after == protected, 'A protected pre-existing acceptance document changed')
        except BaseException:
            summary['passed'] = False
            summary['protected_error'] = traceback.format_exc()
        summary['other_tab_names_before'] = baseline_names
        write(out / 'summary.json', summary)
        print(json.dumps(summary))
    return 0 if summary['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
