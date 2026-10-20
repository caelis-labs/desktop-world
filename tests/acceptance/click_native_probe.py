#!/usr/bin/env python3
"""Opt-in macOS GUI check. Targets only the dedicated click-contract fixture.

Usage: DTW_CUA_WORKER=... python3 click_native_probe.py DTW FIXTURE_ROOT
Start tests/fixtures/click-contract/build-run.sh first. Never replays input.
"""
import json
from pathlib import Path
import subprocess
import sys
import time

from probe import call, exec_call, result, runtime_env


def main():
    root = Path(sys.argv[2]).resolve()
    fixture = json.loads((root / "state.json").read_text())
    pid = fixture["pid"]
    proc = subprocess.Popen([sys.argv[1]], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, env=runtime_env(), encoding="utf-8")
    ident = 1

    def run(script):
        nonlocal ident
        ident += 2
        started = exec_call(proc, ident, {"script": script, "wait_ms": 1000})
        return result(proc, ident + 1, "unused", started["id"])

    try:
        call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "click-native-fixture", "version": "1"}}})
        bound = run(f'const a=await cua.getApp({{pid:{pid},title:"DTW Issue 41 Fixture"}});'
                    'return {pid:a.pid,window_id:a.windowId};')
        assert bound["status"] == "completed", bound
        assert bound["value"]["pid"] == pid, bound
        wid = bound["value"]["window_id"]
        report = {"pid": pid, "window_id": wid, "actions": []}
        cases = [
            ("adapted_ctrl", 'a.click(p,{capture_id:s.capture_id,modifiers:["ctrl"],delivery_mode:"foreground"})'),
            ("native_scalar", 'raw.click(p,{capture_id:s.capture_id,modifier:"ctrl",delivery_mode:"foreground"})'),
            ("adapted_scalar_rejected", 'a.click(p,{capture_id:s.capture_id,modifier:"ctrl",delivery_mode:"foreground"})'),
            ("bound_right", 'a.click(p,{capture_id:s.capture_id,button:"right",delivery_mode:"foreground"})'),
            ("plain_right", 'a.rightClick(p,{delivery_mode:"foreground"})'),
            ("double", 'a.doubleClick(p,{delivery_mode:"foreground"})'),
            ("native_right_ctrl", 'a.rightClick(p,{modifiers:["ctrl"],delivery_mode:"foreground"})'),
        ]
        for name, action in cases:
            before = len(json.loads((root / "state.json").read_text())["events"])
            script = (f'const target={{pid:{pid},windowId:{wid}}};'
                      'const a=await cua.getApp(target);'
                      'const raw=await globalThis.cua.getApp(target);'
                      'const s=await a.getState({include_screenshot:true,include_accessibility_tree:false,max_image_dimension:1000});'
                      'if(!s.capture_id || !s.screenshot_frame_valid || s.screenshot_error) throw new Error("fixture capture unavailable");'
                      'const p=[Math.floor(s.screenshot_width/2),Math.floor(s.screenshot_height*0.75)];'
                      f'return await {action};')
            done = run(script)
            time.sleep(0.5)
            observed = json.loads((root / "state.json").read_text())["events"][before:]
            entry = {"case": name, "receipt": done, "observed_events": observed}
            report["actions"].append(entry)
            (root / "native-report.json").write_text(json.dumps(report, indent=2))
            if name == "adapted_scalar_rejected":
                assert done["status"] == "failed" and "DTW click" in done.get("text", ""), entry
                assert observed == [], entry
            elif done["status"] in ("unknown", "cancelled"):
                break  # Preserve uncertainty; no subsequent input or replay.
        print(json.dumps(report, indent=2))
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        (root / "quit").touch()


if __name__ == "__main__":
    main()
