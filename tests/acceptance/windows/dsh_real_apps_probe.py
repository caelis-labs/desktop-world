"""Actual DSH drives owned Explorer/Notepad rename, edit, save and reopen."""
import argparse
import ctypes
from ctypes import wintypes as w
import json
from pathlib import Path
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import artifact_identity, require
from dsh_client import DshClient
from windows.app_protocol_probe import wait
from windows.desktop_probe import children
from windows.transport import ProcessHandle


def windows():
    user = ctypes.WinDLL('user32')
    callback = ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    user.EnumWindows.argtypes = [callback, w.LPARAM]
    user.IsWindowVisible.argtypes = [w.HWND]
    user.GetClassNameW.argtypes = [w.HWND, w.LPWSTR, ctypes.c_int]
    user.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, ctypes.c_int]
    user.GetWindowThreadProcessId.argtypes = [w.HWND, ctypes.POINTER(w.DWORD)]
    rows = []
    @callback
    def visit(hwnd, _):
        if user.IsWindowVisible(hwnd):
            kind, title, pid = ctypes.create_unicode_buffer(256), ctypes.create_unicode_buffer(1024), w.DWORD()
            user.GetClassNameW(hwnd, kind, len(kind))
            user.GetWindowTextW(hwnd, title, len(title))
            user.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            rows.append({'hwnd': int(hwnd), 'pid': pid.value, 'class': kind.value, 'title': title.value})
        return True
    require(user.EnumWindows(visit, 0), 'read-only window inventory failed')
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('dsh', 'package', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    folder = out / ('DTW-owned-folder-' + out.name)
    folder.mkdir()
    source, destination = folder / 'seed.txt', folder / '验收-137.txt'
    source.write_bytes(b'Owned initial document\r\n')
    package = args.package.resolve(strict=True)
    binary = package / 'bin/dtw.exe'
    report = {'passed': False, 'artifact': artifact_identity(binary, package),
        'business_actor': 'actual DSH model; supervisor only launches an owned folder and reads independent state'}
    client = session = owned = explorer_handle = None
    handles = []
    patch = out / 'cordis.patch.yml'
    patch.write_text("- id: acp\n  name: '@deepseek-ai/dsh-acp'\n  config:\n"
        "    provider: deepseek-account\n    model: deepseek-flash\n- insert:\n"
        "    - id: desktop-world-plugin\n      name: '" + (package / 'cordis/index.mjs').as_uri() + "'\n", encoding='utf-8')
    try:
        before = windows()
        require(not any(row['class'].casefold() == 'notepad' for row in before),
            'Existing Notepad document: do not mix user tabs with this owned lifecycle test')
        previous = {row['hwnd'] for row in before}
        explorer = Path('C:/Windows/explorer.exe')
        subprocess.Popen([str(explorer), str(folder)])
        def new_owned():
            matches = [row for row in windows() if row['class'] == 'CabinetWClass'
                and folder.name in row['title'] and row['hwnd'] not in previous]
            require(len(matches) <= 1, 'Owned Explorer window is ambiguous')
            return matches[0] if matches else None
        owned = wait(new_owned, timeout=15)
        explorer_handle = ProcessHandle(owned['pid'], explorer)
        report['owned_explorer'] = {**owned, 'birth': explorer_handle.birth}
        client = DshClient(args.dsh, patch, out / 'dsh', Path.cwd())
        client.call('initialize', {'protocolVersion': 1, 'clientCapabilities': {},
            'clientInfo': {'name': 'dtw-windows-real-apps-acceptance', 'version': '1'}})
        report['session'] = client.call('session/new', {'cwd': str(Path.cwd()), 'mcpServers': []})
        session = report['session']['sessionId']
        client.call('session/set_config_option', {'sessionId': session, 'configId': 'reasoning_effort', 'value': 'high'})
        pending = [client.process.pid]
        while pending:
            for row in children(pending.pop()):
                pending.append(row['pid'])
                if row['exe'].casefold() == 'dtw.exe':
                    try:
                        handles.append(ProcessHandle(row['pid'], binary))
                    except (AssertionError, OSError):
                        continue
        require(handles, 'Actual DSH-owned installed MCP identity absent')
        report['dsh_owned_dtw_processes'] = [{'pid': h.pid, 'birth': h.birth} for h in handles]
        payload = 'Windows 实机验收 中文🙂\n第二行 café 🚀'
        prompt = f'''这是 Windows 真实应用验收。先加载 desktop-world Skill，并读取其安装包中的 API/相关文档。
业务只准使用 desktop-world apps/exec。禁止 shell、文件读写工具、HTTP、CDP、其他 computer-use、脚本化文件系统或读取本轮日志/状态旁路。
我已打开一个全新的自有 Explorer 窗口，文件夹 {folder}，其中只有 seed.txt。每次操作 Explorer 前核对当前窗口确实是这个自有文件夹；不要导航、修改或关闭其他 Explorer 窗口。
1. 在这个 Explorer 窗口内把 seed.txt 重命名为 验收-137.txt。
2. 使用 Microsoft 记事本（Notepad，不是 Notepad++）打开该文件，完整替换为以下两行，保存：
Windows 实机验收 中文🙂
第二行 café 🚀
3. 关闭这个记事本文档，重新打开 {destination}，通过实际原生读取及截图核对两行、中文和 emoji，随后关闭这个自有文档。
4. 回到自有 Explorer 窗口，核对新文件名存在而旧文件名消失，最后只关闭这个自有 Explorer 窗口。不要使用 app.quit() 关闭 Explorer 整个实例，因为它还管理用户其他窗口和桌面。
若记事本出现其他用户文档或无法证明窗口归属，停止相关输入并报告。派发/消费不确定不得重放，先做新的只读观察。报告实际完成与错误。'''
        (out / 'prompt.txt').write_text(prompt, encoding='utf-8')
        started = time.time() * 1000
        report['prompt_result'] = client.call('session/prompt', {'sessionId': session,
            'prompt': [{'type': 'text', 'text': prompt}]}, timeout=900)
        report['prompt_interval'] = {'start_ms': started, 'end_ms': time.time() * 1000}
        require(destination.exists() and not source.exists(), 'Explorer rename did not complete')
        text = destination.read_bytes().decode('utf-8-sig').replace('\r\n', '\n')
        require(text == payload, 'Notepad saved bytes differ')
        report['saved_bytes'] = destination.read_bytes().hex()
        after = windows()
        require(not any(row['hwnd'] == owned['hwnd'] for row in after), 'Owned Explorer window was not closed by the model')
        require(not any(destination.name in row['title'] and row['class'].casefold() == 'notepad' for row in after),
            'Owned Notepad document was not closed by the model')
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        if client:
            if session:
                try:
                    client.call('session/close', {'sessionId': session}, timeout=30)
                except BaseException as error:
                    report.update(passed=False, session_close_error=str(error))
            report['dsh_cleanup'] = client.close()
            report['passed'] &= report['dsh_cleanup']['exit_code'] == 0 and not report['dsh_cleanup']['forced'] and report['dsh_cleanup']['reader_joined']
        report['dtw_cleanup'] = []
        for handle in handles:
            try:
                code = handle.wait(timeout=8)
                report['dtw_cleanup'].append({'pid': handle.pid, 'exit': code})
                report['passed'] &= code == 0
            except BaseException as error:
                report.update(passed=False, dtw_cleanup_error=str(error))
            finally:
                handle.close()
        if owned and explorer_handle:
            # Only a still-identical owned folder window may be normally closed
            # during cleanup. The shared Explorer process is never terminated.
            current = next((row for row in windows() if row == owned), None)
            if current and explorer_handle.alive():
                user = ctypes.WinDLL('user32')
                user.PostMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
                report['supervisor_owned_explorer_close'] = bool(user.PostMessageW(owned['hwnd'], 0x10, 0, 0))
            explorer_handle.close()
        (out / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        (out / 'rpc-intervals.json').write_text(json.dumps([
            {'name': 'actual-DSH-real-apps-prompt', **report['prompt_interval']}
        ] if 'prompt_interval' in report else [], indent=2), encoding='utf-8')
        print(json.dumps({key: report.get(key) for key in ('passed', 'error')}), flush=True)
    require(report['passed'], report)


if __name__ == '__main__':
    main()
