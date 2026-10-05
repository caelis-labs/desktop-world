#!/usr/bin/env python3
"""Opt-in Windows desktop tasks. All UI effects use a persistent dtw helper.
Requires an open Chrome window. Creates an owned Notepad file, loopback browser
form and Win32 fixtures; Calculator operations change only a test calculation.
Never uses WebDriver/CDP, clipboard injection, mocked providers or UIA sidecars.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import queue
import subprocess
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
ENV = dict(os.environ, GOWORK="off", PYTHONUTF8="1")
NO_CONSOLE = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def run(command, **options):
    return subprocess.run(command, cwd=ROOT, env=ENV, check=True, **options)


def known(fact):
    if "known" in fact:
        return fact["known"]
    if fact.get("status") == "known":
        return fact["value"]
    raise RuntimeError("unknown/redacted fact: " + str(fact))


class Session:
    def __init__(self, helper, directory, flags):
        directory.mkdir(parents=True)
        self.wire = (directory / "wire.jsonl").open("x", encoding="utf-8")
        self.stderr = (directory / "stderr.txt").open("x", encoding="utf-8")
        self.process = subprocess.Popen([str(helper), "serve", *flags], cwd=ROOT,
            env=ENV, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr,
            text=True, encoding="utf-8", creationflags=NO_CONSOLE)
        self.lines = queue.Queue()
        self.sequence = 0
        self.receipts = []

        def pump():
            for line in self.process.stdout:
                self.lines.put(line)
            self.lines.put(None)
        threading.Thread(target=pump, daemon=True).start()
        try:
            self.hello = self.receive()
            if self.hello.get("type") != "hello":
                raise RuntimeError(str(self.hello))
        except BaseException:
            self.close()
            raise

    def log(self, direction, value):
        self.wire.write(json.dumps({"at": datetime.now(timezone.utc).isoformat(),
            "direction": direction, "data": value}, ensure_ascii=False) + "\n")
        self.wire.flush()

    def receive(self):
        try:
            line = self.lines.get(timeout=20)
        except queue.Empty:
            raise RuntimeError("dtw transport timeout: inspect original wire; never replay effects")
        if line is None:
            raise RuntimeError("dtw exited; inspect stderr and original wire")
        value = json.loads(line)
        self.log("response", value)
        return value

    def call(self, op, args, request_id=None, allow_error=False):
        self.sequence += 1
        request = {"id": request_id or f"w-{self.sequence}", "op": op, "args": args}
        self.log("request", request)
        self.process.stdin.write(json.dumps(request, ensure_ascii=False) + "\n")
        self.process.stdin.flush()
        response = self.receive()
        if response.get("id") != request["id"]:
            raise RuntimeError("unexpected dtw response ID")
        if op in ("act", "get") and response.get("result"):
            self.receipts.append(response["result"])
        if response.get("error") and not allow_error:
            raise RuntimeError(json.dumps(response, ensure_ascii=False))
        return response if allow_error else response["result"]

    def observe(self, ref=None, match=None, fields=None, depth=24):
        args = {"scope": {"refs": [ref]} if ref else {"desktop": True},
            "projection": "outline" if ref else "summary",
            "fields": fields or ["name", "role", "app", "window"],
            "budget": {"max_depth": depth, "max_visited_nodes": 512,
                "max_results": 100, "max_output_bytes": 16000, "read_deadline_ms": 3000}}
        if match:
            args["match"] = {"within": ref, **match}
        objects, pages = [], []
        for _ in range(24):
            ob = self.call("observe", args)
            pages.append(ob["coverage"])
            objects.extend(ob.get("objects", []))
            continuation = ob["coverage"].get("continuation")
            if not continuation:
                return objects, pages
            args["continuation"] = continuation
        raise RuntimeError("bounded observation did not finish; absence cannot be inferred")

    def find(self, ref, role, name=None):
        match = {"role": role}
        if name is not None:
            match["name_equals"] = name
        def ready():
            objects, pages = self.observe(ref, match, ["name", "role"])
            coverage = pages[-1]
            if len(objects) == 1 and coverage.get("complete") and not coverage.get("dirty") and not coverage.get("truncated") and not coverage.get("unavailable_sources"):
                return objects[0]["ref"]
            return None
        return await_condition(ready, timeout=5)

    def act(self, steps, request_id=None, allow_error=False):
        steps = [{"id": f"s{i}", **step} for i, step in enumerate(steps)]
        value = self.call("act", {"steps": steps}, request_id, allow_error)
        if not allow_error and (value.get("outcome") != "completed" or value.get("seat_health") == "fenced"):
            raise RuntimeError("uncertain task receipt: " + json.dumps(value))
        return value

    def capture(self, ref):
        result = self.call("capture", {"kind": "window_content", "target": ref,
            "max_pixel_width": 1280, "max_pixel_height": 900})
        if not result.get("files") or result["capture"].get("partial"):
            raise RuntimeError("capture incomplete")
        return result["files"][0]["path"]

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                self.process.wait(timeout=5)
        self.wire.close()
        self.stderr.close()


def step(op, ref, **args):
    return {"op": op, "target": {"ref": ref}, **args}


def press(ref, key, modifiers=None):
    return step("keyboard.press", ref, press={"key": key, "modifiers": modifiers or []})


def await_condition(check, timeout=8):
    deadline = time.monotonic() + timeout
    while True:
        result = check()
        if result:
            return result
        if time.monotonic() >= deadline:
            raise RuntimeError("task result did not become observable; effects were not retried")
        time.sleep(.1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--browser-window", required=True, help="exact current Chrome window title")
    parser.add_argument("--calculator-window", default="Calculator", help="localized Calculator title")
    parser.add_argument("--editor-name", default="Text editor", help="localized Notepad document name")
    parser.add_argument("--helper", type=Path, help="existing dtw executable; default builds current source")
    parser.add_argument("--rounds", type=int, default=12, choices=range(1, 21))
    args = parser.parse_args()
    if platform.system() != "Windows" or platform.machine().lower() not in ("amd64", "x86_64"):
        parser.error("requires an interactive Windows amd64 desktop")
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:6]
    out = ROOT / "artifacts" / ("windows-acceptance-" + stamp)
    out.mkdir(parents=True)
    helper = args.helper.resolve() if args.helper else ROOT / "bin/dtw.exe"
    browser_fixture = ROOT / "bin/DWBrowserFixture.exe"
    native_fixture = ROOT / "bin/DWNativeFixture.exe"
    acceptance = ROOT / "bin/windows-acceptance.test.exe"
    if not args.helper:
        run(["go", "build", "-o", str(helper), "./cmd/dtw"])
    for binary, package in ((browser_fixture, "./tests/native-fixtures/browser"),
            (native_fixture, "./tests/native-fixtures/windows")):
        run(["go", "build", "-o", str(binary), package])
    run(["go", "test", "-c", "-o", str(acceptance), "./tests/acceptance"])
    source_files = run(["git", "ls-files", "-c", "-o", "--exclude-standard", "-z"], capture_output=True).stdout.decode().split("\0")
    manifest = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
        for name in sorted(set(source_files)) if (ROOT / name).is_file()
        and Path(name).suffix in (".go", ".mod", ".mjs", ".py", ".ps1")}
    (out / "source-sha256.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    summary = {"run": stamp, "platform": platform.platform(),
        "base_commit": run(["git", "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip(),
        "helper_version": json.loads(run([str(helper), "version"], capture_output=True, text=True).stdout),
        "helper_sha256": hashlib.sha256(helper.read_bytes()).hexdigest(), "tasks": {}, "passed": False}
    children, session = [], None
    try:
        log = out / "browser-events.jsonl"
        title = "DTW Windows Form " + stamp
        server = subprocess.Popen([str(browser_fixture), "-addr", "127.0.0.1:0", "-title", title,
            "-log", str(log)], cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, encoding="utf-8", creationflags=NO_CONSOLE)
        children.append(server)
        url = server.stdout.readline().strip()
        if not url.startswith("http://127.0.0.1:"):
            raise RuntimeError("loopback fixture failed")
        note = out / ("dtw-note-" + stamp + ".txt")
        note.write_text("DTW acceptance seed", encoding="utf-8")
        subprocess.Popen(["notepad.exe", str(note)], creationflags=NO_CONSOLE)
        subprocess.Popen(["calc.exe"], creationflags=NO_CONSOLE)
        # A read-only session waits for application startup, before granting writes.
        probe = Session(helper, out / "probe", [])
        try:
            def ready():
                objects, _ = probe.observe()
                windows = [o for o in objects if o["kind"] == "window"]
                notes = [o for o in windows if known(o["name"]).startswith(note.name + " - ")]
                browsers = [o for o in windows if known(o["name"]) == args.browser_window]
                calcs = [o for o in windows if known(o["name"]) == args.calculator_window]
                return (known(notes[0]["name"]) if len(notes) == 1 and len(browsers) == 1 and calcs else None)
            note_title = await_condition(ready, timeout=15)
        finally:
            probe.close()
        flags = ["--input-mode", "cooperative", "--assets-dir", str(out / "images")]
        for window_title in (args.browser_window, note_title, args.calculator_window):
            flags.extend(["--write-app-window", window_title])
        session = Session(helper, out / "dtw", flags)
        objects, _ = session.observe()
        def window(name):
            matches = [o for o in objects if o["kind"] == "window" and known(o["name"]) == name]
            if not matches: raise RuntimeError("missing window " + name)
            return matches[0]["ref"]
        browser, notepad, calculator = window(args.browser_window), window(note_title), window(args.calculator_window)
        editor = session.find(notepad, "document", args.editor_name)
        session.act([press(browser, "T", ["primary"]),
            step("keyboard.type_text", browser, type_text={"text": url}), press(browser, "Enter")])
        # Reads may wait for a new document; input is never retried.
        def form_ready():
            objs, coverage = session.observe(browser, {"role": "text_field", "name_equals": "DW 网页内容"}, ["name", "role"])
            return objs[0]["ref"] if len(objs) == 1 and coverage[-1].get("complete") else None
        field = await_condition(form_ready)
        text = "Windows 日常表单验收 🌍"
        form_steps = [step("focus", field), press(field, "A", ["primary"]),
            step("keyboard.type_text", field, type_text={"text": text}), press(field, "Enter")]
        receipt = session.act(form_steps, "browser-form")
        duplicate = session.act(form_steps, "browser-form")
        assert receipt["run_id"] == duplicate["run_id"]
        recovered = session.call("get", {"run_id": receipt["run_id"]})
        assert recovered["run_id"] == receipt["run_id"]
        def events_ready():
            events = [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]
            submits = [e for e in events if e["event"] == "submit"]
            if not submits: return None
            assert len(submits) == 1 and submits[0]["trusted"] and submits[0]["value"] == text
            assert any(e["event"] == "keydown" and e["trusted"] for e in events)
            assert any(e["event"] == "input" and e["trusted"] for e in events)
            return events
        events = await_condition(events_ready)
        summary["tasks"]["browser_form"] = {"passed": True, "trusted_events": len(events),
            "submits": 1, "deduplicated": True, "input": receipt["input"], "image": session.capture(browser)}
        negative = session.act([{"op": "bind", "bind": {"name": "canvas", "require_unique": True,
            "locator": {"within": browser, "role": "button", "name_equals": "DW Canvas Secret Action", "max_depth": 24}}}], allow_error=True)
        assert negative.get("error", {}).get("code") in ("ambiguous_target", "search_incomplete")
        reports = []
        for i in range(args.rounds):
            # Each independent Chromium task discovers its current native
            # element before creating a new plan; never rebind an in-flight one.
            field = session.find(browser, "text_field", "DW 网页内容")
            before = session.call("observe", {"scope": {"refs": [notepad]}, "projection": "detail", "fields": ["name", "role"]})["seat"]
            r = session.act([step("pointer.click", field, click={"button": "left", "count": 1}),
                press(field, "A", ["primary"]), step("keyboard.type_text", field, type_text={"text": f"Windows transaction {i} 🌍"})])
            after = session.call("observe", {"scope": {"refs": [notepad]}, "projection": "detail", "fields": ["name", "role"]})["seat"]
            assert r["input"]["restoration"] in ("restored", "not_borrowed")
            assert before["foreground_window"] == after["foreground_window"]
            assert before["focused_object"] == after["focused_object"]
            assert known(before["pointer"])["x"] == known(after["pointer"])["x"]
            assert known(before["pointer"])["y"] == known(after["pointer"])["y"]
            reports.append(r["input"])
        summary["tasks"]["cooperative_stability"] = {"passed": True, "rounds": args.rounds, "reports": reports}
        rejected = session.act([step("keyboard.type_text", field, type_text={"text": "🌍" * 129})], allow_error=True)
        assert rejected["result"]["steps"][0]["delivery"] == "none"
        assert rejected["result"]["fault"]["code"] == "input_burst_limit"
        note_text = "DTW Windows 验证记录 🌍\n浏览器、文本编辑和计算器使用真实任务。\n"
        session.act([step("focus", editor), press(editor, "A", ["primary"]),
            step("keyboard.type_text", editor, type_text={"text": note_text},
                completion="verify",
                # This Notepad provider exposes paragraph separators as CR.
                after=[{"target": {"ref": editor}, "property": "value", "equals_string": note_text.replace("\n", "\r")},
                    {"target": {"ref": notepad}, "property": "name", "equals_string": "*" + note_title}]),
            {**press(editor, "S", ["primary"]), "completion": "verify", "after": [{"target": {"ref": notepad},
                "property": "name", "equals_string": note_title}]}])
        await_condition(lambda: note.read_text(encoding="utf-8-sig") == note_text)
        summary["tasks"]["notepad_save"] = {"passed": True, "file": str(note),
            "sha256": hashlib.sha256(note.read_bytes()).hexdigest(), "image": session.capture(notepad)}
        buttons, _ = session.observe(calculator, {"role": "button"}, ["name", "role"])
        targets = {known(o["name"]): o["ref"] for o in buttons}
        # Names below match the observed English Calculator UI. Other locales
        # must adapt the labels from their actual observation explicitly.
        names = ["Clear", "One", "Two", "Eight", "Plus", "Two", "Five", "Six", "Equals"]
        r = session.act([step("invoke", targets[name]) for name in names])
        def calculated():
            objs, _ = session.observe(calculator, {"role": "text", "name_contains": "Display is"}, ["name", "role"])
            return any(known(o["name"]) == "Display is 384" for o in objs)
        await_condition(calculated)
        assert r["input"]["restoration"] == "not_borrowed"
        summary["tasks"]["calculator"] = {"passed": True, "expression": "128 + 256", "result": 384,
            "channel": "semantic", "image": session.capture(calculator)}
        # Return the browser form to its submitted text for inspectable evidence.
        session.act([step("set_value", field, set_value={"text": text})])
        summary["receipts"] = session.receipts
        session.close()
        session = None
        # Existing independent tests cover raw input, drag cancellation and stale
        # controls in shared mode, separate from cooperative daily tasks.
        native_title = "DTW Windows Native " + stamp
        native_log = out / "native-events.jsonl"
        child = subprocess.Popen([str(native_fixture), "-title", native_title, "-log", str(native_log)],
            cwd=ROOT, creationflags=NO_CONSOLE)
        children.append(child)
        await_condition(lambda: native_log.exists())
        # Launching a window does not guarantee foreground permission. Prepare
        # the shared-input fixture explicitly through its public UIA focus,
        # before starting the independent SDK task; do not replay failed input.
        prep = Session(helper, out / "native-setup", ["--write-app-window", native_title])
        try:
            owned, _ = prep.observe()
            native_win = next(o["ref"] for o in owned if o["kind"] == "window" and known(o["name"]) == native_title)
            native_edit = prep.find(native_win, "text_field", "内容")
            summary["native_foreground_setup"] = prep.act([step("focus", native_edit)])
        finally:
            prep.close()
        test_env = dict(ENV, DW_NATIVE_FIXTURE_TITLE=native_title, DW_NATIVE_FIXTURE_LOG=str(native_log),
            DW_NATIVE_CAPTURE_PATH=str(out / "native-capture.png"), DW_NATIVE_CANCEL_TEST="1")
        result = subprocess.run([str(acceptance), "-test.v", "-test.run", "^TestNativeFixture$"],
            cwd=ROOT, env=test_env, capture_output=True, encoding="utf-8", timeout=60)
        (out / "native-test.txt").write_text(result.stdout + result.stderr, encoding="utf-8")
        if result.returncode: raise RuntimeError(result.stdout + result.stderr)
        summary["tasks"]["native_shared"] = {"passed": True, "test": "TestNativeFixture"}
        summary["passed"] = True
    except BaseException as error:
        summary["error"] = str(error)
        raise
    finally:
        if session:
            summary["receipts"] = session.receipts
            session.close()
        for child in children:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=5)
        (out / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(out, flush=True)
    print(json.dumps({"passed": True, "tasks": list(summary["tasks"])}, ensure_ascii=False))


if __name__ == "__main__":
    main()
