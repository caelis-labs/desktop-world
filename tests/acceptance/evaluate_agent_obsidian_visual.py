#!/usr/bin/env python3
"""Fresh Gemini verifying desktop screenshot input on a persistent Obsidian picker."""

import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid

from evaluate_agent_bilibili import RULE, add_allow, atomic_write, remove_our_allow
from evaluate_agent_usability import summarize
from host import obsidian_pid


def main():
    source = Path(sys.argv[1]).resolve()
    binary = source if source.is_file() else source / 'bin/dtw'
    if not binary.is_file():
        raise SystemExit(f'DTW executable not found: {binary}')
    root = Path('/private/tmp') / f'dtw-cua-obsidian-final-{uuid.uuid4().hex[:8]}'
    root.mkdir(mode=0o700)
    project = root / 'project'
    project.mkdir()
    plugin = project / '.agents/plugins/desktop-world'
    subprocess.run([str(binary), 'install', 'agy', '--project',
                    str(project)], check=True, stdout=subprocess.DEVNULL)
    with (root / 'plugin-validate.txt').open('w') as out:
        subprocess.run(['agy', 'plugin', 'validate', str(plugin)], stdout=out,
                       stderr=subprocess.STDOUT, check=True)
    profile = root / 'obsidian-profile'
    location = root / 'vault-location'
    location.mkdir()
    vault_name = 'DTW-AGY-NEW-VAULT'
    opened = subprocess.run(['open', '-g', '-na', '/Applications/Obsidian.app', '--args',
                             f'--user-data-dir={profile}'], capture_output=True, text=True)
    if opened.returncode:
        raise RuntimeError(opened.stderr)
    pid = None
    try:
        for _ in range(100):
            try:
                pid = obsidian_pid(profile)
                break
            except (subprocess.CalledProcessError, RuntimeError):
                time.sleep(0.1)
        if pid is None:
            raise RuntimeError('temporary Obsidian PID not found')
        prompt = (
            '你是首次使用本项目 desktop-world 插件的 Gemini。只根据项目安装的 Skill、其链接的参考文件和唯一'
            f' `desktop-world_dtw/exec` 工具，操作刚启动的独立 Obsidian PID {pid}。'
            '这是临时空白配置。请在它的欢迎页新建名为 '
            f'{vault_name} 的本地仓库，位置必须是测试目录 `{location}`；不得选择别的路径。'
            '需要经过 Obsidian 的“浏览”与系统文件选择器。文件选择器可能持续显示，却无法通过 PID/Window ID 绑定。'
            '请从安装的 Skill 自行选择观察和操作路线；遇到这种情况要确认屏幕实际状态，不能把绑定失败当成弹窗消失。'
            '最终从新仓库 UI 读回名称，并由验收脚本独立检查新目录。不要打开旧仓库或个人笔记。'
            '如果窗口身份不明、被其他 App 遮挡、权限拒绝或动作结果 unknown，停止并报告原 id；'
            '不要对旧坐标或未知动作重试。桌面截图仅用于识别临时目标，不读、不操作其他内容；'
            '不用终端、旧 CUA 插件、其他桌面工具、微信或私人内容。'
            '完成后分开报告任务结果和首次使用者批判性评价，包括定位弹窗、脚本语法、回执质量、'
            '调用数、实际用时、失败/重试次数及最需要改进的地方。'
        )
        config_path = Path.home() / '.gemini/config/config.json'
        original = config_path.read_bytes()
        had_rule = RULE in json.loads(original).get('userSettings', {}).get('globalPermissionGrants', {}).get('allow', [])
        injected = add_allow(original)
        log, err = root / 'agent.jsonl', root / 'agent.stderr'
        print(json.dumps({'root': str(root), 'pid': pid, 'profile': str(profile),
                          'target_location': str(location), 'plugin': str(plugin),
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
        expected = location / vault_name
        readback = {'vault_directory_exists': expected.is_dir(),
                    'obsidian_config_exists': (expected / '.obsidian').is_dir(),
                    'location_entries': sorted(p.name for p in location.iterdir())[:20]}
        record = {'binary': str(binary), 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                  'agent_exit_code': agent.returncode if agent else None,
                  'elapsed_seconds': elapsed, 'config_restore': restore,
                  'config_original_sha256': hashlib.sha256(original).hexdigest(),
                  'config_after_sha256': hashlib.sha256(config_path.read_bytes()).hexdigest(),
                  'agent': summarize(log), 'independent_readback': readback}
        (root / 'record.json').write_text(json.dumps(record, ensure_ascii=False, indent=2))
        print(json.dumps({'record': str(root / 'record.json'), 'elapsed_seconds': elapsed,
                          'run_calls': record['agent']['run_calls'],
                          'failed_receipts': record['agent']['failed_receipts'],
                          'config_restore': restore, 'readback': readback},
                         ensure_ascii=False), flush=True)
        if agent.returncode != 0 or not readback['vault_directory_exists'] or not readback['obsidian_config_exists']:
            raise RuntimeError(f'Obsidian vault acceptance did not reach the target; see {root / "record.json"}')
    finally:
        if pid is not None:
            try:
                if obsidian_pid(profile) == pid:
                    os.kill(pid, signal.SIGTERM)
                    for _ in range(20):
                        try:
                            os.kill(pid, 0)
                        except ProcessLookupError:
                            break
                        time.sleep(0.1)
                    else:
                        if obsidian_pid(profile) == pid:
                            os.kill(pid, signal.SIGKILL)
            except (subprocess.CalledProcessError, RuntimeError, ProcessLookupError):
                pass


if __name__ == '__main__':
    main()
