#!/usr/bin/env python3
"""Opt-in C5 native deadline/cancel and idle grant expiry on owned fixtures.

This reference Host checks native lifetime boundaries; the production Harness
semantic/pixel/state flow is a separate Caelis test. Each case intentionally
starts one slow GUI action, then proves its receipt remains unknown even when
an independent late effect is observed. AX can return an unknown provider result
before the app finishes: timeout then interrupts a VM loop, whereas cancel is
sent while the native action is still waiting. Run serially on the test desktop.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import time
from app_protocol_probe import (Fixture, Host, Session, Wire, catalog_app, gone,
                                new_session, require, wait_until, worker_pids, write_json)

SETUP = """
const inventory=(await app.windows()).windows;
const windows=inventory.filter(w=>w.title==='DTW Native Target');
if(windows.length!==1) throw new Error('fixture target count='+windows.length+'; inventory='+JSON.stringify(inventory));
const window_ref=windows[0].window_ref;
const observed=await app.observe({window_ref});
const buttons=observed.elements.filter(e=>e.can_press && e.label.includes('Increment'));
if(buttons.length!==1) throw new Error('ambiguous Increment');
state.uncommitted=true;
"""
PRESS = "await app.press({window_ref,observation_ref:observed.observation_ref,element_ref:buttons[0].element_ref});"
TIMEOUT_SOURCE = SETUP + "try {" + PRESS + "} catch(error) { console.log(String(error)); }\nwhile(true) {}"
CANCEL_SOURCE = SETUP + PRESS + "\nreturn true;"


def check_case(binary, host, fixture, peer, mode):
    fixture.command("slow_action")
    before, peer_before = fixture.counters(), peer.counters()
    session = new_session(binary, host, f"script-{mode}")
    try:
        description = session.exec("describe")
        require(description["script"]["version"] == 1, description)
        target = {"app_ref": Wire.app_ref(description)}
        workers = worker_pids(session.server_pid)
        require(len(workers) == 1, workers)
        command = {"kind": "script", "source": TIMEOUT_SOURCE if mode == "timeout" else CANCEL_SOURCE,
                   "timeout_ms": 1000 if mode == "timeout" else 5000}
        request_id = f"script-{mode}"
        started = time.monotonic()
        session.submit(target, command, request_id)
        def action_started():
            if fixture.state()["actions_started"] == before["actions_started"] + 1:
                return True
            early = session.result(request_id)
            require(early["status"] != "completed", early)
            return False
        wait_until(action_started)
        stop = None
        if mode == "cancel":
            stop = session.exec("cancel", request_id=request_id)
            require(stop["cancel_requested"] and stop["context_state"] == "cancelled"
                    and stop["shutdown_error"] is None, stop)
        result = session.finish(request_id)
        elapsed = time.monotonic() - started
        require(elapsed < 3, f"native script did not settle within bound: {elapsed}s")
        require(result["error"] and result["script"]["state_committed"] is False, result)
        require(result["script"]["shutdown_error"] is None, result)
        steps = result["script"]["steps"]
        require([step["operation"] for step in steps] == ["windows", "observe", "press"], result)
        receipt = steps[-1]["receipt"]
        reason = "timed_out" if mode == "timeout" else "cancelled"
        require(receipt["outcome"]["dispatch"] == "unknown", result)
        if mode == "timeout":
            require(result["script"]["failure"]["kind"] == "timeout", result)
            require(receipt["interruption"] is None
                    and receipt["provider_outcome"]["dispatch"] == "unknown",
                    "AX must return unknown before the VM deadline; preserve that original receipt")
        else:
            require(receipt["interruption"] and receipt["interruption"]["code"] == reason, result)
        require(receipt["provider_outcome"] is not None, "lost actual native provider reply")
        wait_until(lambda: gone(workers[0]))
        require(session.result(request_id) == result, "query changed original script evidence")
        require(session.start(target, command, request_id) == result, "duplicate replayed or changed evidence")
        rejected = session.run(target, {"kind": "script", "source": "return 1;"})
        Wire.rejection_code(rejected, reason)
        wait_until(lambda: fixture.state()["clicks"] == before["clicks"] + 1)
        require(fixture.counters() == {"clicks": before["clicks"] + 1,
                                      "actions_started": before["actions_started"] + 1}, "unexpected replay")
        require(peer.counters() == peer_before, "unapproved app changed")
        # Readback is separate evidence; it must not rewrite the unknown receipt.
        require(session.result(request_id) == result, "late effect changed original receipt")
    finally:
        session.close()
        fixture.command("fast_action")
    reader = new_session(binary, host, f"script-{mode}-readback", ["observe"])
    try:
        _, target = reader.scope()
        observed = Wire.value(reader.run(target, {"kind": "observe"}))
        require(any(f"Count: {before['clicks'] + 1}" in e["label"].split(" | ")
                    for e in observed["elements"]), "late effect absent in independent AX readback")
    finally:
        reader.close()
    return {"mode": mode, "phase": "VM loop after unknown native reply" if mode == "timeout" else "native call in flight",
            "elapsed_seconds": elapsed, "stop": stop,
            "original_result": result, "counter_before": before,
            "counter_after": fixture.counters(), "peer_unchanged": True,
            "native_worker_reaped": True, "duplicate_not_replayed": True,
            "late_effect_independently_confirmed": True}


def check_idle_expiry(binary, host, fixture, peer):
    before, peer_before = fixture.counters(), peer.counters()
    grant = host.grant("idle-expiry", capabilities=["observe"], seconds=2)
    session = Session(binary, grant, host.output / "idle-expiry")
    try:
        description = session.exec("describe")
        target = {"app_ref": Wire.app_ref(description)}
        workers = worker_pids(session.server_pid)
        require(len(workers) == 1, workers)
        command = {"kind": "script", "source": "state.saved=true; return (await app.windows()).coverage;"}
        request_id = "before-idle-expiry"
        session.submit(target, command, request_id)
        original = session.finish(request_id)
        require(not original["error"] and original["script"]["state_committed"], original)
        # No MCP traffic until the native worker disappears. Only the existing
        # stdio lifetime watcher can observe the deadline during this interval.
        wait_until(lambda: gone(workers[0]))
        deadline = json.loads(grant.read_text())["expires_at_unix_ms"]
        cleanup_delay_ms = int(time.time() * 1000) - deadline
        require(0 <= cleanup_delay_ms < 1000, cleanup_delay_ms)
        context = session.exec("describe")["context"]
        require(context["state"] == "closed"
                and context["terminal_reason"]["code"] == "grant_expired", context)
        require(session.result(request_id) == original, "expiry rewrote stored evidence")
        require(session.start(target, command, request_id) == original, "expiry lost dedup evidence")
        Wire.rejection_code(session.run(target, {"kind": "script", "source": "return 1;"}), "grant_expired")
        Wire.rejection_code(session.run(target, {"kind": "windows"}), "grant_expired")
        require(fixture.counters() == before and peer.counters() == peer_before, "read-only expiry changed an app")
        return {"mode": "idle_expiry", "worker_gone_without_mcp_traffic": True,
                "cleanup_after_deadline_ms": cleanup_delay_ms, "context": context,
                "original_result": original, "stored_result_and_dedup_unchanged": True,
                "atomic_and_pure_js_rejected": "grant_expired", "both_apps_unchanged": True}
    finally:
        session.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "fixture", "other_fixture", "output"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--host-decision", choices=("approve",), required=True)
    args = parser.parse_args()
    require(sys.platform == "darwin", "requires a real macOS desktop")
    binary = args.binary.resolve(strict=True)
    output = args.output.absolute()
    output.mkdir(mode=0o700)
    fixture, peer = Fixture(args.fixture), Fixture(args.other_fixture)
    require(fixture.state()["pid"] != peer.state()["pid"], "two fixture processes required")
    host = Host(output, catalog_app(binary, fixture), args.host_decision)
    cases = []
    for mode in ("timeout", "cancel"):
        cases.append(check_case(binary, host, fixture, peer, mode))
        write_json(output / "result.json", {"cases": cases})
    cases.append(check_idle_expiry(binary, host, fixture, peer))
    result = {"checkpoint": "C5-native-lifetime", "cases": cases,
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "authority": "reference Host operator policy; production Harness tested separately"}
    write_json(output / "result.json", result)
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
