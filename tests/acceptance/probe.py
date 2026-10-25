#!/usr/bin/env python3
"""Read-only/JS-only MCP probe. No desktop input or private data access."""

import json
import os
from queue import Empty, Queue
import subprocess
import sys
from threading import Thread
import time


def runtime_env():
    """Keep OS-required variables while proving that Node is not on PATH."""
    env = os.environ.copy()
    if os.name == "nt":
        system_root = env.get("SystemRoot", r"C:\Windows")
        env["PATH"] = os.pathsep.join([
            os.path.join(system_root, "System32"), system_root,
            os.path.join(system_root, "System32", "WindowsPowerShell", "v1.0"),
        ])
    else:
        env["PATH"] = "/usr/bin:/bin"
    return env


def call(proc, request, timeout=20):
    # Windows select() only accepts sockets, not subprocess pipes.
    if not hasattr(proc, "_dtw_responses"):
        proc._dtw_responses = Queue()

        def read_lines():
            try:
                for line in proc.stdout:
                    proc._dtw_responses.put(json.loads(line))
            except Exception as error:
                proc._dtw_responses.put(error)
            finally:
                proc._dtw_responses.put(EOFError("MCP stdout closed"))

        Thread(target=read_lines, daemon=True).start()
    proc.stdin.write(json.dumps(request) + "\n")
    proc.stdin.flush()
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            response = proc._dtw_responses.get(timeout=min(0.2, max(0.001, deadline - time.monotonic())))
        except Empty:
            continue
        if isinstance(response, Exception):
            raise RuntimeError(f"MCP exited {proc.poll()}: {response}") from response
        if response.get("method") == "elicitation/create" and hasattr(proc, "_dtw_elicitation"):
            answer = proc._dtw_elicitation(response)
            if answer is not None:
                proc.stdin.write(json.dumps({"jsonrpc":"2.0", "id":response["id"], **answer}) + "\n")
                proc.stdin.flush()
            continue
        if response.get("id") == request["id"]:
            return response
    raise TimeoutError(f"MCP request {request['id']} timed out")


def exec_call(proc, ident, args):
    response = call(proc, {"jsonrpc": "2.0", "id": ident, "method": "tools/call", "params": {"name": "exec", "arguments": args}})
    return parse_exec_result(response["result"])


def parse_exec_result(result):
    """Test adapter for the model-facing text receipt and explicit JSON pages."""
    text = result["content"][0]["text"]
    try:
        value = json.loads(text)
        if isinstance(value, dict):
            value.setdefault("is_error", result.get("isError", False))
            return value
    except json.JSONDecodeError:
        pass
    first, _, body = text.partition("\n")
    value = {"text": body or text, "is_error": result.get("isError", False)}
    if first.startswith("id="):
        for word in first.split():
            if "=" in word:
                key, val = word.split("=", 1)
                value[key] = val
            elif word == "cancel_requested":
                value["cancel_requested"] = True
        for line in body.splitlines():
            if line.startswith("return value: "):
                try:
                    value["value"] = json.loads(line[len("return value: "):])
                except json.JSONDecodeError:
                    pass
    elif result.get("isError"):
        value["error"] = text
    return value


def result(proc, ident, session, id):
    for _ in range(100):
        current = exec_call(proc, ident, {"operation": "result", "id": id})
        if current["status"] != "running":
            return current
        time.sleep(0.1)
    raise TimeoutError(f"execution {id} did not finish")


def structured_result(proc, ident, session, id):
    chunks = []
    offset = 0
    while True:
        receipt = exec_call(proc, ident, {"operation": "result",
                                          "id": id, "part": "structured", "offset": offset})
        page = receipt["page"]
        chunks.append(page["data"])
        offset = page["next_offset"]
        if offset is None:
            return json.loads("".join(chunks))


def focus_verified(native, window_id):
    if "exact_window_effect" in native:
        return native["exact_window_effect"].get("verified") is True and native.get("observed", {}).get("focused_window_id") == window_id
    return (native.get("landed_on_target") is True and
            native.get("now_fg_hwnd") == native.get("target_hwnd") and
            int(native.get("now_fg_hwnd", "0"), 16) == window_id)


def main():
    binary = sys.argv[1]
    proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                            env=runtime_env(), encoding="utf-8")
    try:
        initialized = call(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "dtw-probe", "version": "1"}}})
        tools = call(proc, {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
        names = [tool["name"] for tool in tools["result"]["tools"]]
        print(json.dumps({"server": initialized["result"]["serverInfo"], "tools": names}))
        assert names == ["exec"], names
        values = []
        for ident, session in [(3, "probe-a"), (5, "probe-a"), (7, "probe-b")]:
            started = exec_call(proc, ident, {"script": "state.count = (state.count || 0) + 1; return state.count;"})
            if "id" not in started:
                print(json.dumps({"run_refusal": started}))
                break
            done = result(proc, ident + 1, session, started["id"])
            values.append({"status": done["status"], "value": done.get("value"), "error_code": done.get("error_code"), "text": done.get("text", "")[:300]})
            if done["status"] != "completed":
                raise AssertionError(f"state probe failed: {values[-1]}")
        print(json.dumps({"state_probe": values}))
        assert [entry["value"] for entry in values] == [1, 2, 3], values
        listed = exec_call(proc, 9, {"script": "return (await cua.listApps()).length;"})
        count = result(proc, 10, "probe-a", listed["id"])
        print(json.dumps({"app_count_probe": {"status": count["status"], "value": count.get("value"), "error_code": count.get("error_code"), "text": count.get("text", "")[:300]}}))
        assert count["status"] == "completed", count
        permissions = exec_call(proc, 11, {"operation": "permissions"})
        print(json.dumps({"permissions": permissions}))
        assert not permissions.get("is_error") and permissions.get("text"), permissions
        if "--cursor" in sys.argv[2:]:
            cursor_results = []
            for ident, enabled in [(12, None), (14, True), (16, None), (18, False)]:
                args = {"operation": "cursor"}
                if enabled is not None:
                    args["enabled"] = enabled
                started = exec_call(proc, ident, args)
                done = result(proc, ident + 1, "probe-a", started["id"])
                detail = exec_call(proc, ident + 20, {"operation": "result", "id": started["id"], "part": "structured"})
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
