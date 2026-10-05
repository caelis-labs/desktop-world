#!/usr/bin/env python3
"""Opt-in real Windows provider and cooperative failure-path acceptance."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from importlib.util import spec_from_file_location, module_from_spec
spec = spec_from_file_location("windows_tasks", Path(__file__).with_name("accept-windows.py"))
h = module_from_spec(spec)
spec.loader.exec_module(h)
ROOT = h.ROOT

def events(path):
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()] if path.exists() else []

def window(session, title):
    objects, _ = session.observe()
    matches = [o for o in objects if o["kind"] == "window" and h.known(o["name"]) == title]
    return matches[0]["ref"] if len(matches) == 1 else None

def pointer(op, ref, u=.5, v=.5, **args):
    return {"op": op, "target": {"anchor": {"target": ref, "u": u, "v": v}}, **args}

def bind(name, within, role, label):
    return {"op": "bind", "bind": {"name": name, "require_unique": True,
        "locator": {"within": within, "role": role, "name_equals": label,
            "required_states": {"enabled": True, "offscreen": False}, "max_depth": 16}}}

def electron_acceptance(executable, helper, out, children, sessions, human_title, rounds):
    title = "DTW Electron " + out.name
    log = out / "electron-events.jsonl"
    native_log = out / "electron-native.jsonl"
    html = (ROOT / "poc/background-input/Fixture.html").read_text(encoding="utf-8").replace("__TITLE__", title)
    html = html.replace("event,value:String(value)", "event,value:String(value),trusted:globalThis.event?.isTrusted??null")
    html = html.replace('<script>', '<button aria-label="POC native dialog" onclick="fixture.dialog()">Native confirmation</button><script>', 1)
    event_lock = threading.Lock()
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass
        def do_GET(self):
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(html.encode("utf-8"))
        def do_POST(self):
            row = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            # Windows append handles do not serialize concurrent seek/write.
            with event_lock:
                with log.open("a", encoding="utf-8") as f:
                    # Preserve intermediate, incomplete surrogate pairs.
                    f.write(json.dumps(row, ensure_ascii=True) + "\n")
            self.send_response(204)
            self.end_headers()
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    config = out / "electron-config.json"
    dialog_title = "DTW Native Confirmation " + out.name
    config.write_text(json.dumps({"url": "http://127.0.0.1:" + str(server.server_port),
        "profile": str(out / "electron-profile"), "log": str(native_log), "dialogTitle": dialog_title}), encoding="utf-8")
    child = subprocess.Popen([str(executable), str(ROOT / "tests/native-fixtures/electron"), str(config)],
        stdout=(out / "electron.stdout").open("w"), stderr=(out / "electron.stderr").open("w"), creationflags=h.NO_CONSOLE)
    children.append(child)
    probe = h.Session(helper, out / "electron-probe", [])
    sessions.append(probe)
    h.await_condition(lambda: window(probe, title), timeout=15)
    session = h.Session(helper, out / "electron", ["--input-mode", "cooperative", "--write-app-window", title,
        "--assets-dir", str(out / "images")])
    sessions.append(session)
    win = window(session, title)
    # A BrowserWindow/title can exist before its renderer accessibility tree.
    # Wait by observation only; do not interpret early zero matches as failure.
    h.await_condition(lambda: any(e["event"] == "ready" for e in events(native_log)), timeout=15)
    def canvas_ready():
        objects, pages = session.observe(win, {"role": "image", "name_equals": "POC Canvas"}, ["name", "role"])
        return objects[0]["ref"] if len(objects) == 1 and pages[-1].get("complete") else None
    canvas = h.await_condition(canvas_ready, timeout=15)
    field = session.find(win, "text_field", "POC text")
    multi = session.find(win, "text_field", "POC multiline")
    submit = session.find(win, "button", "POC submit")
    dialog_button = session.find(win, "button", "POC dialog")
    native_dialog = session.find(win, "button", "POC native dialog")
    human = h.Session(helper, out / "human", ["--write-app-window", human_title])
    sessions.append(human)
    human_win = window(human, human_title)
    human_field = human.find(human_win, "text_field", "内容")
    human.act([h.step("focus", human_field), h.step("pointer.click", human_field, click={"button": "left", "count": 1})])
    def seat():
        return session.call("observe", {"scope": {"refs": [win]}, "projection": "summary", "fields": ["role"]})["seat"]
    receipts = []
    def check(name, steps, event=None, value=None, error=None):
        before = len(events(log))
        before_seat = seat()
        result = session.act(steps, request_id="electron-" + name, allow_error=bool(error))
        if error:
            assert result.get("error", {}).get("code") == error, result
            receipt = result["result"]
        else:
            receipt = result
        after_seat = seat()
        assert receipt.get("input", {}).get("restoration") in ("restored", "not_borrowed"), (receipt, before_seat, after_seat)
        if receipt.get("input", {}).get("restoration") == "restored":
            for prop in ("foreground_application", "foreground_window", "focused_object"):
                assert "known" in before_seat[prop], (prop, before_seat)
                assert before_seat[prop] == after_seat[prop], (prop, before_seat, after_seat)
        if event:
            h.await_condition(lambda: any(e["event"] == event and (value is None or e.get("value") == value)
                for e in events(log)[before:]))
        receipts.append({"name": name, "receipt": receipt, "before_seat": before_seat, "after_seat": after_seat})
        print("Electron:", name, flush=True)
        return receipt
    try:
        check("move", [pointer("pointer.move", canvas)], "move")
        for name, button, count, event in (("left", "left", 1, "click"), ("double", "left", 2, "double"), ("middle", "middle", 1, "middle")):
            check(name, [pointer("pointer.click", canvas, click={"button": button, "count": count})], event)
        check("drag", [pointer("pointer.drag", canvas, .2, .5,
            drag={"to": {"anchor": {"target": canvas, "u": .7, "v": .5}}, "duration_ms": 250})], "drop")
        check("vertical-wheel", [pointer("pointer.scroll", canvas, scroll={"dx": 0, "dy": 3, "unit": "wheel_step"})], "scroll")
        check("horizontal-wheel", [pointer("pointer.scroll", canvas, scroll={"dx": 3, "dy": 0, "unit": "wheel_step"})], "scroll")
        text = "Windows Electron 中文🙂"
        check("unicode-submit", [h.step("pointer.click", field, click={"button": "left", "count": 1}),
            h.step("keyboard.type_text", field, type_text={"text": text}, completion="verify",
                after=[{"target": {"ref": field}, "property": "value", "equals_string": text}]), h.step("invoke", submit)], "submit", text)
        text = "Line1\n中文🙂\nLine3"
        check("multiline", [h.step("pointer.click", multi, click={"button": "left", "count": 1}),
            h.step("keyboard.type_text", multi, type_text={"text": text}, completion="verify",
                after=[{"target": {"ref": multi}, "property": "value", "equals_string": text}])], "multiline", text)
        check("context-menu", [pointer("pointer.click", canvas, click={"button": "right", "count": 1}),
            bind("menu", win, "menu_item", "POC menu commit"), {"op": "invoke", "target": {"bound": "menu"}}], "menu_commit")
        check("html-dialog", [h.step("pointer.click", dialog_button, click={"button": "left", "count": 1}),
            bind("dialog_text", win, "text_field", "POC dialog text"),
            {"op": "pointer.click", "target": {"bound": "dialog_text"}, "click": {"button": "left", "count": 1}},
            {"op": "keyboard.type_text", "target": {"bound": "dialog_text"}, "type_text": {"text": "CONFIRMED-中文"}},
            bind("confirm", win, "button", "POC confirm"), {"op": "invoke", "target": {"bound": "confirm"}}], "dialog_text", "CONFIRMED-中文")
        # Chromium may recreate accessibility nodes after modal transitions.
        canvas = session.find(win, "image", "POC Canvas")
        field = session.find(win, "text_field", "POC text")
        check("oversize-text", [h.step("keyboard.type_text", field, type_text={"text": "🙂" * 129})], error="input_burst_limit")
        check("oversize-drag", [pointer("pointer.drag", canvas,
            drag={"to": {"ref": canvas}, "duration_ms": 501})], error="input_burst_limit")
        # Cancellation may return before native cleanup. Reconcile its original
        # RunID; never create a replacement drag to recover uncertain output.
        cancel_steps = [pointer("pointer.click", canvas, click={"button": "left", "count": 1}),
            pointer("pointer.drag", canvas, .2, .5, drag={"to": {"anchor": {"target": canvas, "u": .7, "v": .5}}, "duration_ms": 500}, timeout_ms=100)]
        cancelled = session.act(cancel_steps, request_id="cancel-drag", allow_error=True)
        assert cancelled.get("error"), cancelled
        run_id = cancelled["result"]["run_id"]
        def reconciled():
            result = session.call("get", {"run_id": run_id}, allow_error=True)["result"]
            return result if result.get("state") == "terminal" and result.get("input", {}).get("restoration") == "restored" else None
        cancelled = h.await_condition(reconciled)
        assert cancelled["seat_health"] == "ready", cancelled
        assert session.act(cancel_steps, request_id="cancel-drag", allow_error=True)["result"]["run_id"] == run_id
        receipts.append({"name": "cancel-drag", "receipt": cancelled})
        check("after-cancel", [pointer("pointer.click", canvas, click={"button": "left", "count": 1})], "click")
        wait_steps = [pointer("pointer.click", canvas, click={"button": "left", "count": 1}),
            {"op": "wait", "timeout_ms": 2500, "after": [{"target": {"ref": field}, "property": "value", "equals_string": "NEVER"}]}]
        began = time.monotonic()
        check("budget-expiry", wait_steps, error="input_lease_expired")
        assert time.monotonic() - began < 1.8
        # A second dtw session simulates the user's choice of a third process.
        third_title = "DTW User Switch " + out.name
        third_child = subprocess.Popen([str(ROOT / "bin/DWNativeFixture.exe"), "-title", third_title,
            "-log", str(out / "third-events.jsonl")], creationflags=h.NO_CONSOLE)
        children.append(third_child)
        h.await_condition(lambda: window(probe, third_title))
        third = h.Session(helper, out / "third", ["--write-app-window", third_title])
        sessions.append(third)
        third_win = window(third, third_title)
        third_field = third.find(third_win, "text_field", "内容")
        human.act([h.step("focus", human_field)])
        interrupted = []
        def running():
            interrupted.append(session.act(wait_steps, request_id="user-switch", allow_error=True))
        worker = threading.Thread(target=running)
        worker.start()
        time.sleep(.25)
        third.act([h.step("focus", third_field)])
        worker.join(5)
        assert not worker.is_alive() and interrupted, "interrupted task failed to settle"
        result = interrupted[0]
        assert result.get("error", {}).get("code") == "user_interrupted" and result["result"]["input"]["restoration"] == "user_superseded", result
        third.observe()
        def third_ready():
            observed = third.call("observe", {"scope": {"refs": [third_win]}, "projection": "summary", "fields": ["role"]})
            observed_seat = observed["seat"]
            return observed_seat if observed_seat.get("foreground_window") == {"known": third_win} and observed_seat.get("focused_object") == {"known": third_field} else None
        h.await_condition(third_ready, timeout=2)
        third.act([h.step("keyboard.type_text", third_field, type_text={"text": "THIRD-KEPT"})])
        h.await_condition(lambda: any(e["event"] == "text_changed" and e["value"] == "THIRD-KEPT" for e in events(out / "third-events.jsonl")))
        receipts.append({"name": "user-switch", "receipt": result["result"]})
        human.act([h.step("focus", human_field)])
        field = session.find(win, "text_field", "POC text")
        submit = session.find(win, "button", "POC submit")
        for i in range(rounds):
            # Observe between independent tasks. Chromium may replace native
            # nodes after a DOM update; never silently rebind a running plan.
            field = session.find(win, "text_field", "POC text")
            submit = session.find(win, "button", "POC submit")
            value = f"Round {i:02d} 中文🙂"
            steps = [h.step("pointer.click", field, click={"button": "left", "count": 1}), h.press(field, "A", ["primary"]),
                h.step("keyboard.type_text", field, type_text={"text": value}, completion="verify",
                    after=[{"target": {"ref": field}, "property": "value", "equals_string": value}]), h.step("invoke", submit)]
            original = check(f"round-{i:02d}", steps, "submit", value)
            replay = session.act(steps, request_id=f"electron-round-{i:02d}")
            assert replay["run_id"] == original["run_id"]
        assert len([e for e in events(log) if e["event"] == "submit"]) == rounds + 1
        assert any(e["event"] == "text" and e.get("trusted") is True for e in events(log))
        assert all(e.get("trusted") is True for e in events(log) if e["event"] in ("click", "double", "middle", "drag", "scroll"))
        menu_objects, menu_pages = session.observe(win, {"role": "menu_item"}, ["name", "role"])
        (out / "electron-native-menu-coverage.json").write_text(json.dumps({"objects": menu_objects, "coverage": menu_pages}, indent=2), encoding="utf-8")
        check("native-menu-shortcut", [h.press(win, "M", ["primary", "shift"])])
        h.await_condition(lambda: any(e["event"] == "native_menu" for e in events(native_log)))
        native_dialog = session.find(win, "button", "POC native dialog")
        check("native-dialog-open", [h.step("invoke", native_dialog)])
        dialog_win = h.await_condition(lambda: window(session, dialog_title))
        confirm = session.find(dialog_win, "button", "Confirm")
        check("native-dialog-confirm", [h.step("invoke", confirm)])
        h.await_condition(lambda: any(e["event"] == "native_dialog_result" and e["value"] == "1" for e in events(native_log)))
        image = session.capture(win)
        # Quit only the owned target while a no-input verification wait runs.
        field = session.find(win, "text_field", "POC text")
        canvas = session.find(win, "image", "POC Canvas")
        wait_steps = [pointer("pointer.click", canvas, click={"button": "left", "count": 1}),
            {"op": "wait", "timeout_ms": 2500, "after": [{"target": {"ref": field}, "property": "value", "equals_string": "NEVER"}]}]
        quitting = []
        worker = threading.Thread(target=lambda: quitting.append(session.act(wait_steps, request_id="app-quit", allow_error=True)))
        worker.start()
        time.sleep(.25)
        child.terminate()
        child.wait(timeout=5)
        worker.join(5)
        assert not worker.is_alive() and quitting and quitting[0].get("error"), quitting
        assert quitting[0]["result"]["seat_health"] == "ready", quitting
        dead = session.call("observe", {"scope": {"refs": [win]}, "projection": "detail", "fields": ["role"]}, allow_error=True)
        gone = session.act([h.step("focus", win)], request_id="closed-window-write", allow_error=True)
        assert gone.get("error", {}).get("code") == "ref_gone" and gone["result"]["steps"][0]["delivery"] == "none", gone
        assert not dead.get("result", {}).get("objects"), dead
        receipts.append({"name": "app-quit", "receipt": quitting[0]["result"]})
        # Transport failure after terminal input: a fresh helper has a new Epoch.
        # Old references fail; no receipt or dedup state is invented across exit.
        crashed = h.Session(helper, out / "crashed-helper", ["--write-app-window", human_title])
        sessions.append(crashed)
        old_win = window(crashed, human_title)
        old_epoch = crashed.call("observe", {"scope": {"refs": [old_win]}, "projection": "summary", "fields": ["role"]})["epoch"]
        crashed.process.terminate()
        crashed.process.wait(timeout=5)
        replacement = h.Session(helper, out / "replacement-helper", ["--write-app-window", human_title])
        sessions.append(replacement)
        new_win = window(replacement, human_title)
        new_epoch = replacement.call("observe", {"scope": {"refs": [new_win]}, "projection": "summary", "fields": ["role"]})["epoch"]
        assert new_epoch != old_epoch
        obsolete = replacement.call("observe", {"scope": {"refs": [old_win]}, "projection": "detail"}, allow_error=True)
        assert obsolete.get("error"), obsolete
        receipts.append({"name": "helper-restart", "old_epoch": old_epoch, "new_epoch": new_epoch, "obsolete_ref": obsolete["error"]})
        return {"receipts": receipts, "image": image, "runtime": events(native_log)[0]}
    finally:
        server.shutdown()
        server.server_close()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--helper", type=Path)
    parser.add_argument("--electron", type=Path)
    parser.add_argument("--rounds", type=int, default=32, choices=range(1, 101))
    args = parser.parse_args()
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:6]
    out = ROOT / "artifacts" / ("windows-stability-" + stamp)
    out.mkdir(parents=True)
    helper = args.helper.resolve() if args.helper else ROOT / "bin/dtw.exe"
    if not args.helper:
        h.run(["go", "build", "-o", str(helper), "./cmd/dtw"])
    h.run(["go", "build", "-o", str(ROOT / "bin/DWNativeFixture.exe"), "./tests/native-fixtures/windows"])
    source_files = h.run(["git", "ls-files", "-c", "-o", "--exclude-standard", "-z"], capture_output=True).stdout.decode().split("\0")
    manifest = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
        for name in sorted(set(source_files)) if (ROOT / name).is_file()
        and Path(name).suffix in (".go", ".mod", ".mjs", ".py", ".ps1", ".cjs", ".html", ".sh")}
    (out / "source-sha256.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    summary = {"passed": False, "helper_version": json.loads(h.run([str(helper), "version"], capture_output=True, text=True).stdout),
        "helper_sha256": hashlib.sha256(helper.read_bytes()).hexdigest(),
        "native_fixture_sha256": hashlib.sha256((ROOT / "bin/DWNativeFixture.exe").read_bytes()).hexdigest(), "scenarios": {}}
    children, sessions = [], []
    try:
        title = "DTW Semantic " + stamp
        log = out / "native-events.jsonl"
        native = subprocess.Popen([str(ROOT / "bin/DWNativeFixture.exe"), "-title", title, "-semantics", "-log", str(log)], creationflags=h.NO_CONSOLE)
        children.append(native)
        probe = h.Session(helper, out / "probe", [])
        sessions.append(probe)
        h.await_condition(lambda: window(probe, title))
        probe.close()
        sessions.remove(probe)
        session = h.Session(helper, out / "native", ["--input-mode", "cooperative", "--write-app-window", title,
            "--assets-dir", str(out / "images")])
        sessions.append(session)
        win = window(session, title)
        checkbox = session.find(win, "checkbox", "Receive updates")
        shanghai = session.find(win, "list_item", "Shanghai")
        archive = session.find(win, "tree_item", "Order archive")
        inspect = session.find(win, "button", "Inspect state")
        def snapshot():
            n = len(events(log))
            session.act([h.step("invoke", inspect)])
            row = h.await_condition(lambda: next((e for e in events(log)[n:] if e["event"] == "state"), None))
            return json.loads(row["value"])
        def state_action(op, ref, prop, value):
            return session.act([h.step(op, ref, **{op: {prop: value}})])
        assert snapshot() == {"checked": False, "selected": ["Beijing"], "expanded": False, "last_visible": False, "last_choice_visible": False}
        on = state_action("set_checked", checkbox, "checked", True)
        count = len([e for e in events(log) if e["event"] == "checked"])
        noop = state_action("set_checked", checkbox, "checked", True)
        assert len([e for e in events(log) if e["event"] == "checked"]) == count
        assert noop["steps"][0]["verification"] == "verified" and noop["steps"][0]["delivery"] == "not_applicable", noop
        state_action("set_selected", shanghai, "selected", True)
        state_action("set_expanded", archive, "expanded", True)
        last = session.find(win, "list_item", "Destination 79")
        scroll_response = session.act([h.step("scroll_into_view", last)], allow_error=True)
        if scroll_response.get("error"):
            summary["scenarios"]["scroll_failure"] = {"original": scroll_response, "oracle": events(log)[-1]}
            raise RuntimeError(json.dumps(scroll_response))
        scrolled = scroll_response["result"]
        state = snapshot()
        assert state == {"checked": True, "selected": ["Beijing", "Shanghai"], "expanded": True, "last_visible": False, "last_choice_visible": True}, state
        tree_last = session.find(win, "tree_item", "Invoice 79")
        tree_scrolled = session.act([h.step("scroll_into_view", tree_last)])
        tree_state = snapshot()
        assert tree_state["last_visible"] is True and tree_state["expanded"] is True, tree_state
        state_action("set_selected", shanghai, "selected", False)
        state_action("set_checked", checkbox, "checked", False)
        state_action("set_expanded", archive, "expanded", False)
        final = snapshot()
        assert final == {"checked": False, "selected": ["Beijing"], "expanded": False, "last_visible": False, "last_choice_visible": False}, final
        summary["scenarios"]["native_semantic_state"] = {"state": state, "final": final,
            "checked_on": on, "checked_noop": noop, "list_scroll": scrolled, "tree_scroll": tree_scrolled, "tree_state": tree_state}
        field = session.find(win, "text_field", "内容")
        cover = session.find(win, "button", "Cover input")
        uncover = session.find(win, "button", "Uncover input")
        donor_title = "DTW Restoration Donor " + stamp
        donor_child = subprocess.Popen([str(ROOT / "bin/DWNativeFixture.exe"), "-title", donor_title,
            "-log", str(out / "donor-events.jsonl")], creationflags=h.NO_CONSOLE)
        children.append(donor_child)
        donor = h.Session(helper, out / "donor", ["--write-app-window", donor_title])
        sessions.append(donor)
        donor_win = h.await_condition(lambda: window(donor, donor_title))
        donor_field = donor.find(donor_win, "text_field", "内容")
        session.act([h.step("invoke", cover)])
        donor.act([h.step("focus", donor_field)])
        def native_seat():
            # The independent donor helper has retained its window and EDIT.
            # Unknown refs in the target-only helper cannot prove restoration.
            return donor.call("observe", {"scope": {"refs": [donor_win]}, "projection": "summary", "fields": ["role"]})["seat"]
        before_seat = native_seat()
        assert before_seat["foreground_window"] == {"known": donor_win}, before_seat
        assert before_seat["focused_object"] == {"known": donor_field}, before_seat
        before_down = len([e for e in events(log) if e["event"] == "pointer_down"])
        refused = session.act([h.step("pointer.click", field, click={"button": "left", "count": 1})], request_id="covered-input", allow_error=True)
        after_seat = native_seat()
        assert refused.get("error", {}).get("code") == "target_not_hittable" and refused["result"]["input"]["restoration"] == "restored", refused
        assert refused["result"]["steps"][0]["delivery"] == "none" and refused["result"]["seat_health"] == "ready", refused
        for prop in ("foreground_window", "focused_object"):
            assert before_seat[prop] == after_seat[prop], (before_seat, after_seat)
        assert len([e for e in events(log) if e["event"] == "pointer_down"]) == before_down
        session.act([h.step("invoke", uncover)])
        donor.act([h.step("focus", donor_field)])
        accepted = session.act([h.step("pointer.click", field, click={"button": "left", "count": 1})])
        assert accepted["input"]["restoration"] == "restored", accepted
        h.await_condition(lambda: len([e for e in events(log) if e["event"] == "pointer_down"]) == before_down + 1)
        summary["scenarios"]["refusal_restoration"] = {"refused": refused["result"], "accepted": accepted, "before_seat": before_seat, "after_seat": after_seat}
        normal_image = session.capture(win)
        resize = session.find(win, "button", "Resize fixture")
        before_geometry = session.call("observe", {"scope": {"refs": [win]}, "projection": "detail", "fields": ["bounds"]})["objects"][0]
        session.act([h.step("invoke", resize)])
        after_geometry = session.call("observe", {"scope": {"refs": [win]}, "projection": "detail", "fields": ["bounds"]})["objects"][0]
        assert before_geometry["geometry_version"] != after_geometry["geometry_version"], (before_geometry, after_geometry)
        resized_image = session.capture(win)
        refused_captures = []
        for button_name in ("Minimize briefly", "Hide briefly"):
            def restored_button():
                objects, pages = session.observe(win, {"role": "button", "name_equals": button_name}, ["name", "role"])
                coverage = pages[-1]
                if len(objects) == 1 and coverage.get("complete") and not coverage.get("dirty") and not coverage.get("unavailable_sources"):
                    return objects[0]["ref"]
                return None
            # A restored HWND can precede UIA readiness. Wait with reads only;
            # never retry the invoke or capture request.
            button = h.await_condition(restored_button)
            before_events = len(events(log))
            session.act([h.step("invoke", button)])
            capture = session.call("capture", {"kind": "window_content", "target": win, "max_pixel_width": 800, "max_pixel_height": 700}, allow_error=True)
            assert capture.get("error", {}).get("code") == "window_not_visible", capture
            h.await_condition(lambda: any(e["event"] == "visibility_restored" for e in events(log)[before_events:]))
            refused_captures.append({"state": button_name, "fault": capture["error"]})
        restored_image = session.capture(win)
        summary["scenarios"]["native_window_capture"] = {"normal": normal_image, "resized": resized_image, "restored": restored_image,
            "refused": refused_captures, "before_geometry": before_geometry, "after_geometry": after_geometry}
        if args.electron:
            summary["scenarios"]["electron"] = electron_acceptance(args.electron.resolve(), helper, out, children, sessions, title, args.rounds)
        summary["passed"] = True
    finally:
        for session in reversed(sessions):
            session.close()
        for child in children:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=5)
        (out / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
        print(out)
    if not summary["passed"]:
        raise RuntimeError("acceptance incomplete; inspect original receipts")

if __name__ == "__main__":
    main()
