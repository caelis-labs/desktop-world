#!/usr/bin/env python3
"""Two isolated Chrome windows: one Runtime context and serialized executions."""

import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
import uuid

from accept_chrome import exact_chrome_pid
from probe import call, exec_call, result, structured_result, focus_verified


def launch(profile, page):
    opened = subprocess.run([
        "open", "-g", "-na", "/Applications/Google Chrome.app", "--args",
        f"--user-data-dir={profile}", "--no-first-run", "--no-default-browser-check",
        "--disable-background-mode", "--force-renderer-accessibility",
        "--disable-translate", f"--app={page.as_uri()}",
    ], capture_output=True, text=True)
    if opened.returncode:
        raise RuntimeError(opened.stderr)
    for _ in range(100):
        try:
            return exact_chrome_pid(profile)
        except (subprocess.CalledProcessError, RuntimeError):
            time.sleep(0.1)
    raise RuntimeError("isolated Chrome PID not found")


def main():
    binary = Path(sys.argv[1]).resolve()
    root = Path("/private/tmp") / f"dtw-cua-dual-{uuid.uuid4().hex[:8]}"
    root.mkdir(mode=0o700)
    page = Path(__file__).parent / "fixtures/input.html"
    targets = []
    proc = None
    try:
        for label in ("a", "b"):
            profile = root / f"chrome-{label}"
            pid = launch(profile, page)
            targets.append((label, profile, pid))
        proc = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True,
                                env={"PATH": "/usr/bin:/bin", "TMPDIR": "/private/tmp"})
        call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "dual-foreground", "version": "1"},
        }})
        bound = []
        for index, (label, _, pid) in enumerate(targets):
            windows = []
            for attempt in range(25):
                ident = 100 + index * 100 + attempt * 2
                started = exec_call(proc, ident, {"script": f"return (await cua.listWindows({{pid:{pid}}})).filter(w=>w.pid==={pid} && w.title==='DTW Input Fixture').map(w=>w.window_id);"})
                done = result(proc, ident + 1, label, started["id"])
                windows = done.get("value")
                if done["status"] == "completed" and isinstance(windows, list) and len(windows) == 1:
                    break
                time.sleep(0.2)
            if not isinstance(windows, list) or len(windows) != 1:
                raise RuntimeError(f"target window not unique: {done}")
            bound.append((label, pid, windows[0]))
        print(json.dumps({"bound": bound, "record": str(root / "record.json")}), flush=True)
        sequential = "--sequential" in sys.argv[2:]
        jobs = []
        outcomes = []
        readback = []
        focus_results = []
        for index, (label, pid, wid) in enumerate(bound):
            if sequential and "--focus-before" in sys.argv[2:]:
                focused = exec_call(proc, 500 + index * 2, {
                    "operation": "focus", "pid": pid, "window_id": wid})
                if focused.get("status") == "running":
                    focused = result(proc, 501 + index * 2, label, focused["id"])
                native_focus = structured_result(proc, 502 + index * 2, label, focused["id"])
                focus_results.append({"status": focused.get("status"),
                                      "focus": native_focus})
                assert focused.get("status") == "completed" and focus_verified(native_focus, wid), focused
            script = (f"const a=await cua.getApp({{pid:{pid},windowId:{wid}}});"
                      "const s=await a.getState({max_elements:80});"
                      "if(!s.screenshot_frame_valid||!String(s.tree_markdown).includes('Drag source'))return {refused:true};"
                      f"return await cua.call('drag',{{pid:{pid},window_id:{wid},from_x:140,from_y:415,to_x:495,to_y:430,duration_ms:800,steps:30,delivery_mode:'foreground'}});")
            started = exec_call(proc, 10 + index, {"script": script})
            jobs.append((label, started["id"]))
            if sequential:
                done = result(proc, 20 + index, label, started["id"])
                outcomes.append({"id": started["id"],
                                 "status": done["status"], "value": done.get("value"),
                                 "error_code": done.get("error_code")})
                observed = exec_call(proc, 30 + index * 2, {"script": f"const a=await cua.getApp({{pid:{pid},windowId:{wid}}});const s=await a.getState({{max_elements:80,include_screenshot:false}});return {{dragged:String(s.tree_markdown).includes('DRAGGED-20261011'),snapshot_id:s.snapshot_id}};"})
                fresh = result(proc, 31 + index * 2, label, observed["id"])
                readback.append({"status": fresh["status"], "value": fresh.get("value")})
        if not sequential:
            for index, (label, id) in enumerate(jobs):
                done = result(proc, 20 + index, label, id)
                outcomes.append({"id": id,
                                 "status": done["status"], "value": done.get("value"),
                                 "error_code": done.get("error_code")})
        for index, (label, pid, wid) in enumerate(bound):
            if sequential:
                continue
            started = exec_call(proc, 30 + index * 2, {"script": f"const a=await cua.getApp({{pid:{pid},windowId:{wid}}});const s=await a.getState({{max_elements:80,include_screenshot:false}});return {{dragged:String(s.tree_markdown).includes('DRAGGED-20261011'),snapshot_id:s.snapshot_id}};"})
            done = result(proc, 31 + index * 2, label, started["id"])
            readback.append({"status": done["status"], "value": done.get("value")})
        record = {"mode": "sequential" if sequential else "concurrent", "targets": bound,
                  "focus_results": focus_results, "outcomes": outcomes, "readback": readback}
        (root / "record.json").write_text(json.dumps(record, ensure_ascii=False, indent=2))
        print(json.dumps(record, ensure_ascii=False), flush=True)
        assert all(o["status"] == "completed" and o.get("value", {}).get("delivery", {}).get("mode") == "foreground" for o in outcomes)
        assert all(r["status"] == "completed" and r.get("value", {}).get("dragged") for r in readback)
    finally:
        if proc is not None:
            proc.stdin.close()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        for _, profile, pid in targets:
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
