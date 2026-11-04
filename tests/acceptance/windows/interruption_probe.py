"""Interrupt an actual held drag; prove paired release, no suffix and worker exit."""
import argparse
import ctypes
import json
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import AppClient, artifact_identity, require
from windows.app_protocol_probe import wait, wait_state
from windows.desktop_probe import children
from windows.transport import ProcessHandle


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', required=True, type=Path)
    parser.add_argument('--fixture-binary', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--mode', choices=['cancel', 'eof', 'host-death', 'deadline'], required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    root = out / 'fixture'
    root.mkdir()
    exe = out / 'Fixture.exe'
    shutil.copy2(args.fixture_binary, exe)
    report = {'passed': False, 'artifact': artifact_identity(args.dtw), 'mode': args.mode}
    client = child = None
    handles = []
    user = ctypes.WinDLL('user32')
    user.GetAsyncKeyState.argtypes = [ctypes.c_int]
    user.GetAsyncKeyState.restype = ctypes.c_short

    def left_held():
        return user.GetAsyncKeyState(1) < 0

    try:
        require(not left_held(), 'left button already held before owned test')
        child = subprocess.Popen([str(exe), str(root), 'A'])
        initial = wait_state(root, lambda state: state['ready'])
        client = AppClient(args.dtw.resolve(), out / 'protocol')
        view = client.capture(exe, 'before-drag')['view']['image']
        b = view['bounds']
        p = initial['pad_point']
        x = (p['x'] - b[0]) * view['width'] / b[2]
        y = (p['y'] - b[1]) * view['height'] / b[3]
        path = [{'x': x + (-130 if index % 2 == 0 else 130), 'y': y + (index % 3) * 10}
                for index in range(8)]
        delay = 'const end=Date.now()+29400;while(Date.now()<end){};await app.capture();' if args.mode == 'deadline' else ''
        source = delay + 'await app.drag(' + json.dumps({'path': path}) + ');await app.click({selector:{role:"button",text:"Increment"}});'
        request_started_ms = time.time() * 1000
        request = client.begin_execute(exe, source)
        until = time.monotonic() + (32 if args.mode == 'deadline' else 8)
        while not left_held():
            require(time.monotonic() < until, 'no actual held drag observed')
            time.sleep(.001)
        report['held_observed'] = True
        # Bind all exact DTW descendants before interrupting their parent.
        pending = [client.transport.proc.pid]
        while pending:
            for row in children(pending.pop()):
                if row['exe'].casefold() == args.dtw.name.casefold():
                    handles.append(ProcessHandle(row['pid'], args.dtw.resolve()))
                    pending.append(row['pid'])
        if args.mode == 'cancel':
            client.cancel(request)
        elif args.mode == 'eof':
            client.transport.proc.stdin.close()
        elif args.mode == 'host-death':
            client.transport.proc.kill()  # Exact owned MCP; intentional failure injection.
            report['injected_host_exit'] = client.transport.proc.wait(timeout=5)
        wait(lambda: not left_held(), timeout=5)
        released = wait_state(root, lambda state: 1 not in state['keys_down_global'])
        time.sleep(.15)
        released = wait_state(root, lambda state: True)
        require(released['increments'] == 0 and released['left_up'] == initial['left_up'] + 1, released)
        report['effect'] = released
        if args.mode in ('cancel', 'deadline'):
            try:
                response = client.transport.receive(request, timeout=5)
                report['original_response'] = response
                data = response['result']['structuredContent']
                require(data.get('error', {}).get('code') in ('cancelled', 'deadline_exceeded', 'timed_out', 'timeout'), data)
            except queue.Empty:
                require(args.mode == 'cancel', 'deadline response absent')
                report['original_response'] = 'unknown: cancellation suppressed original reply'
            report['fresh_recovery'] = client.execute(exe, 'await app.look();return "fresh-after-interruption";')[0]
        if args.mode == 'host-death':
            report['mcp_cleanup'] = client.transport.close(expected_exit=client.transport.proc.returncode)
            client = None
        else:
            report['mcp_cleanup'] = client.close()
            client = None
        report['workers'] = [{'pid': handle.pid, 'exit': handle.wait(timeout=8)} for handle in handles]
        (out / 'rpc-intervals.json').write_text(json.dumps([{'name': 'interrupted-exec-and-cleanup',
            'start_ms': request_started_ms, 'end_ms': time.time() * 1000}], indent=2), encoding='utf-8')
        require(all(row['exit'] == 0 for row in report['workers']), report['workers'])
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        if client:
            try:
                report['mcp_cleanup'] = client.close()
            except BaseException as error:
                report.update(passed=False, mcp_cleanup_error=str(error))
        for handle in handles:
            handle.close()
        if child:
            (root / 'stop').write_text('owned normal close', encoding='utf-8')
            try:
                report['fixture_exit'] = child.wait(timeout=8)
                report['passed'] &= child.returncode == 0
            except subprocess.TimeoutExpired:
                child.terminate()
                child.wait(timeout=5)
                report.update(passed=False, forced_fixture_cleanup=True)
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
