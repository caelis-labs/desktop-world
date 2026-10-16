#!/usr/bin/env python3
"""Owned Chrome UIA/input/PNG acceptance on Windows; no CDP or DOM injection."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
from threading import Lock, Thread
import time
import uuid

from windows_probe import MCP, completed, painted_bounds
from probe import focus_verified


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--chrome", type=Path, default=Path(r"C:\Program Files\Google\Chrome\Application\chrome.exe"))
    parser.add_argument("--output", type=Path, default=Path("artifacts"))
    parser.add_argument("--rounds", type=int, default=5)
    args = parser.parse_args()
    assert os.name == "nt"
    package = args.package.resolve()
    root = args.output.resolve() / ("windows-chrome-" + uuid.uuid4().hex[:8])
    root.mkdir(parents=True)
    profile = root / "profile"
    title = "DTW Chrome acceptance " + root.name
    events = []
    event_lock = Lock()
    html = ("<!doctype html><html lang='en'><meta charset='utf-8'><title>" + title + "</title>"
        "<style>body{font:20px Arial;padding:35px}textarea{display:block;width:650px;height:100px}"
        "button{padding:15px;margin:15px}#pixel{appearance:none;background:rgb(32,160,96);border:0;border-radius:0;box-shadow:none}"
        "#long{height:1800px;background:linear-gradient(white,lightblue)}</style>"
        "<h1 id='status'>READY-CHROME</h1><form id='form'><label for='input'>Test input</label>"
        "<textarea id='input'></textarea><button id='submit'>Submit once</button></form>"
        "<button type='button' id='pixel'>Pixel target</button><label><input id='check' type='checkbox'>Test checkbox</label>"
        "<div id='long'>Scroll surface</div><h2>END-OF-PAGE</h2><script>"
        "let eventSequence=0;function log(kind,e){fetch('/events',{method:'POST',headers:{'Content-Type':'application/json'},"
        "body:JSON.stringify({seq:++eventSequence,kind,value:input.value,trusted:e.isTrusted,key:e.key||null,scroll:window.scrollY})})}"
        "const input=document.getElementById('input');"
        "input.addEventListener('input',e=>log('input',e));input.addEventListener('keydown',e=>log('key',e));"
        "document.getElementById('form').addEventListener('submit',e=>{e.preventDefault();"
        "document.getElementById('status').textContent='SUBMITTED-CHROME';log('submit',e)});"
        "document.getElementById('pixel').addEventListener('click',e=>{"
        "document.getElementById('status').textContent='PIXEL-CHROME';log('pixel',e)});"
        "document.getElementById('check').addEventListener('change',e=>log('check',e));"
        "window.addEventListener('wheel',e=>log('wheel',e));"
        "window.addEventListener('scroll',e=>log('scroll',e));"
        "</script></html>")
    (root / "fixture.html").write_text(html, encoding="utf-8")

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            body = html.encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            assert self.path == "/events"
            event = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            with event_lock:
                events.append(event)
                # SendInput exposes transient lone UTF-16 surrogates while an
                # emoji is being typed. Keep them escaped in the event oracle.
                (root / "events.json").write_text(json.dumps(events, ensure_ascii=True, indent=2), encoding="utf-8")
            self.send_response(204)
            self.end_headers()

        def log_message(self, *_):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    Thread(target=server.serve_forever, daemon=True).start()
    chrome = subprocess.Popen([str(args.chrome.resolve()), "--user-data-dir=" + str(profile),
        "--no-first-run", "--no-default-browser-check", "--disable-background-mode",
        "--force-renderer-accessibility", "--disable-translate", "--window-size=1000,800",
        "--app=http://127.0.0.1:" + str(server.server_port)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    report = {"package": str(package), "pid": chrome.pid, "profile": str(profile),
        "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "source_modified": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True).strip()),
        "adapter_sha256": hashlib.sha256((package / "bin/dtw.exe").read_bytes()).hexdigest(),
        "worker_sha256": hashlib.sha256((package / "libexec/cua-driver.exe").read_bytes()).hexdigest(),
        "fixture_sha256": hashlib.sha256(html.encode("utf-8")).hexdigest(), "checks": []}
    mcp = None
    failed = False

    def oracle(kind, value=None):
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            with event_lock:
                found = sorted([dict(e) for e in events if e["kind"] == kind], key=lambda e:e["seq"])
            if found and (value is None or found[-1]["value"] == value):
                return found
            time.sleep(.05)
        return []

    def check(name, function):
        nonlocal failed
        try:
            entry = {"name": name, "status": "PASS", "detail": function()}
        except Exception as error:
            failed = True
            entry = {"name": name, "status": "FAIL", "error": str(error)}
        report["checks"].append(entry)
        (root / "summary.json").write_text(json.dumps(report, ensure_ascii=True, indent=2), encoding="utf-8")
        print(json.dumps(entry, ensure_ascii=False), flush=True)
        return entry["status"] == "PASS"

    try:
        mcp = MCP(package / "bin/dtw.exe", root)
        for _ in range(30):
            windows = completed(mcp.script(f"return await cua.listWindows({{pid:{chrome.pid}}});"))
            candidates = [w for w in windows if title in w.get("title", "")]
            if len(candidates) == 1:
                break
            assert chrome.poll() is None, "Owned Chrome process exited"
            time.sleep(.25)
        else:
            raise RuntimeError("Owned Chrome window not uniquely discovered")
        window_id = candidates[0]["window_id"]
        report["window"] = candidates[0]
        bind = f"const a=await cua.getApp({{pid:{chrome.pid},windowId:{window_id}}});"

        def observe():
            receipt = mcp.script(bind + "const s=await a.getState({tree_format:'elements',max_elements:100,"
                "include_screenshot:true,max_image_dimension:1000});"
                "return {window_id:a.windowId,valid:!!s.capture_id&&s.screenshot_width>0&&s.screenshot_height>0&&!s.screenshot_error,capture_id:s.capture_id,"
                "width:s.screenshot_width,height:s.screenshot_height,truncated:s.truncated,"
                "input:(s.elements||[]).filter(e=>e.role==='Edit'&&e.label==='Test input'),"
                "button:(s.elements||[]).filter(e=>e.role==='Button'&&e.label==='Submit once')};")
            value = completed(receipt)
            assert value["valid"] and value["capture_id"] and len(value["input"]) == len(value["button"]) == 1, value
            png_bytes = mcp.image(receipt, "chrome-before.png")
            return {"geometry": [value["width"], value["height"]], "png_bytes": png_bytes,
                "input_role": value["input"][0]["role"], "button_role": value["button"][0]["role"]}
        if not check("chrome_uia_and_screenshot", observe):
            raise RuntimeError("Chrome target was not identified; no input attempted")

        def focus():
            receipt = mcp.exec(operation="focus", pid=chrome.pid, window_id=window_id)
            completed(receipt)
            native_focus = mcp.structured(receipt)
            assert focus_verified(native_focus, window_id), receipt
            return native_focus
        if not check("chrome_focus", focus):
            raise RuntimeError("Chrome focus was not verified; no input attempted")

        text = "Chrome 中文 🌍\nWindows " + root.name
        def type_text():
            receipt = mcp.script(bind + "const q=(await a.query({role:'text_field',name:'Test input'})).filter(e=>e.role==='Edit'&&e.label==='Test input'&&e.element_token&&e.display_only!==true);"
                "if(q.length!==1)throw new Error('input is absent or ambiguous: '+JSON.stringify(q));"
                f"return await a.typeText({json.dumps(text)},q[0].element_token,{{delivery_mode:'foreground'}});")
            completed(receipt)
            observed = oracle("input", text)
            assert observed and observed[-1]["value"] == text and all(e["trusted"] for e in observed), observed[-4:]
            return {"unicode_multiline": True, "trusted_input_events": len(observed)}
        if not check("chrome_unicode_keyboard_input", type_text):
            raise RuntimeError("Input reconciled from page events; dependent form submission stopped")

        def submit():
            receipt = mcp.script(bind + "const q=(await a.query({role:'button',name:'Submit once'})).filter(e=>e.role==='Button'&&e.label==='Submit once'&&e.element_token&&e.display_only!==true);"
                "if(q.length!==1)throw new Error('submit is absent or ambiguous');return await a.click(q[0].element_token);")
            completed(receipt)
            observed = oracle("submit")
            assert len(observed) == 1 and observed[0]["value"] == text and observed[0]["trusted"], observed
            queried = mcp.exec(operation="result", id=receipt["id"])
            assert queried["id"] == receipt["id"]
            assert len(oracle("submit")) == 1
            readback = completed(mcp.script(bind + "const s=await a.getState({query:'SUBMITTED-CHROME',max_elements:30,include_screenshot:false});return s.tree_markdown;"))
            assert "SUBMITTED-CHROME" in readback, readback
            return {"trusted_submissions": 1, "ui_readback": True, "original_receipt_no_duplicate": True}
        check("chrome_semantic_submit", submit)

        def pixel():
            receipt = mcp.script(bind + "const s=await a.getState({tree_format:'elements',max_elements:100,"
                "include_screenshot:true,max_image_dimension:800});const q=(s.elements||[]).filter(e=>e.label==='Pixel target');"
                "if(q.length!==1||!s.capture_id||s.screenshot_error)throw new Error('pixel target not observed');"
                "return {frame:q[0].screenshot_frame,capture_id:s.capture_id,width:s.screenshot_width,height:s.screenshot_height};")
            value = completed(receipt)
            mcp.image(receipt, "chrome-pixel.png")
            frame = value["frame"]
            assert frame["w"] > 0 and frame["h"] > 0, frame
            # Downscaling blends boundary colors. Include near-green edge pixels;
            # the geometry tolerance remains three PNG pixels.
            actual = painted_bounds(root / "chrome-pixel.png", tolerance=24)
            declared = [frame[key] for key in ("x", "y", "w", "h")]
            error = [round(a-b, 2) for a,b in zip(actual, declared)]
            report["screenshot_alignment"] = {"painted_bbox": actual,
                "uia_screenshot_frame": declared, "error_pixels": error}
            assert all(abs(delta) <= 3 for delta in error), report["screenshot_alignment"]
            point = [frame["x"] + frame["w"] / 2, frame["y"] + frame["h"] / 2]
            script = bind + f"return await a.click({json.dumps(point)},{{capture_id:{json.dumps(value['capture_id'])},delivery_mode:'foreground'}});"
            completed(mcp.script(script))
            observed = oracle("pixel")
            assert len(observed) == 1 and observed[0]["trusted"], observed
            reused = mcp.script(script)
            assert reused["status"] == "failed" and "capture_not_found" in json.dumps(reused), reused
            assert len(oracle("pixel")) == 1
            return {"scaled_png": [value["width"], value["height"]], "point": point,
                "trusted_clicks": 1, "capture_reuse_refused": True}
        check("chrome_scaled_pixel_and_capture_reuse", pixel)

        def stability():
            for index in range(args.rounds):
                marker = "Chrome 稳定性 🌍 " + str(index)
                receipt = mcp.script(bind + "const q=(await a.query({role:'textfield',name:'Test input'})).filter(e=>e.role==='Edit'&&e.label==='Test input'&&e.element_token&&e.display_only!==true);"
                    "if(q.length!==1)throw new Error('input is absent or ambiguous');"
                    "await a.pressKey('ctrl+a',q[0].element_token,{delivery_mode:'foreground'});"
                    f"return await a.typeText({json.dumps(marker)},q[0].element_token,{{delivery_mode:'foreground'}});")
                completed(receipt)
                observed = oracle("input", marker)
                assert observed and observed[-1]["value"] == marker and observed[-1]["trusted"], observed[-4:]
                before = len(oracle("submit"))
                receipt = mcp.script(bind + "const q=(await a.query({role:'button',name:'Submit once'})).filter(e=>e.role==='Button'&&e.label==='Submit once'&&e.element_token&&e.display_only!==true);"
                    "if(q.length!==1)throw new Error('submit is absent or ambiguous');return await a.click(q[0].element_token);")
                completed(receipt)
                observed = oracle("submit", marker)
                assert len(observed) == before+1 and observed[-1]["trusted"], observed[-2:]
            return {"rounds": args.rounds, "independent_page_event_checks": args.rounds}
        check("chrome_repeated_input_and_submit", stability)

        def scroll():
            q = completed(mcp.script(bind + "return await a.query({name:'Scroll surface'});"))
            assert len(q) == 1, q
            receipt = mcp.script(bind + f"return await a.scroll({json.dumps(q[0]['element_token'])},'down',5,{{delivery_mode:'foreground'}});")
            completed(receipt)
            observed = oracle("scroll")
            assert observed and observed[-1]["scroll"] > 0, observed
            assert oracle("wheel") and all(e["trusted"] for e in oracle("wheel"))
            return {"scroll_y": observed[-1]["scroll"], "trusted_wheel_events": len(oracle("wheel"))}
        check("chrome_scroll", scroll)
    except Exception as error:
        failed = True
        report["fatal_error"] = str(error)
        print(json.dumps({"fatal_error": str(error)}, ensure_ascii=False), flush=True)
    finally:
        if mcp:
            try:
                mcp.close()
            except Exception as error:
                failed = True
                report["shutdown_error"] = str(error)
        if chrome.poll() is None:
            # This PID was launched with our unique profile; never kill Chrome by name.
            subprocess.run(["taskkill", "/PID", str(chrome.pid), "/T", "/F"],
                capture_output=True, timeout=15)
            chrome.wait(timeout=10)
        server.shutdown()
        server.server_close()
        report["application_events"] = events
        report["status"] = "FAIL" if failed else "PASS"
        (root / "summary.json").write_text(json.dumps(report, ensure_ascii=True, indent=2), encoding="utf-8")
        print(json.dumps({"record": str(root / "summary.json"), "status": report["status"]}), flush=True)
    if failed:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
