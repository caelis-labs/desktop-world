#!/usr/bin/env python3
"""Opt-in, read-only C5.2 matrix sample; the operator controls owned fixture state.

No app launch, focus, action, inventory retry, or fallback. Each context makes a
predeclared atomic/script/atomic or script/atomic/script comparison. All outcomes
are retained even if one fails. Raw native diagnostics are sampled afterwards,
so they never warm AX before the first public-protocol call.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

from app_protocol_probe import Fixture, Host, Wire, catalog_app, new_session, require, write_json


def validate_reply(reply, kind, count, error):
    if error:
        require(reply.get("error"), reply)
        if kind == "windows":
            require(reply["value"] is None, "unproven inventory published a value")
        else:
            require("result" not in reply["value"], "failed script published an inventory")
        if kind == "script":
            require(reply["script"]["state_committed"] is False, reply)
            steps = reply["script"]["steps"]
            require(len(steps) == 1 and steps[0]["operation"] == "windows"
                    and steps[0]["error"]["code"] == error, reply)
        failure = reply["script"]["steps"][0]["error"] if kind == "script" else reply["error"]
        require(failure.get("code") == error, reply)
        if error == "window_inventory_unproven":
            evidence = failure.get("details", {})
            require(evidence.get("coverage", {}).get("status") == "unproven", reply)
            require(evidence["coverage"]["complete_within_scope"] is False, reply)
            require(isinstance(evidence.get("diagnostics", {}).get("counters"), dict), reply)
        return
    value = Wire.value(reply)
    inventory = value["result"] if kind == "script" else value
    windows = inventory["windows"]
    require(len(windows) == count, inventory)
    coverage = inventory.get("coverage", {})
    require(coverage.get("status") == ("available" if count else "empty_within_scope"), inventory)
    require(coverage.get("complete_within_scope") is True, inventory)
    require(coverage.get("supported_count") == count, inventory)
    if count == 2:
        require(sorted(w["title"] for w in windows) == ["DTW Native Peer", "DTW Native Target"], inventory)
        require(len({w["window_ref"] for w in windows}) == 2, inventory)


def native_sample(binary, app):
    request = {"operation": "windows", "request_id": 1,
               "deadline_unix_ms": int(time.time() * 1000) + 5000,
               "app": {**app, "generation": int(app["generation"])}}
    raw = subprocess.run([str(binary), "--dtw-native-worker"], input=json.dumps(request) + "\n",
                         text=True, capture_output=True, timeout=7, check=True)
    return json.loads(raw.stdout)["result"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "fixture", "peer", "output"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--state", required=True)
    parser.add_argument("--active", choices=("yes", "no", "either"), default="either")
    parser.add_argument("--contexts", type=int, choices=range(1, 7), default=3)
    parser.add_argument("--first", choices=("windows", "script"), default="windows")
    parser.add_argument("--expected-windows", type=int, choices=(0, 2), default=2)
    parser.add_argument("--expected-error")
    parser.add_argument("--host-decision", choices=("approve",), required=True)
    args = parser.parse_args()
    require(sys.platform == "darwin", "requires the real macOS owned fixture")
    binary = args.binary.resolve(strict=True)
    output = args.output.absolute()
    output.mkdir(mode=0o700)
    fixture, peer = Fixture(args.fixture), Fixture(args.peer)
    before, peer_before = fixture.state(), peer.state()
    require(before["pid"] != peer_before["pid"], "distinct owned processes required")
    host = Host(output, catalog_app(binary, fixture), args.host_decision)
    report = {"state": args.state, "first": args.first, "fixture_before": before,
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "authority": "reference Host; production Harness evidence is separate",
              "contexts": [], "failures": []}
    order = [args.first, "script" if args.first == "windows" else "windows", args.first]
    def save():
        write_json(output / "result.json", report)
    try:
        for index in range(args.contexts):
            case = {"index": index, "calls": []}
            report["contexts"].append(case)
            session = new_session(binary, host, f"context-{index}", ["observe"])
            try:
                target = {"app_ref": Wire.app_ref(session.exec("describe"))}
                for kind in order:
                    command = {"kind": kind}
                    if kind == "script":
                        command["source"] = "state.attempted=true; return await app.windows();"
                    reply = session.run(target, command)
                    after, peer_after = fixture.state(), peer.state()
                    case["calls"].append({"kind": kind, "reply": reply, "fixture": after})
                    save()
                    # Changed identity/state is a fixture-control failure, not an inventory retry.
                    require(after["pid"] == before["pid"] and peer_after["pid"] == peer_before["pid"], "fixture restarted")
                    for current, original in ((after, before), (peer_after, peer_before)):
                        require(all(current[k] == original[k] for k in ("clicks", "actions_started")), "read-only call caused an action")
                    if args.active != "either":
                        require(after["active"] == (args.active == "yes"), "fixture activation changed during the case")
                    try:
                        validate_reply(reply, kind, args.expected_windows, args.expected_error)
                    except AssertionError as error:
                        report["failures"].append({"context": index, "kind": kind, "error": str(error)})
            finally:
                session.close()
            case["native_after"] = native_sample(binary, host.app)
            save()
    finally:
        report["fixture_after"] = fixture.state()
        report["peer_after"] = peer.state()
        save()
    require(not report["failures"], f"inventory matrix failed; retained {output / 'result.json'}")
    print(json.dumps({"state": args.state, "contexts": args.contexts, "calls": args.contexts * 3,
                      "passed": True, "evidence": str(output / "result.json")}))


if __name__ == "__main__":
    main()
