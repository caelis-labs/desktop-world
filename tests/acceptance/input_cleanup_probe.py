#!/usr/bin/env python3
# HISTORICAL: this probe targets the retired C4/C5 grant/custom-Host entry.
# Preserve it as dated evidence; use standard_agent_probe.py for current MCP acceptance.
"""Opt-in, one-click macOS input shutdown acceptance on two owned fixtures.

Requires exclusive control of the desktop. Each mode submits exactly one click
in a fresh context. A missed down/up interval is a failed sample, retained on
disk; this program never retries that action. parent_death kills the owned
app-serve process (the native worker's parent), and does not invent a receipt.
"""
import argparse
import ctypes
import hashlib
import json
import queue
from pathlib import Path
import subprocess
import sys
import threading
import time
import uuid

from app_protocol_probe import (Fixture, Host, Wire, capture_image, catalog_app,
                                fixture_increment_point, gone, new_session,
                                require, wait_until, worker_pids, write_json)


class CGPoint(ctypes.Structure):
    _fields_ = [("x", ctypes.c_double), ("y", ctypes.c_double)]


class MouseState:
    def __init__(self):
        self.cg = ctypes.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
        self.cg.CGEventSourceButtonState.argtypes = [ctypes.c_int32, ctypes.c_uint32]
        self.cg.CGEventSourceButtonState.restype = ctypes.c_bool

    def sample(self):
        before = time.monotonic_ns()
        down = bool(self.cg.CGEventSourceButtonState(0, 0))
        return {"down": down, "before_monotonic_ns": before,
                "after_monotonic_ns": time.monotonic_ns()}

    def emergency_release(self):
        # Only called after recording a failed owned sample and closing its
        # context. Never contributes evidence for a passing test.
        self.cg.CGEventCreate.argtypes = [ctypes.c_void_p]
        self.cg.CGEventCreate.restype = ctypes.c_void_p
        self.cg.CGEventGetLocation.argtypes = [ctypes.c_void_p]
        self.cg.CGEventGetLocation.restype = CGPoint
        self.cg.CGEventCreateMouseEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint32,
                                                  CGPoint, ctypes.c_uint32]
        self.cg.CGEventCreateMouseEvent.restype = ctypes.c_void_p
        self.cg.CGEventPost.argtypes = [ctypes.c_uint32, ctypes.c_void_p]
        self.cg.CGEventPost.restype = None
        cf = ctypes.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
        cf.CFRelease.argtypes = [ctypes.c_void_p]
        cf.CFRelease.restype = None
        current = self.cg.CGEventCreate(None)
        require(current, "cannot read current pointer for emergency release")
        try:
            up = self.cg.CGEventCreateMouseEvent(None, 2, self.cg.CGEventGetLocation(current), 0)
            require(up, "cannot create emergency left-up")
            try:
                self.cg.CGEventPost(0, up)
            finally:
                cf.CFRelease(up)
        finally:
            cf.CFRelease(current)


class Observer:
    def __init__(self, binary, output):
        self.events = []
        self.messages = queue.Queue()
        self.log = (output / "mouse-state.jsonl").open("w")
        self.error = (output / "mouse-state.stderr").open("w")
        self.child = subprocess.Popen([str(binary), "15000"], stdout=subprocess.PIPE,
                                      stderr=self.error, text=True, bufsize=1)
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    def _read(self):
        try:
            for line in self.child.stdout:
                value = json.loads(line)
                value["received_monotonic_ns"] = time.monotonic_ns()
                self.log.write(json.dumps(value) + "\n")
                self.log.flush()
                self.events.append(value)
                self.messages.put(value)
        except Exception as error:
            self.messages.put(error)
        finally:
            self.messages.put(EOFError("mouse-state observer exited"))

    def wait(self, kind, seconds=5):
        deadline = time.monotonic() + seconds
        while True:
            message = self.messages.get(timeout=max(0, deadline - time.monotonic()))
            if isinstance(message, BaseException):
                raise message
            if message["event"] == kind:
                return message
            require(message["event"] not in ("end", "transition_limit"), message)

    def close(self):
        if self.child.poll() is None:
            self.child.terminate()
        self.child.wait(timeout=2)
        self.reader.join(timeout=2)
        require(not self.reader.is_alive(), "mouse observer reader failed to exit")
        self.child.stdout.close()
        self.log.close()
        self.error.close()


