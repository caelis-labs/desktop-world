"""Actual DSH drives owned Explorer/Notepad rename, edit, save and reopen."""
import argparse
import ctypes
from ctypes import wintypes as w
import json
from pathlib import Path
import subprocess
import sys
import threading
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
    parser.add_argument('--owned-notepad-window', type=int, required=True,
        help='Fresh single-tab Untitled window prepared and visually checked by the supervisor; other user windows stay open')
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
    title_events, title_errors = [], []
    title_stop = threading.Event()
    title_thread = None
    patch = out / 'cordis.patch.yml'
    patch.write_text("- id: acp\n  name: '@deepseek-ai/dsh-acp'\n  config:\n"
        "    provider: deepseek-account\n    model: deepseek-flash\n- insert:\n"
        "    - id: desktop-world-plugin\n      name: '" + (package / 'cordis/index.mjs').as_uri() + "'\n", encoding='utf-8')
    try:
        before = windows()
        owned_notepad = [row for row in before if row['hwnd'] == args.owned_notepad_window
            and row['class'].casefold() == 'notepad' and row['title'] == 'Untitled - Notepad']
        require(len(owned_notepad) == 1, 'Prepared owned Untitled window is absent or changed')
        report['prepared_notepad'] = owned_notepad[0]
        report['other_notepad_windows_before'] = [row['hwnd'] for row in before
            if row['class'].casefold() == 'notepad' and row['hwnd'] != args.owned_notepad_window]
        def observe_titles():
            last = None
            try:
                while not title_stop.is_set():
                    current = next((row for row in windows() if row['hwnd'] == args.owned_notepad_window), None)
                    if current != last:
                        title_events.append({'time_ms': time.time() * 1000, 'window': current})
                        last = current
                    title_stop.wait(.01)
            except BaseException as error:
                title_errors.append(type(error).__name__ + ': ' + str(error))
        title_thread = threading.Thread(target=observe_titles, daemon=True)
        title_thread.start()
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
已另外准备一个全新、只有一个空白标签的 Untitled - Notepad 窗口。其他记事本窗口及其恢复的用户标签必须保留，绝不能切入、修改、保存或关闭它们。不要调用 Notepad app.quit()。
1. 在这个 Explorer 窗口内把 seed.txt 重命名为 验收-137.txt。
2. 使用已准备的空白 Microsoft 记事本窗口打开该文件。每个有输入的 exec 开头都先 await app.look({{full:true}})，检查 windows[0].title 是当前步骤预期的自有标题，否则立即抛错停止输入。打开文件前仅允许 Untitled - Notepad，打开后必须是 验收-137.txt - Notepad 或 *验收-137.txt - Notepad。Open 面板需等新的只读观察确认已出现，并同时核对其后是这个自有窗口；不要在被模态禁用的父窗口中继续键入。禁止 Alt+Tab、Windows 键、全局窗口切换或任何用户文档操作。
先从当前自有 window 的 document 元素使用 setValue({{target:观察到的ID,value:''}}) 完整清空文档（本机记事本全选替换可能保留尾部真实换行，不能将该换行裁掉作成功）。在同一 exec 内使用 typeText 输入完整以下两行，再 Ctrl+S 保存。不要靠连续按 Enter 拼接两行：
Windows 实机验收 中文🙂
第二行 café 🚀
3. 只关闭这个自有记事本文档，回到原空白 Untitled 标签，重新打开 {destination}，通过实际原生读取及截图核对恰好两行、中文和 emoji，以及没有额外尾部换行，随后只关闭这个自有文档，保留空白窗口。每次输入前仍需在同个 exec 核对自有窗口标题。Document 元素 ID 必须属于已核对的 windows[0].id，不得使用其他窗口的元素。
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
        require(all(any(row['hwnd'] == hwnd for row in after)
            for hwnd in report['other_notepad_windows_before']), 'An existing user Notepad window was closed')
        expected_titles = iter(['Untitled - Notepad', '验收-137.txt - Notepad',
            'Untitled - Notepad', '验收-137.txt - Notepad', 'Untitled - Notepad'])
        expected_title = next(expected_titles)
        for event in title_events:
            if event['window'] and event['window']['title'] == expected_title:
                expected_title = next(expected_titles, None)
        require(expected_title is None, 'Independent owned window close/reopen/close sequence absent')
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        title_stop.set()
        if title_thread:
            title_thread.join(timeout=3)
            report['title_witness'] = {'events': title_events, 'errors': title_errors,
                'joined': not title_thread.is_alive()}
            report['passed'] &= not title_thread.is_alive() and not title_errors
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
