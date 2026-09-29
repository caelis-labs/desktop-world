"""Syntax and internal consistency checks for the RFC examples; not an engine."""
from __future__ import annotations

import json
from pathlib import Path


def verify_plan(envelope: dict) -> None:
    plan = envelope["args"]
    assert envelope["world"] == plan["epoch"], "world/epoch mismatch"
    assert plan["request_id"].startswith(plan["epoch"] + ":")
    steps = plan["steps"]
    assert 0 < len(steps) <= 16
    assert 0 < plan["timeout_ms"] <= 10000
    aliases: set[str] = set()
    ids: set[str] = set()
    for step in steps:
        assert step["id"] not in ids, "duplicate step ID"
        ids.add(step["id"])
        target = step.get("target", {})
        if "bound" in target:
            assert target["bound"] in aliases, "alias used before binding"
        if step["op"] == "bind":
            binding = step["bind"]
            assert binding["require_unique"] is True
            assert binding["name"] not in aliases
            aliases.add(binding["name"])
        for predicate in step.get("before", []) + step.get("after", []):
            ptarget = predicate["target"]
            if "bound" in ptarget:
                assert ptarget["bound"] in aliases
        if "timeout_ms" in step:
            assert 0 < step["timeout_ms"] <= plan["timeout_ms"]


def main() -> None:
    root = Path(__file__).resolve().parent
    paths = sorted((root / "examples" / "protocol").glob("*.json"))
    assert paths, "no JSON examples found"
    plans = {}
    receipts = []
    for path in paths:
        obj = json.loads(path.read_text(encoding="utf-8"))
        if obj.get("op") == "world.act":
            verify_plan(obj)
            plans[obj["args"]["request_id"]] = obj["args"]
        if obj.get("state") == "terminal" and "steps" in obj:
            receipts.append(obj)
    for receipt in receipts:
        plan = plans[receipt["request_id"]]
        assert [s["id"] for s in plan["steps"]] == [s["id"] for s in receipt["steps"]]
        assert receipt["steps"][-1]["state"] == "skipped"
        assert receipt["steps"][-2]["delivery"] == "none"
    print(f"OK: {len(paths)} JSON examples, {len(plans)} plan(s), {len(receipts)} partial receipt(s).")
    print("These checks do not validate native desktop behavior or implement the full wire schema.")


if __name__ == "__main__":
    main()
