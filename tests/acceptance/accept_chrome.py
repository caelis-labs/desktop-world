#!/usr/bin/env python3
"""Run the prior isolated Chrome acceptance task through packaged DTW.

The only UI target is a new Chrome process with a unique temporary profile and
a local page created here. No existing Chrome profile/window is addressed.
"""

import base64
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time
import uuid

from probe import call, exec_call, result


def exact_chrome_pid(profile):
    lines = subprocess.check_output(["pgrep", "-fl", "Google Chrome"], text=True).splitlines()
    matches = []
    for line in lines:
        if str(profile) not in line or "Contents/MacOS/Google Chrome" not in line or "Helper" in line:
            continue
        match = re.match(r"^\s*(\d+)\s", line)
        if match:
            matches.append(int(match.group(1)))
    if len(matches) != 1:
        raise RuntimeError(f"expected one isolated Chrome process, got {len(matches)}")
    return matches[0]


def run_script(proc, ident, session, script):
    started = exec_call(proc, ident, {"session": session, "script": script})
    if "execution_id" not in started:
        raise RuntimeError(f"run refused: {started}")
    return started["execution_id"], result(proc, ident + 1, session, started["execution_id"])


def save_image(proc, ident, session, execution_id, destination):
    response = call(proc, {"jsonrpc": "2.0", "id": ident, "method": "tools/call", "params": {"name": "exec", "arguments": {
        "session": session, "operation": "result", "execution_id": execution_id, "part": "image", "image_index": 0}}})
    for content in response["result"].get("content", []):
        if content.get("type") == "image":
            destination.write_bytes(base64.b64decode(content["data"]))
            return True
    return False


