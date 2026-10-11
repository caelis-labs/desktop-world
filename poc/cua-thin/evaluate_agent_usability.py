#!/usr/bin/env python3
"""Fresh Antigravity usability run on a new, isolated Chrome fixture."""

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
from probe import call


ALLOW_RULE = "mcp(desktop-world_dtw/exec)"


def restore_test_allow(path, before, injected):
    current = path.read_bytes()
    if current == injected:
        temporary = path.with_name(path.name + ".dtw-poc-restore-" + uuid.uuid4().hex)
        temporary.write_bytes(before)
        os.replace(temporary, path)
        return "byte_identical_restore"
    config = json.loads(current)
    allowed = config.setdefault("userSettings", {}).setdefault("globalPermissionGrants", {}).setdefault("allow", [])
    if ALLOW_RULE in allowed:
        allowed.remove(ALLOW_RULE)
        temporary = path.with_name(path.name + ".dtw-poc-restore-" + uuid.uuid4().hex)
        temporary.write_text(json.dumps(config, ensure_ascii=False, indent=2) + "\n")
        os.replace(temporary, path)
    return "concurrent_config_change_rule_removed"


def summarize(log):
    calls = []
    result = None
    for line in log.read_text().splitlines():
        event = json.loads(line)
        if event.get("event") == "result":
            result = event["result"]
        step = event.get("step_update", {})
        if step.get("state") not in ("DONE", "ERROR") or step.get("tool_name") != "call_mcp_tool":
            continue
        info = step.get("tool_info", {})
        params = info.get("parameters", {})
        if params.get("ServerName") != "desktop-world_dtw" or params.get("ToolName") != "exec":
            continue
        args = params.get("Arguments", {})
        output = info.get("output", "")
        try:
            value = json.loads(output)
        except (TypeError, json.JSONDecodeError):
            value = {}
        calls.append({"operation": args.get("operation", "run"), "tool_step_state": step.get("state"), "script_sha256":
                      hashlib.sha256(args.get("script", "").encode()).hexdigest() if args.get("script") else None,
                      "execution_id": value.get("execution_id"), "status": value.get("status"),
                      "error_code": value.get("error_code"), "is_error": value.get("is_error"),
                      "duration_seconds": step.get("duration_seconds")})
    return {"conversation_id": result.get("conversation_id") if result else None,
            "agent_turn_status": result.get("status") if result else None,
            "agent_response": result.get("response") if result else None,
            "exec_calls": calls,
            "run_calls": sum(c["operation"] == "run" for c in calls),
            "failed_receipts": sum(c["status"] in ("failed", "unknown", "refused") or c["is_error"] is True
                                   or c["tool_step_state"] == "ERROR" for c in calls)}


