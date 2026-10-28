#!/usr/bin/env python3
"""Opt-in standard MCP + real macOS GUI probe, using only owned AppKit fixtures.

Run with exclusive access to the test desktop:
  python3 tests/acceptance/standard_agent_probe.py --dtw /path/to/dtw --output /tmp/new-run
Add --pixel-click only when the fixture can remain foreground and unobscured.
The probe builds two isolated app bundles from native-core/main.swift, calls the
ordinary stdio MCP interface, and records independent fixture state. It never
installs Agent configuration or retries an action. This is external MCP-client
and native GUI evidence, not a live LLM-host acceptance test.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import plistlib
import queue
import shutil
import subprocess
import sys
import threading
import time
import traceback
import uuid


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def wait_until(predicate, timeout=5):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.02)
    raise AssertionError("condition did not settle before deadline")


def stop_owned(child):
    """Only signal the exact child created by this invocation."""
    forced = []
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        forced.append("terminate")
        child.terminate()
        try:
            child.wait(timeout=2)
        except subprocess.TimeoutExpired:
            forced.append("kill")
            child.kill()
            child.wait(timeout=2)
    return {"pid": child.pid, "returncode": child.returncode, "forced": forced}


class Fixture:
    def __init__(self, root, bundle_id, executable):
        self.root, self.app_id = root, bundle_id
        self.executable = executable
        self.child = None
        self.log = None

    @classmethod
    def prepare(cls, root, bundle_id, source, compiled=None):
        macos = root / "DTWNativeFixture.app/Contents/MacOS"
        macos.mkdir(parents=True)
        executable = macos / "DTWNativeFixture"
        metadata = {"CFBundleExecutable": executable.name,
                    "CFBundleIdentifier": bundle_id,
                    "CFBundleName": "DTW Standard Probe",
                    "CFBundlePackageType": "APPL",
                    "CFBundleVersion": "1",
                    "NSHighResolutionCapable": True}
        with (macos.parent / "Info.plist").open("wb") as stream:
            plistlib.dump(metadata, stream)
        if compiled is None:
            with (root / "build.log").open("w") as log:
                subprocess.run(["xcrun", "swiftc", "-module-cache-path", str(root / "swift-cache"),
                                str(source), "-o", str(executable)],
                               stdout=log, stderr=subprocess.STDOUT, check=True, timeout=90)
        else:
            shutil.copy2(compiled, executable)
        return cls(root, bundle_id, executable)

    def start(self):
        self.log = (self.root / "fixture.log").open("w")
        # Direct launch retains a Popen owner for this exact executable/PID.
        self.child = subprocess.Popen([str(self.executable), str(self.root)],
                                      stdout=self.log, stderr=subprocess.STDOUT)
        state = wait_until(lambda: self.state() if (self.root / "state.json").exists() else None,
                           timeout=10)
        require(state["pid"] == self.child.pid, "fixture state does not belong to the launched PID")
        return state

    def state(self):
        state = json.loads((self.root / "state.json").read_text())
        if self.child is not None:
            require(state["pid"] == self.child.pid, "fixture PID changed")
        return state

    def counters(self):
        state = self.state()
        return {key: state[key] for key in ("clicks", "actions_started")}

    def command(self, command):
        require(self.child.poll() is None, "owned fixture already exited")
        (self.root / "command").write_text(command)
        acknowledged = wait_until(lambda: self.state() if self.state()["command"] == command else None)
        if command == "uncover":
            target_id = next(w["id"] for w in acknowledged["windows"] if w["title"] == "DTW Native Target")
            # An activation request is asynchronous. Its command acknowledgement
            # does not prove that the owned foreground test condition holds.
            # This waits only on fixture state, without warming up AX or DTW.
            def foreground():
                state = self.state()
                return state if state["active"] and state["key_window"] == target_id else None
            return wait_until(foreground)
        return acknowledged

    def close(self):
        try:
            if self.child is None:
                return {"started": False}
            if self.child.poll() is None:
                (self.root / "command").write_text("quit")
            return stop_owned(self.child)
        finally:
            if self.log is not None:
                self.log.close()


def process_table():
    rows = subprocess.check_output(["ps", "-axo", "pid=,ppid=,command="], text=True).splitlines()
    result = {}
    for row in rows:
        fields = row.strip().split(None, 2)
        if len(fields) == 3:
            result[int(fields[0])] = (int(fields[1]), fields[2])
    return result


class Mcp:
    def __init__(self, binary, output):
        self.output = output
        self.serial = 0
        self.messages = queue.Queue()
        self.pending = {}
        # Fixture case names never go over the tool boundary. DTW supplies IDs.
        self.executions = {}
        self.exec_frames = {}
        self.exec_summaries = {}
        self.projection_checks = []
        self.lock = threading.Lock()
        self.transcript = (output / "mcp.jsonl").open("w")
        self.stderr = (output / "dtw-stderr.log").open("w")
        try:
            self.child = subprocess.Popen([str(binary), "mcp"], stdin=subprocess.PIPE,
                                          stdout=subprocess.PIPE, stderr=self.stderr,
                                          text=True, bufsize=1)
        except BaseException:
            self.stderr.close()
            self.transcript.close()
            raise
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    def _record(self, direction, message):
        with self.lock:
            self.transcript.write(json.dumps({"direction": direction,
                                             "at_unix_ms": int(time.time() * 1000),
                                             "message": message}, ensure_ascii=False) + "\n")
            self.transcript.flush()

    def _read(self):
        try:
            for line in self.child.stdout:
                message = json.loads(line)
                self._record("response", message)
                self.messages.put(message)
        except Exception as error:
            self.messages.put(error)
        finally:
            self.messages.put(EOFError("MCP stdout closed"))

    def notify(self, method, params=None):
        message = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            message["params"] = params
        self._send(message)

    def _send(self, message):
        self._record("request", message)
        self.child.stdin.write(json.dumps(message) + "\n")
        self.child.stdin.flush()

    def begin(self, method, params):
        self.serial += 1
        request_id = self.serial
        self._send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        return request_id

    def receive(self, request_id, timeout=35):
        if request_id in self.pending:
            return self.pending.pop(request_id)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            message = self.messages.get(timeout=max(0.001, deadline - time.monotonic()))
            if isinstance(message, Exception):
                raise message
            if message.get("id") == request_id:
                return message
            if "id" in message:
                # A cancelled exec may still deliver its response; preserve it.
                self.pending[message["id"]] = message
        raise TimeoutError(f"MCP response {request_id} timed out")

    def rpc(self, method, params):
        reply = self.receive(self.begin(method, params))
        require("error" not in reply, f"JSON-RPC error: {reply}")
        return reply["result"]

    @staticmethod
    def decode(result):
        data = result.get("structuredContent")
        texts = [item["text"] for item in result.get("content", []) if item.get("type") == "text"]
        require(isinstance(data, dict) and len(texts) == 1,
                "expected structuredContent plus one JSON text")
        require(json.loads(texts[0]) == data, "text and structured result differ")
        return data

    def tool(self, name, arguments):
        result = self.rpc("tools/call", {"name": name, "arguments": arguments})
        return self.decode(result), result

    def execute(self, app, case, source, **options):
        params = {"name": "exec", "arguments": {"app": app, "source": source, **options}}
        rpc_id = self.begin("tools/call", params)
        response = self.receive(rpc_id)
        require("error" not in response, response)
        result = response["result"]
        data = self.decode(result)
        require(data.get("app") == app and isinstance(data.get("execution_id"), str),
                "exec did not return its app and generated execution ID")
        require("request_id" not in data, "internal request ID leaked")
        self.executions[(app, case)] = data["execution_id"]
        self.exec_frames[(app, case)] = {"jsonrpc": "2.0", "id": rpc_id, "method": "tools/call", "params": params}
        self.exec_summaries[(app, case)] = data
        # Acceptance inspects the authoritative ledger explicitly. Ordinary
        # Agent workflows consume exec directly and do not need this query.
        full = self.receipt(app, case)
        require("steps" not in data.get("script", {}), "exec leaked the full ledger")
        for key in ("app", "execution_id", "status", "value", "error", "context_reset", "state_reset", "payload_expired"):
            require(data.get(key) == full.get(key), f"projection changed {key}")
        if "script" in full:
            require(data["script"]["summary"]["steps"] == len(full["script"]["steps"]), "summary lost steps")
            for key in ("failure", "state_committed", "shutdown_error"):
                require(data["script"].get(key) == full["script"].get(key), f"projection changed {key}")
        self.projection_checks.append({"case":case, "app":app,
            "compact_json_bytes":len(json.dumps(data, separators=(",", ":"), ensure_ascii=False).encode()),
            "full_json_bytes":len(json.dumps(full, separators=(",", ":"), ensure_ascii=False).encode()),
            "summary":data.get("script", {}).get("summary")})
        return full, result

    def receipt(self, app, case=None):
        arguments = {"app": app}
        execution_id = self.executions.get((app, case)) if case is not None else None
        if execution_id is not None:
            arguments["execution_id"] = execution_id
        data, _ = self.tool("result", arguments)
        require(data.get("app") == app and isinstance(data.get("execution_id"), str),
                "result did not identify the recorded execution")
        if execution_id is not None:
            require(data["execution_id"] == execution_id, "result changed the execution identity")
        elif case is not None:
            self.executions[(app, case)] = data["execution_id"]
        return data

    def close(self):
        workers = {pid: command for pid, (parent, command) in process_table().items()
                   if parent == self.child.pid and any(flag in command for flag in
                        ("--dtw-native-worker", "--dtw-scene-worker"))}
        try:
            try:
                self.child.stdin.close()
            except (BrokenPipeError, OSError):
                # Still reap this exact process when the transport already failed.
                pass
            result = stop_owned(self.child)
            self.reader.join(timeout=2)
            require(not self.reader.is_alive(), "MCP reader did not exit")
            def workers_reaped():
                current = process_table()
                return all(current.get(pid, (None, None))[1] != command
                           for pid, command in workers.items())
            wait_until(workers_reaped, timeout=5)
            result["native_workers"] = list(workers)
            result["native_workers_reaped"] = True
            return result
        finally:
            self.stderr.close()
            if not self.reader.is_alive():
                self.transcript.close()


def successful(data):
    require(data.get("status") == "completed" and not data.get("error"), data)
    require(data.get("script", {}).get("state_committed") is True, data)
    return data["value"]["result"]


def no_dispatch(data):
    require(bool(data.get("error")), data)
    steps = data.get("script", {}).get("steps", [])
    for step in steps:
        if step.get("receipt"):
            require(step["receipt"]["outcome"]["dispatch"] == "not_dispatched", data)
    return data.get("error")


SELECT_WINDOW = """
const selected=(await app.windows()).windows.filter(w=>w.title==="DTW Native Target");
if(selected.length!==1) throw new Error("owned target window is ambiguous");
const window_ref=selected[0].window_ref;
"""
SEMANTIC = SELECT_WINDOW + """
const before=await app.observe({window_ref});
const matches=before.elements.filter(e=>e.role==="button" && e.name==="Increment"
  && e.enabled===true && e.actions.includes("press"));
