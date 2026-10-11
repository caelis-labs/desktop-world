#!/usr/bin/env python3
"""Fresh Agent edits only a copied DOCX in a new WPS process."""

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

from probe import call, exec_call, result


def wps_pids():
    output = subprocess.check_output(["pgrep", "-fl", "/Applications/wpsoffice.app/Contents/MacOS/wpsoffice"], text=True)
    return {int(line.split(None, 1)[0]) for line in output.splitlines()
            if "/Applications/wpsoffice.app/Contents/MacOS/wpsoffice" in line}


def main():
    package = Path(sys.argv[1]).resolve()
    project = Path(sys.argv[2]).resolve()
    fixture = Path("/private/tmp/agy-cua-acceptance-20261011/test-data/wps-test.docx")
    root = Path("/private/tmp") / f"dtw-cua-agent-wps-{uuid.uuid4().hex[:8]}"
    root.mkdir(mode=0o700)
    document = root / "DTW-CUA-AGENT-POC.docx"
    shutil.copy2(fixture, document)
    before_hash = hashlib.sha256(document.read_bytes()).hexdigest()
    before_pids = wps_pids()
    opened = subprocess.run(["open", "-g", "-na", "/Applications/wpsoffice.app", str(document)], capture_output=True, text=True)
    if opened.returncode:
        raise RuntimeError(opened.stderr)
    pid = None
    for _ in range(100):
        fresh = wps_pids() - before_pids
        if len(fresh) == 1:
            pid = fresh.pop()
            break
        time.sleep(0.1)
    if pid is None:
        raise RuntimeError("new WPS PID not isolated")
    marker = "DTW-CUA-AGENT-MARKER-20261011"
    prompt = (
        f"请只用本项目 desktop-world Skill 和 desktop-world_dtw/exec，在一个新 Agent 会话完成真实 WPS 测试任务。"
        f"只操作刚启动的独立 WPS PID {pid} 中标题为 {document.name} 的窗口；文档是临时副本 {document}。"
        f"先用精确 PID/window ID 发现、截图，确认仅有测试文案 Cua Driver acceptance test / Original line。"
        f"在文档测试文本处插入唯一标记 {marker}，保留原文并保存。"
        "WPS 编辑区是无 AX 文本节点的画布。先查看同 Session 截图，在测试文字附近选一个精确窗口像素点；"
        "调用 cua.call('type_text',{pid,window_id,x,y,text,delivery_mode:'foreground'}) 聚焦并输入；"
        "然后用 cua.call('hotkey',{pid,window_id,keys:['cmd','s'],delivery_mode:'foreground'}) 保存。"
        "本项目 Skill 和 exec schema 足够完成操作；不要阅读旧 CUA 插件、额外 WORKFLOW 或其他 schema。"
        "只调用这一个 MCP exec 工具进行桌面操作；不要调用终端、旧 CUA 插件、浏览器工具，也不要触碰其他 WPS 文档、微信或私人内容。"
        "如果目标窗口不符、权限拒绝、结果 unknown 或无法确定编辑位置，停止并报告原 execution_id；不要重放动作。"
        "最后报告实际工具、PID/window ID、输入路由、保存回执以及可验证的结果。"
    )
    log = root / "agent.jsonl"
    stderr = root / "agent.stderr"
    print(json.dumps({"pid": pid, "document": str(document), "log": str(log), "before_sha256": before_hash}), flush=True)
    try:
        with log.open("w") as out, stderr.open("w") as err:
            agent = subprocess.run(["agy", "--print", prompt, "--model", "gemini-3.8-flash-high",
                                    "--output-format", "stream-json", "--print-timeout", "360s"],
                                   cwd=project, stdout=out, stderr=err, timeout=380)
        converted = subprocess.run(["/usr/bin/textutil", "-convert", "txt", "-stdout", str(document)],
                                   capture_output=True, text=True)
        readback = {"agent_exit_code": agent.returncode,
                    "textutil_exit_code": converted.returncode,
                    "marker_saved": marker in converted.stdout,
                    "original_preserved": "Original line" in converted.stdout,
                    "after_sha256": hashlib.sha256(document.read_bytes()).hexdigest(),
                    "text_preview": converted.stdout[:180]}
        (root / "independent-readback.json").write_text(json.dumps(readback, ensure_ascii=False, indent=2))
        print(json.dumps(readback, ensure_ascii=False), flush=True)
    finally:
        if pid in wps_pids():
            os.kill(pid, signal.SIGTERM)
            for _ in range(40):
                if pid not in wps_pids():
                    break
                time.sleep(0.1)
            if pid in wps_pids():
                os.kill(pid, signal.SIGKILL)


if __name__ == "__main__":
    main()
