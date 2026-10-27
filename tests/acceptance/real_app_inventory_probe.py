#!/usr/bin/env python3
"""Read-only DTW inventory sample for one separately launched, owned real app.

The Swift observer is a separate diagnostic, not part of the provider. The cold
sample records CG only before DTW's first read, so it does not prewarm AX. This
probe never focuses an app, repairs AX, retries a failed request or sends input.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from app_protocol_probe import Host, Wire, new_session, require, write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "observer", "output"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--launch", required=True)
    parser.add_argument("--stage", required=True)
    parser.add_argument("--cold", action="store_true")
    parser.add_argument("--samples", type=int, default=3)
    args = parser.parse_args()
    require(1 <= args.samples <= 10, "bounded sample count required")
    output = args.output.absolute()
    output.mkdir(mode=0o700)  # Never overwrite failed evidence.
    binary, observer = args.binary.resolve(strict=True), args.observer.resolve(strict=True)

    def snapshot(mode):
        reply = subprocess.run([str(observer), str(args.pid), args.launch, mode],
                               check=True, capture_output=True, text=True, timeout=10)
        return json.loads(reply.stdout)

    catalog = subprocess.run([str(binary), "app", "catalog"], check=True,
                             capture_output=True, text=True, timeout=15)
    matches = [app for app in json.loads(catalog.stdout)["apps"] if app["pid"] == args.pid]
    require(len(matches) == 1, "owned PID is absent or ambiguous")
    app = {key: matches[0][key] for key in ("identity", "pid", "generation")}
    host = Host(output, app, "approve")
    result = {"stage": args.stage, "app": app, "cold": args.cold,
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "observer_sha256": hashlib.sha256(observer.read_bytes()).hexdigest(),
              "probe_source_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "observer_source_sha256": hashlib.sha256(
                  (Path(__file__).resolve().parents[1] / "fixtures/real-app-inventory.swift").read_bytes()).hexdigest(),
              "samples": []}
    # Every sample has a fresh Host context + DTW provider process. Script-first
    # and atomic-first paths both get independent first-contact observations.
    for index in range(args.samples):
        for first in ("script", "atomic"):
            sample = {"index": index, "first": first,
                      "before": snapshot("cg" if args.cold and not result["samples"] else "inspect")}
            result["samples"].append(sample)
            write_json(output / "result.json", result)
            session = new_session(binary, host, f"{index}-{first}", capabilities=["observe"])
            try:
                target = {"app_ref": Wire.app_ref(session.exec("describe"))}
                sample["replies"] = []
                for kind in (first, "atomic" if first == "script" else "script"):
                    command = {"kind": "windows"} if kind == "atomic" else {
                        "kind": "script", "source": "return await app.windows();", "timeout_ms": 3000}
                    reply = session.run(target, command)
                    sample["replies"].append({"kind": kind, "reply": reply})
                    write_json(output / "result.json", result)
            finally:
                session.close()
                sample["after"] = snapshot("inspect")
                write_json(output / "result.json", result)
    print(json.dumps({"stage": args.stage, "samples": len(result["samples"]), "evidence": str(output)}))


if __name__ == "__main__":
    main()
