#!/usr/bin/env python3
"""Fresh Gemini acceptance of screenshot rectangle -> AX candidate -> effect."""

import hashlib
import json
import os
from pathlib import Path
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
    root = Path('/private/tmp') / f'dtw-cua-region-agent-{uuid.uuid4().hex[:8]}'
    root.mkdir(mode=0o700)
    project = root / 'project'
    project.mkdir()
    plugin = project / '.agents/plugins/desktop-world'
    subprocess.run([str(package / 'bin/dtw'), 'install', 'antigravity', '--project',
                    str(project)], check=True, stdout=subprocess.DEVNULL)
    with (root / 'plugin-validate.txt').open('w') as out:
        subprocess.run(['agy', 'plugin', 'validate', str(plugin)], stdout=out,
                       stderr=subprocess.STDOUT, check=True)
    html = root / 'region.html'
    html.write_text('<!doctype html><meta charset="utf-8"><title>DTW Region Agent</title>'
                    '<style>body{font:24px system-ui;margin:35px}.choices{display:flex;gap:70px}'
                    'button{font:20px system-ui;padding:22px}.left{width:300px;display:grid;'
                    'gap:9px}.target{background:#a4e9ae;border:4px solid #168840}</style>'
                    '<h1 id="status">READY-REGION-AGENT</h1><div class="choices">'
                    '<div class="left"><button>Archive</button><button>Preview</button>'
                    '<button>Export</button><button>Cancel</button></div>'
                    '<button class="target" onclick="document.getElementById(\'status\').'
                    'textContent=\'CLICKED-REGION-AGENT\'">Choose green target</button></div>')
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
                time.sleep(.1)
        if pid is None:
            raise RuntimeError('temporary Chrome PID not found')
        prompt = (
            '你是首次安装 desktop-world 插件的 Gemini。只读项目内该插件 Skill 和它链接的 api.d.ts、'
            f'RECIPES.md；只调用唯一的 desktop-world_dtw/exec 操作临时 Chrome PID {pid}、'
            '窗口标题精确为 DTW Region Agent。不要操作其他窗口，不用终端、旧 CUA 插件、DOM、CDP、'
            '浏览器专用工具或宿主文件工具看网页内容。页面有左侧多个按钮和右侧绿色目标。'
            '这次重点验收截图圈范围：先取得该窗口的截图，从返回 PNG 像素范围圈住绿色按钮，'
            '按 Skill 的 region candidates 脚本让工具返回候选坐标和元素 token。'
            '核对候选和新截图后，仅点击该绿色按钮一次，最终用新的窗口状态确认'
            ' CLICKED-REGION-AGENT。若返回的坐标或 token 不可靠，请停止并说明具体缺陷，'
            '不要盲目点击/重试。请分别报告实际结果和首次使用者批判性评价：'
            '脚本语法与结果是否易懂、视觉圈定与坐标准确性、调用数、实际用时、失败和重试次数。'
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
                'clientInfo': {'name': 'region-independent-readback', 'version': '1'}}})
            _, after = run_script(proc, 2, 'region-independent-readback',
                                  f'const a=await cua.getApp({{pid:{pid},title:"DTW Region Agent"}});'
                                  'const s=await a.getState({query:"REGION-AGENT",max_elements:40,'
                                  'include_screenshot:false});'
                                  'return {clicked:String(s.tree_markdown).includes('
                                  '"CLICKED-REGION-AGENT"),tree:s.tree_markdown};')
        finally:
            proc.stdin.close()
            proc.wait(timeout=8)
        record = {'package': str(package), 'agent_exit_code': agent.returncode if agent else None,
                  'elapsed_seconds': elapsed, 'config_restore': restore,
                  'config_original_sha256': hashlib.sha256(original).hexdigest(),
                  'config_after_sha256': hashlib.sha256(config_path.read_bytes()).hexdigest(),
                  'agent': summarize(log), 'independent_readback': after}
        path = root / 'record.json'
        path.write_text(json.dumps(record, ensure_ascii=False, indent=2))
        print(json.dumps({'record': str(path), 'elapsed_seconds': elapsed,
                          'run_calls': record['agent']['run_calls'],
                          'failed_receipts': record['agent']['failed_receipts'],
                          'config_restore': restore,
                          'independent_readback': after.get('value')}, ensure_ascii=False), flush=True)
    finally:
        if pid is not None:
            try:
                if exact_chrome_pid(profile) == pid:
                    os.kill(pid, signal.SIGTERM)
            except (subprocess.CalledProcessError, RuntimeError, ProcessLookupError):
                pass


if __name__ == '__main__':
    main()
