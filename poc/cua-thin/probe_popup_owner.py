#!/usr/bin/env python3
"""Find the owner of a temporary Obsidian Browse popup across all processes."""

import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time
import uuid

from accept_chrome import run_script
from probe import call, exec_call, result


def obsidian_pid(profile):
    lines = subprocess.check_output(["pgrep", "-fl", "Obsidian.app/Contents/MacOS/Obsidian"], text=True).splitlines()
    matches = []
    for line in lines:
        if str(profile) in line:
            match = re.match(r"^\s*(\d+)\s", line)
            if match:
                matches.append(int(match.group(1)))
    if len(matches) != 1:
        raise RuntimeError(f"expected one temporary Obsidian PID, got {len(matches)}")
    return matches[0]


def focus(proc, ident, session, pid, window_id=None):
    args = {"session": session, "operation": "focus", "pid": pid, "wait_ms": 3000}
    if window_id is not None:
        args["window_id"] = window_id
    initial = exec_call(proc, ident, args)
    if initial.get("status") == "running":
        initial = result(proc, ident + 1, session, initial["execution_id"])
    detail = exec_call(proc, ident + 2, {"session": session, "operation": "result",
                                       "execution_id": initial["execution_id"], "part": "structured"})
    try:
        initial["native"] = json.loads(detail["page"]["data"])
    except (KeyError, json.JSONDecodeError):
        initial["native"] = None
    return initial


