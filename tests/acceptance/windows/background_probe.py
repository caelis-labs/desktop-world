"""Prove Windows' supported background routes with a separate foreground witness."""
import argparse
import ctypes
from ctypes import wintypes as w
import json
from pathlib import Path
import shutil
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import AppClient, artifact_identity, require
from windows.app_protocol_probe import wait_state


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--fixture-binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    report = {'passed': False, 'artifact': artifact_identity(args.dtw), 'cases': [],
              'scope': 'Windows UIA/posted input and clean background-only refusals; no background keyboard claim'}
    children, clients, intervals = [], [], []
    holder = holder_request = None
    user = ctypes.WinDLL('user32')
    # Read in the same physical coordinate space as the native observer.
    # On this host GetPhysicalCursorPos is also virtualized for unaware Python.
    user.GetCursorPos.argtypes = [ctypes.POINTER(w.POINT)]
    user.SetThreadDpiAwarenessContext.argtypes = [ctypes.c_void_p]
    user.SetThreadDpiAwarenessContext.restype = ctypes.c_void_p

    def state(root):
        return wait_state(root, lambda state: True)

    try:
        for label in ('A', 'B'):
            root = out / label
            root.mkdir()
            exe = out / ('Target.exe' if label == 'A' else 'Witness.exe')
            shutil.copy2(args.fixture_binary, exe)
            child = subprocess.Popen([str(exe), str(root), label])
            children.append((child, root))
            wait_state(root, lambda state: state['ready'])
        target, witness = out / 'A', out / 'B'
        # A new Form.Activate can legitimately lose foreground locking to a
        # hidden Windows service. A declared actor prepares only our witness
        # and keeps its execution alive while background behavior is measured.
        holder = AppClient(args.dtw.resolve(), out / 'foreground-actor')
        clients.append(holder)
        holder_request = holder.begin_execute(out / 'Witness.exe',
            'await app.focus();const end=Date.now()+25000;while(Date.now()<end){};')
        wait_state(witness, lambda state: state['foreground'])
        require(not state(target)['foreground'], 'owned target is not background')
        client = AppClient(args.dtw.resolve(), out / 'protocol')
        clients.append(client)
        app = out / 'Target.exe'
        image = client.capture(app, 'background')['view']['image']
        point = state(target)['pad_point']
        b = image['bounds']
        pixel = {'x': (point['x'] - b[0]) * image['width'] / b[2],
                 'y': (point['y'] - b[1]) * image['height'] / b[3]}

        def exercise(name, source, active=client, refusal=False):
            cursor = w.POINT()
            previous = user.SetThreadDpiAwarenessContext(ctypes.c_void_p(-4))
            require(previous, 'cannot enter read-only physical DPI context')
            try:
                require(user.GetCursorPos(ctypes.byref(cursor)), 'system cursor unreadable')
            finally:
                user.SetThreadDpiAwarenessContext(previous)
            start = time.time() * 1000
            response = active.execute(app, source, allow_error=refusal)[0]
            end = time.time() * 1000
            intervals.append({'name': name, 'start_ms': start, 'end_ms': end,
                              'foreground_pid': children[1][0].pid, 'cursor': {'x': cursor.x, 'y': cursor.y}})
            require(not state(target)['foreground'] and state(witness)['foreground'], 'foreground witness lost')
            if refusal:
                require(response.get('error', {}).get('code') == 'foreground_required', response)
                dispatch = response.get('effects', {}).get('dispatch', {})
                require(dispatch.get('not_dispatched') == 1 and not dispatch.get('unknown') and not dispatch.get('dispatched'), response)
            report['cases'].append({'name': name, 'response': response, 'passed': True})

        # UIA Invoke may itself focus/click in a provider implementation. Its
        # business effect is checked, but it is not a Windows background promise.
        invoked = client.execute(app, 'await app.click({selector:{role:"button",text:"Increment"}});')[0]
        wait_state(target, lambda state: state['increments'] == 1)
        report['uia_provider_effect'] = {'response': invoked, 'target_foreground': state(target)['foreground'],
                                         'background_guarantee': False}
        # Ordinary Form.Activate can be refused by Windows foreground locking.
        # An explicit DTW request prepares our own witness before measurement.
        wait_state(witness, lambda state: state['foreground'])
        image = client.capture(app, 'after-uia-provider')['view']['image']
        point = state(target)['pad_point']
        b = image['bounds']
        pixel = {'x': (point['x'] - b[0]) * image['width'] / b[2],
                 'y': (point['y'] - b[1]) * image['height'] / b[3]}
        exercise('posted_click', 'await app.click(' + json.dumps(pixel) + ');')
        wait_state(target, lambda state: state['left_up'] == 1)
        exercise('posted_wheel', 'await app.scroll(' + json.dumps({**pixel, 'delta_y': 2}) + ');')
        wait_state(target, lambda state: state['wheel_delta'] == -240)
        exercise('focused_read', 'await app.look({role:"text_field",text:"Message"});')
        strict = AppClient(args.dtw.resolve(), out / 'strict-protocol', mcp_args=('--input-policy', 'background-only'))
        clients.append(strict)
        strict.capture(app, 'strict-background')
        exercise('keyboard_clean_refusal', 'await app.keypress("CTRL+A");', strict, True)
        exercise('hover_clean_refusal', 'await app.move(' + json.dumps(pixel) + ');', strict, True)
        require(state(target)['text'] == '' and state(witness)['text'] == '', 'background probe leaked keyboard input')
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        report['cleanup'] = []
        if holder_request is not None:
            try:
                response = holder.transport.receive(holder_request, timeout=30)['result']
                report['witness_setup'] = response.get('structuredContent')
                require(not response.get('isError'), response)
            except BaseException as error:
                report['passed'] = False
                report['cleanup'].append({'foreground_setup_error': str(error)})
        for client in clients:
            try:
                report['cleanup'].append({'mcp': client.close()})
            except BaseException as error:
                report['passed'] = False
                report['cleanup'].append({'error': str(error)})
        for child, root in children:
            (root / 'stop').write_text('owned normal close', encoding='utf-8')
            try:
                report['cleanup'].append({'fixture_pid': child.pid, 'exit': child.wait(timeout=8)})
                report['passed'] &= child.returncode == 0
            except subprocess.TimeoutExpired:
                child.terminate()
                child.wait(timeout=5)
                report['passed'] = False
                report['cleanup'].append({'forced_fixture': child.pid})
        (out / 'independent-intervals.json').write_text(json.dumps(intervals, indent=2) + '\n', encoding='utf-8')
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    require(report['passed'], report)


if __name__ == '__main__':
    main()
