#!/usr/bin/env python3
"""Real Chrome UIA selection and SendInput acknowledgement regression.

Creates one disposable browser profile/workbench. All app observation and input
use DTW MCP; disk reads independently verify only this fixture's side effects.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import traceback
import uuid

from cursor_probe import Mcp, read, require, wait, write


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dtw', type=Path, required=True)
    p.add_argument('--chrome', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    args = p.parse_args()
    require(os.name == 'nt' and sys.flags.utf8_mode, 'Use Windows: python -X utf8')
    binary, chrome = args.dtw.resolve(strict=True), args.chrome.resolve(strict=True)
    out = args.output.resolve()
    require(not out.exists(), 'Use a new output directory')
    out.mkdir(parents=True)
    summary = {'passed': False, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
               'cases': [], 'cleanup': []}
    client = browser = server = None
    title = 'DTW Workbench delivery-' + uuid.uuid4().hex[:12]
    try:
        fixture = Path(__file__).resolve().parents[2] / 'fixtures/browser-workbench/server.py'
        with (out / 'server.stdout').open('wb') as so, (out / 'server.stderr').open('wb') as se:
            server = subprocess.Popen([sys.executable, str(fixture), '--output', str(out),
                                       '--case', title.removeprefix('DTW Workbench ')],
                                      stdout=so, stderr=se, creationflags=subprocess.CREATE_NO_WINDOW)
        def url():
            require(server.poll() is None, 'Owned fixture server exited')
            try:
                return json.loads((out / 'server.stdout').read_text(encoding='utf-8'))['url']
            except (ValueError, KeyError):
                return None
        address = wait(url)
        browser = subprocess.Popen([str(chrome), '--user-data-dir=' + str(out / 'profile'),
                                    '--no-first-run', '--no-default-browser-check',
                                    '--force-renderer-accessibility', '--new-window', address])
        client = Mcp(binary, out / 'mcp')
        def apps():
            v = client.tool('apps', {})
            return v if any(a['app'].casefold() == str(chrome).casefold() for a in v['apps']) else None
        catalog = wait(apps)
        app = next(a['app'] for a in catalog['apps'] if a['app'].casefold() == str(chrome).casefold())
        def window():
            rows, _ = client.execute(app, 'owned-windows', 'return (await app.windows()).windows;')
            return next((w['window_ref'] for w in rows if w['title'].startswith(title)), None)
        ref = json.dumps(wait(window, 20))
        client.execute(app, 'open-owned-editor', f'''
await app.focus({{window_ref:{ref}}});
await app.keypress({{window_ref:{ref},keys:["ESC"]}});
await app.setValue({{window_ref:{ref},selector:{{role:"text_field",text:"Search inventory"}},value:"DTW-0088"}});
await app.press({{window_ref:{ref},selector:{{role:"button",text:"Edit DTW-0088"}}}});
await app.keypress({{window_ref:{ref},keys:["TAB"]}});
return await app.keypress({{window_ref:{ref},keys:["TAB"]}});''')
        cases = [
            ('empty-terminal-caret', '', 'A🐦', 'A🐦'),
            ('append-terminal-caret', '', '中😀\n尾', 'A🐦中😀\n尾'),
            ('replace-selection-before-surrogate', f'''
await app.keypress({{window_ref:{ref},keys:["CTRL","HOME"]}});
await app.keypress({{window_ref:{ref},keys:["SHIFT","RIGHT"]}});''',
             '改🐦', '改🐦🐦中😀\n尾'),
            ('replace-all-multiline', f'await app.keypress({{window_ref:{ref},keys:["CTRL","A"]}});',
             '第一批：已核对 🐦\n待补货', '第一批：已核对 🐦\n待补货'),
            ('literal-object-marker', f'await app.keypress({{window_ref:{ref},keys:["CTRL","A"]}});',
             '实字\ufffc🐦', '实字\ufffc🐦'),
            ('append-after-literal-object-marker', '', '😀', '实字\ufffc🐦😀'),
        ]
        for name, prepare, typed, expected in cases:
            result, ledger = client.execute(app, name, f'''
{prepare}
const receipt=await app.typeText({{window_ref:{ref},text:{json.dumps(typed)}}});
const found=await app.find({{window_ref:{ref},selector:{{role:"text_field",text:"Handoff note"}}}});
return {{dispatch:receipt.outcome.dispatch,text:found.element.value}};''')
            require(result == {'dispatch': 'dispatched', 'text': expected}, 'Native readback differs')
            require(ledger['script']['shutdown_error'] is None, 'Delivery cleanup unconfirmed')
            state = wait(lambda: (s if (s := read(out / 'state.json')) and
                                  any(e.get('type') == 'input' and e.get('control') == 'note' and
                                      e.get('value') == expected for e in s['events']) else None))
            require(not state['saves'], 'Keyboard probe unexpectedly saved a record')
            summary['cases'].append({'case': name, 'native_readback_exact': True,
                                     'independent_input_event_exact': True})
        client.execute(app, 'cancel-owned-editor',
                       f'return await app.press({{window_ref:{ref},selector:{{role:"button",text:"Cancel"}}}});')
        client.execute(app, 'close-owned-browser',
                       f'return await app.keypress({{window_ref:{ref},keys:["ALT","F4"]}});')
        require(browser.wait(timeout=10) == 0, 'Owned browser exit failed')
        final = read(out / 'state.json')
        require(not final['saves'] and all(not i['note'] and not i['reviewed'] and
                    i['quantity'] == 10 + int(i['sku'][4:]) % 25 for i in final['items']),
                'Probe changed persistent fixture records')
        summary['passed'] = True
    except BaseException:
        summary['failure'] = traceback.format_exc()
    finally:
        if client:
            try:
                summary['cleanup'].append(client.close())
            except BaseException:
                summary['passed'] = False
                summary['cleanup'].append({'mcp_error': traceback.format_exc()})
        if browser:
            if browser.poll() is None:
                browser.terminate()  # Exact owned Popen handle, never another profile.
                summary['passed'] = False
                summary['cleanup'].append({'forced_owned_browser_exit': browser.wait(timeout=5)})
            else:
                summary['cleanup'].append({'normal_owned_browser_exit': browser.returncode})
        if server:
            if server.poll() is None:
                server.terminate()
            summary['cleanup'].append({'stopped_owned_fixture_server': server.wait(timeout=5)})
        write(out / 'summary.json', summary)
        print(json.dumps(summary))
    return 0 if summary['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