if(matches.length!==1) throw new Error("no unique structured Increment button");
const button=matches[0];
state.lastSemantic={window_ref,observation_ref:before.observation_ref,element_ref:button.element_ref};
const action=await app.press(state.lastSemantic);
const after=await app.observe({window_ref});
state.saved="before-cancel";
return {button,action,after,old:state.lastSemantic};
"""
CAPTURE = SELECT_WINDOW + """
const shot=await app.capture({window_ref});
state.lastCapture=shot;
state.saved="before-cancel";
return shot;
"""
CANCEL = """
await app.windows();
state.uncommitted="must-not-survive";
while(true) {}
"""


def save_png(output, result, metadata):
    images = [item for item in result.get("content", []) if item.get("type") == "image"]
    require(len(images) == 1 and images[0].get("mimeType") == "image/png",
            "expected one MCP PNG image")
    pixels = base64.b64decode(images[0]["data"], validate=True)
    require(len(pixels) > 33 and pixels.startswith(b"\x89PNG\r\n\x1a\n")
            and pixels[12:16] == b"IHDR", "invalid PNG header")
    width, height = int.from_bytes(pixels[16:20], "big"), int.from_bytes(pixels[20:24], "big")
    require(width > 0 and height > 0, "empty PNG dimensions")
    require([width, height] == [metadata["image"]["width"], metadata["image"]["height"]],
            "PNG dimensions differ from script metadata")
    path = output / "capture.png"
    path.write_bytes(pixels)
    return {"path": str(path), "sha256": hashlib.sha256(pixels).hexdigest(),
            "bytes": len(pixels), "width": width, "height": height}


def run_checks(mcp, target, peer, output, pixel_click, evidence):
    initialized = mcp.rpc("initialize", {"protocolVersion": "2025-06-18",
                                        "capabilities": {},
                                        "clientInfo": {"name": "dtw-standard-agent-probe", "version": "1"}})
    require(initialized["protocolVersion"] == "2025-06-18", initialized)
    mcp.notify("notifications/initialized")
    tools = mcp.rpc("tools/list", {})["tools"]
    by_name = {tool["name"]: tool for tool in tools}
    require(set(by_name) == {"apps", "exec", "result"}, tools)
    for name in ("exec", "result"):
        schema = by_name[name]["inputSchema"]
        require(schema["properties"]["app"]["type"] == "string" and "app" in schema["required"], schema)
    require(set(by_name["exec"]["inputSchema"]["required"]) == {"app", "source"}, "exec requires bookkeeping")
    require(set(by_name["exec"]["inputSchema"]["properties"]) == {"app", "source"}, "exec exposes extra input")
    require(by_name["result"]["inputSchema"]["required"] == ["app"], "latest result requires an execution ID")
    discovered, _ = mcp.tool("apps", {})
    entries = discovered["apps"]
    ids = [entry["app"] for entry in entries]
    require(len(ids) == len(set(ids)), "apps did not deduplicate app IDs")
    require(not any(set(entry) & {"pid", "generation", "identity", "handle"} for entry in entries),
            "apps exposed internal process identity")
    for fixture in (target, peer):
        require(ids.count(fixture.app_id) == 1, f"owned fixture missing from apps: {fixture.app_id}")
    require(set(discovered["script"]["methods"]) == {"windows", "observe", "capture", "press", "click"},
            "script methods are not discoverable")
    require("request_id" not in discovered["script"]["entry"], "discovery advertised a model request ID")
    evidence["initialize"], evidence["tools"], evidence["apps"] = initialized, tools, discovered
    write_json(output / "evidence.json", evidence)

    target.command("uncover")
    before, peer_before = target.counters(), peer.counters()
    semantic, _ = mcp.execute(target.app_id, "semantic", SEMANTIC)
    value = successful(semantic)
    require(value["action"]["outcome"]["dispatch"] == "dispatched", value)
    require(value["action"]["outcome"]["evidence"]["kind"] == "unverified", value)
    wait_until(lambda: target.counters()["clicks"] == before["clicks"] + 1)
    require(target.counters()["actions_started"] == before["actions_started"] + 1,
            "semantic action repeated")
    require(peer.counters() == peer_before, "semantic action changed the other owned app")
    require(mcp.receipt(target.app_id, "semantic") == semantic, "original semantic receipt changed")
    latest = mcp.receipt(target.app_id)
    require(latest.pop("selection", None) == "latest_accepted", "latest lookup is not explicitly identified")
    require(latest == semantic, "latest result differs from the accepted execution")
    # Transport retry reuses the standard RPC id, without a model-supplied key.
    frame = mcp.exec_frames[(target.app_id, "semantic")]
    mcp._send(frame)
    replay = mcp.decode(mcp.receive(frame["id"])["result"])
    require(replay == mcp.exec_summaries[(target.app_id, "semantic")] and target.counters()["clicks"] == before["clicks"] + 1,
            "same RPC frame executed the action again")
    evidence["semantic"] = {"record": semantic, "before": before, "after": target.counters(),
                            "peer_unchanged": True, "original_result_preserved": True,
                            "same_rpc_replay_preserved": True, "latest_lookup_preserved": True}
    write_json(output / "evidence.json", evidence)

    stable = target.counters()
    stale, _ = mcp.execute(target.app_id, "stale-reference", "return await app.press(state.lastSemantic);")
    no_dispatch(stale)
    foreign_source = "return await app.press(" + json.dumps(value["old"]) + ");"
    foreign, _ = mcp.execute(peer.app_id, "wrong-app-reference", foreign_source)
    no_dispatch(foreign)
    unavailable, _ = mcp.execute("dev.dtw.absent." + uuid.uuid4().hex, "absent-app", foreign_source)
    no_dispatch(unavailable)
    require(unavailable["error"]["code"] == "app_unavailable", unavailable)
    require(target.counters() == stable and peer.counters() == peer_before,
            "rejected reference or app request changed a fixture")
    evidence["negative_controls"] = {"stale": stale, "wrong_app_reference": foreign,
                                     "absent_app": unavailable, "counters_unchanged": True}

    target.command("snapshot")
    target.command("uncover")
    capture, capture_reply = mcp.execute(target.app_id, "capture", CAPTURE)
    shot = successful(capture)
    require(shot["image"]["coordinate_space"] == "image_pixels", shot)
    png = save_png(output, capture_reply, shot)
    evidence["capture"] = {"record": capture, "png": png}
    write_json(output / "evidence.json", evidence)

    if pixel_click:
        # The owned fixture's button center is (160, 90) from the content's
        # bottom left; native-core/main.swift uses 320px-wide content.
        width, height = png["width"], png["height"]
        x, y = width / 2, height - 90 * width / 320
        require(0 <= x < width and 0 <= y < height, "fixture point falls outside its image")
        source = ("const s=state.lastCapture; return await app.click({window_ref:s.window_ref,"
                  "observation_ref:s.observation_ref,x:" + json.dumps(x) + ",y:" + json.dumps(y) + "});")
        clicked, _ = mcp.execute(target.app_id, "pixel-click", source)
        action = successful(clicked)
        require(action["outcome"]["dispatch"] == "dispatched", clicked)
        wait_until(lambda: target.counters()["clicks"] == stable["clicks"] + 1)
        require(target.counters()["actions_started"] == stable["actions_started"] + 1,
                "pixel action repeated")
        require(peer.counters() == peer_before, "pixel action changed peer")
        evidence["pixel_click"] = {"record": clicked, "before": stable, "after": target.counters(),
                                    "point": {"x": x, "y": y}}
    else:
        evidence["pixel_click"] = {"status": "not_run", "reason": "--pixel-click not selected"}
    write_json(output / "evidence.json", evidence)

    persisted, _ = mcp.execute(target.app_id, "state-before-cancel", "return state.saved;")
    require(successful(persisted) == "before-cancel", persisted)
    unchanged = target.counters()
    rpc_id = mcp.begin("tools/call", {"name": "exec", "arguments": {
        "app": target.app_id, "source": CANCEL}})
    def native_read_finished():
        record = mcp.receipt(target.app_id, "cancel-original")
        require(record.get("status") == "running", f"cancel sample ended before notification: {record}")
        steps = record.get("script", {}).get("steps", [])
        return record if any(step["operation"] == "windows" and step["status"] == "completed"
                             for step in steps) else None
    entered = wait_until(native_read_finished, timeout=4)
    mcp.notify("notifications/cancelled", {"requestId": rpc_id, "reason": "Owned fixture cancellation probe"})
    def cancelled_result():
        record = mcp.receipt(target.app_id, "cancel-original")
        return record if record.get("status") == "completed" else None
    original = wait_until(cancelled_result, timeout=5)
    require(original.get("error") and original["script"]["failure"]["kind"] == "cancelled", original)
    require(original["script"]["state_committed"] is False, original)
    require(original["script"].get("shutdown_error") is None, original)
    require(mcp.receipt(target.app_id, "cancel-original") == original, "cancel receipt changed")
    restored, _ = mcp.execute(target.app_id, "after-cancel",
                              "const keys=Object.keys(state); return {keys,saved:state.saved,inventory:await app.windows()};")
    restored_value = successful(restored)
    require(restored_value["saved"] == "before-cancel" and "uncommitted" not in restored_value["keys"], restored)
    require(restored.get("context_reset") is True and restored.get("state_reset") is False, restored)
    require(mcp.receipt(target.app_id, "cancel-original") == original, "new context replaced old receipt")
    counters = {"target_before": unchanged, "target_after": target.counters(),
                "peer_before": peer_before, "peer_after": peer.counters()}
    counters_unchanged = counters["target_before"] == counters["target_after"] and counters["peer_before"] == counters["peer_after"]
    evidence["cancellation"] = {"rpc_id": rpc_id, "entered": entered, "original": original,
                                "recovery": restored, "committed_state_preserved": True,
                                "counters": counters, "counters_unchanged": counters_unchanged}
    write_json(output / "evidence.json", evidence)
    require(counters_unchanged, f"cancel or recovery counter change: {counters}")

    repeated_source = "state.repeated=(state.repeated??0)+1;return state.repeated;"
    first, _ = mcp.execute(target.app_id, "same-source-1", repeated_source)
    second, _ = mcp.execute(target.app_id, "same-source-2", repeated_source)
    require(successful(first) == 1 and successful(second) == 2, "new call was deduplicated by script content")
    require(first["execution_id"] != second["execution_id"], "execution ID reused for a new call")
    evidence["execution_identity"] = {"model_supplied_ids": False, "same_source_runs_twice": True,
                                      "ids_distinct": True}

    # Logical app state and window refs survive A→B→A. Each short-lived scene
    # owner is reaped independently of the resident app/AX context.
    sequence = []
    for index, fixture in enumerate((target, peer, target)):
        fixture.command("snapshot")
        fixture.command("uncover")
        request_id = f"app-switch-capture-{index}"
        source = "const previous=state.switchMarker ?? null;\n" + (
            SELECT_WINDOW if index < 2 else "const window_ref=state.switchWindow;\n") + """
