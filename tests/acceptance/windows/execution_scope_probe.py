"""Owned Windows scope/away-return acceptance; public apps/exec, no model claim.

A separate MCP connection supplies only the declared foreground actor. All
business effects come from the target connection and independent fixture files.
No shell/UIA input, window-service manipulation or unknown input replay.
"""
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
              'scope': 'Owned foreground actor and native fixture; direct-script evidence, not DSH model acceptance'}
    children, clients, intervals = [], [], []
    holder = holder_request = None
    user = ctypes.WinDLL('user32', use_last_error=True)
    user.GetCursorPos.argtypes = [ctypes.POINTER(w.POINT)]
    user.GetCursorPos.restype = w.BOOL
    user.GetTopWindow.argtypes = [w.HWND]
    user.GetTopWindow.restype = w.HWND
    user.GetWindow.argtypes = [w.HWND, w.UINT]
    user.GetWindow.restype = w.HWND
    user.GetWindowThreadProcessId.argtypes = [w.HWND, ctypes.POINTER(w.DWORD)]
    user.GetWindowThreadProcessId.restype = w.DWORD
    user.SetThreadDpiAwarenessContext.argtypes = [ctypes.c_void_p]
    user.SetThreadDpiAwarenessContext.restype = ctypes.c_void_p
    user.SetThreadDpiAwarenessContext(ctypes.c_void_p(-4))

    def position():
        point = w.POINT()
        require(user.GetCursorPos(ctypes.byref(point)), 'physical cursor unreadable')
        return [point.x, point.y]

    def state(root):
        return wait_state(root, lambda value: True)

    def stack(windows):
        # Enumerate the actual desktop order, reporting only owned HWNDs.
        order = []
        current = user.GetTopWindow(None)
        visited = set()
        while current and current not in visited:
            if current in windows:
                order.append(windows[current])
            visited.add(current)
            current = user.GetWindow(current, 2)  # GW_HWNDNEXT
        require(len(order) == len(windows), 'Owned windows missing from desktop stack')
        return order

    def receive(client, request, name, started, timeout=8):
        response = client.transport.receive(request, timeout=timeout)
        intervals.append({'name': name, 'start_ms': started, 'end_ms': time.time() * 1000})
        return response['result']['structuredContent'], response['result']

    def record(name, **details):
        report['cases'].append({'name': name, 'passed': True, **details})

    try:
        windows = {}
        for label, name, placement in [('A', 'Target', 'A'), ('C', 'Neighbour', 'B'), ('B', 'Witness', 'B')]:
            root = out / label
            root.mkdir()
            exe = out / (name + '.exe')
            shutil.copy2(args.fixture_binary, exe)
            child = subprocess.Popen([str(exe), str(root), placement])
            children.append((child, root))
            snapshot = wait_state(root, lambda value: value['ready'])
            handle = int(snapshot['hwnd'])
            owner = w.DWORD()
            user.GetWindowThreadProcessId(handle, ctypes.byref(owner))
            require(owner.value == child.pid, f'Unexpected owner for {name}')
            windows[handle] = name
        target_root, witness_root = out / 'A', out / 'B'
        target, witness = out / 'Target.exe', out / 'Witness.exe'
        holder = AppClient(args.dtw.resolve(), out / 'foreground-actor')
        clients.append(holder)
        holder_started = time.time() * 1000
        holder_request = holder.begin_execute(witness,
            'await app.focus();const end=Date.now()+25000;while(Date.now()<end){};return "owned-foreground-setup";')
        wait_state(witness_root, lambda value: value['foreground'])
        require(not state(target_root)['foreground'], 'background target setup was not established')
        initial_order = stack(windows)
        report['initial_stack'] = initial_order
        require(initial_order.index('Neighbour') < initial_order.index('Target'), initial_order)
        client = AppClient(args.dtw.resolve(), out / 'target-protocol')
        clients.append(client)
        image = client.capture(target, 'before-keyboard')['view']['image']
        bounds, editor = image['bounds'], state(target_root)['editor_bounds']
        editor_point = {'x': (editor['x'] + editor['width'] / 2 - bounds[0]) * image['width'] / bounds[2],
                        'y': (editor['y'] + editor['height'] / 2 - bounds[1]) * image['height'] / bounds[3]}
        before = position()
        payload = 'Windows scope 中文🙂\r\nsecond café 🚀'
        result, _ = client.execute(target,
            'await app.click({selector:{role:"button",text:"Increment"}});'
            'await app.click(' + json.dumps(editor_point) + ');'
            'await app.keypress("CTRL+A");await app.typeText(' + json.dumps(payload) + ');await app.keypress("CTRL+S");')
        final = wait_state(target_root, lambda value: value['increments'] == 1 and value['saves'] == 1 and value['text'] == payload)
        require((target_root / 'saved.txt').read_bytes().decode('utf-8') == payload, 'saved bytes differ')
        # State files are a sampled independent oracle, not a synchronous
        # response to SetForegroundWindow. Wait for their next publication.
        wait_state(witness_root, lambda value: value['foreground'])
        wait_state(target_root, lambda value: not value['foreground'])
        require(position() == before, 'keyboard script left physical cursor displaced')
        record('automatic_keyboard_script_and_exact_original_return', response=result)
        # Normal foreground activation raises Target. Returning focus to
        # Witness must leave Target in front of Neighbour.
        returned_order = stack(windows)
        report['returned_stack'] = returned_order
        require(returned_order.index('Target') < returned_order.index('Neighbour'),
                {'initial_stack': initial_order, 'returned_stack': returned_order})
        record('focus_return_preserves_windows_activation_order',
               initial_stack=initial_order, returned_stack=returned_order)

        view = client.capture(target, 'before-pointer')['view']['image']
        bounds, point = view['bounds'], state(target_root)['pad_point']
        pixel = {'x': (point['x'] - bounds[0]) * view['width'] / bounds[2],
                 'y': (point['y'] - bounds[1]) * view['height'] / bounds[3]}
        before = position()
        result, _ = client.execute(target, 'await app.move(' + json.dumps(pixel) + ');')
        wait_state(target_root, lambda value: value['moves'] > final['moves'])
        wait_state(witness_root, lambda value: value['foreground'])
        require(position() == before, 'hover did not return pointer')
        record('physical_hover_borrow_and_return', response=result)

        # One independent actor creates an actual away-and-return while the
        # target script is alive. Its focus operation is the entire actor task.
        actor = AppClient(args.dtw.resolve(), out / 'handoff-actor')
        clients.append(actor)
        started = time.time() * 1000
        request = client.begin_execute(target,
            'await app.focus();const end=Date.now()+5000;while(Date.now()<end){};'
            'await app.keypress("F24");await app.click({selector:{role:"button",text:"Increment"}});')
        wait_state(target_root, lambda value: value['foreground'])
        actor_started = time.time() * 1000
        actor_request = actor.begin_execute(witness,
            'await app.focus();const end=Date.now()+350;while(Date.now()<end){};return "owned-away-return";')
        wait_state(witness_root, lambda value: value['foreground'])
        actor_result, actor_response = receive(actor, actor_request, 'declared-away-return-actor', actor_started)
        require(not actor_response.get('isError'), actor_result)
        wait_state(target_root, lambda value: value['foreground'])
        result, response = receive(client, request, 'target-external-handoff', started)
        require(response.get('isError') and result.get('error', {}).get('code') == 'execution_focus_changed', result)
        require(state(target_root)['increments'] == 1, 'suffix after handoff was delivered')
        journal = [json.loads(line) for line in (target_root / 'events.jsonl').read_text(encoding='utf-8').splitlines()]
        require(not any(row['kind'] == 'key_down' and row['data']['key'] == 135 for row in journal), 'F24 was delivered after external handoff')
        record('away_and_return_is_sticky_without_reacquisition', response=result, actor=actor_result)

        result, _ = client.execute(target,
            'await app.look();await app.keypress("F24");await app.click({selector:{role:"button",text:"Increment"}});')
        wait_state(target_root, lambda value: value['increments'] == 2)
        require(state(witness_root)['text'] == '', 'business text reached the witness')
        record('same_connection_fresh_execution_recovers', response=result)
        report['target_final'] = state(target_root)
        report['witness_final'] = state(witness_root)
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        report['cleanup'] = []
        if holder_request is not None:
            try:
                result, response = receive(holder, holder_request, 'owned-foreground-setup', holder_started, timeout=30)
                report['foreground_setup_result'] = result
                require(not response.get('isError'), result)
            except BaseException as error:
                report['passed'] = False
                report['cleanup'].append({'foreground_setup_error': str(error)})
        for client in reversed(clients):
            try:
                report['cleanup'].append({'mcp': client.close()})
            except BaseException as error:
                report['passed'] = False
                report['cleanup'].append({'mcp_error': str(error)})
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
        (out / 'rpc-intervals.json').write_text(json.dumps(intervals, indent=2), encoding='utf-8')
        (out / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    require(report['passed'], report)


if __name__ == '__main__':
    main()
