#!/usr/bin/env python3
"""Fresh agy CLI creation of a new DOCX from a verified temporary WPS window."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time
import uuid

from evaluate_agent_bilibili import RULE, add_allow, atomic_write, remove_our_allow
from evaluate_agent_usability import summarize
from host import wps_pids


def main():
    if len(sys.argv) != 4:
        raise SystemExit(f'usage: {sys.argv[0]} DTW_BINARY EXISTING_TEMP_DOCX WPS_PID')
    source = Path(sys.argv[1]).resolve()
    binary = source if source.is_file() else source / 'bin/dtw'
    if not binary.is_file():
        raise SystemExit(f'DTW executable not found: {binary}')
    baseline_document = Path(sys.argv[2]).resolve()
    pid = int(sys.argv[3])
    if not baseline_document.is_file() or not baseline_document.is_relative_to('/private/tmp'):
        raise SystemExit('baseline must be an existing temporary WPS test document')
    if pid not in wps_pids():
        raise SystemExit('baseline WPS PID not running')
    handles = subprocess.run(['lsof', '-Fn', '-p', str(pid)], capture_output=True, text=True, check=True)
    if f'n{baseline_document}\n' not in handles.stdout:
        raise SystemExit('baseline WPS PID does not have the specified temporary document open')
    baseline_sha256 = hashlib.sha256(baseline_document.read_bytes()).hexdigest()
    root = Path('/private/tmp') / f'dtw-cua-wps-new-{uuid.uuid4().hex[:8]}'
    root.mkdir(mode=0o700)
    project = root / 'project'
    project.mkdir()
    plugin = project / '.agents/plugins/desktop-world'
    subprocess.run([str(binary), 'install', 'agy', '--project', str(project)],
                   check=True, stdout=subprocess.DEVNULL)
    with (root / 'plugin-validate.txt').open('w') as out:
        subprocess.run(['agy', 'plugin', 'validate', str(plugin)], stdout=out,
                       stderr=subprocess.STDOUT, check=True)
    document = root / 'DTW-AGY-NEW-DOCUMENT.docx'
    marker = 'DTW-AGY-WPS-NEW-20261012'
    prompt = (
        '你是首次安装项目 desktop-world 插件的 Gemini。只读项目内该插件 Skill 及其链接的参考文件，'
        f'只用唯一 `desktop-world_dtw/exec`。起点是 WPS PID {pid} 的临时测试文档窗口 '
        f'`{baseline_document.name}`；该文件位于 `{baseline_document}`，只能用来确认 WPS 身份，'
        '绝对不要读取或修改其正文，也不要在该窗口输入文字。'
        '请通过 WPS 的“新建空白文字文档”界面或快捷键从零新建文档；新文档可能属于另一个 WPS PID，'
        '必须重新列出 WPS 窗口并验证新窗口是空白文档，身份不确定就停止。'
        '仅在确认新空白窗口后，在正文输入单独一行 '
        f'`{marker}`，再通过 WPS 的界面保存为本地 DOCX，绝对路径必须精确为 `{document}`。'
        '操作后从新文档的 WPS 界面读回正文与保存状态。'
        '不要打开、覆盖其他文件，也不要使用终端、宿主文件工具、旧插件、浏览器工具、云文档或微信。'
        '若保存对话框无法确认目标路径、权限不足、窗口身份不明或动作结果 unknown，立即停止并报告原 id；'
        '不要盲目输入或重复保存。请分开报告实机结果及首次使用者评价，包括调用数、失败与重试。'
    )
    config_path = Path.home() / '.gemini/config/config.json'
    original = config_path.read_bytes()
    had_rule = RULE in json.loads(original).get('userSettings', {}).get('globalPermissionGrants', {}).get('allow', [])
    injected = add_allow(original)
    log, err = root / 'agent.jsonl', root / 'agent.stderr'
    print(json.dumps({'root': str(root), 'baseline_pid': pid, 'document': str(document),
                      'log': str(log), 'rule_preexisting': had_rule}), flush=True)
    agent = None
    restore = 'preexisting_rule_unchanged'
    start = time.monotonic()
    try:
        if not had_rule:
            atomic_write(config_path, injected)
        with log.open('w') as out, err.open('w') as stderr:
            agent = subprocess.run(['agy', '--print', prompt, '--model', 'gemini-3.8-flash-high',
                                    '--output-format', 'stream-json', '--print-timeout', '900s'],
                                   cwd=project, stdout=out, stderr=stderr, timeout=930)
    finally:
        if not had_rule:
            restore = remove_our_allow(config_path, original, injected)
    elapsed = round(time.monotonic() - start, 2)
    converted = subprocess.run(['/usr/bin/textutil', '-convert', 'txt', '-stdout', str(document)],
                               capture_output=True, text=True) if document.is_file() else None
    readback = {'document_exists': document.is_file(),
                'textutil_exit_code': converted.returncode if converted else None,
                'marker_saved': marker in converted.stdout if converted else False,
                'baseline_unchanged': hashlib.sha256(baseline_document.read_bytes()).hexdigest() == baseline_sha256,
                'text_preview': converted.stdout[:180] if converted else None,
                'location_entries': sorted(p.name for p in root.iterdir())[:20]}
    record = {'binary': str(binary), 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'agent_exit_code': agent.returncode if agent else None,
              'elapsed_seconds': elapsed, 'config_restore': restore,
              'config_original_sha256': hashlib.sha256(original).hexdigest(),
              'config_after_sha256': hashlib.sha256(config_path.read_bytes()).hexdigest(),
              'agent': summarize(log), 'independent_readback': readback}
    path = root / 'record.json'
    path.write_text(json.dumps(record, ensure_ascii=False, indent=2))
    print(json.dumps({'record': str(path), 'elapsed_seconds': elapsed,
                      'run_calls': record['agent']['run_calls'],
                      'failed_receipts': record['agent']['failed_receipts'],
                      'config_restore': restore, 'readback': readback}, ensure_ascii=False), flush=True)
    if agent.returncode != 0 or not readback['marker_saved'] or not readback['baseline_unchanged']:
        raise RuntimeError(f'WPS new-document acceptance did not reach the target; see {path}')


if __name__ == '__main__':
    main()
