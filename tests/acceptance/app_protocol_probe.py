#!/usr/bin/env python3
"""Opt-in, serial C4 macOS acceptance with a trusted reference Host.

Start two owned native-core fixtures separately; this program never launches an
unrelated app. --host-decision is operator input, never an MCP argument. The
optional Caelis run uses an isolated Store and a temporary plugin. It demonstrates
Host preauthorization, not a production dynamic app-approval UI.

Example (run only with exclusive ownership of the test desktop):
  python3 app_protocol_probe.py /path/dtw /tmp/fixture-a /tmp/fixture-b /tmp/c4-run \
      --host-decision approve --stage semantic

The protocol-specific response shape is kept in Wire for the C4 contract review.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import threading
import time
from urllib.parse import urlsplit
import uuid


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def wait_until(predicate, timeout=5):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        value = predicate()
        if value:
            return value
        time.sleep(0.02)
    raise AssertionError("condition did not become true before deadline")


class Fixture:
    def __init__(self, root):
        self.root = root.resolve(strict=True)
        state = self.state()
        require(isinstance(state["pid"], int), "fixture has no PID")

    def state(self):
        return json.loads((self.root / "state.json").read_text())

    def command(self, command):
        (self.root / "command").write_text(command)
        return wait_until(lambda: self.state() if self.state()["command"] == command else None)

    def counters(self):
        state = self.state()
        return {key: state[key] for key in ("clicks", "actions_started")}


class Host:
    """Authority is held outside the untrusted MCP request stream."""
    def __init__(self, output, app, decision):
        self.output, self.app, self.decision = output, app, decision
        self.record("host_configuration", decision=decision, app=app,
                    source="reference_harness_operator_cli",
                    meaning="Fixture policy; not production dynamic user approval")

    def record(self, event, **fields):
        record = {"event": event, "at_unix_ms": int(time.time() * 1000), **fields}
        with (self.output / "host-decisions.jsonl").open("a") as stream:
            stream.write(json.dumps(record, ensure_ascii=False) + "\n")

    def grant(self, name, app=None, capabilities=None, seconds=120):
        app = self.app if app is None else app
        capabilities = ["observe", "press", "capture", "click"] if capabilities is None else capabilities
        path = self.output / f"{name}.grant.json"
        require(not path.exists(), f"grant file already exists: {path}")
        proposal = {"version": 1, "grant_id": f"c4-{name}-{uuid.uuid4()}", "app": app,
                    "capabilities": capabilities,
                    "expires_at_unix_ms": int(time.time() * 1000) + seconds * 1000}
        allowed = {"observe", "press", "capture", "click"}
        approved = (self.decision == "approve" and app == self.app and bool(capabilities)
                    and len(set(capabilities)) == len(capabilities) and set(capabilities) <= allowed)
        self.record("proposal", proposal=proposal)
        self.record("decision", grant_id=proposal["grant_id"], approved=approved,
                    reason="explicit fixture Host policy" if approved else "outside approved fixture policy")
        if not approved:
            require(not path.exists(), "denied proposal wrote a grant")
            return None
        raw = (json.dumps(proposal, ensure_ascii=False, indent=2) + "\n").encode()
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(raw)
        self.record("grant_written", grant_id=proposal["grant_id"], path=str(path),
                    sha256=hashlib.sha256(raw).hexdigest())
        return path


class Wire:
    """Strict response assertions; do not convert unknown action states to success."""
    @staticmethod
    def payload(reply):
        require("error" not in reply, f"MCP JSON-RPC error: {reply}")
        result = reply["result"]
        value = result.get("structuredContent")
        require(isinstance(value, dict), "protocol result lacks structuredContent")
        texts = [part["text"] for part in result.get("content", []) if part.get("type") == "text"]
        require(len(texts) == 1, "protocol result must contain one JSON text representation")
        require(json.loads(texts[0]) == value, "text and structured protocol evidence differ")
        return value

    @staticmethod
    def app_ref(described):
        return described["app"]["app_ref"]

    @staticmethod
    def terminal(reply):
        require(reply["status"] in ("running", "completed"), reply)
        return reply["status"] == "completed"

    @staticmethod
    def value(reply):
        require(reply["status"] == "completed" and not reply.get("error"), reply)
        return reply["value"]

    @staticmethod
    def target(app_ref, windows):
        matches = [window for window in windows["windows"] if window["title"] == "DTW Native Target"]
        require(len(matches) == 1, f"fixture target not unambiguous: {windows}")
        return {"app_ref": app_ref, "window_ref": matches[0]["window_ref"]}

    @staticmethod
    def press(observation):
        buttons = [element for element in observation["elements"]
                   if element["can_press"] and "Increment" in element["label"]]
        require(len(buttons) == 1, f"Increment not uniquely observed: {observation}")
        return {"kind": "press", "observation_ref": observation["observation_ref"],
                "element_ref": buttons[0]["element_ref"]}

    @staticmethod
    def receipt(reply):
        receipt = reply.get("receipt")
        require(isinstance(receipt, dict), f"missing action receipt: {reply}")
        return receipt

    @staticmethod
    def dispatched(reply):
        receipt = Wire.receipt(reply)
        require(receipt["outcome"]["dispatch"] == "dispatched", reply)
        require(receipt["outcome"]["evidence"]["kind"] == "unverified", reply)

    @staticmethod
    def rejected(reply):
        if reply.get("receipt"):
            require(Wire.receipt(reply)["outcome"]["dispatch"] == "not_dispatched", reply)
        else:
            require(bool(reply.get("error")), f"expected explicit rejection: {reply}")

    @staticmethod
    def rejection_code(reply, code):
        Wire.rejected(reply)
        error = (Wire.receipt(reply)["outcome"].get("error") if reply.get("receipt") else reply.get("error"))
        require(isinstance(error, dict) and error.get("code") == code, reply)


class Session:
    def __init__(self, binary, grant, output, host_parent=False):
        self.output = output
        output.mkdir()
        self.stderr = (output / "stderr.log").open("w")
        self.transcript = (output / "mcp.jsonl").open("w")
        self.serial = 0
        self.last_images = []
        self.messages = queue.Queue()
        command = [str(binary), "app", "serve", "--grant", str(grant)]
        if host_parent:
            # This parent owns only the test server. It inherits the protocol
            # pipes; killing it must exercise dtw's parent watchdog, not EOF.
            command = [sys.executable, "-c", """
