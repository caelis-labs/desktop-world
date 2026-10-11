#!/usr/bin/env python3
"""Fresh Gemini editing a copied DOCX in a private WPS process using one MCP tool."""

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

from host import wps_pids
from evaluate_agent_bilibili import RULE, add_allow, atomic_write, remove_our_allow
from evaluate_agent_usability import summarize


def main():
    package = Path(sys.argv[1]).resolve()
    fixture = Path(sys.argv[2]).resolve()
    if not fixture.is_file():
        raise SystemExit(f"usage: {sys.argv[0]} PACKAGE EXISTING_TEST_DOCX")
    root = Path('/private/tmp') / f'dtw-cua-wps-agent-{uuid.uuid4().hex[:8]}'
    root.mkdir(mode=0o700)
    project = root / 'project'
    project.mkdir()
    plugin = project / '.agents/plugins/desktop-world'
    subprocess.run([str(package / 'bin/dtw'), 'install', 'antigravity', '--project',
                    str(project)], check=True, stdout=subprocess.DEVNULL)
    with (root / 'plugin-validate.txt').open('w') as out:
        subprocess.run(['agy', 'plugin', 'validate', str(plugin)], stdout=out,
                       stderr=subprocess.STDOUT, check=True)
    document = root / 'DTW-CUA-WPS-ACCEPTANCE.docx'
    shutil.copy2(fixture, document)
    original_sha = hashlib.sha256(document.read_bytes()).hexdigest()
    prior_pids = wps_pids()
    opened = subprocess.run(['open', '-g', '-na', '/Applications/wpsoffice.app', str(document)],
                            capture_output=True, text=True)
    if opened.returncode:
        raise RuntimeError(opened.stderr)
    pid = None
    try:
        for _ in range(100):
            fresh = wps_pids() - prior_pids
            if len(fresh) == 1:
                pid = fresh.pop()
                break
            time.sleep(0.1)
        if pid is None:
            raise RuntimeError('temporary WPS PID not isolated')
        marker = 'DTW-CUA-GEMINI-ACCEPT-20261011'
        prompt = (
            '你是首次使用本项目 desktop-world 插件的 Gemini。只根据项目安装的 Skill、其链接的参考文件和'
            f'唯一 `desktop-world_dtw/exec` MCP 工具，操作新启动的临时 WPS PID {pid} 中的'
            f' `{document.name}`。这是独立副本，绝对路径 {document}。'
            '先确认窗口身份和文档可见的 Original line，然后在文档正文末尾插入一行'
            f' `{marker}`，保留原文并保存到这个副本。'
            '请自己从 Skill 判断是用语义控件还是截图坐标，动作后读回可见状态；保存回执不能代替磁盘验证。'
            '只操作这个临时 WPS 窗口；不要用终端、旧 CUA 插件、浏览器工具或碰其他文档、微信、私人内容。'
            '如果目标身份、输入落点或动作结果无法确认，停止并报告原 execution_id，不要重复输入。'
            '最后分别报告实际任务结果与首次使用者的批判性评价：发现、脚本语法、截图/输入、结果输出、'
            '调用数、实际用时、失败/重试次数和最需改进的地方。'
        )
        config_path = Path.home() / '.gemini/config/config.json'
        original_config = config_path.read_bytes()
        had_rule = RULE in json.loads(original_config).get('userSettings', {}).get('globalPermissionGrants', {}).get('allow', [])
        injected = add_allow(original_config)
        log, err = root / 'agent.jsonl', root / 'agent.stderr'
        print(json.dumps({'root': str(root), 'pid': pid, 'document': str(document),
                          'plugin': str(plugin), 'log': str(log), 'rule_preexisting': had_rule}),
              flush=True)
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
                restore = remove_our_allow(config_path, original_config, injected)
        elapsed = round(time.monotonic() - start, 2)
        converted = subprocess.run(['/usr/bin/textutil', '-convert', 'txt', '-stdout', str(document)],
                                   capture_output=True, text=True)
        readback = {'textutil_exit_code': converted.returncode,
                    'marker_saved': marker in converted.stdout,
                    'original_preserved': 'Original line' in converted.stdout,
                    'before_sha256': original_sha,
                    'after_sha256': hashlib.sha256(document.read_bytes()).hexdigest(),
                    'text_preview': converted.stdout[:180]}
        record = {'package': str(package), 'agent_exit_code': agent.returncode if agent else None,
                  'elapsed_seconds': elapsed, 'config_restore': restore,
                  'config_original_sha256': hashlib.sha256(original_config).hexdigest(),
                  'config_after_sha256': hashlib.sha256(config_path.read_bytes()).hexdigest(),
                  'agent': summarize(log), 'independent_readback': readback}
        (root / 'record.json').write_text(json.dumps(record, ensure_ascii=False, indent=2))
        print(json.dumps({'record': str(root / 'record.json'), 'elapsed_seconds': elapsed,
                          'run_calls': record['agent']['run_calls'],
                          'failed_receipts': record['agent']['failed_receipts'],
                          'config_restore': restore, 'readback': readback},
                         ensure_ascii=False), flush=True)
    finally:
        if pid is not None and pid in wps_pids():
            os.kill(pid, signal.SIGTERM)
            for _ in range(40):
                if pid not in wps_pids():
                    break
                time.sleep(0.1)
            if pid in wps_pids():
                os.kill(pid, signal.SIGKILL)


if __name__ == '__main__':
    main()
