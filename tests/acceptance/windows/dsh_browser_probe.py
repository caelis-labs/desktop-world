"""Owned Chromium profile and local web/canvas/vision tasks through actual DSH."""
import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import artifact_identity, require
from dsh_client import DshClient
from windows.browser_feature_audit import owned_browser_pid, close_owned_browser
from windows.desktop_probe import children
from windows.transport import ProcessHandle, read


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dsh', type=Path, required=True)
    parser.add_argument('--package', type=Path, required=True)
    parser.add_argument('--browser', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    package = args.package.resolve(strict=True)
    browser = args.browser.resolve(strict=True)
    report = {'passed': False, 'harness': 'installed DSH public ACP profile',
        'artifact': artifact_identity(package / 'bin/dtw.exe', package),
        'business_actor': 'DSH model through full Cordis plugin; no direct GUI supervisor input'}
    path = Path(__file__).resolve().parents[2] / 'fixtures/computer-use-app/fixture.py'
    spec = importlib.util.spec_from_file_location('dsh_web_fixture', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    fixture = module.ComputerUseFixture(out / 'fixture', 'dsh-browser')
    client = browser_handle = launcher = None
    session = browser_pid = None
    handles = []
    navigation = []
    navigation_lock = threading.Lock()
    patch = out / 'cordis.patch.yml'
    patch.write_text("- id: acp\n  name: '@deepseek-ai/dsh-acp'\n  config:\n"
        "    provider: deepseek-account\n    model: deepseek-flash\n- insert:\n"
        "    - id: desktop-world-plugin\n      name: '" + (package / 'cordis/index.mjs').as_uri() + "'\n", encoding='utf-8')
    try:
        fixture.prepare(build=False)
        handler = fixture.servers[1][0].RequestHandlerClass
        original_get = handler.do_GET
        def observe_navigation(request):
            if request.headers.get('Sec-Fetch-Dest') == 'document':
                with navigation_lock:
                    navigation.append({'path': request.path.split('?', 1)[0], 'time_ms': time.time() * 1000})
            return original_get(request)
        handler.do_GET = observe_navigation
        # The unmodified portable fixture owns the pages. Bind the new kernel
        # process, fresh profile and exact loopback URL without user tabs.
        child = subprocess.Popen([str(browser), '--user-data-dir=' + str(out / 'browser-profile'),
            '--no-first-run', '--no-default-browser-check', '--disable-sync', '--disable-background-mode',
            '--guest', '--force-renderer-accessibility', '--new-window', '--window-size=1200,950', fixture.web_url])
        launcher = ProcessHandle(child.pid, browser)
        until = time.monotonic() + 15
        while browser_pid is None:
            browser_pid = owned_browser_pid(browser, 'Release operations desk', launcher.birth)
            require(time.monotonic() < until, 'Owned browser did not publish its initial title')
            time.sleep(.03)
        browser_handle = ProcessHandle(browser_pid, browser)
        report['browser'] = {'path': str(browser), 'pid': browser_pid, 'birth': browser_handle.birth,
            'profile': str(out / 'browser-profile'), 'url': fixture.web_url}
        client = DshClient(args.dsh, patch, out / 'dsh', Path.cwd())
        client.call('initialize', {'protocolVersion': 1, 'clientCapabilities': {},
            'clientInfo': {'name': 'dtw-windows-browser-acceptance', 'version': '1'}})
        report['session'] = client.call('session/new', {'cwd': str(Path.cwd()), 'mcpServers': []})
        session = report['session']['sessionId']
        client.call('session/set_config_option', {'sessionId': session, 'configId': 'reasoning_effort', 'value': 'high'})
        pending = [client.process.pid]
        while pending:
            for row in children(pending.pop()):
                pending.append(row['pid'])
                if row['exe'].casefold() == 'dtw.exe':
                    try:
                        handles.append(ProcessHandle(row['pid'], package / 'bin/dtw.exe'))
                    except (AssertionError, OSError):
                        continue
        require(handles, 'Actual DSH-owned installed MCP identity absent')
        report['dsh_owned_dtw_processes'] = [{'pid': h.pid, 'birth': h.birth} for h in handles]
        payload = 'DSH Browser 中文🙂 café 🚀'
        message = '交付确认 中文🙂'
        prompt = f'''这是 Windows 实机验收。先加载本完整插件的 desktop-world Skill，并读取安装包内 API/相关 Skill 文档。
业务操作只能使用 desktop-world apps/exec。禁止 shell、文件读写、HTTP、CDP、Playwright、其他 computer-use 或读取 fixture 源码/状态/事件等旁路。
只操作我拥有的 {browser} 新建独立 profile 窗口，其网址为 {fixture.web_url}，起始标题 Release operations desk。
每次输入前核对当前窗口属于这个 loopback 网站；不要操作其他浏览器窗口、用户 profile 或其他文件。遇到未知派发/消费不得重放。
完成这些目标：
1. 在 Web note 输入完整内容“{payload}”，Commit web note 恰好一次。
2. Load noisy catalogue，找到并激活 Audit checkpoint 137 恰好一次；完整分段读取 Web reference 到 UTF-16 EOF，报告总长度和末尾标记。
3. 打开 New review record，先尝试空表保存以检查必填校验，不得产生记录；随后填写 Review note 为 Windows 验收137、quantity 为137、Approved 勾选、Priority为Urgent、日期2026-11-07，Save review 恰好一次。重新打开表单检查后取消。
4. 在此网站内实际连续导航四次：Canvas chat → Operations desk → Image challenge → Operations desk；核对每次目的页已加载。
5. 打开 Canvas chat。通过真实截图定位绘制的 Release check 137 消息，在其右键菜单中 Pin；发送一次“{message}”。不要从 AX 标签猜测 canvas 消息坐标。
6. 打开 Image challenge，只从 DTW capture 图片读出八个图形字符，填写并 Verify image 恰好一次，不得读取图片文件或隐藏答案。
最后捕获图片，报告完成情况、实际错误和不确定项；不要关闭其他浏览器窗口。'''
        (out / 'prompt.txt').write_text(prompt, encoding='utf-8')
        started = time.time() * 1000
        report['prompt_result'] = client.call('session/prompt', {'sessionId': session,
            'prompt': [{'type': 'text', 'text': prompt}]}, timeout=1200)
        report['prompt_interval'] = {'start_ms': started, 'end_ms': time.time() * 1000}
        state = read(fixture.output / 'web/state.json')
        require(state, 'Web oracle absent')
        report['final_state'] = state
        require(state['note'] == payload and len(state['saves']) == 1, 'Web note or save count differs')
        require(state['checkpoint_hits'] == [{'id': 137, 'activation': 1}], 'Checkpoint effect differs')
        require(len(state['records']) == 1 and state['records'][0]['note'] == 'Windows 验收137'
            and state['records'][0]['number'] == 137 and state['records'][0]['approved'] is True
            and state['records'][0]['priority'] == 'Urgent' and state['records'][0]['date'] == '2026-11-07', 'Review record differs')
        events = [json.loads(line) for line in (fixture.output / 'web/events.jsonl').read_text(encoding='utf-8').splitlines()]
        saved_event = next(row for row in events if row['type'] == '/api/record' and not row['error'])
        observations = [row for row in events if row['type'] == '/api/observation' and not row['error']]
        require(any(row['seq'] < saved_event['seq'] and row['data']['review']['invalid'] > 0
            for row in observations), 'Actual empty-form validation not observed before the save')
        reopened = next((row for row in observations if row['seq'] > saved_event['seq']
            and row['data']['review']['open']), None)
        require(reopened and any(row['seq'] > reopened['seq'] and not row['data']['review']['open']
            for row in observations), 'Reopened form and later cancellation not observed')
        release = next(row for row in state['chat']['messages'] if row['id'] == 'release-137')
        require(release['pinned'], 'Canvas release was not pinned')
        require(sum(row['text'] == message for row in state['chat']['messages']) == 1, 'Canvas message count differs')
        require(state['chat']['actions'] == [{'action':'pin','id':'release-137'}, {'action':'send','text':message}],
            'Canvas action journal differs or contains duplicate effects')
        require(len(state['vision']['attempts']) == 1 and state['vision']['attempts'][0].get('passed'), 'Image challenge did not pass exactly once')
        paths = [row['path'] for row in navigation]
        require(any(paths[index:index + 4] == ['/canvas', '/', '/vision', '/'] for index in range(len(paths))),
            {'four_consecutive_document_navigations_missing': paths})
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        if client:
            if session:
                try:
                    report['session_close'] = client.call('session/close', {'sessionId': session}, timeout=30)
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
        if browser_handle:
            # Normal close of only the kernel-proven disposable browser process.
            try:
                close_owned_browser(browser_pid, browser, browser_handle.birth,
                    ('Release operations desk', 'Canvas release chat', 'Image verification'))
                code = browser_handle.wait(timeout=8)
                report['browser_cleanup'] = {'pid': browser_pid, 'exit': code}
                report['passed'] &= code == 0
            except BaseException as error:
                report.update(passed=False, browser_cleanup_error=str(error))
            finally:
                browser_handle.close()
        if launcher:
            launcher.close()
        report['fixture_cleanup'] = fixture.close()
        report['passed'] &= report['fixture_cleanup']['passed']
        report['navigation'] = navigation
        (out / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        (out / 'rpc-intervals.json').write_text(json.dumps([
            {'name': 'actual-DSH-browser-prompt', **report['prompt_interval']}
        ] if 'prompt_interval' in report else [], indent=2), encoding='utf-8')
        print(json.dumps({key: report.get(key) for key in ('passed', 'error', 'prompt_result')}), flush=True)
    require(report['passed'], report)


if __name__ == '__main__':
    main()