def main():
    package = Path(sys.argv[1]).resolve()
    root = Path("/private/tmp") / f"dtw-cua-chrome-{uuid.uuid4().hex[:8]}"
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
        raise RuntimeError(f"isolated Chrome launch failed: {opened.stderr}")
    pid = None
    for _ in range(100):
        try:
            pid = exact_chrome_pid(profile)
            break
        except (subprocess.CalledProcessError, RuntimeError):
            time.sleep(0.1)
    if pid is None:
        raise RuntimeError("isolated Chrome pid not found")
    print(json.dumps({"target_pid": pid, "target_profile": str(profile), "target_html": str(html)}))
    proc = subprocess.Popen([str(package / "bin/dtw")], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True, env={"PATH": "/usr/bin:/bin", "TMPDIR": "/private/tmp"})
    session = "chrome-acceptance"
    record = {"pid": pid, "profile": str(profile), "html": str(html)}
    try:
        call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "dtw-chrome-acceptance", "version": "1"}}})
        before_script = f"const app = await cua.getApp({{pid:{pid}}}); const s = await app.getState({{max_elements:100,timeout_ms:5000}}); return {{pid:app.pid,window_id:app.windowId,title:app.title,snapshot_id:s.snapshot_id,ready:String(s.tree_markdown||'').includes('READY-20261011'),button:String(s.tree_markdown||'').includes('Advance'),screenshot_frame_valid:s.screenshot_frame_valid,screenshot_error:s.screenshot_error,tree:String(s.tree_markdown||'').slice(0,1800)}};"
        for attempt in range(20):
            before_id, before = run_script(proc, 2 + attempt * 10, session, before_script)
            if before.get("status") == "completed" and before.get("value", {}).get("ready"):
                break
            if before.get("status") == "unknown":
                raise RuntimeError(f"unknown read outcome; original receipt {before_id}")
            time.sleep(0.5)
        record["before"] = before
        if before.get("image_count", 0):
            record["before_image_saved"] = save_image(proc, 200, session, before_id, root / "before.png")
        print(json.dumps({"before": before}))
        if before.get("status") != "completed" or not before.get("value", {}).get("ready") or not before.get("value", {}).get("button"):
            raise RuntimeError("target not identified with READY marker and Advance button; no click attempted")
        window_id = before["value"]["window_id"]
        stale_script = (f"const a=await cua.getApp({{pid:{pid},windowId:{window_id}}});"
                        "const matches=await a.query({role:'button',name:'Advance'});"
                        "if(matches.length!==1||!matches[0].element_token)return {refused:false,reason:'no_token'};"
                        "const old=matches[0].element_token;"
                        "await a.getState({max_elements:80,include_screenshot:false});"
                        "try{const action=await a.click(old);return {refused:false,action};}"
                        "catch(e){return {refused:true,code:e.code||null,message:String(e).slice(0,300)};}")
        stale_id, stale = run_script(proc, 210, session, stale_script)
        record["stale"] = stale
        print(json.dumps({"stale": stale}))
        if stale.get("status") != "completed" or not stale.get("value", {}).get("refused"):
            raise RuntimeError(f"stale element not refused; inspect original receipt {stale_id}; no second click")
        fresh_script = f"const a=await cua.getApp({{pid:{pid},windowId:{window_id}}});const s=await a.getState({{max_elements:80,include_screenshot:false}});return {{ready:String(s.tree_markdown||'').includes('READY-20261011'),clicked:String(s.tree_markdown||'').includes('CLICKED-20261011'),snapshot_id:s.snapshot_id}};"
        fresh_id, fresh = run_script(proc, 212, session, fresh_script)
        record["after_stale"] = fresh
        if fresh.get("status") != "completed" or not fresh.get("value", {}).get("ready"):
            raise RuntimeError(f"stale-element path changed the page; inspect {fresh_id}; no second click")
        token_script = f"const a=await cua.getApp({{pid:{pid},windowId:{window_id}}});const q=await a.query({{role:'button',name:'Advance'}});return q.length===1?q[0].element_token:null;"
        token_id, token_result = run_script(proc, 214, session, token_script)
        token = token_result.get("value")
        if token_result.get("status") != "completed" or not isinstance(token, str):
            raise RuntimeError(f"could not get a fixture element token from {token_id}")
        foreign_script = (f"const a=await cua.getApp({{pid:{pid},windowId:{window_id}}});"
                          f"try{{const action=await a.click({json.dumps(token)});return {{refused:false,action}};}}"
                          "catch(e){return {refused:true,code:e.code||null,message:String(e).slice(0,300)};}")
        foreign_id, foreign = run_script(proc, 216, "chrome-foreign", foreign_script)
        record["cross_session_token"] = foreign
        print(json.dumps({"cross_session_token": foreign}))
        foreign_readback_id, foreign_readback = run_script(proc, 218, session, fresh_script)
        record["after_cross_session_token"] = foreign_readback
        print(json.dumps({"after_cross_session_token": foreign_readback}))
        if foreign.get("status") != "completed" or not foreign.get("value", {}).get("refused"):
            raise RuntimeError(f"foreign Session element token not refused; inspect {foreign_id} and {foreign_readback_id}; no second click")
        if foreign_readback.get("status") != "completed" or not foreign_readback.get("value", {}).get("ready"):
            raise RuntimeError(f"foreign Session path changed the page; inspect {foreign_readback_id}; no second click")
        click_script = f"const app=await cua.getApp({{pid:{pid},windowId:{window_id}}}); return await app.click({{role:'button',name:'Advance'}});"
        click_id, clicked = run_script(proc, 201, session, click_script)
        record["click"] = clicked
        print(json.dumps({"click": clicked}))
        if clicked.get("status") == "unknown":
            raise RuntimeError(f"unknown click outcome; original receipt {click_id}; no replay")
        after_script = f"const app=await cua.getApp({{pid:{pid},windowId:{window_id}}}); const s=await app.getState({{max_elements:100,timeout_ms:5000}}); return {{clicked:String(s.tree_markdown||'').includes('CLICKED-20261011'),ready:String(s.tree_markdown||'').includes('READY-20261011'),snapshot_id:s.snapshot_id,tree:String(s.tree_markdown||'').slice(0,1800)}};"
        after_id, after = run_script(proc, 203, session, after_script)
        record["after"] = after
        if after.get("image_count", 0):
            record["after_image_saved"] = save_image(proc, 205, session, after_id, root / "after.png")
        print(json.dumps({"after": after}))
        print(json.dumps({"postcondition": bool(after.get("value", {}).get("clicked")), "record": str(root / "record.json")}))
    finally:
        (root / "record.json").write_text(json.dumps(record, ensure_ascii=False, indent=2))
        proc.stdin.close()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        (root / "dtw-stderr.txt").write_text(proc.stderr.read()[-4000:])
        try:
            if exact_chrome_pid(profile) == pid:
                os.kill(pid, signal.SIGTERM)
        except (subprocess.CalledProcessError, RuntimeError, ProcessLookupError):
            pass
        # Keep evidence, remove the disposable browser profile after its process exits.
        for _ in range(50):
            if not profile.exists():
                break
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                shutil.rmtree(profile)
                break
            time.sleep(0.1)


if __name__ == "__main__":
    main()
