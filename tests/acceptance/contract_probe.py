#!/usr/bin/env python3
"""Exercise the packaged MCP receipt contract without desktop input."""

import json
import os
import subprocess
import sys
import time

from probe import call, exec_call, result, runtime_env


def run(proc, ident, session, script):
    started = exec_call(proc, ident, {"session": session, "script": script})
    assert "execution_id" in started, started
    done = result(proc, ident + 1, session, started["execution_id"]) if started["status"] == "running" else started
    return started["execution_id"], done


def main():
    proc = subprocess.Popen(
        [sys.argv[1]], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True,
        env=runtime_env(), encoding="utf-8",
    )
    report = {}
    try:
        call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "contract-probe", "version": "1"},
        }})
        for ident, session in [(2, "a"), (4, "a"), (6, "b")]:
            _, done = run(proc, ident, session,
                          "state.n=(state.n||0)+1; return state.n;")
            report.setdefault("state", []).append([session, done["status"], done.get("value")])
        assert report["state"] == [["a", "completed", 1], ["a", "completed", 2], ["b", "completed", 1]]
        default_id, default = run(proc, 18, "default", "return 42;")
        report["default_session"] = {"status": default["status"], "value": default.get("value")}
        assert report["default_session"] == {"status": "completed", "value": 42}
        assert not any(key in default for key in (
            "cancel_requested", "error_code", "is_error", "text", "text_truncated",
            "text_compacted", "full_text_bytes", "structured_bytes", "image_count",
        )), default
        full_text = exec_call(proc, 20, {"session": "default", "operation": "result",
                                         "execution_id": default_id, "part": "text"})
        assert "run_script: ok" in full_text["page"]["data"], full_text

        large_id, large = run(proc, 8, "a", "return 'x'.repeat(100000);")
        page = exec_call(proc, 10, {"session": "a", "operation": "result",
                                    "execution_id": large_id, "part": "structured"})
        report["large"] = {"status": large["status"], "summary_bytes": len(json.dumps(large)),
                           "structured_bytes": large.get("structured_bytes"),
                           "text_bytes": len(large.get("text", "")),
                           "text_truncated_by_adapter": large.get("text_truncated"),
                           "page_data_bytes": len(page["page"]["data"]),
                           "next_offset": page["page"]["next_offset"]}
        assert large["status"] == "completed", report["large"]
        assert report["large"]["summary_bytes"] < 15000
        assert page["page"]["next_offset"] is not None

        _, medium = run(proc, 23, "a", "return {marker:'y'.repeat(2200)};")
        report["medium_value"] = {"status": medium["status"],
                                  "value_inline": "value" in medium,
                                  "marker_bytes": len(medium.get("value", {}).get("marker", ""))}
        assert report["medium_value"] == {"status": "completed", "value_inline": True,
                                          "marker_bytes": 2200}

        _, long_script = run(proc, 25, "a", "/*" + "x" * 40000 + "*/ return 64;")
        report["upstream_script_limit"] = {"status": long_script["status"],
                                           "value": long_script.get("value")}
        assert report["upstream_script_limit"] == {"status": "completed", "value": 64}

        foreign = exec_call(proc, 11, {"session": "b", "operation": "result",
                                       "execution_id": large_id})
        report["foreign_receipt"] = foreign
        assert foreign.get("error") == "execution_not_found"

        bad_id, bad = run(proc, 12, "a", "throw new Error('DTW-UNIQUE-ERROR');")
        report["script_error"] = {"status": bad["status"], "error_code": bad.get("error_code"),
                                  "preserved_marker": "DTW-UNIQUE-ERROR" in json.dumps(bad),
                                  "native_error": bad.get("native_error")}
        assert bad["status"] == "failed" and report["script_error"]["preserved_marker"]

        started = exec_call(proc, 14, {"session": "a", "wait_ms": 0, "script":
                                       "await cua.sleep(1500); return 'SLEPT-ONCE';"})
        original = started["execution_id"]
        other = exec_call(proc, 21, {"session": "b", "wait_ms": 0, "script":
                                     "state.n=(state.n||0)+1; return state.n;"})
        cancel = exec_call(proc, 15, {"session": "a", "operation": "cancel",
                                      "execution_id": original})
        terminal = result(proc, 16, "a", original)
        other_terminal = result(proc, 22, "b", other["execution_id"])
        report["cancel"] = {"execution_id": original, "immediate": cancel["status"],
                            "requested": cancel["cancel_requested"],
                            "terminal": terminal["status"], "terminal_value": terminal.get("value"),
                            "terminal_requested": terminal.get("cancel_requested", False),
                            "other_session": {"execution_id": other["execution_id"],
                                              "status": other_terminal["status"],
                                              "value": other_terminal.get("value"),
                                              "cancel_requested": other_terminal.get("cancel_requested", False)}}
        assert cancel["cancel_requested"] and terminal["execution_id"] == original
        assert other_terminal["status"] == "completed" and other_terminal.get("value") == 2
        assert not other_terminal.get("cancel_requested", False)
        print(json.dumps(report, ensure_ascii=False, indent=2))
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        print(json.dumps({"exit_code": proc.returncode, "stderr_tail": proc.stderr.read()[-500:]}))


if __name__ == "__main__":
    main()
