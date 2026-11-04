"""Real worker on an owned non-input desktop: reject new native input.

No SwitchDesktop, SetThreadDesktop, ACL, lock-screen or security setting changes.
Uses the private native worker solely to establish a real desktop mismatch; the
public GUI regressions use apps/exec. Refusal at admission is distinguished from
mid-dispatch desktop loss (not induced by this diagnostic).
"""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import artifact_identity, require
from windows.app_protocol_probe import wait_state
from windows.desktop_probe import Desktop
from windows.desktop_process import DesktopProcess
from windows.transport import ProcessHandle


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--fixture-binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    root = out / 'fixture'
    root.mkdir()
    report = {'passed': False, 'artifact': artifact_identity(args.dtw), 'cases': [],
              'switch_desktop': False, 'acl_changes': False,
              'boundary': 'real native worker preparation admission; no mid-dispatch switch induced'}
    desktop = fixture = worker = None
    owner = ProcessHandle(os.getpid(), Path(sys.executable).resolve())
    replies = queue.Queue()
    streams = []
    serial = 0
    try:
        desktop = Desktop()
        name = desktop.station_name + '\\' + desktop.desktop_name
        report['worker_desktop'] = name
        fixture_error = (out / 'fixture.stderr').open('wb')
        worker_error = (out / 'worker.stderr').open('wb')
        streams += [fixture_error, worker_error]
        fixture = DesktopProcess([str(args.fixture_binary.resolve()), str(root), 'A'], desktop=name,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=fixture_error)
        initial = wait_state(root, lambda state: state['ready'])
        worker = DesktopProcess([str(args.dtw.resolve()), '--dtw-native-worker', str(os.getpid()), str(owner.birth)],
            desktop=name, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=worker_error,
            env=dict(os.environ, DTW_AX_READINESS_TRACE='1', DTW_INPUT_TRACE='1'))

        def reader():
            try:
                for line in worker.stdout:
                    with (out / 'native-replies.jsonl').open('ab') as log:
                        log.write(line)
                    replies.put(json.loads(line))
            finally:
                replies.put(EOFError('native worker stdout closed'))
        thread = threading.Thread(target=reader, daemon=True)
        thread.start()

        def request(operation, **arguments):
            nonlocal serial
            serial += 1
            message = dict(request_id=str(serial), operation=operation,
                           deadline_unix_ms=int(time.time() * 1000) + 15000, **arguments)
            with (out / 'native-requests.jsonl').open('a', encoding='utf-8') as log:
                log.write(json.dumps(message) + '\n')
            worker.stdin.write((json.dumps(message) + '\n').encode('utf-8'))
            worker.stdin.flush()
            reply = replies.get(timeout=20)
            require(isinstance(reply, dict) and reply['request_id'] == str(serial), repr(reply))
            return reply['result']

        resolved = request('resolve_app', app_id=str(args.fixture_binary.resolve()).lower())
        report['resolved'] = resolved
        app = next(row['app'] for row in resolved['apps'] if row['app']['pid'] == fixture.pid)
        windows = request('windows', app=app)
        report['windows'] = windows
        target = windows['windows'][0]['target']
        for operation in ('press', 'click', 'scroll', 'keypress', 'typeText', 'move', 'drag', 'close_window', 'quit_app'):
            result = request(operation, target=target, binding='not-used-before-admission',
                             element=1, text='MUST-NOT-TYPE', keys=['CTRL', 'A'], path=[], x=10, y=10,
                             delta_x=0, delta_y=1)
            require(result.get('error') == 'desktop_unavailable' and result.get('dispatch') == 'not_dispatched', result)
            report['cases'].append({'operation': operation, 'result': result, 'passed': True})
        after = wait_state(root, lambda state: state['sequence'] > initial['sequence'])
        require(after['text'] == initial['text'] and after['increments'] == 0 and after['saves'] == 0
                and after['left_up'] == 0 and after['right_up'] == 0, after)
        report['business_unchanged'] = after
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        if worker:
            worker.stdin.close()
            try:
                report['worker_exit'] = worker.wait(timeout=8)
                report['passed'] &= report['worker_exit'] == 0
            except BaseException as error:
                worker.terminate()
                worker.wait(timeout=5)
                report.update(passed=False, forced_worker=str(error))
            worker.stdout.close()
        if fixture:
            (root / 'stop').write_text('owned normal close', encoding='utf-8')
            try:
                report['fixture_exit'] = fixture.wait(timeout=8)
                report['passed'] &= report['fixture_exit'] == 0
            except BaseException as error:
                fixture.terminate()
                fixture.wait(timeout=5)
                report.update(passed=False, forced_fixture=str(error))
            fixture.stdin.close()
            fixture.stdout.close()
        for stream in streams:
            stream.close()
        owner.close()
        if desktop:
            desktop.close()
            report['owned_desktop_removed'] = not desktop.exists()
            report['passed'] &= report['owned_desktop_removed']
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
