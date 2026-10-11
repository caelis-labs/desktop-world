#!/usr/bin/env python3
"""Check the one-tool native pass-through contract without touching desktop UI."""

import json
import subprocess
import sys

from probe import call, exec_call, result


def main():
    proc = subprocess.Popen(
        [sys.argv[1]], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True,
        env={"PATH": "/usr/bin:/bin", "TMPDIR": "/private/tmp"},
    )
    report = {}
    try:
        call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "native-ops-probe", "version": "1"},
        }})
        listed = call(proc, {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
        tools = listed["result"]["tools"]
        assert [tool["name"] for tool in tools] == ["exec"]
        operations = tools[0]["inputSchema"]["properties"]["operation"]["enum"]
        assert "menu" in operations and "desktop" in operations
        report["visible_tools"] = [tool["name"] for tool in tools]
        report["operations"] = operations

        invalid_menu = exec_call(proc, 3, {"operation": "menu", "pid": 1,
                                           "window_id": 1, "path": []})
        invalid_cap = exec_call(proc, 4, {"operation": "desktop",
                                          "max_image_dimension": -1})
        assert "error" in invalid_menu and "error" in invalid_cap
        report["validation"] = {"menu": invalid_menu["error"],
                                "desktop": invalid_cap["error"]}

        started = exec_call(proc, 5, {"operation": "menu", "pid": 987654321,
                                      "window_id": 987654321, "path": ["File", "Open"],
                                      "wait_ms": 0})
        assert "execution_id" in started
        terminal = result(proc, 6, "default", started["execution_id"])
        assert terminal["status"] == "failed" and terminal["is_error"]
        report["native_menu_dispatch"] = {"status": terminal["status"],
                                          "error_code": terminal.get("error_code"),
                                          "text": terminal.get("text", "")[:300]}

        script = "try { await cua.call('get_desktop_state',{}); return 'unexpected'; } catch(e) { return {code:e.code||null, message:String(e).slice(0,300)}; }"
        started = exec_call(proc, 7, {"script": script, "wait_ms": 0})
        terminal = result(proc, 8, "default", started["execution_id"])
        assert terminal["status"] == "completed"
        assert terminal.get("value", {}).get("code") == "not_allowed", terminal
        report["script_native_boundary"] = terminal["value"]
        print(json.dumps(report, ensure_ascii=False, indent=2))
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        print(json.dumps({"exit_code": proc.returncode,
                          "stderr_tail": proc.stderr.read()[-800:]}))


if __name__ == "__main__":
    main()
