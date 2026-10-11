#!/usr/bin/env python3
"""Inspect WPS new-document UI in an exact temporary process/document."""

import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
import uuid

from accept_chrome import run_script
from accept_agent_wps import wps_pids
from probe import call
from probe_modal_focus import focus


def main():
    package = Path(sys.argv[1]).resolve()
    mode = sys.argv[2] if len(sys.argv) > 2 else "hotkey"
    if mode not in ("hotkey", "button"):
        raise SystemExit("mode must be hotkey or button")
    source = Path("/private/tmp/agy-cua-acceptance-20261011/test-data/wps-test.docx")
    root = Path("/private/tmp") / f"dtw-cua-wps-new-{mode}-{uuid.uuid4().hex[:8]}"
    root.mkdir(mode=0o700)
    document = root / "DTW-CUA-WPS-NEW-TEST.docx"
    shutil.copy2(source, document)
    before_pids = wps_pids()
    opened = subprocess.run(["open", "-g", "-na", "/Applications/wpsoffice.app", str(document)],
                            capture_output=True, text=True)
    if opened.returncode:
        raise RuntimeError(opened.stderr)
    pid = None
    proc = None
    prior_pid = None
    record = {"root": str(root), "document": str(document), "mode": mode}
    try:
        for _ in range(100):
            fresh = wps_pids() - before_pids
            if len(fresh) == 1:
                pid = fresh.pop()
                break
            time.sleep(0.1)
        if pid is None:
            raise RuntimeError("one temporary WPS PID was not isolated")
        record["pid"] = pid
        proc = subprocess.Popen([str(package / "bin/dtw")], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                env={"PATH": "/usr/bin:/bin", "TMPDIR": "/private/tmp"})
        call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "dtw-wps-new-probe", "version": "1"}}})
        session = "wps-new"
        _, active = run_script(proc, 2, session,
                               "return (await cua.listApps()).filter(a=>a.active).map(a=>({pid:a.pid,name:a.name}));")
        if active.get("status") == "completed" and len(active.get("value", [])) == 1:
            prior_pid = active["value"][0]["pid"]
        _, before = run_script(proc, 4, session,
                               f"const w=(await cua.listWindows({{pid:{pid}}})).filter(x=>x.pid==={pid}"
                               f"&&x.title.includes({json.dumps(document.stem)}));"
                               "return w.map(x=>({pid:x.pid,window_id:x.window_id,title:x.title,bounds:x.bounds}));")
        record["before"] = before
        targets = before.get("value", [])
        if before.get("status") != "completed" or len(targets) != 1:
            raise RuntimeError("temporary document window not uniquely identified")
        window_id = targets[0]["window_id"]
        record["window_id"] = window_id
        _, discover = run_script(proc, 6, session,
                                 f"const a=await cua.getApp({{pid:{pid},windowId:{window_id}}});"
                                 "const s=await a.getState({max_elements:300,include_screenshot:false});"
                                 "const lines=String(s.tree_markdown||'').split('\\n');"
                                 "return {snapshot_id:s.snapshot_id,truncated:s.truncated,"
                                 "new_controls:lines.filter(x=>/新建|创建|模板/.test(x)&&/AX(Button|MenuItem|Tab|StaticText)/.test(x))"
                                 ".slice(0,15).map(x=>x.slice(0,140))};")
        record["discover"] = discover
        record["focus_main"] = focus(proc, 20, session, pid, window_id)
        _, before_ids = run_script(proc, 30, session,
                                   "return (await cua.listWindows()).map(w=>w.window_id);")
        ids = before_ids.get("value", [])
        if before_ids.get("status") != "completed":
            raise RuntimeError("global window discovery failed")
        action_js = ("const q=await a.query({role:'button',name:'新建'});"
                     "if(q.length!==1)return {found:q.length};"
                     "const action=await a.click({role:'button',name:'新建'},"
                     "{delivery_mode:'foreground'});") if mode == "button" else (
                     "const action=await a.pressKey('cmd+n',undefined,"
                     "{delivery_mode:'foreground'});")
        _, new = run_script(proc, 32, session,
                            f"const a=await cua.getApp({{pid:{pid},windowId:{window_id}}});"
                            + action_js +
                            f"const before=new Set({json.dumps(ids)});"
                            f"const shape=ws=>ws.filter(w=>w.pid==={pid}||"
                            "(!before.has(w.window_id)&&w.app_name==='Open and Save Panel Service'&&w.title==='打开'))"
                            ".map(w=>({pid:w.pid,window_id:w.window_id,app_name:w.app_name,"
                            "title:w.title,layer:w.layer,is_on_screen:w.is_on_screen,bounds:w.bounds}));"
                            "const immediate=await cua.listWindows();await cua.sleep(700);"
                            "const later=await cua.listWindows();"
                            "const state=await a.getState({max_elements:350,include_screenshot:false});"
                            "const tree=String(state.tree_markdown||'');"
                            "return {action,immediate:shape(immediate),later:shape(later),"
                            "after_snapshot_id:state.snapshot_id,after_truncated:state.truncated,"
                            "after_controls:tree.split('\\n').filter(x=>/新建|空白|模板/.test(x)"
                            "&&/AX(Button|MenuItem|Tab|StaticText)/.test(x)).slice(0,20).map(x=>x.slice(0,140))};")
        record["new"] = new
    finally:
        if proc is not None:
            if prior_pid is not None and prior_pid != pid:
                try:
                    _, current = run_script(proc, 90, "wps-new",
                                            "return (await cua.listApps()).filter(a=>a.active).map(a=>a.pid);")
                    if current.get("value") == [pid]:
                        record["restore_prior"] = focus(proc, 92, "wps-new", prior_pid)
                except Exception as exc:
                    record["restore_error"] = str(exc)
            proc.stdin.close()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        (root / "record.json").write_text(json.dumps(record, ensure_ascii=False, indent=2))
        print(json.dumps({"record": str(root / "record.json"), "pid": pid,
                          "window_id": record.get("window_id"),
                          "discover": record.get("discover", {}).get("value"),
                          "new": record.get("new", {}).get("value")}, ensure_ascii=False)[:4500], flush=True)
        if pid is not None and pid in wps_pids():
            os.kill(pid, signal.SIGTERM)
            for _ in range(40):
                if pid not in wps_pids():
                    break
                time.sleep(0.1)
            if pid in wps_pids():
                os.kill(pid, signal.SIGKILL)


if __name__ == "__main__":
    main()
