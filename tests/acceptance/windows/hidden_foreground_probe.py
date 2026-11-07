"""Cold keyboard consumption with a naturally existing hidden foreground.

Never activates/stops GameInput or changes foreground policies. The owned test
window is merely shown without activation; DTW must prepare it automatically.
"""
import argparse
import ctypes
from ctypes import wintypes as w
import json
from pathlib import Path
import shutil
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import AppClient, artifact_identity, require
from windows.app_protocol_probe import wait_state
from windows.desktop_probe import children
from windows.transport import ProcessHandle


def foreground():
    user = ctypes.WinDLL('user32')
    user.GetForegroundWindow.restype = w.HWND
    user.GetClassNameW.argtypes = [w.HWND,w.LPWSTR,ctypes.c_int]
    user.GetWindowThreadProcessId.argtypes = [w.HWND,ctypes.POINTER(w.DWORD)]
    user.IsWindowVisible.argtypes = [w.HWND]
    hwnd, kind, pid = user.GetForegroundWindow(), ctypes.create_unicode_buffer(256), w.DWORD()
    thread = user.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    user.GetClassNameW(hwnd, kind, len(kind))
    return {'hwnd': int(hwnd or 0), 'pid': pid.value, 'thread': thread,
        'class': kind.value, 'visible': bool(user.IsWindowVisible(hwnd))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('dtw', 'fixture-binary', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--installed-package', type=Path)
    parser.add_argument('--release-control', action='store_true')
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    report = {'passed': False, 'status': 'not_run', 'artifact': artifact_identity(args.dtw, args.installed_package),
        'scope': 'direct cold keyboard diagnostic, separate from DSH model acceptance',
        'before': foreground()}
    child = client = None
    handles = []
    root = out / 'fixture'
    root.mkdir()
    try:
        if report['before']['visible'] or report['before']['class'] != 'GameInputServiceWindow':
            report['reason'] = 'Naturally existing hidden GameInput foreground absent; no service or foreground manipulation performed'
            return
        (root / 'show-without-activation').write_text('owned background setup', encoding='utf-8')
        exe = out / 'HiddenForegroundFixture.exe'
        shutil.copy2(args.fixture_binary, exe)
        child = subprocess.Popen([str(exe), str(root), 'A'])
        initial = wait_state(root, lambda value: value['ready'])
        report['before_input'] = foreground()
        require(report['before_input'] == report['before'] and not initial['foreground'], 'Owned fixture masked the hidden-foreground precondition')
        client = AppClient(args.dtw.resolve(), out / 'protocol')
        payload = 'Hidden foreground 中文🙂\r\npaired Ctrl+S café 🚀'
        result, response = client.execute(exe,
            'await app.keypress("CTRL+A");await app.typeText(' + json.dumps(payload) + ');await app.keypress("CTRL+S");', allow_error=True)
        report['response'] = result
        report['after_input'] = foreground()
        state = wait_state(root, lambda value: True)
        if args.release_control:
            require(response.get('isError') and state['text'] == '' and state['saves'] == 0, 'Old release unexpectedly consumed keyboard')
            require('did not become foreground' in result.get('error', {}).get('message', ''), result)
            report['status'] = 'reproduced_release_refusal'
        else:
            require(not response.get('isError'), result)
            state = wait_state(root, lambda value: value['saves'] == 1 and value['text'] == payload)
            require((root/'saved.txt').read_bytes().decode('utf-8') == payload, 'Saved bytes differ')
            require(not state['keys_down_global'], 'Global release not proven by fixture')
            report['status'] = 'keyboard_consumption_and_release_proven'
        report['final_state'] = state
        pending = [client.transport.proc.pid]
        while pending:
            for row in children(pending.pop()):
                if row['exe'].casefold() == args.dtw.name.casefold():
                    handles.append(ProcessHandle(row['pid'], args.dtw.resolve()))
                    pending.append(row['pid'])
        report['passed'] = True
    except BaseException as error:
        report['status'] = 'failed'
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        if client:
            report['mcp_cleanup'] = client.close()
        report['workers'] = []
        for handle in handles:
            try:
                code = handle.wait(timeout=8)
                report['workers'].append({'pid':handle.pid,'exit':code})
                report['passed'] &= code == 0
            finally:
                handle.close()
        if child:
            (root/'stop').write_text('owned normal close', encoding='utf-8')
            report['fixture_exit'] = child.wait(timeout=8)
            report['passed'] &= child.returncode == 0
        (out/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(json.dumps({key:report.get(key) for key in ('passed','status','reason','error')}),flush=True)


if __name__ == '__main__':
    main()
