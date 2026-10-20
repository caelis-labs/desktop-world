#!/usr/bin/env python3
"""Check click argument handling in the packaged, pinned worker; no input target.

Every native probe omits both a PID and coordinates, so even a regression cannot
dispatch GUI input. globalThis.cua intentionally reaches the unadapted prelude to
record upstream behavior separately from DTW's public script-local cua object.
"""
import json
import subprocess
import sys

from probe import call, exec_call, result, runtime_env


def main():
    version = subprocess.check_output([sys.argv[1], "--version"], text=True).strip()
    assert "CUA 0.37.0 source 2d2f99228d4d" in version, version
    proc = subprocess.Popen([sys.argv[1]], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, env=runtime_env(), encoding="utf-8")
    try:
        call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "click-contract", "version": "1"}}})
        script = """
const cases = [
  ["native_plural", true, "click", {modifiers:["ctrl"]}],
  ["native_array", true, "click", {modifier:["ctrl"]}],
  ["native_scalar", true, "click", {modifier:"ctrl"}],
  ["native_right_capture", true, "right_click", {capture_id:"never-a-real-capture"}],
  ["native_double_modifier", true, "double_click", {modifier:["ctrl"]}],
  ["adapted_plural", false, "click", {modifiers:["ctrl"]}],
  ["adapted_array", false, "click", {modifier:["ctrl"]}],
  ["adapted_scalar", false, "click", {modifier:"ctrl"}],
  ["adapted_conflict", false, "click", {modifier:["ctrl"],modifiers:["shift"]}],
  ["adapted_unknown_key", false, "click", {modifiers:["typo"]}],
  ["adapted_right_capture", false, "right_click", {capture_id:"never-a-real-capture"}],
  ["adapted_double_modifier", false, "double_click", {modifiers:["ctrl"]}],
  ["adapted_right_zoom", false, "right_click", {from_zoom:true}],
];
const report = {platform:cua.computer.target, cases:{}};
for (const [name,raw,tool,args] of cases) {
  try {
    await (raw ? globalThis.cua : cua).call(tool,args);
    report.cases[name] = {unexpected_success:true};
  } catch (e) {
    report.cases[name] = {code:e.code,message:e.message};
  }
}
return report;
"""
        started = exec_call(proc, 2, {"script": script, "wait_ms": 1000})
        done = result(proc, 3, "unused", started["id"])
        assert done["status"] == "completed", done
        report = done["value"]
        report["baseline"] = version
        cases = report["cases"]
        for name, entry in cases.items():
            assert "unexpected_success" not in entry, (name, entry)
        assert "unknown argument modifiers" in cases["native_plural"]["message"], report
        assert "unknown argument capture_id" in cases["native_right_capture"]["message"], report
        assert cases["native_scalar"] == cases["native_array"], report
        if report["platform"] == "mac":
            assert "unknown argument modifier" in cases["native_double_modifier"]["message"], report
        # Both array spellings reach the same native failure for the missing target.
        assert cases["adapted_plural"] == cases["adapted_array"] == cases["native_array"], report
        for name in ["adapted_scalar", "adapted_conflict", "adapted_unknown_key",
                     "adapted_right_capture", "adapted_double_modifier", "adapted_right_zoom"]:
            assert cases[name]["message"].startswith("DTW "), (name, report)
        print(json.dumps(report, ensure_ascii=False, indent=2))
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        assert proc.returncode == 0, proc.stderr.read()[-1500:]


if __name__ == "__main__":
    main()
