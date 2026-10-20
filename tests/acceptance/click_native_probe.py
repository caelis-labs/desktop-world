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

from probe import call, exec_call, result, runtime_env, structured_result


EXPECTED = {
    "adapted_ctrl": [("leftDown", 1, True), ("leftUp", 1, True)],
    "native_scalar": [("leftDown", 1, False), ("leftUp", 1, False)],
    "adapted_scalar_rejected": [],
    "bound_right": [("rightDown", 1, False), ("rightUp", 1, False)],
    "plain_right": [("rightDown", 1, False), ("rightUp", 1, False)],
    "double": [("leftDown", 1, False), ("leftUp", 1, False),
               ("leftDown", 2, False), ("leftUp", 2, False)],
    "native_right_ctrl": [("rightDown", 1, True), ("rightUp", 1, True)],
}


def validate_case(name, receipt, events):
    if name == "adapted_scalar_rejected":
        assert receipt["status"] == "failed" and "DTW click" in receipt.get("text", ""), receipt
    else:
        assert receipt["status"] == "completed", receipt
    actual = [(e.get("kind"), e.get("count"), e.get("ctrl")) for e in events]
    assert actual == EXPECTED[name], {"case": name, "expected": EXPECTED[name], "actual": actual}


def observe_case(root, prefix, expected_count):
    """Wait for this call's exact append-only event window, then a quiet interval."""
    deadline = time.monotonic() + 3
    unchanged_since = time.monotonic()
    last = []
    while True:
        events = json.loads((root / "state.json").read_text())["events"]
        assert events[:len(prefix)] == prefix, "fixture event history changed"
        observed = events[len(prefix):]
        now = time.monotonic()
        if observed != last:
            last, unchanged_since = observed, now
        if (len(observed) >= expected_count and now - unchanged_since >= 0.5) or now >= deadline:
            return observed
        time.sleep(0.05)


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--check-report":
        report = json.loads(Path(sys.argv[2]).read_text())
        assert [x["case"] for x in report["actions"]] == list(EXPECTED), "incomplete case report"
        for entry in report["actions"]:
            validate_case(entry["case"], entry["receipt"], entry["observed_events"])
        print("All recorded event assertions passed")
        return
    root = Path(sys.argv[2]).resolve()
    ready_deadline = time.monotonic() + 5
    while not (root / "state.json").exists() and time.monotonic() < ready_deadline:
        time.sleep(0.05)
    fixture = json.loads((root / "state.json").read_text())
    assert fixture["events"] == [], "start a new fixture for this run"
    pid = fixture["pid"]
    proc = subprocess.Popen([sys.argv[1]], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, env=runtime_env(), encoding="utf-8")
    ident = 1

    def run(script):
        nonlocal ident
        ident += 2
        started = exec_call(proc, ident, {"script": script, "wait_ms": 1000})
        done = result(proc, ident + 1, "unused", started["id"])
        detail = structured_result(proc, ident + 2, "unused", started["id"])
        ident += 1
        return done, detail

    try:
        call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "click-native-fixture", "version": "1"}}})
        bound, _ = run(f'const a=await cua.getApp({{pid:{pid},title:"DTW Issue 41 Fixture"}});'
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
        prefix = []
        for name, action in cases:
            assert json.loads((root / "state.json").read_text())["events"] == prefix, "late/intervening fixture event"
            lines = [f'const target={{pid:{pid},windowId:{wid}}};',
                     'const a=await cua.getApp(target);',
                     'const raw=await globalThis.cua.getApp(target);',
                     'const s=await a.getState({include_screenshot:true,include_accessibility_tree:false,max_image_dimension:1000});',
                     'if(!s.capture_id || !s.screenshot_frame_valid || s.screenshot_error) throw new Error("fixture capture unavailable");',
                     'const p=[Math.floor(s.screenshot_width/2),Math.floor(s.screenshot_height*0.75)];',
                     f'return await {action};']
            done, detail = run("\n".join(lines))
            observed = observe_case(root, prefix, len(EXPECTED[name]))
            entry = {"case": name, "receipt": done, "structured": detail,
                     "event_window": {"start": len(prefix), "end": len(prefix) + len(observed)},
                     "observed_events": observed}
            report["actions"].append(entry)
            (root / "native-report.json").write_text(json.dumps(report, indent=2))
            validate_case(name, done, observed)
            if name == "adapted_scalar_rejected":
                assert detail["error"]["line"] == len(lines), entry
                assert f"at line {len(lines)}" in done["text"], entry
            else:
                assert detail["calls"][-1]["op"] in ("click", "right_click", "double_click"), entry
                assert detail["calls"][-1]["line"] == len(lines), entry
                assert f"line {len(lines)} {detail['calls'][-1]['op']} ok" in done["text"], entry
            prefix += observed
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
