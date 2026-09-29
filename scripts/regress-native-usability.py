#!/usr/bin/env python3
"""Opt-in warm regression using a named, already-open TextEdit test document.

Exercises the real Open/Go to Folder panels without raw input. Never edits the
document or reads its file contents. This is developer regression, not a cold test.
"""
import argparse
import json
import subprocess
import time
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--helper", required=True)
p.add_argument("--window", required=True)
p.add_argument("--log", type=Path, required=True)
p.add_argument("--finder", action="store_true", help="create a uniquely named test folder in the named isolated Finder window")
args = p.parse_args()
session = subprocess.Popen([args.helper, "serve", "--write-app", "访达" if args.finder else "文本编辑", "--full-output"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
log = args.log.open("x")
sequence = 0


def call(op, body):
    global sequence
    sequence += 1
    req = {"id": f"regression-{sequence}", "op": op, "args": body}
    started = time.monotonic()
    session.stdin.write(json.dumps(req, ensure_ascii=False) + "\n")
    session.stdin.flush()
    line = session.stdout.readline()
    if not line:
        raise RuntimeError("helper closed")
    reply = json.loads(line)
    log.write(json.dumps({"request": req, "response": reply, "elapsed_ms": 1000*(time.monotonic()-started)}, ensure_ascii=False)+"\n")
    log.flush()
    if reply.get("error"):
        raise RuntimeError(reply["error"])
    return reply["result"]


def observe(ref=None, projection="summary"):
    return call("observe", {"scope": {"refs": [ref]} if ref else {"desktop": True}, "projection": projection, "fields": ["name", "role", "app", "window", "bounds", "states"], "budget": {"read_deadline_ms": 10000, "max_output_bytes": 1000000, "max_results": 1024, "max_depth": 8}})


def press(ref, key, modifiers=None):
    return call("act", {"steps": [{"id": "key", "op": "keyboard.press", "target": {"ref": ref}, "press": {"key": key, "modifiers": modifiers or []}}]})


def focus_of(ob):
    focused = ob["seat"]["focused_object"]["value"]
    window = ob["seat"]["foreground_window"].get("value")
    detail = observe(focused, "detail")
    obj = next(o for o in detail["objects"] if o["ref"] == focused)
    if args.finder and window is None:
        assert not obj.get("window") and obj.get("app") == ob["seat"]["foreground_application"].get("value"), "windowless editor app/focus mismatch"
    else:
        assert obj.get("window") == window, ("missing/wrong live window relationship", obj, window)
    return focused, window, obj


try:
    hello = json.loads(session.stdout.readline())
    assert hello.get("type") == "hello", hello
    ob = observe()
    windows = [o for o in ob["objects"] if o["kind"] == "window" and o.get("name", {}).get("value") == args.window]
    assert len(windows) == 1, "test window missing or ambiguous"
    window = windows[0]
    call("act", {"steps": [{"id": "focus", "op": "focus", "target": {"ref": window["ref"]}}]})
    focused, _, _ = focus_of(observe(window["ref"], "detail"))
    if args.finder:
        press(focused, "N", ["primary", "shift"])
        time.sleep(.2)
        focused, _, _ = focus_of(observe(window["app"]))
        folder = "DW-回归-" + args.log.stem
        call("act", {"steps":[{"id":"rename","op":"set_value","target":{"ref":focused},"set_value":{"text":folder}},{"id":"commit","op":"keyboard.press","target":{"ref":focused},"press":{"key":"Enter"}}]})
        ob = observe(window["ref"], "outline")
        assert any(o.get("name", {}).get("value") == folder for o in ob["objects"]), "created folder absent"
        print(json.dumps({"passed":True,"calls":sequence,"raw_input":False,"test":"Finder inline rename and Enter","folder":folder}))
        raise SystemExit(0)
    press(focused, "O", ["primary"])
    time.sleep(.25)
    focused, panel, obj = focus_of(observe(window["app"]))
    assert panel != window["ref"], "Open panel did not appear"
    press(focused, "G", ["primary", "shift"])
    time.sleep(.2)
    focused, _, _ = focus_of(observe(window["app"]))
    press(focused, "Escape")
    time.sleep(.2)
    focused, panel, obj = focus_of(observe(window["app"]))
    # Moving to a descendant via the window anchor must pass the hit test.
    target = observe(panel, "detail")["objects"][0]
    rb = obj["bounds"]["value"]["rect"]
    wb = target["bounds"]["value"]["rect"]
    u = (rb.get("x",0)+rb["width"]*.5-wb.get("x",0))/wb["width"]
    v = (rb.get("y",0)+rb["height"]*.5-wb.get("y",0))/wb["height"]
    assert 0 <= u <= 1 and 0 <= v <= 1, "focused node geometry outside panel"
    call("act", {"steps": [{"id":"move","op":"pointer.move","target":{"anchor":{"target":panel,"u":u,"v":v}}}]})
    press(focused, "Escape")
    print(json.dumps({"passed":True,"calls":sequence,"raw_input":False,"test":"TextEdit native panels and window-descendant hit test"}))
finally:
    session.stdin.close()
    session.wait(timeout=10)
    log.close()