from pathlib import Path
import subprocess, sys
server = subprocess.Popen([sys.argv[1], 'app', 'serve', '--grant', sys.argv[2]])
Path(sys.argv[3]).write_text(str(server.pid))
raise SystemExit(server.wait())
""", str(binary), str(grant), str(output / "server.pid")]
        self.child = subprocess.Popen(command,
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      stderr=self.stderr, text=True, bufsize=1)
        self.server_pid = self.child.pid
        threading.Thread(target=self._read, daemon=True).start()
        try:
            if host_parent:
                pid_file = output / "server.pid"
                wait_until(lambda: pid_file.exists() and pid_file.read_text().strip())
                self.server_pid = int(pid_file.read_text())
            self.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                   "clientInfo": {"name": "dtw-c4-reference-host", "version": "1"}})
            self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})
            tools = self.rpc("tools/list", {})["result"]["tools"]
            require([tool["name"] for tool in tools] == ["exec"], tools)
        except BaseException:
            self.close()
            raise

    def _read(self):
        try:
            for line in self.child.stdout:
                self.messages.put(json.loads(line))
        except Exception as error:
            self.messages.put(error)
        finally:
            self.messages.put(EOFError("MCP process closed stdout"))

    def _record(self, direction, message):
        self.transcript.write(json.dumps({"direction": direction, "message": message}) + "\n")
        self.transcript.flush()

    def _send(self, message):
        self._record("request", message)
        self.child.stdin.write(json.dumps(message) + "\n")
        self.child.stdin.flush()

    def rpc(self, method, params):
        self.serial += 1
        ident = self.serial
        self._send({"jsonrpc": "2.0", "id": ident, "method": method, "params": params})
        end = time.monotonic() + 20
        while True:
            message = self.messages.get(timeout=max(0, end - time.monotonic()))
            if isinstance(message, Exception):
                raise message
            self._record("response", message)
            if message.get("id") == ident:
                return message
            require("id" not in message, f"unexpected response ID: {message}")

    def exec(self, operation, **fields):
        reply = self.rpc("tools/call", {"name": "exec", "arguments": {
            "version": 1, "operation": operation, **fields}})
        self.last_images = [item for item in reply.get("result", {}).get("content", [])
                            if item.get("type") == "image"]
        payload = Wire.payload(reply)
        require(payload.get("version") == 1, payload)
        return payload

    def start(self, target, command, request_id):
        return self.exec("submit", request_id=request_id, target=target, command=command)

    def submit(self, target, command, request_id=None):
        request_id = request_id or f"request-{uuid.uuid4()}"
        reply = self.start(target, command, request_id)
        require(reply.get("request_id") == request_id and reply.get("status") in ("running", "completed"), reply)
        return request_id

    def result(self, request_id):
        return self.exec("result", request_id=request_id)

    def finish(self, request_id):
        def terminal():
            reply = self.result(request_id)
            require(reply.get("request_id") == request_id, reply)
            return reply if Wire.terminal(reply) else None
        return wait_until(terminal, timeout=20)

    def run(self, target, command):
        request_id = f"request-{uuid.uuid4()}"
        reply = self.start(target, command, request_id)
        if reply.get("error") and "status" not in reply:
            return reply
        require(reply.get("request_id") == request_id and reply.get("status") in ("running", "completed"), reply)
        return reply if Wire.terminal(reply) else self.finish(request_id)

    def scope(self):
        description = self.exec("describe")
        app_ref = Wire.app_ref(description)
        windows = Wire.value(self.run({"app_ref": app_ref}, {"kind": "windows"}))
        return description, Wire.target(app_ref, windows)

    def close(self):
        try:
            # Closing this handle is cleanup only. Parent-death tests check both
            # child processes have disappeared before calling this method.
            self.child.stdin.close()
            if self.child.poll() is None:
                try:
                    self.child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.child.kill()
                    self.child.wait(timeout=5)
                    raise AssertionError("MCP failed to exit on EOF; killed owned test process")
        finally:
            self.stderr.close()
            self.transcript.close()


def catalog_app(binary, fixture):
    result = subprocess.run([str(binary), "app", "catalog"], capture_output=True, text=True, timeout=15)
    require(result.returncode == 0, result.stderr)
    catalog = json.loads(result.stdout)
    require(catalog["version"] == 1, catalog)
    matches = [{key: entry[key] for key in ("identity", "pid", "generation")}
               for entry in catalog["apps"] if entry["pid"] == fixture.state()["pid"]]
    require(len(matches) == 1, "fixture PID not uniquely present in kernel app catalog")
    app = matches[0]
    require(isinstance(app["generation"], str) and app["generation"].isdecimal(), app)
    require(int(app["generation"]) > 0 and app["identity"], app)
    return app


def new_session(binary, host, name, capabilities=None, host_parent=False):
    grant = host.grant(name, capabilities=capabilities)
    require(grant is not None, "Host denied this session")
    return Session(binary, grant, host.output / name, host_parent=host_parent)


def check_readonly_grant(binary, host, fixture, peer, foreign_target, foreign_command):
    before, peer_before = fixture.counters(), peer.counters()
    session = new_session(binary, host, "readonly-grant", ["observe"])
    try:
        _, target = session.scope()
        # A grant for the same native app does not make another context's public
        # references usable. Do not translate foreign refs to local targets.
        Wire.rejection_code(session.run(foreign_target, foreign_command), "wrong_app")
        observed = Wire.value(session.run(target, {"kind": "observe"}))
        denied = session.run(target, Wire.press(observed))
        Wire.rejection_code(denied, "capability_denied")
        require(Wire.receipt(denied)["provider_outcome"] is None, denied)
        Wire.value(session.run(target, {"kind": "observe"}))
        require(fixture.counters() == before and peer.counters() == peer_before,
                "read-only grant or cross-context rejection changed an app")
        return {"input_denied": True, "provider_not_called": True, "cross_context_ref_denied": True,
                "both_apps_unchanged": True}
    finally:
        session.close()


def check_semantic(binary, host, fixture, peer):
    fixture.command("uncover")
    before, peer_before = fixture.counters(), peer.counters()
    session = new_session(binary, host, "semantic")
    try:
        description, target = session.scope()
        require({key: description["app"][key] for key in ("identity", "pid", "generation")} == host.app,
                "described scope differs from Host-approved exact app instance")
        for field in ("approved", "grant", "capabilities", "session"):
            rejection = session.exec("describe", **{field: True})
            Wire.rejection_code(rejection, "invalid_request")
        wrong = session.exec("submit", request_id="wrong-app", target={"app_ref": "unapproved-app"},
                             command={"kind": "windows"})
        if wrong.get("status") == "running":
            wrong = session.finish("wrong-app")
        Wire.rejection_code(wrong, "wrong_app")
        observed = Wire.value(session.run(target, {"kind": "observe"}))
        command = Wire.press(observed)
        action = session.run(target, command)
        Wire.dispatched(action)
        wait_until(lambda: fixture.state()["clicks"] == before["clicks"] + 1)
        replay = session.run(target, Wire.press(observed))
        Wire.rejected(replay)
        after = Wire.value(session.run(target, {"kind": "observe"}))
        require(any(f"Count: {before['clicks'] + 1}" in e["label"] for e in after["elements"]), after)
        repeated = session.result(action["request_id"])
        require(repeated == action, "original action result changed or was replayed")
        duplicate = session.start(target, command, action["request_id"])
        require(duplicate == action, "identical accepted request did not return its original result")
        conflict = session.start(target, {"kind": "observe"}, action["request_id"])
        Wire.rejection_code(conflict, "request_conflict")
        cancelled = session.exec("cancel", request_id=action["request_id"])
        require(cancelled["cancel_requested"] is False and cancelled["context_state"] == "open", cancelled)
        require(peer.counters() == peer_before, "unapproved app changed")
        require(fixture.state()["actions_started"] == before["actions_started"] + 1, "action was replayed")
        result = {"stage": "semantic", "description": description, "action": action,
                "counter_before": before, "counter_after": fixture.counters(),
                "other_app_unchanged": True, "rejected": ["fake authorization", "wrong app", "old reference"],
                "original_receipt_preserved": True, "duplicate_request_did_not_replay": True,
                "request_conflict_rejected": True, "late_cancel_did_not_cancel_context": True}
    finally:
        session.close()
    result["readonly_grant"] = check_readonly_grant(binary, host, fixture, peer, target, command)
    return result


def capture_image(session, target):
    value = Wire.value(session.run(target, {"kind": "capture"}))
    require(value["kind"] == "pixels" and value["image"]["coordinate_space"] == "image_pixels", value)
    require(len(session.last_images) == 1, "capture result must carry one MCP image")
    image = session.last_images[0]
    require(image["mimeType"] == "image/png", image.get("mimeType"))
    pixels = base64.b64decode(image["data"], validate=True)
    require(len(pixels) >= 24 and pixels.startswith(b"\x89PNG\r\n\x1a\n")
            and pixels[12:16] == b"IHDR", "capture has no PNG IHDR")
    width, height = int.from_bytes(pixels[16:20], "big"), int.from_bytes(pixels[20:24], "big")
    require(width > 0 and height > 0, "PNG dimensions must be positive")
    require((width, height) == (value["image"]["width"], value["image"]["height"]), value)
    path = session.output / "capture.png"
    path.write_bytes(pixels)
    return value, {"image": str(path), "sha256": hashlib.sha256(pixels).hexdigest(), "image_size": [width, height]}


def fixture_increment_point(width, height):
    # tests/fixtures/native-core/main.swift: 320x220 content; Increment's
    # center is (160, 90) measured from the content's bottom left. The SCK
    # image includes the title bar, so convert from the image's bottom edge.
    scale = width / 320
    x, y = width / 2, height - 90 * scale
    require(width > 0 and height > 0 and 0 <= x < width and 0 <= y < height,
            "fixture Increment center is outside captured image")
    return x, y


def check_capture(binary, host, fixture, peer):
    fixture.command("uncover")
    before, peer_before = fixture.counters(), peer.counters()
    session = new_session(binary, host, "capture", ["observe", "capture"])
    try:
        _, target = session.scope()
        _, image = capture_image(session, target)
        require(fixture.counters() == before and peer.counters() == peer_before, "capture changed an app")
        return {"stage": "capture", **image, "both_apps_unchanged": True,
                "visual_inspection": "pending independent reviewer; PNG validity is not a visual verdict"}
    finally:
        session.close()


def check_pixel(binary, host, fixture, peer):
    # The fixture only handles a changed command. Restore first so uncover is
    # executed even when the previous stage left its command set to uncover.
    fixture.command("restore")
    fixture.command("uncover")
    before, peer_before = fixture.counters(), peer.counters()
    session = new_session(binary, host, "pixel", ["observe", "capture", "click"])
    try:
        _, target = session.scope()
        captured, image = capture_image(session, target)
        x, y = fixture_increment_point(*image["image_size"])
        command = {"kind": "click", "observation_ref": captured["observation_ref"], "x": x, "y": y}
        invalid = session.run(target, {**command, "x": -1})
        Wire.rejection_code(invalid, "invalid_point")
        require(Wire.receipt(invalid)["provider_outcome"] is None, invalid)
        require(fixture.counters() == before and peer.counters() == peer_before,
                "invalid pixel point changed an app")
        action = session.run(target, command)
        Wire.dispatched(action)
        count = before["clicks"] + 1
        wait_until(lambda: fixture.state()["clicks"] == count)
        # Reuse under a new request ID before observing again: a fresh observe
        # would itself invalidate the old ref and mask a consumption defect.
        replay = session.run(target, command)
        Wire.rejection_code(replay, "stale_observation")
        require(Wire.receipt(replay)["provider_outcome"] is None, replay)
        after = Wire.value(session.run(target, {"kind": "observe"}))
        require(any(f"Count: {count}" in element["label"].split(" | ") for element in after["elements"]), after)
        require(fixture.counters() == {"clicks": count, "actions_started": before["actions_started"] + 1},
                "pixel action was not performed exactly once")
        require(peer.counters() == peer_before, "unapproved app changed")
        return {"stage": "pixel", **image, "pixel_point": [x, y],
                "request_id": action["request_id"], "receipt": Wire.receipt(action),
                "invalid_point_rejected_without_consuming_ref": True, "consumed_ref_rejected": True,
                "counter_before": before, "counter_after": fixture.counters(), "ax_count": count,
                "other_app_unchanged": True}
    finally:
        session.close()


def worker_pids(parent):
    result = subprocess.run(["pgrep", "-P", str(parent)], capture_output=True, text=True)
    return [int(value) for value in result.stdout.split()]


def gone(pid):
    return subprocess.run(["ps", "-p", str(pid), "-o", "pid="], capture_output=True).returncode != 0


def check_output_failure(binary, host, operation):
    """Keep stdin open until exit; do not turn an output failure into an EOF test."""
    output = host.output / f"lifecycle-{operation}"
    output.mkdir()
    grant = host.grant(operation, capabilities=["observe"])
    require(grant is not None, "Host denied output lifecycle fixture")
    diagnostics = (output / "stderr.log").open("w")
    child = subprocess.Popen([str(binary), "app", "serve", "--grant", str(grant)],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=diagnostics, text=True, bufsize=1)
    workers = []
    try:
        initialize = {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "dtw-output-lifecycle-probe", "version": "1"}}}
        child.stdin.write(json.dumps(initialize) + "\n")
        child.stdin.flush()
        initial_line = queue.Queue()
        # Exactly one response is read. There is no reader thread consuming the
        # later tools/list responses during the backpressure case.
        threading.Thread(target=lambda: initial_line.put(child.stdout.readline()), daemon=True).start()
        reply = json.loads(initial_line.get(timeout=20))
        require(reply.get("id") == 1 and reply.get("result", {}).get("protocolVersion") == "2025-06-18", reply)
        write_json(output / "initialize.json", {"request": initialize, "response": reply})
        child.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
        child.stdin.flush()
        workers = worker_pids(child.pid)
        require(len(workers) == 1, workers)
        if operation == "broken_stdout":
            child.stdout.close()
        else:
            require(operation == "stdout_backpressure", operation)
        count = 1 if operation == "broken_stdout" else 256
        requests = [{"jsonrpc": "2.0", "id": ident, "method": "tools/list", "params": {}}
                    for ident in range(2, count + 2)]
        write_json(output / "requests.json", requests)
        payload = "".join(json.dumps(request) + "\n" for request in requests)
        write_done = threading.Event()
        write_errors = []

        def send():
            try:
                child.stdin.write(payload)
                child.stdin.flush()
            except (OSError, ValueError) as error:
                # A saturated server may exit before consuming the whole burst.
                write_errors.append(str(error))
            finally:
                write_done.set()

        threading.Thread(target=send, daemon=True).start()
        child.wait(timeout=10)
        require(child.returncode == 2, f"{operation}: expected exit 2, got {child.returncode}")
        require(not child.stdin.closed, "stdin was closed before the output failure was detected")
        require(write_done.wait(timeout=1), "output failure left the client write blocked")
        wait_until(lambda: gone(workers[0]))
        if operation == "stdout_backpressure":
            errors = [json.loads(line) for line in (output / "stderr.log").read_text().splitlines()]
            require(any(error.get("error", {}).get("code") == "output_backpressure" for error in errors), errors)
        return {"operation": operation, "server": child.pid, "worker": workers[0],
                "exit_code": child.returncode, "worker_reaped": True,
                "stdin_open_until_exit": True, "tools_list_requests_prepared": count,
                "client_write_errors": write_errors}
    finally:
        if child.poll() is None:
            child.kill()
            child.wait(timeout=5)
        try:
            child.stdin.close()
        except BrokenPipeError:
            pass
        child.stdout.close()
        diagnostics.close()
        for worker in workers:
            wait_until(lambda: gone(worker))


def check_cancel(binary, host, fixture, peer):
    fixture.command("slow_action")
    before, peer_before = fixture.counters(), peer.counters()
    session = new_session(binary, host, "cancelled")
    try:
        _, target = session.scope()
        workers = worker_pids(session.server_pid)
        require(len(workers) == 1, f"expected one native worker: {workers}")
        observed = Wire.value(session.run(target, {"kind": "observe"}))
        request_id = session.submit(target, Wire.press(observed))
        wait_until(lambda: fixture.state()["actions_started"] == before["actions_started"] + 1)
        cancel_reply = session.exec("cancel", request_id=request_id)
        require(cancel_reply["cancel_requested"] is True and cancel_reply["context_state"] == "cancelled"
                and cancel_reply["shutdown_error"] is None, cancel_reply)
        action = session.finish(request_id)
        receipt = Wire.receipt(action)
        require(receipt["outcome"]["dispatch"] == "unknown", action)
        require(receipt["interruption"]["code"] == "cancelled", action)
        wait_until(lambda: gone(workers[0]))
        require(session.result(request_id) == action, "cancel result lost original receipt")
        require(session.start(target, Wire.press(observed), request_id) == action,
                "same cancelled request lost its original receipt")
        Wire.rejection_code(session.run(target, Wire.press(observed)), "cancelled")
        wait_until(lambda: fixture.state()["clicks"] == before["clicks"] + 1)
        require(fixture.state()["actions_started"] == before["actions_started"] + 1, "cancel caused replay")
        require(peer.counters() == peer_before, "unapproved app changed")
    finally:
        session.close()
        fixture.command("fast_action")
    reader = new_session(binary, host, "cancel-readback", ["observe"])
    try:
        _, target = reader.scope()
        after = Wire.value(reader.run(target, {"kind": "observe"}))
        require(any(f"Count: {before['clicks'] + 1}" in e["label"] for e in after["elements"]), after)
    finally:
        reader.close()
    return {"stage": "cancel", "cancel": cancel_reply, "original_action": action,
            "late_effect_confirmed": True, "worker_reaped": True, "replayed": False,
            "other_app_unchanged": True}


def check_lifecycle(binary, host, fixture, peer):
    before, peer_before = fixture.counters(), peer.counters()
    cases = []
    for operation in ("close", "eof", "server_death", "host_parent_death"):
        session = new_session(binary, host, f"lifecycle-{operation}",
                              host_parent=operation == "host_parent_death")
        try:
            session.scope()
            workers = worker_pids(session.server_pid)
            require(len(workers) == 1, workers)
            if operation == "close":
                closed = session.exec("close")
                require(closed["context_state"] == "closed" and closed["shutdown_error"] is None, closed)
                description = session.exec("describe")
                denied = session.run({"app_ref": Wire.app_ref(description)}, {"kind": "windows"})
                Wire.rejection_code(denied, "closed")
            elif operation in ("server_death", "host_parent_death"):
                session.child.kill()
                session.child.wait(timeout=5)
            else:
                session.child.stdin.close()
                session.child.wait(timeout=5)
            wait_until(lambda: gone(workers[0]))
            if operation == "host_parent_death":
                wait_until(lambda: gone(session.server_pid))
            cases.append({"operation": operation, "server": session.server_pid,
                          "parent": session.child.pid if operation == "host_parent_death" else os.getpid(),
                          "worker": workers[0], "worker_reaped": True,
                          "server_exited": gone(session.server_pid)})
        finally:
            session.close()
    require(fixture.counters() == before and peer.counters() == peer_before, "lifecycle test changed an app")
    for operation in ("stdout_backpressure", "broken_stdout"):
        cases.append(check_output_failure(binary, host, operation))
    require(fixture.counters() == before and peer.counters() == peer_before, "output lifecycle test changed an app")
    return {"stage": "lifecycle", "cases": cases, "both_apps_unchanged": True}


def isolated_caelis_config(path):
    """Reuse the existing acceptance's local gateway flow without copying secrets."""
    source = json.loads(path.read_text())
    endpoints = source["models"]["provider_endpoints"]
    require(bool(endpoints), "Caelis local model gateway is missing")
    for endpoint in endpoints:
        url = urlsplit(endpoint["base_url"])
        require(endpoint.get("auth_type") == "none" and url.scheme == "http"
                and url.hostname in ("127.0.0.1", "::1") and not url.username
                and not url.password and not url.query and not url.fragment,
                "acceptance permits only local, unauthenticated model gateways")
    selected = {key: source[key] for key in ("schema_version", "models", "model_profiles", "runtime")}
    selected["model_profiles"]["profiles"] = [profile for profile in selected["model_profiles"]["profiles"]
                                               if "provider" in profile.get("backend", {})]
    sensitive = {"api_key", "api_key_env", "access_token", "refresh_token", "auth_token", "authorization",
                 "headers", "extra_headers", "custom_headers", "credentials", "credential", "credential_ref",
                 "credential_path", "secret", "password", "token"}
    def no_credentials(value):
        if isinstance(value, dict):
            require(not sensitive.intersection(key.lower() for key in value), "selected Caelis config contains credential fields")
            # Per-model overrides must not bypass the endpoint check above.
            if value.get("base_url"):
                url = urlsplit(value["base_url"])
                require(url.scheme == "http" and url.hostname in ("127.0.0.1", "::1")
                        and not url.username and not url.password and not url.query and not url.fragment,
                        "selected model overrides the local gateway")
            if value.get("auth_type"):
                require(value["auth_type"] == "none", "selected model overrides unauthenticated gateway")
            for child in value.values():
                no_credentials(child)
        elif isinstance(value, list):
            for child in value:
                no_credentials(child)
    no_credentials(selected)
    profiles = selected["model_profiles"]
    require(profiles["default_profile_id"] in {profile["id"] for profile in profiles["profiles"]},
            "default model profile is not a native local-gateway provider")
    return selected


