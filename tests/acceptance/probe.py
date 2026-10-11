#!/usr/bin/env python3
"""Read-only/JS-only MCP probe. No desktop input or private data access."""

import json
import os
import select
import subprocess
import sys
import time


def call(proc, request, timeout=20):
    proc.stdin.write(json.dumps(request) + "\n")
    proc.stdin.flush()
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not select.select([proc.stdout], [], [], 0.2)[0]:
            continue
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError(f"MCP exited {proc.poll()}")
        response = json.loads(line)
        if response.get("id") == request["id"]:
            return response
    raise TimeoutError(f"MCP request {request['id']} timed out")


def exec_call(proc, ident, args):
    response = call(proc, {"jsonrpc": "2.0", "id": ident, "method": "tools/call", "params": {"name": "exec", "arguments": args}})
    return json.loads(response["result"]["content"][0]["text"])


def result(proc, ident, session, execution_id):
    for _ in range(100):
        current = exec_call(proc, ident, {"session": session, "operation": "result", "execution_id": execution_id})
        if current["status"] != "running":
            return current
        time.sleep(0.1)
    raise TimeoutError(f"execution {execution_id} did not finish")


def main():
    binary = sys.argv[1]
    proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                            env={"PATH": "/usr/bin:/bin", "TMPDIR": "/private/tmp"})
    try:
        initialized = call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "dtw-probe", "version": "1"}}})
        tools = call(proc, {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
        names = [tool["name"] for tool in tools["result"]["tools"]]
        print(json.dumps({"server": initialized["result"]["serverInfo"], "tools": names}))
        assert names == ["exec"], names
        values = []
        for ident, session in [(3, "probe-a"), (5, "probe-a"), (7, "probe-b")]:
            started = exec_call(proc, ident, {"session": session, "script": "state.count = (state.count || 0) + 1; return state.count;"})
            if "execution_id" not in started:
                print(json.dumps({"run_refusal": started}))
                break
            done = result(proc, ident + 1, session, started["execution_id"])
            values.append({"session": session, "status": done["status"], "value": done.get("value"), "error_code": done.get("error_code"), "text": done.get("text", "")[:300]})
            if done["status"] != "completed":
                raise AssertionError(f"state probe failed: {values[-1]}")
        print(json.dumps({"state_probe": values}))
        assert [entry["value"] for entry in values] == [1, 2, 1], values
        listed = exec_call(proc, 9, {"session": "probe-a", "script": "return (await cua.listApps()).length;"})
        count = result(proc, 10, "probe-a", listed["execution_id"])
        print(json.dumps({"app_count_probe": {"status": count["status"], "value": count.get("value"), "error_code": count.get("error_code"), "text": count.get("text", "")[:300]}}))
        assert count["status"] == "completed", count
        permissions = exec_call(proc, 11, {"operation": "permissions"})
        print(json.dumps({"permissions": permissions}))
        if "--cursor" in sys.argv[2:]:
            cursor_results = []
            for ident, enabled in [(12, None), (14, True), (16, None), (18, False)]:
                args = {"session": "probe-a", "operation": "cursor"}
                if enabled is not None:
                    args["enabled"] = enabled
                started = exec_call(proc, ident, args)
                done = result(proc, ident + 1, "probe-a", started["execution_id"])
                detail = exec_call(proc, ident + 20, {"session": "probe-a", "operation": "result", "execution_id": started["execution_id"], "part": "structured"})
                cursor_results.append({"enabled_request": enabled, "status": done["status"],
                                       "error_code": done.get("error_code"), "text": done.get("text", "")[:400],
                                       "structured": json.loads(detail["page"]["data"])})
            print(json.dumps({"cursor_probe": cursor_results}))
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        stderr = proc.stderr.read()
        if stderr:
            print(json.dumps({"stderr_tail": stderr[-1600:]}))
        print(json.dumps({"exit_code": proc.returncode}))


if __name__ == "__main__":
    main()
