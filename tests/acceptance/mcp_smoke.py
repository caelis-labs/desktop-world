#!/usr/bin/env python3
"""Cross-platform, desktop-independent smoke check for a packaged DTW worker."""

import json
from pathlib import Path
from queue import Empty, Queue
import subprocess
import sys
from threading import Thread

from probe import parse_exec_result


def main():
    binary = Path(sys.argv[1]).resolve()
    process = subprocess.Popen(
        [str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, bufsize=1,
    )
    responses = Queue()

    def read_lines():
        for line in process.stdout:
            responses.put(json.loads(line))

    reader = Thread(target=read_lines, daemon=True)
    reader.start()
    request_id = 0

    def call(method, params):
        nonlocal request_id
        request_id += 1
        process.stdin.write(json.dumps({
            "jsonrpc": "2.0", "id": request_id,
            "method": method, "params": params,
        }) + "\n")
        process.stdin.flush()
        try:
            response = responses.get(timeout=30)
        except Empty as error:
            raise TimeoutError(f"{method} timed out; process exit={process.poll()}") from error
        assert response.get("id") == request_id, response
        assert "error" not in response, response
        return response["result"]

    def exec_tool(arguments):
        result = call("tools/call", {"name": "exec", "arguments": arguments})
        assert not result.get("isError"), result
        return parse_exec_result(result)

    try:
        server = call("initialize", {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "dtw-cross-platform-smoke", "version": "1"},
        })["serverInfo"]
        tools = call("tools/list", {})["tools"]
        assert [tool["name"] for tool in tools] == ["exec"], tools
        assert "session" not in tools[0]["inputSchema"]["properties"]
        values = []
        for session in ("one", "one", "two"):
            response = exec_tool({
                "script": "state.n=(state.n||0)+1; return state.n;",
            })
            if response["status"] == "running":
                original_id = response["id"]
                for _ in range(30):
                    response = exec_tool({
                        "operation": "result",
                        "id": original_id,
                    })
                    if response["status"] != "running":
                        break
            assert response["status"] == "completed", response
            values.append(response["value"])
        assert values == [1, 2, 3], values
        print(json.dumps({"server": server, "tools": ["exec"], "state": values}))
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        if process.returncode != 0:
            raise RuntimeError(f"DTW exited {process.returncode}: {process.stderr.read()[-800:]}")


if __name__ == "__main__":
    main()