def main():
    package = Path(sys.argv[1]).resolve()
    mode = sys.argv[2] if len(sys.argv) > 2 else "background"
    if mode not in ("background", "foreground", "fastfocus"):
        raise SystemExit("mode must be background, foreground, or fastfocus")
    click_mode = "foreground" if mode == "fastfocus" else mode
    root = Path("/private/tmp") / f"dtw-cua-popup-owner-{mode}-{uuid.uuid4().hex[:8]}"
    root.mkdir(mode=0o700)
    profile = root / "obsidian-profile"
    opened = subprocess.run(["open", "-g", "-na", "/Applications/Obsidian.app", "--args",
                             f"--user-data-dir={profile}"], capture_output=True, text=True)
    if opened.returncode:
        raise RuntimeError(opened.stderr)
    pid = None
    proc = None
    prior_pid = None
    record = {"root": str(root), "delivery_mode": mode}
    try:
        for _ in range(100):
            try:
                pid = obsidian_pid(profile)
                break
            except (subprocess.CalledProcessError, RuntimeError):
                time.sleep(0.1)
        if pid is None:
            raise RuntimeError("temporary Obsidian PID not found")
        record["pid"] = pid
        proc = subprocess.Popen([str(package / "bin/dtw")], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                env={"PATH": "/usr/bin:/bin", "TMPDIR": "/private/tmp"})
        call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "dtw-modal-focus-probe", "version": "1"}}})
        session = "modal-focus"
        _, active = run_script(proc, 2, session,
                               "return (await cua.listApps()).filter(a=>a.active).map(a=>({pid:a.pid,name:a.name}));")
        record["prior_active"] = active.get("value")
        if active.get("status") == "completed" and len(active.get("value", [])) == 1:
            prior_pid = active["value"][0]["pid"]
        for attempt in range(30):
            _, before = run_script(proc, 10 + attempt * 2, session,
                                   f"const w=(await cua.listWindows({{pid:{pid}}})).filter(x=>x.pid==={pid});"
                                   "if(w.length!==1)return {windows:w};"
                                   f"const a=await cua.getApp({{pid:{pid},windowId:w[0].window_id}});"
                                   "const s=await a.getState({max_elements:80,include_screenshot:false});"
                                   "return {windows:w,tree:s.tree_markdown,snapshot_id:s.snapshot_id};")
            if before.get("status") == "completed" and "创建" in before.get("value", {}).get("tree", ""):
                break
            time.sleep(0.5)
        record["before"] = before
        windows = before.get("value", {}).get("windows", [])
        if len(windows) != 1 or "新建仓库" not in before.get("value", {}).get("tree", ""):
            raise RuntimeError("temporary welcome window not uniquely identified")
        window_id = windows[0]["window_id"]
        record["main_window_id"] = window_id
        _, create = run_script(proc, 100, session,
                               f"const a=await cua.getApp({{pid:{pid},windowId:{window_id}}});"
                               "const q=await a.query({role:'button',name:'创建'});"
                               "if(q.length!==1)return {found:q.length};"
                               "await a.click({role:'button',name:'创建'});"
                               "const s=await a.getState({max_elements:80,include_screenshot:false});"
                               "return {found:q.length,tree:s.tree_markdown};")
        record["create_form"] = create
        if create.get("status") != "completed" or "浏览" not in create.get("value", {}).get("tree", ""):
            raise RuntimeError("temporary new-vault form did not expose Browse")
        record["focus_main_before_browse"] = focus(proc, 120, session, pid, window_id)
        _, global_before = run_script(proc, 140, session,
                                      "return (await cua.listWindows()).map(w=>({pid:w.pid,window_id:w.window_id,z_index:w.z_index}));")
        if global_before.get("status") != "completed":
            raise RuntimeError("global before-window discovery failed")
        prior_ids = [w["window_id"] for w in global_before["value"]]
        record["global_before_count"] = len(prior_ids)
        baseline_js = json.dumps(prior_ids)
        _, browse = run_script(proc, 102, session,
                               f"const a=await cua.getApp({{pid:{pid},windowId:{window_id}}});"
                               "const q=await a.query({role:'button',name:'浏览'});"
                               "if(q.length!==1)return {found:q.length};"
                               f"const action=await a.click({{role:'button',name:'浏览'}},{{delivery_mode:{json.dumps(click_mode)}}});"
                               f"const before=new Set({baseline_js});"
                               "const panel=w=>w.app_name==='Open and Save Panel Service'&&w.title==='打开';"
                               "const shape=ws=>ws.filter(w=>(!before.has(w.window_id)&&panel(w))"
                               f"||w.pid==={pid}).map(w=>({{pid:w.pid,window_id:w.window_id,app_name:w.app_name,"
                               "title:w.title,z_index:w.z_index,is_on_screen:w.is_on_screen,bounds:w.bounds,layer:w.layer}));"
                               "const top=ws=>ws.filter(w=>w.is_on_screen && w.on_current_space===true && w.layer===0 && w.z_index!==null)"
                               ".sort((a,b)=>b.z_index-a.z_index).slice(0,4).map(w=>({pid:w.pid,"
                               "window_id:w.window_id,z_index:w.z_index,"
                               f"title:(panel(w)||w.pid==={pid})?w.title:null}}));"
                               "const immediate=await cua.listWindows();"
                               f"await cua.sleep({0 if mode == 'fastfocus' else 700});const later=await cua.listWindows();"
                               "return {found:q.length,action,immediate_new_or_target:shape(immediate),"
                               "later_new_or_target:shape(later),top_immediate:top(immediate),"
                               "top_later:top(later),total_immediate:immediate.length,total_later:later.length,"
                               "active:(await cua.listApps()).filter(x=>x.active).map(x=>({pid:x.pid,name:x.name}))};")
        record["browse"] = browse
        if browse.get("status") != "completed" or browse.get("value", {}).get("found") != 1:
            raise RuntimeError("Browse not uniquely opened; no focus action")
        source = "immediate_new_or_target" if mode == "fastfocus" else "later_new_or_target"
        dialogs = [w for w in browse["value"][source]
                   if w.get("title") == "打开" and (w.get("is_on_screen") or mode == "fastfocus")]
        if len(dialogs) == 1:
            record["dialog_window"] = {k: dialogs[0].get(k) for k in ("pid", "window_id", "title", "is_on_screen", "bounds")}
            record["focus_dialog"] = focus(proc, 104, session, dialogs[0]["pid"], dialogs[0]["window_id"])
        else:
            record["dialog_window"] = None
            record["dialog_exposure"] = f"{len(dialogs)} globally visible Open windows after 700 ms"
        _, windows_after = run_script(proc, 106, session,
                                      f"return (await cua.listWindows({{pid:{pid}}})).filter(x=>x.pid==={pid}).map(x=>({{pid:x.pid,window_id:x.window_id,title:x.title}}));")
        record["windows_after"] = windows_after
        observed = record.get("focus_dialog", {}).get("native") or {}
        record["focused_window_id"] = observed.get("observed", {}).get("focused_window_id") if isinstance(observed, dict) else None
    finally:
        if proc is not None:
            if prior_pid is not None and prior_pid != pid:
                try:
                    _, current = run_script(proc, 200, "modal-focus",
                                            "return (await cua.listApps()).filter(a=>a.active).map(a=>a.pid);")
                    if current.get("value") == [pid]:
                        record["restore_prior"] = focus(proc, 202, "modal-focus", prior_pid)
                except Exception as exc:
                    record["restore_error"] = str(exc)
            proc.stdin.close()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
            record["worker_stderr_tail"] = proc.stderr.read()[-800:]
        (root / "record.json").write_text(json.dumps(record, ensure_ascii=False, indent=2))
        print(json.dumps({"record": str(root / "record.json"), "pid": pid,
                          "main_window_id": record.get("main_window_id"),
                          "new_immediate": browse.get("value", {}).get("immediate_new_or_target"),
                          "new_later": browse.get("value", {}).get("later_new_or_target"),
                          "top_later": browse.get("value", {}).get("top_later"),
                          "focus_status": record.get("focus_dialog", {}).get("status"),
                          "focus_error_code": record.get("focus_dialog", {}).get("error_code"),
                          "focused_window_id": record.get("focused_window_id")}, ensure_ascii=False), flush=True)
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


if __name__ == "__main__":
    main()