def mid_pair_hit(down, trigger, up):
    """Direct post-send sampling prevents a polling-delayed up false positive."""
    before = trigger["sample"]
    after = trigger["sample_after_write"]
    return (before["down"] and after["down"]
            and down["monotonic_ns"] <= before["before_monotonic_ns"]
            <= before["after_monotonic_ns"] <= trigger["written_monotonic_ns"]
            <= after["before_monotonic_ns"] <= after["after_monotonic_ns"]
            < up["monotonic_ns"])


def check_case(binary, observer_binary, host, fixture, peer, mouse, mode):
    # Reset a previous slow-action/slow-AX test without reading another app.
    for command in ("fast_action", "fast_ax", "restore", "uncover"):
        fixture.command(command)
    before, peer_before = fixture.counters(), peer.counters()
    session = new_session(binary, host, f"input-{mode}", ["observe", "capture", "click"])
    evidence = {"mode": mode, "status": "running", "counter_before": before,
                "peer_before": peer_before, "mid_pair_hit": False,
                "emergency_release": False, "server_pid": session.server_pid}
    evidence_file = session.output / "result.json"
    observer = None
    owned_down = False
    submitted = False
    failure = None

    def save():
        write_json(evidence_file, evidence)

    try:
        _, target = session.scope()
        workers = worker_pids(session.server_pid)
        require(len(workers) == 1, workers)
        evidence["worker_pid"] = workers[0]
        captured, image = capture_image(session, target)
        evidence["capture"] = image
        x, y = fixture_increment_point(*image["image_size"])
        command = {"kind": "click", "observation_ref": captured["observation_ref"], "x": x, "y": y}
        observer = Observer(observer_binary, session.output)
        ready = observer.wait("ready")
        require(not ready["down"] and not mouse.sample()["down"], "left button was down before this owned action")
        request_id = f"input-{mode}-{uuid.uuid4()}"
        evidence.update(request_id=request_id, target=target, pixel_point=[x, y], ready=ready)
        save()
        evidence["submit"] = session.start(target, command, request_id)
        require(evidence["submit"].get("status") in ("running", "completed"), evidence["submit"])
        submitted = True
        try:
            down = observer.wait("down")
        except (queue.Empty, EOFError):
            evidence["original_result"] = session.finish(request_id)
            raise AssertionError("no OS down observed; this sample cannot prove mid-pair cleanup")
        owned_down = True
        evidence["down"] = down

        if mode == "parent_death":
            evidence["trigger"] = {"sample": mouse.sample(), "kind": "kill_owned_app_serve"}
            session.child.kill()
            evidence["trigger"]["written_monotonic_ns"] = time.monotonic_ns()
            evidence["trigger"]["sample_after_write"] = mouse.sample()
            session.child.wait(timeout=2)
            evidence["receipt_available"] = False
            evidence["receipt_note"] = "Parent was killed; no MCP receipt is inferred"
        else:
            original_send = session._send

            def timed_send(message):
                arguments = message.get("params", {}).get("arguments", {})
                if arguments.get("operation") == mode:
                    evidence["trigger"] = {"sample": mouse.sample(), "kind": f"exec.{mode}"}
                    original_send(message)
                    evidence["trigger"]["written_monotonic_ns"] = time.monotonic_ns()
                    evidence["trigger"]["sample_after_write"] = mouse.sample()
                else:
                    original_send(message)

            session._send = timed_send
            try:
                evidence["stop"] = (session.exec("cancel", request_id=request_id)
                                    if mode == "cancel" else session.exec("close"))
                evidence["trigger"]["rpc_returned_monotonic_ns"] = time.monotonic_ns()
            finally:
                session._send = original_send
        save()

        up = observer.wait("up", seconds=2)
        evidence["up"] = up
        evidence["mid_pair_hit"] = mid_pair_hit(down, evidence["trigger"], up)
        require(evidence["mid_pair_hit"], "shutdown missed the observed down/up interval; keep this failed sample and use a new directory")
        require(not mouse.sample()["down"], "left button remains down after observed up")
        wait_until(lambda: gone(workers[0]), timeout=3)
        evidence["worker_reaped"] = True
        if mode == "parent_death":
            require(gone(session.server_pid), "owned parent did not exit")
        else:
            stopped = evidence["stop"]
            expected = "cancelled" if mode == "cancel" else "closed"
            require(stopped["context_state"] == expected and stopped["shutdown_error"] is None, stopped)
            if mode == "cancel":
                require(stopped["cancel_requested"], stopped)
            action = session.finish(request_id)
            evidence["original_result"] = action
            receipt = Wire.receipt(action)
            require(receipt["outcome"]["dispatch"] == "unknown", action)
            require(receipt["interruption"] and receipt["interruption"]["code"] == expected, action)
            require(receipt["provider_outcome"] is not None, "original provider outcome was lost")
            require(session.result(request_id) == action, "original receipt changed")

        expected_counters = {"clicks": before["clicks"] + 1, "actions_started": before["actions_started"] + 1}
        wait_until(lambda: fixture.counters() == expected_counters)
        # Independent late observation, not an input retry or receipt rewrite.
        stable_until = time.monotonic() + 2
        while time.monotonic() < stable_until:
            require(fixture.counters() == expected_counters, "late duplicate click or count changed")
            require(peer.counters() == peer_before, "unapproved fixture changed")
            require(not mouse.sample()["down"], "left button became pressed again")
            time.sleep(0.02)
        transitions = [event["event"] for event in observer.events if event["event"] in ("down", "up")]
        require(transitions == ["down", "up"], f"unexpected input transitions: {transitions}")
        if mode != "parent_death":
            require(session.result(request_id) == evidence["original_result"], "late GUI effect changed original receipt")
        evidence.update(status="passed", counter_after=fixture.counters(), peer_unchanged=True,
                        late_stable_seconds=2, transitions=transitions, no_replay=True)
    except BaseException as error:
        failure = error
        evidence.update(status="failed", failure=f"{type(error).__name__}: {error}",
                        counter_after=fixture.counters(), peer_after=peer.counters())
        save()
    finally:
        try:
            session.close()
        except BaseException as error:
            failure = failure or error
            evidence.update(status="failed", cleanup_failure=f"{type(error).__name__}: {error}")
        # Rescue is allowed only for an observed down from this accepted action.
        # It is explicitly a failure, never a successful cleanup result.
        if submitted and owned_down and mouse.sample()["down"]:
            time.sleep(0.5)
            if mouse.sample()["down"]:
                evidence.update(status="failed", emergency_release=True)
                failure = failure or AssertionError("native cleanup left the owned left button down")
                save()
                mouse.emergency_release()
                evidence["emergency_after"] = mouse.sample()
        if observer is not None:
            observer.close()
        save()
    if failure is not None:
        raise failure
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "fixture", "other_fixture", "output"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--host-decision", choices=("approve",), required=True)
    parser.add_argument("--mode", choices=("cancel", "close", "parent_death", "all"), default="all")
    args = parser.parse_args()
    require(sys.platform == "darwin", "requires macOS and exclusive owned-fixture desktop access")
    binary = args.binary.resolve(strict=True)
    output = args.output.absolute()
    output.mkdir(mode=0o700)  # Never overwrite a prior passing or failed sample.
    root = Path(__file__).resolve().parents[2]
    source = root / "tests/fixtures/mouse-state.swift"
    observer_binary = output / "mouse-state"
    subprocess.run(["swiftc", "-warnings-as-errors", "-module-cache-path", str(output / "swift-cache"),
                    str(source), "-o", str(observer_binary)], check=True, timeout=120)
    fixture, peer = Fixture(args.fixture), Fixture(args.other_fixture)
    require(fixture.state()["pid"] != peer.state()["pid"], "two separate owned fixture processes required")
    host = Host(output, catalog_app(binary, fixture), args.host_decision)
    summary = {"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
               "probe_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
               "observer_sha256": hashlib.sha256(source.read_bytes()).hexdigest(), "cases": [],
               "authority": "Explicit reference Host; no production dynamic-approval claim"}
    write_json(output / "result.json", summary)
    mouse = MouseState()
    try:
        for mode in (("cancel", "close", "parent_death") if args.mode == "all" else (args.mode,)):
            summary["cases"].append(check_case(binary, observer_binary, host, fixture, peer, mouse, mode))
            write_json(output / "result.json", summary)
    except BaseException as error:
        summary["failure"] = f"{type(error).__name__}: {error}"
        write_json(output / "result.json", summary)
        raise
    print(json.dumps(summary, ensure_ascii=False))


if __name__ == "__main__":
    main()
