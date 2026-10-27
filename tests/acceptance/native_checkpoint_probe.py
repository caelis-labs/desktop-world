#!/usr/bin/env python3
"""Serial, opt-in real macOS GUI checkpoints against the owned AppKit fixture.

This uses the DTW internal harness under test and independent fixture state.
It is not a production protocol or a substitute for Windows/package acceptance.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import select
import subprocess
import time


def wait_until(predicate, timeout=5):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        result = predicate()
        if result:
            return result
        time.sleep(0.02)
    raise AssertionError("condition did not become true before deadline")


class Fixture:
    def __init__(self, root):
        self.root = root
        wait_until(lambda: (root / "state.json").exists(), timeout=10)

    def state(self):
        return json.loads((self.root / "state.json").read_text())

    def command(self, command):
        (self.root / "command").write_text(command)
        return wait_until(lambda: self.state() if self.state()["command"] == command else None)


class Session:
    def __init__(self, binary, fixture, output):
        self.output = output
        output.mkdir(parents=True, exist_ok=True)
        self.log = []
        self.stderr = (output / "stderr.log").open("w")
        self.child = subprocess.Popen([str(binary), str(fixture.state()["pid"]), str(output)],
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      stderr=self.stderr, text=True)
        self.ready = self.read()
        assert self.ready.get("ready"), self.ready
        self.log.append({"ready": self.ready})

    def read(self):
        assert select.select([self.child.stdout], [], [], 20)[0], "harness reply timed out"
        line = self.child.stdout.readline()
        assert line, "harness exited without reply"
        return json.loads(line)

    def call(self, operation, **fields):
        request = dict(operation=operation, **fields)
        self.child.stdin.write(json.dumps(request) + "\n")
        self.child.stdin.flush()
        reply = self.read()
        self.log.append({"request": request, "reply": reply})
        return reply

    def close(self):
        if self.child.poll() is None:
            self.child.stdin.write('{"operation":"exit"}\n')
            self.child.stdin.flush()
            self.child.wait(timeout=5)
        self.stderr.close()
        (self.output / "transcript.json").write_text(json.dumps(self.log, indent=2) + "\n")
        assert self.child.returncode == 0, self.child.returncode


def check_readonly(binary, fixture, output):
    state = fixture.state()
    catalog = json.loads(subprocess.check_output([str(binary), "--catalog"], text=True, timeout=10))
    own = [a for a in catalog["apps"] if a["pid"] == state["pid"]]
    assert len(own) == 1 and own[0]["generation"] > 0 and own[0]["identity"]
    session = Session(binary, fixture, output)
    try:
        windows = session.call("windows")["windows"]
        expected = {w["id"] for w in state["windows"]}
        assert {w["id"] for w in windows} == expected
        fixture.command("duplicate")
        duplicate = session.call("windows")["windows"]
        assert len({w["title"] for w in duplicate}) == 1
        assert {w["id"] for w in duplicate} == expected
        fixture.command("hide")
        hidden = session.call("windows")["windows"]
        target = state["windows"][0]["id"]
        assert not any(w["id"] == target and w["visible"] for w in hidden)
        fixture.command("show")
        assert any(w["id"] == target and w["visible"] for w in session.call("windows")["windows"])
        assert fixture.state()["clicks"] == state["clicks"]
        return {"checkpoint": "C2-macOS.1", "app": own[0], "windows": sorted(expected), "clicks_unchanged": True}
    finally:
        session.close()


def check_capture(binary, fixture, output):
    fixture.command("uncover")
    session = Session(binary, fixture, output)
    try:
        target = fixture.state()["windows"][0]["id"]
        session.call("windows")
        image = session.call("capture", window=target)
        assert image.get("image"), image
        path = Path(image["image"]["path"])
        assert path.read_bytes().startswith(b"\x89PNG\r\n\x1a\n")
        return {"checkpoint": "C2-macOS.2 screenshot preview", "image": image["image"], "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    finally:
        session.close()


def check_pixels(binary, fixture, output):
    fixture.command("uncover")  # Explicit fixture setup; DTW never auto-focuses.
    session = Session(binary, fixture, output)
    try:
        target = fixture.state()["windows"][0]["id"]
        session.call("windows")
        baseline = fixture.state()["clicks"]
        def capture():
            # Fixture acknowledgement is not a compositor-ready barrier. Probe
            # readiness with bounded fresh reads; never retry a dispatched action.
            for _ in range(3):
                result = session.call("capture", window=target)
                if result.get("image"):
                    return result
                assert result.get("error") == "StaleObservation", result
            raise AssertionError(result)
        def click(observation):
            width = observation["image"]["width"]
            # The fixture's button center was checked in the saved screenshot.
            return session.call("click", window=target, observation=observation["observation"],
                                x=width/2, y=observation["image"]["height"]-90*width/320)
        observed = capture()
        result = click(observed)
        assert result["dispatch"] == "Dispatched" and result["evidence"] == "Unverified", result
        wait_until(lambda: fixture.state()["clicks"] == baseline + 1)
        replay = click(observed)
        assert replay["dispatch"] == "NotDispatched" and replay["error"] == "StaleObservation", replay
        for scenario, restore in [("move", "restore"), ("occlude", "uncover"), ("hide", "show"), ("close", None)]:
            observed = capture()
            fixture.command(scenario)
            result = click(observed)
            assert result["dispatch"] == "NotDispatched" and result["error"], (scenario, result)
            assert fixture.state()["clicks"] == baseline + 1, scenario
            if restore:
                fixture.command(restore)
                session.call("windows")
        return {"checkpoint": "C2-macOS.2", "successful_clicks": 1,
                "independent_counter_before": baseline, "independent_counter_after": fixture.state()["clicks"],
                "rejected": ["replay", "move", "occlude", "hide", "close"]}
    finally:
        session.close()


def worker_pids(parent):
    result = subprocess.run(["pgrep", "-P", str(parent)], capture_output=True, text=True)
    return [int(value) for value in result.stdout.split()]


def gone(pid):
    # A zombie is not accepted as reaped evidence.
    return subprocess.run(["ps", "-p", str(pid), "-o", "pid="], capture_output=True).returncode != 0


def check_lifecycle(binary, fixture, output):
    results = []
    for operation in ["close", "cancel", "eof", "parent_death"]:
        session = Session(binary, fixture, output / operation)
        children = worker_pids(session.child.pid)
        assert len(children) == 1, children
        worker = children[0]
        if operation in ("close", "cancel"):
            reply = session.call(operation)
            assert not reply.get("error"), reply
            wait_until(lambda: gone(worker))
            assert session.call("windows")["error"] in ("Closed", "Cancelled")
            session.close()
        elif operation == "eof":
            session.child.stdin.close()
            session.child.wait(timeout=5)
            wait_until(lambda: gone(worker))
            session.close()
        else:
            session.child.kill()
            session.child.wait(timeout=5)
            wait_until(lambda: gone(worker))
            session.stderr.close()
            (session.output / "transcript.json").write_text(json.dumps(session.log, indent=2))
        results.append({"operation": operation, "worker": worker, "reaped": True})
    return {"checkpoint": "C2 macOS native lifecycle", "cases": results}


def increment_button(observation):
    buttons = [e for e in observation.get("elements", []) if e["can_press"] and "Increment" in e["label"]]
    assert len(buttons) == 1, observation
    return buttons[0]["id"]


def check_semantic(binary, fixture, output):
    fixture.command("uncover")
    session = Session(binary, fixture, output)
    try:
        state = fixture.state()
        target, peer = [w["id"] for w in state["windows"]]
        baseline = state["clicks"]
        session.call("windows")
        fixture.command("floating_peer")
        observed = session.call("observe", window=target)
        button = increment_button(observed)
        wrong = session.call("press", window=peer, observation=observed["observation"], element=button)
        assert wrong["dispatch"] == "NotDispatched" and wrong["error"] == "TargetChanged", wrong
        action = session.call("press", window=target, observation=observed["observation"], element=button)
        assert action["dispatch"] == "Dispatched" and action["evidence"] == "Unverified", action
        wait_until(lambda: fixture.state()["clicks"] == baseline+1)
        replay = session.call("press", window=target, observation=observed["observation"], element=button)
        assert replay["dispatch"] == "NotDispatched" and replay["error"] == "StaleObservation", replay
        readback = session.call("observe", window=target)
        assert any(f"Count: {baseline+1}" in e["label"] for e in readback["elements"]), readback
        fixture.command("not_ready")
        unavailable = session.call("observe", window=target)
        assert "elements" in unavailable and not any(e["can_press"] for e in unavailable["elements"]), unavailable
        visible = session.call("capture", window=target)
        assert visible.get("image"), visible
        fixture.command("ready")
        observed = session.call("observe", window=target)
        button = increment_button(observed)
        fixture.command("sheet")
        blocked = session.call("press", window=target, observation=observed["observation"], element=button)
        assert blocked["dispatch"] == "NotDispatched" and "unsupported_modal" in blocked["error"], blocked
        assert fixture.state()["clicks"] == baseline+1
        fixture.command("dismiss_sheet")
        fixture.command("slow_ax")
        slow = session.call("observe", window=target)
        assert slow.get("error") and slow["elapsed_ms"] < 1000, slow
        fixture.command("fast_ax")
        assert increment_button(session.call("observe", window=target))
        return {"checkpoint":"C3-macOS semantic", "counter_before":baseline,"counter_after":fixture.state()["clicks"],
                "rejected":["wrong window","replay","attached sheet"],"ax_delay_bounded":slow,"pixels_available_with_ax_content_absent":True,"nonmodal_floating_peer_allowed":True}
    finally:
        session.close()


def check_cancel_inflight(binary, fixture, output):
    fixture.command("slow_action")
    state = fixture.state()
    target = state["windows"][0]["id"]
    baseline, started = state["clicks"], state["actions_started"]
    session = Session(binary, fixture, output / "cancelled")
    try:
        children = worker_pids(session.child.pid)
        assert len(children) == 1
        session.call("windows")
        observed = session.call("observe", window=target)
        button = increment_button(observed)
        action = session.call("press", window=target, observation=observed["observation"], element=button, cancel_after_ms=150)
        assert action["dispatch"] == "Unknown" and action["interruption"] == "Cancelled", action
        assert action["elapsed_ms"] < 1000, action
        assert fixture.state()["actions_started"] == started+1, fixture.state()
        wait_until(lambda: gone(children[0]))
        replay = session.call("press", window=target, observation=observed["observation"], element=button)
        assert replay["dispatch"] == "NotDispatched" and replay["error"] == "Cancelled", replay
        wait_until(lambda: fixture.state()["clicks"] == baseline+1)
        assert fixture.state()["actions_started"] == started+1
    finally:
        session.close()
    fixture.command("fast_action")
    # A distinct trusted read context establishes the late effect. No action is replayed.
    reader = Session(binary, fixture, output / "independent-readback")
    try:
        reader.call("windows")
        after = reader.call("observe", window=target)
        assert any(f"Count: {baseline+1}" in e["label"] for e in after["elements"]), after
        button = increment_button(after)
        fixture.command("close")
        closed = reader.call("press", window=target, observation=after["observation"], element=button)
        assert closed["dispatch"] == "NotDispatched" and closed["error"] == "TargetChanged", closed
        assert fixture.state()["clicks"] == baseline+1
    finally:
        reader.close()
    return {"checkpoint":"C3-macOS cancellation", "action":action,"counter_before":baseline,
            "counter_after":fixture.state()["clicks"],"late_effect_readback":True,"worker_reaped":True,"replayed":False,"closed_window_rejected":True}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("fixture", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--stage", choices=["readonly", "capture", "pixels", "lifecycle", "semantic", "cancel-inflight"], required=True)
    args = parser.parse_args()
    assert os.uname().sysname == "Darwin", "requires a real macOS desktop"
    args.output.mkdir(parents=True, exist_ok=True)
    function = {"readonly": check_readonly, "capture": check_capture, "pixels": check_pixels, "lifecycle": check_lifecycle, "semantic":check_semantic, "cancel-inflight":check_cancel_inflight}[args.stage]
    result = function(args.binary, Fixture(args.fixture), args.output)
    (args.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
