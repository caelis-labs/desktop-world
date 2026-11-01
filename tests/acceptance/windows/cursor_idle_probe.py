#!/usr/bin/env python3
"""Opt-in Windows idle-cursor acceptance through two ordinary MCP connections.

Only DTW supplies application input to an owned fixture. Independent process,
window and physical-pointer observations prove presentation and cleanup. Packet
replay boundaries are covered by pure Presentation tests, not a public backdoor.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import traceback

from cursor_probe import Mcp, ProcessHandle, read, require, wait, write


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(os.name == 'nt' and sys.flags.utf8_mode, 'Use Windows and python -X utf8')
    binary, output = args.dtw.resolve(strict=True), args.output.resolve()
    require(not output.exists(), 'Use a new evidence directory')
    output.mkdir(parents=True)
    summary = {'passed': False, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
               'cases': [], 'cleanup': [], 'scope': 'owned Windows fixture; ordinary MCP connections; no live host config changes',
               'late_packet_scope': 'pure authenticated Presentation state tests; no network replay injection'}
    clients, handles = [], []
    fixture = None
    serial = 0
    try:
        here = Path(__file__).parent
        compile_result = subprocess.run(['powershell.exe', '-NoProfile', '-ExecutionPolicy', 'Bypass',
            '-File', str(here / 'compile-cursor.ps1'), '-OutputDirectory', str(output / 'build')], capture_output=True)
        (output / 'compile.stdout').write_bytes(compile_result.stdout)
        (output / 'compile.stderr').write_bytes(compile_result.stderr)
        require(compile_result.returncode == 0, 'Owned fixture compilation failed')
        directory = output / 'A'
        directory.mkdir()
        executable = directory / 'Fixture.exe'
        shutil.copy2(output / 'build/Fixture.exe', executable)
        fixture = subprocess.Popen([str(executable), str(directory), 'A'])

        def state():
            value = wait(lambda: read(directory / 'state.json'), timeout=1)
            require(value is None or value['pid'] == fixture.pid, 'Fixture PID changed')
            return value

        wait(lambda: (s if (s := state()) and s['ready'] and s['bounds_ok'] else None))

        def snapshot():
            nonlocal serial
            serial += 1
            path = output / ('observer-' + str(serial) + '.json')
            subprocess.run([str(output / 'build/CursorObserver.exe'), '--snapshot', str(path), state()['hwnd']],
                           check=True, creationflags=subprocess.CREATE_NO_WINDOW)
            return read(path)

        def marker(scene, client):
            rows = [row for row in scene['windows'] if row['parent'] == client.proc.pid]
            require(len(rows) <= 1, 'One MCP connection has multiple cursor windows')
            return rows[0] if rows else None

        def connect(label):
            client = Mcp(binary, output / label)
            clients.append(client)
            apps = client.tool('apps', {})['apps']
            app = next(row['app'] for row in apps if row['app'].casefold() == str(executable).casefold())
            selected, _ = client.execute(app, 'select-owned-window', '''const windows=(await app.windows()).windows;
const own=windows.filter(w=>w.title==='DTW Windows Smoke A');if(own.length!==1)throw Error('Owned window not unique');
state.ownedWindow=own[0].window_ref;return own[0];''')
            return client, app, selected['window_ref']

        first, app, window = connect('first')
        second, other_app, other_window = connect('second')

        def click(client, app, window, label, dx=0):
            before = state()
            x = before['pad_point']['x'] + dx - before['bounds']['x']
            y = before['pad_point']['y'] - before['bounds']['y']
            _, ledger = client.execute(app, label, f'''const window_ref={json.dumps(window)};
await app.focus({{window_ref}});const c=await app.capture({{window_ref}});
return await app.click({{window_ref,observation_ref:c.observation_ref,x:{x},y:{y}}});''')
            wait(lambda: state()['left_up'] == before['left_up'] + 1)
            def visible():
                scene = snapshot()
                row = marker(scene, client)
                return (scene, row) if row and row['visible'] and max(row['palette_pixels']) >= 20 else None
            scene, row = wait(visible, 3)
            require(scene['foreground_before'] and scene['foreground_after'], 'Cursor changed foreground')
            receipts = [step['receipt'] for step in ledger['script']['steps'] if step['operation'] == 'click']
            require(len(receipts) == 1 and receipts[0]['outcome']['dispatch'] == 'dispatched', 'Original click was not dispatched')
            require(receipts[0]['outcome']['evidence']['kind'] == 'unverified', 'Presentation verified a business effect')
            return scene, row, ledger

        def idle(client, app, window, label, before, identity, ledger):
            started = time.monotonic()
            samples, hidden_at = [], None
            last_read = -1
            while time.monotonic() - started < 3.5:
                scene = snapshot()
                row = marker(scene, client)
                require(row and (row['pid'], row['hwnd'], row['birth']) ==
                        (identity['pid'], identity['hwnd'], identity['birth']), 'Idle replaced the session cursor identity')
                require(scene['system_cursor'] == before['system_cursor'], 'Idle moved the physical cursor')
                require(scene['foreground_before'] and scene['foreground_after'], 'Idle stole target foreground')
                elapsed = time.monotonic() - started
                samples.append({'elapsed': elapsed, 'visible': row['visible'], 'system_cursor': scene['system_cursor']})
                if not row['visible']:
                    hidden_at = hidden_at or elapsed
                    if elapsed - hidden_at >= .35:
                        break
                elif hidden_at is not None:
                    raise AssertionError('Idle z-order maintenance revived the expired cursor')
                if elapsed - last_read >= .3:
                    result, _ = client.execute(app, label + '-read-only', 'return (await app.windows()).windows;')
                    require(any(row['window_ref'] == window for row in result), 'Idle invalidated the retained window reference')
                    last_read = elapsed
                time.sleep(.04)
            write(output / (label + '-samples.json'), samples)
            require(hidden_at is not None and time.monotonic() - started - hidden_at >= .35,
                    'Cursor did not stay hidden with its MCP connection open')
            require(client.proc.poll() is None, 'Idle closed the MCP server')
            require(client.tool('result', {'app': app, 'execution_id': ledger['execution_id']}) == ledger,
                    'Idle changed the original retained receipt')
            summary['cases'].append({'case': label, 'hidden_after_observed_seconds': hidden_at,
                                     'connection_open': True, 'reference_and_receipt_retained': True})

        scene, identity, ledger = click(first, app, window, 'first-pointer')
        handle = ProcessHandle(identity['pid'], binary, birth=identity['birth'])
        handles.append(handle)
        idle(first, app, window, 'success-and-read-only-idle', scene, identity, ledger)
        recovered, row, ledger = click(first, app, window, 'same-connection-pointer-after-idle')
        require((row['pid'], row['hwnd'], row['birth']) == (identity['pid'], identity['hwnd'], identity['birth']),
                'New operation lost stable helper identity after idle')
        first.execute(app, 'owned-script-exception', "throw Error('owned idle exception control');", error='owned idle exception control')
        idle(first, app, window, 'exception-idle', recovered, identity, ledger)
        scene2, identity2, ledger2 = click(second, other_app, other_window, 'warm-second-helper', -120)
        require(identity2['pid'] != identity['pid'], 'MCP connections shared a cursor process')
        handle2 = ProcessHandle(identity2['pid'], binary, birth=identity2['birth'])
        handles.append(handle2)
        idle(second, other_app, other_window, 'second-connection-idle', scene2, identity2, ledger2)

        click(first, app, window, 'isolation-first-fresh')
        time.sleep(1)
        mixed, _, _ = click(second, other_app, other_window, 'isolation-second-fresh', -120)
        require(marker(mixed, first)['visible'] and marker(mixed, second)['visible'], 'Isolation setup missed both active markers')
        def independent_expiry():
            scene = snapshot()
            one, two = marker(scene, first), marker(scene, second)
            require(one and two, 'Idle destroyed a helper instead of hiding')
            return scene if not one['visible'] and two['visible'] else None
        proof = wait(independent_expiry, 2)
        write(output / 'independent-idle-expiry.json', proof)
        require(handle.alive() and handle2.alive(), 'Idle reclaimed a live connection helper')
        summary['cases'].append({'case': 'independent-session-idle-clocks', 'first_hidden_second_visible': True})

        click(second, other_app, other_window, 'second-before-first-eof', -120)
        summary['cleanup'].append({'first_mcp': first.close()})
        clients.remove(first)
        require(handle.wait() == 0, 'First helper did not exit normally after EOF')
        scene = snapshot()
        require(marker(scene, first) is None and marker(scene, second)['visible'], 'One EOF affected the other MCP cursor')
        summary['cases'].append({'case': 'eof-keeps-other-session-alive', 'helper_exit': 0})
        summary['passed'] = True
    except BaseException:
        summary['failure'] = traceback.format_exc()
    finally:
        for client in clients:
            try:
                summary['cleanup'].append({'mcp': client.close()})
            except BaseException:
                summary['cleanup'].append({'mcp_error': traceback.format_exc()})
                summary['passed'] = False
        for handle in handles:
            try:
                code = handle.wait()
                summary['cleanup'].append({'helper_pid': handle.pid, 'exit_code': code})
                summary['passed'] &= code == 0
            except BaseException:
                summary['cleanup'].append({'helper_error': traceback.format_exc()})
                summary['passed'] = False
            finally:
                handle.close()
        if fixture:
            (directory / 'stop').write_text('stop', encoding='utf-8')
            try:
                code = fixture.wait(timeout=5)
                summary['cleanup'].append({'fixture_exit': code, 'forced': False})
                summary['passed'] &= code == 0
            except subprocess.TimeoutExpired:
                fixture.kill()
                summary['cleanup'].append({'fixture_forced': True})
                summary['passed'] = False
        write(output / 'summary.json', summary)
        print(json.dumps(summary))
    return 0 if summary['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