def check_caelis(binary, host, fixture, peer, repository):
    """Real installed Caelis CLI, temporary plugin, frozen Host preauthorization."""
    executable = shutil.which("caelis")
    require(executable, "--caelis requires the actual Caelis CLI")
    config_path = Path(os.environ.get("CAELIS_ACCEPTANCE_CONFIG", Path.home() / ".caelis/config.json"))
    config = isolated_caelis_config(config_path)
    root = host.output / "caelis"
    root.mkdir(mode=0o700)
    store, workspace, plugin = (root / name for name in ("store", "workspace", "plugin"))
    store.mkdir(mode=0o700)
    workspace.mkdir()
    write_json(store / "config.json", config)
    os.chmod(store / "config.json", 0o600)
    (plugin / "bin").mkdir(parents=True)
    write_json(plugin / "plugin.json", {
        "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name": "desktop-world", "version": (repository / "version.txt").read_text().strip(),
        "description": "Owned DTW app-protocol acceptance fixture", "license": "MIT",
    })
    shutil.copy2(binary, plugin / "bin/dtw")
    grant = host.grant("caelis", seconds=360)
    require(grant is not None, "Host denied Caelis session")
    write_json(plugin / "mcp.json", {"$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
               "mcpServers": {"desktop-world": {"type": "stdio", "command": "./bin/dtw",
               "args": ["app", "serve", "--grant", str(grant)]}}})
    # Fixture metadata is independent of any product packaging contract.
    skill = plugin / "skills/desktop-world/SKILL.md"
    skill.parent.mkdir(parents=True)
    skill.write_text("""---
name: desktop-world
description: Operate the one Host-preauthorized DTW C4 fixture app.
---
Use desktop_world__exec only. Start with {\"version\":1,\"operation\":\"describe\"}.
Read its protocol and authorized scope. Use submit with a unique request_id,
target {app_ref,window_ref?}, and command {kind:windows|observe|capture|press|click}.
Use result with the original request_id until terminal. Never resubmit an action
to poll. Observe the window titled DTW Native Target; press its observed Increment
element once, then observe again and report Count. Do not change grants or use
legacy operations/scripts. Host startup policy has already approved this app;
tool arguments cannot grant permission. Stop on an actual failure.
""")
    environment = os.environ.copy()
    for name in ("CAELIS_CONTROL_URL", "CAELIS_STORE_DIR", "DTW_CUA_WORKER", "DTW_STOP_FILE"):
        environment.pop(name, None)
    installed = subprocess.run([executable, "plugin", "install", str(plugin), "--json", "--embedded", "--store-dir", str(store)],
                               cwd=workspace, env=environment, capture_output=True, text=True, timeout=60)
    (root / "install.stdout").write_text(installed.stdout)
    (root / "install.stderr").write_text(installed.stderr)
    require(installed.returncode == 0, installed.stderr[-2000:])
    fixture.command("uncover")
    before, peer_before = fixture.counters(), peer.counters()
    prompt = ("Use the installed Desktop World Skill to observe the Host-preauthorized fixture app, "
              "press Increment exactly once in DTW Native Target, then observe again and report Count. "
              "Use only Skill, ToolSearch, Read of plugin Skill files, and desktop_world__exec. "
              "Do not use shell, raw HTTP, other desktop tools, change configuration, or read fixture files. "
              "Poll the original request_id; never replay an action to retrieve its result. "
              "Stop and report a real blocker instead of inventing success.")
    (root / "prompt.txt").write_text(prompt)
    with (root / "transcript.jsonl").open("w") as output, (root / "stderr.log").open("w") as error:
        agent = subprocess.run([executable, "--embedded", "--store-dir", str(store), "-format", "jsonl", "-p", prompt],
                               cwd=workspace, env=environment, stdout=output, stderr=error, text=True, timeout=300)
    require(agent.returncode == 0, (root / "stderr.log").read_text()[-2000:])
    events = [json.loads(line).get("envelope", {}).get("update", {})
              for line in (root / "transcript.jsonl").read_text().splitlines()]
    calls = [event for event in events if event.get("sessionUpdate") == "tool_call"]
    require(calls, "no tool trace in real Caelis output")
    allowed = {"Skill", "ToolSearch", "Read", "desktop_world__exec"}
    require(all(call.get("name") in allowed for call in calls), "Caelis used an unapproved tool")
    for call in calls:
        if call["name"] == "Read":
            require(Path(call["rawInput"]["path"]).resolve().is_relative_to((plugin / "skills").resolve()),
                    "Caelis read outside plugin Skill files")
    requests = [call["rawInput"] for call in calls if call["name"] == "desktop_world__exec"]
    require(requests and requests[0] == {"version": 1, "operation": "describe"}, requests)
    presses = [request for request in requests if request.get("command", {}).get("kind") == "press"]
    require(len(presses) == 1, f"expected one submitted press, got {len(presses)}")
    press_index = requests.index(presses[0])
    require(any(request.get("command", {}).get("kind") == "observe" for request in requests[:press_index])
            and any(request.get("command", {}).get("kind") == "observe" for request in requests[press_index + 1:]),
            "Caelis must perform fresh observations before and after its action")
    wait_until(lambda: fixture.state()["clicks"] == before["clicks"] + 1)
    require(fixture.state()["actions_started"] == before["actions_started"] + 1, "Caelis replayed action")
    require(peer.counters() == peer_before, "unapproved app changed")
    return {"stage": "caelis", "actual_cli": executable, "dtw_calls": len(requests),
            "transcript": str(root / "transcript.jsonl"), "counter_before": before,
            "counter_after": fixture.counters(), "other_app_unchanged": True,
            "authorization": "reference Host preauthorization; not production dynamic approval"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("fixture", type=Path)
    parser.add_argument("other_fixture", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--host-decision", choices=("approve", "deny"), required=True)
    parser.add_argument("--stage", choices=("semantic", "capture", "pixel", "cancel", "lifecycle"), default="semantic")
    parser.add_argument("--caelis", action="store_true")
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    require(sys.platform == "darwin", "requires a real macOS desktop")
    binary = args.binary.resolve(strict=True)
    args.output = args.output.absolute()
    args.output.mkdir(mode=0o700)  # Refuse reused evidence/grant directories.
    fixture, peer = Fixture(args.fixture), Fixture(args.other_fixture)
    require(fixture.state()["pid"] != peer.state()["pid"], "other fixture must be another app process")
    before, peer_before = fixture.counters(), peer.counters()
    app, other_app = catalog_app(binary, fixture), catalog_app(binary, peer)
    host = Host(args.output, app, args.host_decision)
    require(host.grant("denied-other-app", app=other_app) is None, "Host approved unrelated app")
    if args.host_decision == "deny":
        require(host.grant("denied-selected-app") is None, "Host denial issued a grant")
        require(fixture.counters() == before and peer.counters() == peer_before, "denial changed an app")
        result = {"stage": "host-denied", "grant_files": 0, "mcp_started": False, "both_apps_unchanged": True}
    else:
        stage = {"semantic": check_semantic, "capture": check_capture, "pixel": check_pixel,
                 "cancel": check_cancel, "lifecycle": check_lifecycle}[args.stage]
        result = stage(binary, host, fixture, peer)
        if args.caelis:
            result["caelis"] = check_caelis(binary, host, fixture, peer, args.repository.resolve(strict=True))
    write_json(args.output / "result.json", result)
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
