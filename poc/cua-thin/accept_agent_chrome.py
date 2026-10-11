#!/usr/bin/env python3
"""Fresh Antigravity/Gemini Chrome task with only DTW actions in the prompt."""

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
from probe import call


def main():
    package = Path(sys.argv[1]).resolve()
    project = Path(sys.argv[2]).resolve()
    root = Path("/private/tmp") / f"dtw-cua-agent-chrome-{uuid.uuid4().hex[:8]}"
    root.mkdir(mode=0o700)
    profile = root / "chrome-profile"
    html = root / "driver-test.html"
    html.write_text("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Cua Driver acceptance</title>"
                    "<body><h1 id=\"status\">READY-20261011</h1><button id=\"advance\" "
                    "onclick=\"document.getElementById('status').textContent='CLICKED-20261011'\">Advance</button></body></html>")
    opened = subprocess.run(["open", "-g", "-na", "/Applications/Google Chrome.app", "--args",
                             f"--user-data-dir={profile}", "--no-first-run", "--no-default-browser-check",
                             "--disable-background-mode", "--force-renderer-accessibility",
                             "--disable-translate", f"--app={html.as_uri()}"], capture_output=True, text=True)
    if opened.returncode:
        raise RuntimeError(opened.stderr)
    pid = None
    for _ in range(100):
        try:
            pid = exact_chrome_pid(profile)
            break
        except (subprocess.CalledProcessError, RuntimeError):
            time.sleep(0.1)
    if pid is None:
        raise RuntimeError("isolated Chrome pid not found")
    prompt = (f"请在一个新的会话中只用本项目 desktop-world Skill 和 desktop-world_dtw 的 exec MCP 工具完成这个真实桌面任务。"
              f"目标是刚启动的独立 Chrome 进程 PID {pid}，只在它的 Cua Driver acceptance 窗口操作；它使用临时 profile {profile}，页面文件 {html}。"
              "先经 exec 脚本用精确 PID 发现窗口并读取 AX/截图；确认 READY-20261011 和 Advance 后只点击 Advance 一次；"
              "再用新的 CUA 观察确认 CLICKED-20261011。若目标不匹配、权限拒绝或结果 unknown，停止并报告原 execution_id，不要重试动作。"
              "不要操作其他 Chrome 窗口、Obsidian、微信或任何私人内容。不要调用终端、浏览器专用工具、旧 CUA 插件或其他桌面控制方式。"
              "最后报告实际 MCP 工具、窗口 PID/ID、输入路由及新快照的结果。")
    log = root / "agent.jsonl"
    stderr = root / "agent.stderr"
    print(json.dumps({"pid": pid, "profile": str(profile), "log": str(log)}), flush=True)
    try:
        with log.open("w") as out, stderr.open("w") as err:
            agent = subprocess.run(["agy", "--print", prompt, "--model", "gemini-3.8-flash-high",
                                    "--output-format", "stream-json", "--print-timeout", "240s"],
                                   cwd=project, stdout=out, stderr=err, timeout=260)
        print(json.dumps({"agent_exit_code": agent.returncode}), flush=True)
        # An independent CUA process reads the target after the Agent turn.
        proc = subprocess.Popen([str(package / "bin/dtw")], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True, env={"PATH": "/usr/bin:/bin", "TMPDIR": "/private/tmp"})
        try:
            call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "independent-readback", "version": "1"}}})
            _, after = run_script(proc, 2, "independent-readback",
                                  f"const w=(await cua.listWindows({{pid:{pid}}})).filter(x=>x.pid==={pid} && x.title==='Cua Driver acceptance'); if(w.length!==1) return {{target_windows:w.length}}; const a=await cua.getApp({{pid:{pid},windowId:w[0].window_id}}); const s=await a.getState({{max_elements:80,include_screenshot:false}}); return {{pid:a.pid,window_id:a.windowId,clicked:String(s.tree_markdown||'').includes('CLICKED-20261011'),ready:String(s.tree_markdown||'').includes('READY-20261011'),snapshot_id:s.snapshot_id}};")
            print(json.dumps({"independent_readback": after}), flush=True)
            (root / "independent-readback.json").write_text(json.dumps(after, ensure_ascii=False, indent=2))
        finally:
            proc.stdin.close()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
    finally:
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


if __name__ == "__main__":
    main()
