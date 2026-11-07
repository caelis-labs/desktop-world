"""Actual installed DSH + full Cordis package, with owned native GUI oracles.

The supervisor only launches fixtures, observes state and sends public ACP user
prompts/cancel. It never performs business input through AppClient or OS APIs.
"""
import argparse
import ctypes
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import artifact_identity, require
from dsh_client import DshClient
from windows.app_protocol_probe import wait, wait_state
from windows.desktop_probe import children
from windows.transport import ProcessHandle, read


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dsh', type=Path, required=True)
    parser.add_argument('--package', type=Path, required=True)
    parser.add_argument('--fixture-binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--mode', choices=('native', 'stop'), required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    package = args.package.resolve(strict=True)
    binary = package / 'bin/dtw.exe'
    report = {'passed': False, 'mode': args.mode, 'harness': 'installed DSH public ACP profile',
        'artifact': artifact_identity(binary, package), 'cases': [],
        'business_actor': 'DSH model through bundled Skill and full Cordis plugin'}
    patch = out / 'cordis.patch.yml'
    patch.write_text("- id: acp\n  name: '@deepseek-ai/dsh-acp'\n  config:\n"
        "    provider: deepseek-account\n    model: deepseek-flash\n- insert:\n"
        "    - id: desktop-world-plugin\n      name: '" + (package / 'cordis/index.mjs').as_uri() + "'\n", encoding='utf-8')
    fixture = child = client = None
    handles = []
    session = None
    stop_thread = None
    cancel_done = threading.Event()
    stop_errors = []
    try:
        if args.mode == 'native':
            path = Path(__file__).resolve().parents[2] / 'fixtures/computer-use-app/fixture.py'
            spec = importlib.util.spec_from_file_location('dsh_owned_fixture', path)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            fixture = module.ComputerUseFixture(out / 'fixture', 'dsh-native')
            fixture.prepare(build=False)
            fixture.build(compiled_native=args.fixture_binary)
            fixture.start()
            exe = fixture.bundle
            payload = 'DSH Windows 中文🙂\n第二行 café 🚀'
            destination = fixture.output / 'dsh-owned-document.txt'
            goal = f'''仅操作我拥有的应用 {exe}，标题 DTW Computer Use Windows。
将 Document editor 的完整文档设为以下两行（保持中文与 emoji）：
DSH Windows 中文🙂
第二行 café 🚀
勾选 Reviewed，选择 Checkpoint 137。通过 Workbench File 原生菜单执行 Commit document，恰好一次。
完整分段读取 Long native reference 到 UTF-16 EOF，报告总长度与末尾标记，不能只读截断预览。
打开并关闭 owned modal。打开 Open document 面板并取消，证明未打开文件。
再通过 Save document as 的原生面板保存到 {destination}，打开该文件，核验两行文字。
再次另存为同一路径，在覆盖确认中选择否，然后取消 Save 面板；最终成功保存和成功打开应各为一次。
最后捕获图片，依据图片核对两行排版，并报告实际完成情况与遇到的错误。'''
        else:
            root = out / 'fixture'
            root.mkdir()
            exe = out / 'StopFixture.exe'
            shutil.copy2(args.fixture_binary, exe)
            child = subprocess.Popen([str(exe), str(root), 'A'])
            initial = wait_state(root, lambda value: value['ready'])
            goal = f'''仅操作我拥有的应用 {exe}，标题 DTW Windows Smoke A。
先观察和截图，在蓝色 Owned pointer / wheel pad 的中央区域内做一次实际按住左键的折线拖拽：
规划八个交替点，每段图像距离约 150–200 像素，总路径遵守 API 的 256 样本上限，所有点留在蓝色区域内。随后再点击 Increment 一次。
本轮评估会在实际拖拽途中由用户取消。正常开始拖拽即可，不要替用户取消，不要模拟取消结果。'''
        common = '''这是 Windows 实机验收。先加载随完整插件注册的 desktop-world Skill，并读取其安装目录中的 API。
业务操作只能使用 desktop-world 的 apps/exec。skill/read 只准读取该安装包的 Skill/API 文档。
禁止 shell、PowerShell、Python、文件写工具、HTTP、CDP、Playwright、其他 computer-use、读取 fixture 源码/配置/状态/事件等旁路。
不要操作其他窗口或文件。只按实际观察到的内容执行，派发或消费不确定的动作禁止重放；先做新观察。
工具回复只证明其声明的事实，最终结论必须保留错误和不确定项。\n'''
        prompt = common + goal
        (out / 'prompt.txt').write_text(prompt, encoding='utf-8')
        client = DshClient(args.dsh, patch, out / 'dsh', Path.cwd())
        report['initialize'] = client.call('initialize', {'protocolVersion': 1,
            'clientCapabilities': {}, 'clientInfo': {'name': 'dtw-windows-dsh-acceptance', 'version': '1'}})
        created = client.call('session/new', {'cwd': str(Path.cwd()), 'mcpServers': []})
        session = created['sessionId']
        report['session'] = created
        client.call('session/set_config_option', {'sessionId': session,
            'configId': 'reasoning_effort', 'value': 'high'})
        pending = [client.process.pid]
        while pending:
            for row in children(pending.pop()):
                pending.append(row['pid'])
                if row['exe'].casefold() == binary.name.casefold():
                    try:
                        handle = ProcessHandle(row['pid'], binary)
                    except (AssertionError, OSError):
                        continue
                    handles.append(handle)
        require(handles, 'DSH did not own the exact installed MCP executable')
        report['dsh_owned_dtw_processes_before'] = [{'pid': h.pid, 'birth': h.birth} for h in handles]
        if args.mode == 'stop':
            user = ctypes.WinDLL('user32')
            user.GetAsyncKeyState.argtypes = [ctypes.c_int]
            user.GetAsyncKeyState.restype = ctypes.c_short
            require(user.GetAsyncKeyState(1) >= 0, 'left button already held')
            def stop_when_dragging():
                until = time.monotonic() + 480
                try:
                    while time.monotonic() < until and not cancel_done.is_set():
                        state = read(root / 'state.json')
                        if state and state['pid'] == child.pid and state['drag_moves'] > 0 and user.GetAsyncKeyState(1) < 0:
                            report['held_before_cancel'] = state
                            report['cancel_sent_ms'] = time.time() * 1000
                            client.send({'jsonrpc': '2.0', 'method': 'session/cancel', 'params': {'sessionId': session}})
                            return
                        time.sleep(.002)
                    stop_errors.append('No owned actual held drag observed before prompt settled')
                except BaseException as error:
                    stop_errors.append(str(error))
            stop_thread = threading.Thread(target=stop_when_dragging, daemon=True)
            stop_thread.start()
        started = time.time() * 1000
        report['prompt_result'] = client.call('session/prompt', {'sessionId': session,
            'prompt': [{'type': 'text', 'text': prompt}]}, timeout=600)
        report['prompt_interval'] = {'start_ms': started, 'end_ms': time.time() * 1000}
        cancel_done.set()
        if stop_thread:
            stop_thread.join(timeout=3)
            require(not stop_thread.is_alive() and not stop_errors, stop_errors)
            require('held_before_cancel' in report, 'cancel did not coincide with a held drag')
            require(report['prompt_result'].get('stopReason') == 'cancelled', report['prompt_result'])
            released = wait_state(root, lambda value: 1 not in value['keys_down_global']
                and value['left_up'] == initial['left_up'] + 1)
            require(released['increments'] == 0, 'suffix reached fixture after user cancellation')
            report['after_cancel'] = released
            report['same_dtw_processes_after_cancel'] = [{'pid': h.pid, 'live': h.alive()} for h in handles]
            require(all(h.alive() for h in handles), 'MCP was replaced during user cancellation')
            recovery = common + f'''继续同一会话，只操作 {exe}。上次已由用户取消，禁止重放拖拽或旧点击。
先通过新的 apps/exec 只读观察并截图当前状态。然后再开始一条新的 exec，将 Message 完整设为 fresh after DSH user cancel，点击 Save 一次，最后读取核对并捕获图片。'''
            recovery_started = time.time() * 1000
            report['recovery_result'] = client.call('session/prompt', {'sessionId': session,
                'prompt': [{'type': 'text', 'text': recovery}]}, timeout=360)
            report['recovery_interval'] = {'start_ms': recovery_started, 'end_ms': time.time() * 1000}
            report['recovery_state'] = wait_state(root, lambda value: value['saves'] == 1
                and value['saved'] == 'fresh after DSH user cancel')
            require(report['recovery_state']['increments'] == 0, 'old suffix was replayed during recovery')
        else:
            state = wait(lambda: fixture.state())
            report['final_state'] = state
            require(state['text'].replace('\r\n', '\n') == payload, 'native document differs')
            require(state['commits'] == 1 and state['reviewed'] and state['checks'] == 1
                and state['checkpoint'] == 137 and not state['modal'], 'native business state differs')
            require(state['saves'] == 1 and state['opens'] == 1, 'file lifecycle counts differ')
            require(destination.read_bytes().decode('utf-8').replace('\r\n', '\n') == payload, 'saved bytes differ')
            events = [json.loads(line) for line in (fixture.output / 'native-events.jsonl').read_text(encoding='utf-8').splitlines()]
            require(sum(event['kind'] == 'menu_commit' for event in events) == 1, 'native menu commit was not independently observed exactly once')
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        cancel_done.set()
        if stop_thread:
            stop_thread.join(timeout=3)
        if client:
            if session:
                try:
                    report['session_close'] = client.call('session/close', {'sessionId': session}, timeout=30)
                except BaseException as error:
                    report.update(passed=False, session_close_error=str(error))
            report['dsh_cleanup'] = client.close()
            report['passed'] &= not report['dsh_cleanup']['forced'] and report['dsh_cleanup']['exit_code'] == 0 and report['dsh_cleanup']['reader_joined']
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
        if fixture:
            report['fixture_cleanup'] = fixture.close()
            report['passed'] &= report['fixture_cleanup']['passed']
        if child:
            (root / 'stop').write_text('owned normal close', encoding='utf-8')
            try:
                report['fixture_exit'] = child.wait(timeout=8)
                report['passed'] &= child.returncode == 0
            except subprocess.TimeoutExpired:
                child.terminate()
                child.wait(timeout=5)
                report.update(passed=False, forced_fixture_cleanup=True)
        (out / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        (out / 'rpc-intervals.json').write_text(json.dumps([
            {'name': 'actual-DSH-' + name, **report[name]} for name in ('prompt_interval', 'recovery_interval') if name in report
        ], indent=2), encoding='utf-8')
        print(json.dumps({key: report.get(key) for key in ('passed', 'error', 'session', 'mode')}), flush=True)
    require(report['passed'], report)


if __name__ == '__main__':
    main()
