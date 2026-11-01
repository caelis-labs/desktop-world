#!/usr/bin/env python3
"""Opt-in owned Chrome Ctrl+O/dialog and Alt+F4 terminal-keypress probe.

Only DTW supplies application input. A separate browser profile and process
exit establish cleanup independently; no shortcut or unknown action is replayed.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import traceback
import uuid

from cursor_probe import Mcp, require, wait, write


def owned_window_facts(pid):
    """Read-only Win32 process/owner proof; never activates or attaches a queue."""
    from ctypes import wintypes as w
    user = ctypes.WinDLL('user32', use_last_error=True)
    callback_type = ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    user.EnumWindows.argtypes = [callback_type, w.LPARAM]
    user.EnumWindows.restype = w.BOOL
    user.GetWindowThreadProcessId.argtypes = [w.HWND, ctypes.POINTER(w.DWORD)]
    user.GetWindowThreadProcessId.restype = w.DWORD
    user.GetWindow.argtypes = [w.HWND, w.UINT]
    user.GetWindow.restype = w.HWND
    user.GetAncestor.argtypes = [w.HWND, w.UINT]
    user.GetAncestor.restype = w.HWND
    user.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, ctypes.c_int]
    user.GetWindowTextW.restype = ctypes.c_int
    user.IsWindowEnabled.argtypes = [w.HWND]
    user.IsWindowEnabled.restype = w.BOOL
    user.IsWindowVisible.argtypes = [w.HWND]
    user.IsWindowVisible.restype = w.BOOL
    rows = []

    @callback_type
    def collect(hwnd, _):
        process = w.DWORD()
        thread = user.GetWindowThreadProcessId(hwnd, ctypes.byref(process))
        if process.value == pid and user.IsWindowVisible(hwnd):
            text = ctypes.create_unicode_buffer(1024)
            user.GetWindowTextW(hwnd, text, len(text))
            rows.append({'hwnd': hwnd, 'pid': process.value, 'thread': thread, 'title': text.value,
                         'owner': user.GetWindow(hwnd, 4), 'root_owner': user.GetAncestor(hwnd, 3),
                         'enabled': bool(user.IsWindowEnabled(hwnd))})
        return True

    ctypes.set_last_error(0)
    result = user.EnumWindows(collect, 0)
    error = ctypes.get_last_error()
    require(bool(result), f'Independent EnumWindows unproven: result={result}, error={error}')
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--chrome', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', choices=['all', 'close'], default='all')
    parser.add_argument('--observed-dialog', action='store_true',
                        help='Prove a new same-process owned HWND independently when the strict DTW modal wait cannot prove it')
    args = parser.parse_args()
    binary, chrome = args.dtw.resolve(strict=True), args.chrome.resolve(strict=True)
    output = args.output.resolve()
    require(os.name == 'nt' and sys.flags.utf8_mode and not output.exists(),
            'Use Windows, python -X utf8 and a new output directory')
    output.mkdir(parents=True)
    title = 'DTW-terminal-' + uuid.uuid4().hex
    page = output / 'owned.html'
    page.write_text('<!doctype html><title>' + title + '</title><p>Owned keyboard terminal probe</p>', encoding='utf-8')
    summary = {'passed': False, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
               'cases': [], 'cleanup': []}
    browser = client = app = window = None
    try:
        browser = subprocess.Popen([str(chrome), '--user-data-dir=' + str(output / 'profile'),
                                    '--no-first-run', '--no-default-browser-check',
                                    '--force-renderer-accessibility', '--new-window', page.as_uri()])
        summary['owned_browser_pid'] = browser.pid
        client = Mcp(binary, output / 'mcp', env=dict(os.environ, DTW_AX_READINESS_TRACE='1'))

        def owned_window():
            catalog = client.tool('apps', {})
            app = next((row['app'] for row in catalog['apps'] if row['app'].casefold() == str(chrome).casefold()), None)
            if app is None:
                return None
            windows, _ = client.execute(app, 'owned-windows', 'return (await app.windows()).windows;')
            matches = [row for row in windows if row['title'].startswith(title)]
            require(len(matches) <= 1, 'Ambiguous owned browser window')
            return (app, matches[0]['window_ref']) if matches else None

        app, window = wait(owned_window, 20)
        wr = json.dumps(window)
        client.execute(app, 'prepare-owned-browser', f'return await app.focus({{window_ref:{wr}}});')
        if args.case != 'close':
            if args.observed_dialog:
                before, _ = client.execute(app, 'before-dialog-windows', 'return (await app.windows()).windows;')
                before_facts = owned_window_facts(browser.pid)
                roots = [row for row in before_facts if row['title'].startswith(title)]
                require(len(roots) == 1, 'Independent private-browser root is ambiguous')
                receipt, _ = client.execute(app, 'ctrl-o-once', f'return await app.keypress({{window_ref:{wr},keys:["CTRL","O"]}});')
                before_refs = {row['window_ref'] for row in before}
                before_hwnds = {row['hwnd'] for row in before_facts}

                def new_owned_dialog():
                    rows, _ = client.execute(app, 'after-dialog-windows', 'return (await app.windows()).windows;')
                    facts = owned_window_facts(browser.pid)
                    new = [row for row in facts if row['hwnd'] not in before_hwnds
                           and row['root_owner'] == roots[0]['hwnd'] and row['owner']]
                    pairs = [(row, fact) for row in rows if row['window_ref'] not in before_refs
                             for fact in new if fact['title'] == row['title']]
                    require(len(pairs) <= 1, 'New owned dialog is ambiguous')
                    return {'window': pairs[0][0], 'facts': pairs[0][1], 'root': roots[0]} if pairs else None

                observed = wait(new_owned_dialog, 4)
                write(output / 'observed-dialog-proof.json', {'before': before, 'before_facts': before_facts, 'after': observed})
                modal_ref = observed['window']['window_ref']
                opened = {'receipt': receipt, 'observed_owned_window': observed,
                          'strict_modal_wait_verified': False}
                client.execute(app, 'retain-strict-modal-limitation', f'return await app.waitForWindow({{window_ref:{wr},state:"dialog",timeout_ms:250}});', error='dialog_not_present')
            else:
                opened, _ = client.execute(app, 'ctrl-o-opens-dialog', f'''const receipt=await app.keypress({{window_ref:{wr},keys:['CTRL','O']}});
const modal=await app.waitForWindow({{window_ref:{wr},state:'dialog',timeout_ms:4000}});
state.ownedTerminalModal=modal.window_ref;return {{receipt,modal}};''')
                require(opened['modal']['kind'] == 'dialog' and opened['modal']['relation'] == 'child_modal', 'Unproven owned modal')
                modal_ref = opened['modal']['window_ref']
            require(opened['receipt']['outcome']['dispatch'] == 'dispatched', 'Ctrl+O not dispatched')
            require(opened['receipt']['outcome']['evidence']['kind'] == 'unverified', 'Queue read must not verify dialog effect')
            summary['cases'].append({'case': 'ctrl-o', 'result': opened, 'passed': True})
            dismissed, _ = client.execute(app, 'escape-owned-dialog', f'''const receipt=await app.keypress({{window_ref:{json.dumps(modal_ref)},keys:['ESC']}});
const closed=await app.waitForWindow({{window_ref:{json.dumps(modal_ref)},state:'closed',timeout_ms:4000}});
const ready=await app.waitForWindow({{window_ref:{wr},state:'unblocked',timeout_ms:4000}});
return {{receipt,closed,ready}};''')
            require(dismissed['receipt']['outcome']['dispatch'] == 'dispatched', 'Escape not dispatched')
            if args.observed_dialog:
                require(all(row['hwnd'] != observed['facts']['hwnd'] for row in owned_window_facts(browser.pid)),
                        'Independent owned dialog HWND still visible after Escape')
            summary['cases'].append({'case': 'escape-dialog', 'result': dismissed, 'passed': True})
        closed, _ = client.execute(app, 'alt-f4-once', f'''const receipt=await app.keypress({{window_ref:{wr},keys:['ALT','F4']}});
const closed=await app.waitForWindow({{window_ref:{wr},state:'closed',timeout_ms:4000}});return {{receipt,closed}};''')
        require(closed['receipt']['outcome']['dispatch'] == 'dispatched', 'Alt+F4 not dispatched')
        require(closed['receipt']['outcome']['evidence']['kind'] == 'unverified', 'Queue read must not verify browser exit')
        require(browser.wait(timeout=10) == 0, 'Owned browser did not exit normally')
        summary['cases'].append({'case': 'alt-f4', 'result': closed, 'independent_browser_exit': 0, 'passed': True})
        summary['passed'] = True
    except BaseException:
        summary['failure'] = traceback.format_exc()
        if client and app and window:
            try:
                diagnostic, _ = client.execute(app, 'failure-read-only',
                    f'return {{windows:(await app.windows()).windows,view:await app.observe({{window_ref:{json.dumps(window)},limit:80}})}};')
                summary['failure_read_only'] = diagnostic
            except BaseException:
                summary['failure_read_only_error'] = traceback.format_exc()
    finally:
        # Even if a receipt was unknown, observe the process before cleanup;
        # never issue a second Alt+F4 to make the first result look successful.
        if browser:
            try:
                code = browser.wait(timeout=2)
                summary['cleanup'].append({'browser_exit': code, 'forced': False})
            except subprocess.TimeoutExpired:
                browser.terminate()
                summary['cleanup'].append({'browser_exit': browser.wait(timeout=5), 'forced': True})
                summary['passed'] = False
        if client:
            try:
                summary['cleanup'].append({'mcp': client.close()})
            except BaseException:
                summary['cleanup'].append({'mcp_error': traceback.format_exc()})
                summary['passed'] = False
        write(output / 'summary.json', summary)
        print(json.dumps(summary, ensure_ascii=True))
    return 0 if summary['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
