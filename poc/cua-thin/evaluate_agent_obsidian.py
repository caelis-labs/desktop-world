#!/usr/bin/env python3
"""Fresh Gemini creating only a temporary Obsidian vault through its native picker."""

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
from probe_modal_focus import obsidian_pid


def main():
    package = Path(sys.argv[1]).resolve()
    root = Path('/private/tmp') / f'dtw-cua-obsidian-agent-{uuid.uuid4().hex[:8]}'
    root.mkdir(mode=0o700)
    project = root / 'project'
    project.mkdir()
    plugin = project / '.agents/plugins/desktop-world'
    subprocess.run([sys.executable, str(Path(__file__).with_name('adapt_antigravity.py')),
                    str(package), str(plugin)], check=True, stdout=subprocess.DEVNULL)
    with (root / 'plugin-validate.txt').open('w') as out:
        subprocess.run(['agy', 'plugin', 'validate', str(plugin)], stdout=out,
                       stderr=subprocess.STDOUT, check=True)
    profile = root / 'obsidian-profile'
    location = root / 'vault-location'
    location.mkdir()
    vault_name = 'DTW-CUA-FINAL-ACCEPTANCE'
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
            '需要经过 Obsidian 的“浏览”与系统文件选择器。先发现新出现的真实窗口及其所有者，再选择目标文件夹，'
            '最终从新仓库 UI 读回名称。不要打开旧仓库或个人笔记。'
            '如果选择器消失、窗口身份不明、权限拒绝或动作结果 unknown，停止并报告原 execution_id；'
            '不要对旧坐标或未知动作重试。只操作这个临时进程，不用终端、旧 CUA 插件、其他桌面工具、微信或私人内容。'
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
                                        '--output-format', 'stream-json', '--print-timeout', '480s'],
                                       cwd=project, stdout=out, stderr=stderr, timeout=500)
        finally:
            if not had_rule:
                restore = remove_our_allow(config_path, original, injected)
        elapsed = round(time.monotonic() - start, 2)
        expected = location / vault_name
        readback = {'vault_directory_exists': expected.is_dir(),
                    'obsidian_config_exists': (expected / '.obsidian').is_dir(),
                    'location_entries': sorted(p.name for p in location.iterdir())[:20]}
        record = {'package': str(package), 'agent_exit_code': agent.returncode if agent else None,
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