const shot=await app.capture({window_ref});
state.switchMarker=true; state.switchWindow=window_ref;
return {previous,shot};
"""
        record, response = mcp.execute(fixture.app_id, request_id, source)
        returned = successful(record)
        require(returned["previous"] == (True if index == 2 else None), "app state lost or leaked")
        require(record.get("context_reset") is False, "app switch unnecessarily retired native refs")
        require(record.get("state_reset") is False, "app switch reset logical state")
        if index == 2:
            require(returned["shot"]["window_ref"] == sequence[0]["record"]["value"]["result"]["shot"]["window_ref"],
                    "app switch replaced the retained window ref")
        directory = output / request_id
        directory.mkdir()
        picture = save_png(directory, response, returned["shot"])
        sequence.append({"app": fixture.app_id, "record": record, "png": picture})
    require(mcp.receipt(target.app_id, "semantic") == semantic,
            "app switching replaced the original action receipt")
    require(target.counters() == unchanged and peer.counters() == peer_before,
            "cross-app capture performed an action")
    evidence["app_switch_capture"] = {"sequence": sequence, "old_receipt_preserved": True,
                                      "state_and_window_refs_preserved": True, "counters_unchanged": True}
    run_session_checks(mcp, target, peer, output, evidence)
    expected = {key: before[key] + 1 + int(pixel_click) for key in before}
    require(target.counters() == expected and peer.counters() == peer_before,
            "final independent counters show an extra or missing effect")
    evidence["fixture_final"] = {"target": target.state(), "peer": peer.state(),
                                 "expected_target_counters": expected}
    write_json(output / "evidence.json", evidence)


def run_session_checks(first, target, peer, output, evidence):
    """Two ordinary MCP processes; no Host-specific coordination or input."""
    from concurrent.futures import ThreadPoolExecutor
    root = output / "second-session"
    root.mkdir()
    second = Mcp(Path(evidence["binary"]), root)
    record = {"second_pid": second.child.pid}
    counters = (target.counters(), peer.counters())
    evidence["independent_sessions"] = record
    try:
        second.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "dtw-second-session-probe", "version": "1"}})
        second.notify("notifications/initialized")
        isolated, _ = second.execute(target.app_id, "after-cancel", "const before=Object.keys(state);state.otherSession=true;return before;")
        require(successful(isolated) == [], "second session inherited first session state")
        original = successful(first.execute(target.app_id, "session-isolation", "return {saved:state.saved,leaked:state.otherSession??null};")[0])
        require(original == {"saved": "before-cancel", "leaked": None}, original)
        setup, _ = second.execute(peer.app_id, "second-window", SELECT_WINDOW + "state.window=window_ref;return window_ref;")
        successful(setup)
        captures = []
        with ThreadPoolExecutor(max_workers=2) as pool:
            for round_number in range(3):
                requests = [(first, target, "state.switchWindow"), (second, peer, "state.window")]
                futures = [pool.submit(client.execute, fixture.app_id, f"concurrent-{round_number}",
                           f"return await app.capture({{window_ref:{window}}});")
                           for client, fixture, window in requests]
                for index, future in enumerate(futures):
                    result, response = future.result(timeout=35)
                    metadata = successful(result)
                    destination = root / f"capture-{round_number}-{index}"
                    destination.mkdir()
                    captures.append({"session": index, "round": round_number,
                                     "png": save_png(destination, response, metadata)})
        record.update({"same_app_state_isolated": True, "distinct_app_captures": captures})
    finally:
        record["cleanup"] = second.close()
        write_json(output / "evidence.json", evidence)
    require(not record["cleanup"].get("forced") and record["cleanup"].get("returncode", 0) == 0,
            "second session did not close cleanly")
    surviving, response = first.execute(target.app_id, "after-second-session-close",
                                        "return await app.capture({window_ref:state.switchWindow});")
    destination = root / "first-session-after-peer-close"
    destination.mkdir()
    record["first_session_survives_peer_close"] = save_png(destination, response, successful(surviving))
    require((target.counters(), peer.counters()) == counters, "read-only session checks changed fixture counters")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dtw", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="New directory; never overwritten")
    parser.add_argument("--pixel-click", action="store_true", help="Also click the owned foreground fixture once")
    args = parser.parse_args()
    require(sys.platform == "darwin", "requires real macOS GUI")
    binary = args.dtw.resolve(strict=True)
    require(binary.is_file(), "--dtw must name an existing executable")
    output = args.output.absolute()
    output.mkdir(mode=0o700)
    source = Path(__file__).resolve().parents[1] / "fixtures/native-core/main.swift"
    evidence = {"kind": "standard-external-mcp-client-and-native-gui", "live_llm_host": False,
                "binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "fixture_source": str(source), "fixture_source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                "pixel_click_requested": args.pixel_click, "started_unix_ms": int(time.time() * 1000)}
    fixtures, mcp = [], None
    cleanup = []
    failure = None
    try:
        run_id = uuid.uuid4().hex
        target = Fixture.prepare(output / "target", "dev.dtw.standard.target." + run_id, source)
        fixtures.append(target)
        peer = Fixture.prepare(output / "peer", "dev.dtw.standard.peer." + run_id, source, target.executable)
        fixtures.append(peer)
        evidence["fixture_start"] = {"target": target.start(), "peer": peer.start(),
                                     "target_app": target.app_id, "peer_app": peer.app_id}
        write_json(output / "evidence.json", evidence)
        mcp = Mcp(binary, output)
        evidence["dtw_pid"] = mcp.child.pid
        run_checks(mcp, target, peer, output, args.pixel_click, evidence)
    except BaseException as error:
        failure = {"type": type(error).__name__, "message": str(error)}
        (output / "failure.txt").write_text(traceback.format_exc())
    finally:
        for name, resource in [("dtw", mcp), *[(f.app_id, f) for f in reversed(fixtures)]]:
            if resource is None:
                continue
            try:
                result = resource.close()
                cleanup.append({"resource": name, **result})
                if result.get("forced") or result.get("returncode", 0) != 0:
                    failure = failure or {"type": "CleanupFailure", "message": f"unclean exit: {name}"}
            except BaseException as error:
                cleanup.append({"resource": name, "error": str(error)})
                failure = failure or {"type": "CleanupFailure", "message": str(error)}
        if mcp is not None:
            evidence["exec_projection"] = mcp.projection_checks
        evidence["cleanup"] = cleanup
        evidence["failure"] = failure
        evidence["passed"] = failure is None
        evidence["finished_unix_ms"] = int(time.time() * 1000)
        write_json(output / "evidence.json", evidence)
    print(json.dumps({"passed": failure is None, "evidence": str(output / "evidence.json"),
                      "checks": [key for key in ("semantic", "execution_identity", "negative_controls", "capture", "pixel_click",
                                                 "cancellation", "app_switch_capture", "independent_sessions") if key in evidence],
                      "pixel_click": args.pixel_click, "failure": failure}, ensure_ascii=False))
    return 0 if failure is None else 1


if __name__ == "__main__":
    sys.exit(main())
