#!/usr/bin/env python3
"""Fresh Gemini acceptance on an isolated, real Chrome window with a large AX tree."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
import uuid

from accept_chrome import exact_chrome_pid, run_script
from evaluate_agent_bilibili import RULE, add_allow, atomic_write, remove_our_allow
from evaluate_agent_usability import summarize
from probe import call


def main():
    package = Path(sys.argv[1]).resolve()
    root = Path('/private/tmp') / f'dtw-cua-visual-agent-{uuid.uuid4().hex[:8]}'
    root.mkdir(mode=0o700)
    project = root / 'project'
    project.mkdir()
    plugin = project / '.agents/plugins/desktop-world'
    subprocess.run([str(package / 'bin/dtw'), 'install', 'antigravity', '--project',
                    str(project)], check=True, stdout=subprocess.DEVNULL)
    with (root / 'plugin-validate.txt').open('w') as out:
        subprocess.run(['agy', 'plugin', 'validate', str(plugin)], stdout=out,
                       stderr=subprocess.STDOUT, check=True)
    html = root / 'visual_large.html'
    shutil.copy2(Path(__file__).parent / 'fixtures/visual_large.html', html)
    profile = root / 'chrome-profile'
    opened = subprocess.run(['open', '-g', '-na', '/Applications/Google Chrome.app', '--args',
                             f'--user-data-dir={profile}', '--no-first-run',
                             '--no-default-browser-check', '--disable-background-mode',
                             '--force-renderer-accessibility', '--disable-translate',
                             f'--app={html.as_uri()}'], capture_output=True, text=True)
    if opened.returncode:
        raise RuntimeError(opened.stderr)
    pid = None
    try:
        for _ in range(100):
            try:
                pid = exact_chrome_pid(profile)
                break
            except (subprocess.CalledProcessError, RuntimeError):
                time.sleep(0.1)
        if pid is None:
            raise RuntimeError('isolated Chrome PID not found')
        prompt = (
            '你是首次接触本项目 desktop-world 插件的 Gemini。只根据项目安装的 Skill、其链接的参考文件和唯一'
            f' `desktop-world_dtw/exec` 工具，操作刚启动的临时 Chrome PID {pid}、标题精确为'
            ' DTW Visual Acceptance 的窗口。不要使用终端、浏览器专用工具、旧 CUA 插件、DOM、CDP 或网页 API。'
            '这个页面有 800 个无关可访问按钮与一个画布。先说明 AX 大树是否截断和默认输出是否可控；'
            '随后用窗口截图找到画布中的红色圆盘，从同一张截图选坐标并点击，重新观察并确认标题出现 VISUAL-CLICKED。'
            '接着用该插件提供的 CUA 原生菜单能力尝试对这个精确 Chrome 窗口执行“重新载入此页”；'
            '菜单路径必须从实际观察确认，不要猜测。确认重载后标题同时包含 VISUAL-CLICKED 与 RELOAD-1。'
            '如果菜单无法可靠定位，保留失败回执并报告，不能改用快捷键却声称菜单通过。'
            '所有动作只限这个临时窗口。若权限拒绝、目标身份不明或动作结果 unknown，停止并保留原 execution_id；'
            '不要盲目重放。完成后从使用者角度批判性评价发现、语法、截图坐标、菜单、结果输出、调用数、'
            '实际用时、失败/重试次数及最需要改进的地方。任务结果与体验评价分开。'
        )
        config_path = Path.home() / '.gemini/config/config.json'
        original = config_path.read_bytes()
        had_rule = RULE in json.loads(original).get('userSettings', {}).get('globalPermissionGrants', {}).get('allow', [])
        injected = add_allow(original)
        log, err = root / 'agent.jsonl', root / 'agent.stderr'
        print(json.dumps({'root': str(root), 'pid': pid, 'plugin': str(plugin),
                          'log': str(log), 'rule_preexisting': had_rule}), flush=True)
        agent = None
        restore = 'preexisting_rule_unchanged'
        start = time.monotonic()
        try:
            if not had_rule:
                atomic_write(config_path, injected)
            with log.open('w') as out, err.open('w') as stderr:
                agent = subprocess.run(['agy', '--print', prompt, '--model', 'gemini-3.8-flash-high',
                                        '--output-format', 'stream-json', '--print-timeout', '480s'],
                                       cwd=project, stdout=out, stderr=stderr, timeout=500)
        finally:
            if not had_rule:
                restore = remove_our_allow(config_path, original, injected)
        elapsed = round(time.monotonic() - start, 2)
        proc = subprocess.Popen([str(package / 'bin/dtw')], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                env={'PATH': '/usr/bin:/bin', 'TMPDIR': '/private/tmp'})
        try:
            call(proc, {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
                'protocolVersion': '2025-06-18', 'capabilities': {},
                'clientInfo': {'name': 'visual-independent-readback', 'version': '1'}}})
            _, after = run_script(proc, 2, 'visual-independent-readback',
                                  f'const a=await cua.getApp({{pid:{pid},title:"DTW Visual Acceptance"}});'
                                  'const s=await a.getState({query:"VISUAL",max_elements:30,include_screenshot:false});'
                                  'return {window_id:a.windowId,title:a.title,tree:s.tree_markdown,'
                                  'truncated:s.truncated,element_count:s.element_count};')
        finally:
            proc.stdin.close()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        record = {'package': str(package), 'agent_exit_code': agent.returncode if agent else None,
                  'elapsed_seconds': elapsed, 'config_restore': restore,
                  'config_original_sha256': hashlib.sha256(original).hexdigest(),
                  'config_after_sha256': hashlib.sha256(config_path.read_bytes()).hexdigest(),
                  'agent': summarize(log), 'independent_readback': after}
        (root / 'record.json').write_text(json.dumps(record, ensure_ascii=False, indent=2))
        print(json.dumps({'record': str(root / 'record.json'), 'elapsed_seconds': elapsed,
                          'run_calls': record['agent']['run_calls'],
                          'failed_receipts': record['agent']['failed_receipts'],
                          'config_restore': restore, 'independent_readback': after.get('value')},
                         ensure_ascii=False), flush=True)
    finally:
        if pid is not None:
            try:
                if exact_chrome_pid(profile) == pid:
                    os.kill(pid, signal.SIGTERM)
            except (subprocess.CalledProcessError, RuntimeError, ProcessLookupError):
                pass
            for _ in range(50):
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    if profile.exists():
                        shutil.rmtree(profile)
                    break
                time.sleep(0.1)


if __name__ == '__main__':
    main()
