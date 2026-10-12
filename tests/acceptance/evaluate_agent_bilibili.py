#!/usr/bin/env python3
"""Fresh Agent usability run against one already opened Bilibili window in user Chrome.

The caller must supply the exact Chrome PID and native window ID. This script
temporarily allows only the DTW exec MCP permission, then restores it. It does
not close Chrome or modify unrelated tabs.
"""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid

from accept_chrome import run_script
from evaluate_agent_usability import summarize
from probe import call


RULE = "mcp(desktop-world_dtw/exec)"


def atomic_write(path, data):
    temporary = path.with_name(path.name + ".dtw-test-" + uuid.uuid4().hex)
    try:
        temporary.write_bytes(data)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def add_allow(data):
    config = json.loads(data)
    allowed = config.setdefault("userSettings", {}).setdefault("globalPermissionGrants", {}).setdefault("allow", [])
    if RULE not in allowed:
        allowed.append(RULE)
    return (json.dumps(config, ensure_ascii=False, indent=2) + "\n").encode()


def remove_our_allow(path, before, injected):
    current = path.read_bytes()
    if current == injected:
        atomic_write(path, before)
        return "byte_identical_restore"
    config = json.loads(current)
    allowed = config.setdefault("userSettings", {}).setdefault("globalPermissionGrants", {}).setdefault("allow", [])
    if RULE in allowed:
        allowed.remove(RULE)
        atomic_write(path, (json.dumps(config, ensure_ascii=False, indent=2) + "\n").encode())
    return "concurrent_config_change_rule_removed"


def main():
    package = Path(sys.argv[1]).resolve()
    pid, window_id = int(sys.argv[2]), int(sys.argv[3])
    root = Path("/private/tmp") / f"dtw-cua-bilibili-agent-{uuid.uuid4().hex[:8]}"
    root.mkdir(mode=0o700)
    project = root / "project"
    project.mkdir()
    plugin = project / ".agents/plugins/desktop-world"
    subprocess.run([str(package / "bin/dtw"), "install", "antigravity", "--project",
                    str(project)], check=True, stdout=subprocess.DEVNULL)
    with (root / "plugin-validate.txt").open("w") as out:
        subprocess.run(["agy", "plugin", "validate", str(plugin)], stdout=out,
                       stderr=subprocess.STDOUT, check=True)
    prompt = (
        "你是首次安装 DTW 薄插件的使用者，请只按本项目 desktop-world Skill，"
        f"只用 desktop-world_dtw/exec 操作我现有 Chrome 的 B 站测试窗口 PID {pid}、原生窗口 ID {window_id}。"
        "该窗口已打开 B 站综合热门。先用原生辅助功能树观察真实热门页，解释树是否截断、定位是否高效；"
        "浏览热门列表并打开一个公开热门视频。确认该视频原先没有被点赞，精确定位点赞按钮，真实点赞一次，"
        "从新的 UI 状态确认生效，再点同一视频一次撤销，并重新读回未点赞状态。"
        "如果未登录、按钮或初始状态不明、页面变化、出现验证码/授权、动作结果不明，立刻停止，不猜测、不重复输入，"
        "保留原 id 并报告阻塞。不要浏览私人标签、账号资料或微信；不要评论、关注、收藏。"
        "不要用终端、浏览器自动化工具、旧 CUA 插件、Chrome 调试协议、DOM 注入或网页 API；桌面操作只用薄插件 exec。"
        "可在这个测试窗口内切换标签，但不得操作其他窗口。"
        "请从真实首次使用者角度批判性评价安装发现、Skill、脚本语法、定位/截图、结果输出、"
        "实际用时、exec 调用数、失败次数、重试次数和三个最大缺陷。将任务实际结果与体验评价分开写，失败照实写。"
    )
    log, err = root / "agent.jsonl", root / "agent.stderr"
    config_path = Path.home() / ".gemini/config/config.json"
    original = config_path.read_bytes()
    injected = add_allow(original)
    (root / "config-original.sha256").write_text(hashlib.sha256(original).hexdigest() + "\n")
    agent = None
    start = time.monotonic()
    restore = None
    try:
        atomic_write(config_path, injected)
        print(json.dumps({"root": str(root), "pid": pid, "window_id": window_id,
                          "plugin": str(plugin), "config_allow": RULE}), flush=True)
        with log.open("w") as out, err.open("w") as stderr:
            agent = subprocess.run(["agy", "--print", prompt, "--model", "gemini-3.8-flash-high",
                                    "--output-format", "stream-json", "--print-timeout", "480s"],
                                   cwd=project, stdout=out, stderr=stderr, timeout=500)
    finally:
        restore = remove_our_allow(config_path, original, injected)
    elapsed = round(time.monotonic() - start, 2)
    proc = subprocess.Popen([str(package / "bin/dtw")], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                            env={"PATH": "/usr/bin:/bin", "TMPDIR": "/private/tmp"})
    try:
        call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "bilibili-independent-readback", "version": "1"}}})
        _, after = run_script(proc, 2, "bilibili-readback",
                              f"const w=(await cua.listWindows({{pid:{pid}}})).filter(x=>x.pid==={pid}&&x.window_id==={window_id});"
                              "if(w.length!==1)return {target_windows:w.length};"
                              f"const a=await cua.getApp({{pid:{pid},windowId:{window_id}}});"
                              "const s=await a.getState({max_elements:1500,timeout_ms:5000,include_screenshot:false});"
                              "const t=String(s.tree_markdown||'');"
                              "return {title:w[0].title,window_id:w[0].window_id,snapshot_id:s.snapshot_id,"
                              "tree_chars:t.length,truncated:s.truncated,like_controls:t.split('\\n')"
                              ".filter(x=>/点赞|已赞/.test(x)&&/AX/.test(x)).slice(0,6).map(x=>x.slice(0,140))};")
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    record = {"agent_exit_code": agent.returncode if agent else None,
              "elapsed_seconds": elapsed, "config_restore": restore,
              "config_original_sha256": hashlib.sha256(original).hexdigest(),
              "config_after_sha256": hashlib.sha256(config_path.read_bytes()).hexdigest(),
              "agent": summarize(log), "independent_readback": after}
    (root / "record.json").write_text(json.dumps(record, ensure_ascii=False, indent=2))
    print(json.dumps({"record": str(root / "record.json"), "agent_exit_code": agent.returncode,
                      "elapsed_seconds": elapsed, "config_restore": restore,
                      "run_calls": record["agent"]["run_calls"],
                      "failed_receipts": record["agent"]["failed_receipts"],
                      "independent_readback": after.get("value")}, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