def main():
    package = Path(sys.argv[1]).resolve()
    root = Path("/private/tmp") / f"dtw-cua-usability-{uuid.uuid4().hex[:8]}"
    root.mkdir(mode=0o700)
    project = root / "project"
    project.mkdir()
    plugin = project / ".agents/plugins/desktop-world"
    subprocess.run([sys.executable, str(Path(__file__).with_name("adapt_antigravity.py")),
                    str(package), str(plugin)], check=True)
    with (root / "plugin-validate.txt").open("w") as out:
        subprocess.run(["agy", "plugin", "validate", str(plugin)], stdout=out,
                       stderr=subprocess.STDOUT, check=True)
    profile = root / "chrome-profile"
    html = root / "input.html"
    shutil.copy2(Path(__file__).parent / "fixtures/input.html", html)
    opened = subprocess.run(["open", "-g", "-na", "/Applications/Google Chrome.app", "--args",
                             f"--user-data-dir={profile}", "--no-first-run", "--no-default-browser-check",
                             "--disable-background-mode", "--force-renderer-accessibility",
                             "--disable-translate", f"--app={html.as_uri()}"], capture_output=True, text=True)
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
            raise RuntimeError("isolated Chrome PID not found")
        prompt = (
            f"你刚获得本项目安装的 desktop-world 插件。请按其 Skill 自行摸索使用，"
            f"只用该插件的 desktop-world_dtw/exec 控制临时 Chrome PID {pid}、标题 DTW Input Fixture 的窗口。"
            "任务：在 Message 输入框填入 USABILITY-20261011 并点击 Apply，确认页面标题出现 INPUT-USABILITY-20261011；"
            "随后打开页面菜单选择 green，确认页面标题变为 MENU-GREEN-20261011。"
            "只操作这个临时页面；不要使用终端、浏览器专用工具、旧 CUA 插件或触碰其他窗口、微信、私人内容。"
            "如果目标身份或动作结果无法确认，停止并报告原执行 ID；不要盲目重复输入。"
            "完成后请以普通首次使用者的角度批判性评价：安装与发现、Skill 是否足够、脚本语法难点、"
            "结果输出是否好读、用了多少次 exec/失败/重试、哪些步骤最耗时，以及最想改的三个地方。"
            "任务结果与体验评价分开写，失败也照实写。"
        )
        log = root / "agent.jsonl"
        err = root / "agent.stderr"
        print(json.dumps({"root": str(root), "pid": pid, "plugin": str(plugin), "log": str(log)}), flush=True)
        start = time.monotonic()
        config_path = Path.home() / ".gemini/config/config.json"
        original = config_path.read_bytes()
        config = json.loads(original)
        allowed = config.setdefault("userSettings", {}).setdefault("globalPermissionGrants", {}).setdefault("allow", [])
        added_rule = ALLOW_RULE not in allowed
        if added_rule:
            allowed.append(ALLOW_RULE)
        injected = (json.dumps(config, ensure_ascii=False, indent=2) + "\n").encode()
        if added_rule:
            temporary = config_path.with_name(config_path.name + ".dtw-poc-" + uuid.uuid4().hex)
            temporary.write_bytes(injected)
            os.replace(temporary, config_path)
        try:
            with log.open("w") as out, err.open("w") as stderr:
                agent = subprocess.run(["agy", "--print", prompt, "--model", "gemini-3.8-flash-high",
                                        "--output-format", "stream-json", "--print-timeout", "300s"],
                                       cwd=project, stdout=out, stderr=stderr, timeout=320)
        finally:
            config_restore = (restore_test_allow(config_path, original, injected)
                              if added_rule else "preexisting_rule_unchanged")
        elapsed = time.monotonic() - start
        proc = subprocess.Popen([str(package / "bin/dtw")], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                env={"PATH": "/usr/bin:/bin", "TMPDIR": "/private/tmp"})
        try:
            call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "usability-independent-readback", "version": "1"}}})
            _, after = run_script(proc, 2, "independent-readback",
                                  f"const w=(await cua.listWindows({{pid:{pid}}})).filter(x=>x.pid==={pid} && x.title==='DTW Input Fixture');"
                                  f"if(w.length!==1)return {{target_windows:w.length}};"
                                  f"const a=await cua.getApp({{pid:{pid},windowId:w[0].window_id}});"
                                  "const s=await a.getState({max_elements:100,include_screenshot:false});"
                                  "return {window_id:a.windowId,tree:s.tree_markdown,snapshot_id:s.snapshot_id};")
        finally:
            proc.stdin.close()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        record = {"agent_exit_code": agent.returncode, "elapsed_seconds": round(elapsed, 2),
                  "config_restore": config_restore,
                  "config_original_sha256": hashlib.sha256(original).hexdigest(),
                  "config_after_sha256": hashlib.sha256(config_path.read_bytes()).hexdigest(),
                  "agent": summarize(log), "independent_readback": after}
        (root / "record.json").write_text(json.dumps(record, ensure_ascii=False, indent=2))
        print(json.dumps({"record": str(root / "record.json"), "agent_exit_code": agent.returncode,
                          "elapsed_seconds": round(elapsed, 2), "run_calls": record["agent"]["run_calls"],
                          "failed_receipts": record["agent"]["failed_receipts"],
                          "config_restore": config_restore,
                          "independent_readback": after.get("value")}, ensure_ascii=False), flush=True)
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


if __name__ == "__main__":
    main()
