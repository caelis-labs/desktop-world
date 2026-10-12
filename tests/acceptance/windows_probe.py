#!/usr/bin/env python3
"""Windows live acceptance of the packaged DTW, using only owned fixture windows.

Host code launches fixtures and reads their independent event/disk oracles.
All window discovery, reads, screenshots and input go through DTW's sole exec.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
import uuid

from PIL import Image, ImageChops

from probe import call, exec_call, result, runtime_env, parse_exec_result, focus_verified


class MCP:
    def __init__(self, binary, root):
        self.root = root
        self.ident = 0
        self.records = []
        self.uncertain = False
        self.stderr = (root / "mcp-stderr.log").open("w", encoding="utf-8")
        self.proc = subprocess.Popen([str(binary)], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=self.stderr, text=True,
            encoding="utf-8", bufsize=1, env=runtime_env())
        self.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "dtw-windows-live", "version": "1"}})
        tools = self.request("tools/list", {})["result"]["tools"]
        assert [tool["name"] for tool in tools] == ["exec"], tools

    def request(self, method, params):
        self.ident += 1
        return call(self.proc, {"jsonrpc": "2.0", "id": self.ident,
            "method": method, "params": params}, timeout=45)

    def exec(self, **args):
        if self.uncertain:
            raise RuntimeError("An original execution is unresolved; further scripts stopped")
        response = self.request("tools/call", {"name": "exec", "arguments": args})
        assert "error" not in response, response
        receipt = parse_exec_result(response["result"])
        original = receipt.get("id")
        deadline = time.monotonic() + 45
        while receipt.get("status") == "running" and time.monotonic() < deadline:
            time.sleep(.05)
            response = self.request("tools/call", {"name": "exec", "arguments": {
                "session": args.get("session", "default"), "operation": "result",
                "id": original}})
            receipt = parse_exec_result(response["result"])
        self.records.append({"arguments": args, "receipt": receipt})
        self.flush()
        if receipt.get("status") in ("running", "unknown"):
            self.uncertain = True
            raise RuntimeError(f"Unresolved original execution {original}: {receipt}")
        if receipt.get("status") == "completed" and "...truncated " in receipt.get("text", ""):
            receipt = dict(receipt, value=self.structured(receipt, args.get("session", "default"))["value"])
        return receipt

    def structured(self, receipt, session="windows-live"):
        chunks = []
        offset = 0
        while True:
            response = self.request("tools/call", {"name": "exec", "arguments": {
                "session": session, "operation": "result", "id": receipt["id"],
                "part": "structured", "offset": offset}})
            page = json.loads(response["result"]["content"][0]["text"])["page"]
            chunks.append(page["data"])
            offset = page["next_offset"]
            if offset is None:
                return json.loads("".join(chunks))

    def script(self, script, session="windows-live"):
        return self.exec(session=session, script=script)

    def image(self, receipt, name, session="windows-live"):
        response = self.request("tools/call", {"name": "exec", "arguments": {
            "session": session, "operation": "result", "part": "image",
            "id": receipt["id"]}})
        for block in response["result"].get("content", []):
            if block.get("type") == "image":
                payload = base64.b64decode(block["data"])
                assert payload.startswith(b"\x89PNG\r\n\x1a\n"), block["mimeType"]
                (self.root / name).write_bytes(payload)
                return len(payload)
        raise AssertionError("No retained screenshot")

    def flush(self):
        (self.root / "receipts.json").write_text(
            json.dumps(self.records, ensure_ascii=False, indent=2), encoding="utf-8")

    def close(self):
        self.proc.stdin.close()
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
            raise RuntimeError("MCP needed forced termination")
        finally:
            self.stderr.close()
        assert self.proc.returncode == 0, self.proc.returncode


def completed(receipt):
    assert receipt.get("status") == "completed", receipt
    return receipt.get("value")


def native_events(root):
    path = root / "events.tsv"
    if not path.exists():
        return []
    return [{"kind": line.split("\t", 1)[0],
        "value": base64.b64decode(line.split("\t", 1)[1]).decode("utf-8")}
        for line in path.read_text(encoding="utf-8").splitlines() if "\t" in line]


def painted_bounds(path, color=(32, 160, 96), tolerance=0):
    """Find a fixture-owned flat color independently of UIA geometry."""
    with Image.open(path) as image:
        masks = [channel.point(lambda pixel, expected=expected: 255 if abs(pixel-expected) <= tolerance else 0)
                 for channel, expected in zip(image.convert("RGB").split(), color)]
        box = ImageChops.multiply(ImageChops.multiply(masks[0], masks[1]), masks[2]).getbbox()
    assert box, "Fixture's green pixel button not found in PNG"
    return [box[0], box[1], box[2]-box[0], box[3]-box[1]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--output", type=Path, default=Path("artifacts"))
    parser.add_argument("--inspect", action="store_true", help="Only discover and capture the native fixture")
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--dpi-aware", action="store_true", help="Also test the fixture with system DPI awareness")
    parser.add_argument("--picker-mode", choices=("semantic", "pixel", "skip"), default="semantic")
    args = parser.parse_args()
    assert os.name == "nt", "This harness requires a Windows interactive desktop"
    package = args.package.resolve()
    root = args.output.resolve() / ("windows-live-" + uuid.uuid4().hex[:8])
    root.mkdir(parents=True)
    title = "DTW native acceptance " + root.name
    fixture_source = Path(__file__).parent / "fixtures/windows_forms.cs"
    fixture = root / "windows-fixture.exe"
    compiler = Path(os.environ["SystemRoot"]) / "Microsoft.NET/Framework64/v4.0.30319/csc.exe"
    subprocess.run([str(compiler), "/nologo", "/target:winexe", "/platform:x64",
        "/reference:System.Windows.Forms.dll", "/reference:System.Drawing.dll",
        "/out:" + str(fixture), str(fixture_source.resolve())], check=True)
    (root / "picker.txt").write_text("DTW owned picker fixture", encoding="utf-8")
    fixture_process = subprocess.Popen([str(fixture), str(root), title] + (["dpi-aware"] if args.dpi_aware else []))
    report = {"package": str(package), "pid": fixture_process.pid, "title": title,
        "fixture_binary_sha256": hashlib.sha256(fixture.read_bytes()).hexdigest(),
        "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "source_modified": bool(subprocess.check_output(["git", "status", "--porcelain"], text=True).strip()),
        "adapter_sha256": hashlib.sha256((package / "bin/dtw.exe").read_bytes()).hexdigest(),
        "worker_sha256": hashlib.sha256((package / "libexec/cua-driver.exe").read_bytes()).hexdigest(),
        "dpi_aware_fixture": args.dpi_aware, "picker_mode": args.picker_mode, "checks": []}
    mcp = None
    failed = False

    def check(name, function):
        nonlocal failed
        try:
            detail = function()
            entry = {"name": name, "status": "PASS", "detail": detail}
        except Exception as error:
            failed = True
            entry = {"name": name, "status": "FAIL", "error": str(error)}
        report["checks"].append(entry)
        (root / "summary.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        print(json.dumps(entry, ensure_ascii=False), flush=True)
        return entry["status"] == "PASS"

    try:
        for _ in range(100):
            if any(e["kind"] == "ready" for e in native_events(root)):
                break
            assert fixture_process.poll() is None, "Native fixture exited"
            time.sleep(.1)
        else:
            raise TimeoutError("Native fixture did not become ready")
        mcp = MCP(package / "bin/dtw.exe", root)
        discovery = completed(mcp.script(f"return await cua.listWindows({{pid:{fixture_process.pid}}});"))
        candidates = [w for w in discovery if w.get("title") == title]
        assert len(candidates) == 1, discovery
        window_id = candidates[0]["window_id"]
        report["window"] = candidates[0]
        bind = f"const a=await cua.getApp({{pid:{fixture_process.pid},windowId:{window_id}}});"

        def read(query=None, screenshot=False):
            options = {"tree_format": "elements", "max_elements": 80,
                "include_screenshot": screenshot, "max_image_dimension": 1000}
            if query:
                options["query"] = query
            return mcp.script(bind + f"const s=await a.getState({json.dumps(options)});"
                "return {window_id:a.windowId,tree:s.tree_markdown,elements:s.elements,"
                "truncated:s.truncated,capture_id:s.capture_id,valid:!!s.capture_id&&s.screenshot_width>0&&s.screenshot_height>0&&!s.screenshot_error,"
                "width:s.screenshot_width,height:s.screenshot_height,error:s.screenshot_error};")

        def capture():
            receipt = read(screenshot=True)
            value = completed(receipt)
            assert value["valid"] and value["capture_id"], value
            size = mcp.image(receipt, "native-before.png")
            return {"width": value["width"], "height": value["height"], "png_bytes": size}

        if not check("native_discovery_and_screenshot", capture):
            raise RuntimeError("Native fixture could not be observed; no input attempted")
        initial = read()
        completed(initial)
        # Preserve full bounded tree even if the adapter omits its large inline value.
        tree_response = mcp.request("tools/call", {"name": "exec", "arguments": {
            "session": "windows-live", "operation": "result", "id": initial["id"],
            "part": "structured"}})
        (root / "native-tree.json").write_text(json.dumps(tree_response, indent=2), encoding="utf-8")
        if args.inspect:
            report["input_query"] = completed(mcp.script(bind + "return await a.query({role:'Edit',name:'Test input'});"))
            print(json.dumps({"record": str(root / "summary.json"), "tree": str(root / "native-tree.json")}))
            return

        def region_recipe():
            receipt = mcp.script(bind + "const s=await a.getState({tree_format:'elements',max_elements:80,"
                "include_screenshot:true,max_image_dimension:1000});"
                "const q=(s.elements||[]).filter(e=>e.label==='Pixel target');"
                "if(q.length!==1||!s.capture_id||s.screenshot_error)throw new Error('region target not observed');"
                "return {frame:q[0].screenshot_frame,actions:q[0].actions};")
            value = completed(receipt)
            frame = value["frame"]
            recipe = (package / "skills/desktop-world/RECIPES.md").read_text(encoding="utf-8")
            section = recipe.split("## Region candidates from a window screenshot", 1)[1]
            code = re.search(r"```js\n(.*?)\n```", section, re.S).group(1)
            for token, number in {"WINDOW_ID": window_id, "PID": fixture_process.pid,
                "IMAGE_X": frame["x"], "IMAGE_Y": frame["y"],
                "IMAGE_W": frame["w"], "IMAGE_H": frame["h"]}.items():
                code = code.replace(token, str(number))
            candidates = completed(mcp.script(code))
            exact = [c for c in candidates["candidates"] if c["label"] == "Pixel target"]
            assert len(exact) == 1, {"native_actions": value["actions"], "recipe_result": candidates}
            return {"candidate_count": len(exact), "basis": exact[0]["basis"]}
        check("packaged_region_recipe_windows_candidate", region_recipe)

        def frame_alignment():
            receipt = mcp.script(bind + "const s=await a.getState({tree_format:'elements',max_elements:80,"
                "include_screenshot:true,max_image_dimension:1000});"
                "const q=(s.elements||[]).filter(e=>e.label==='Pixel target');"
                "if(q.length!==1||!s.capture_id||s.screenshot_error)throw new Error('target screenshot not observed');"
                "return {frame:q[0].screenshot_frame};")
            frame = completed(receipt)["frame"]
            mcp.image(receipt, "native-alignment.png")
            actual = painted_bounds(root / "native-alignment.png")
            declared = [frame[k] for k in ("x","y","w","h")]
            error = [round(a-b, 2) for a,b in zip(actual,declared)]
            report["screenshot_alignment"] = {"painted_bbox": actual, "uia_screenshot_frame": declared, "error_pixels": error}
            assert all(abs(delta) <= 3 for delta in error), report["screenshot_alignment"]
            return report["screenshot_alignment"]
        alignment_ok = check("native_screenshot_frame_alignment", frame_alignment)

        def focus():
            receipt = mcp.exec(session="windows-live", operation="focus",
                pid=fixture_process.pid, window_id=window_id)
            completed(receipt)
            native_focus = mcp.structured(receipt)
            assert focus_verified(native_focus, window_id), receipt
            return native_focus
        if not check("native_focus", focus):
            raise RuntimeError("Fixture focus could not be verified; no input attempted")

        text = "Windows 验收 🌍\n第二行 " + root.name

        def input_text():
            receipt = mcp.script(bind + "const q=(await a.query({role:'Edit',name:'Test input'})).filter(e=>e.element_token&&e.display_only!==true);"
                "if(q.length!==1)throw new Error('input is absent or ambiguous');"
                f"return await a.typeText({json.dumps(text)},q[0].element_token,{{delivery_mode:'foreground'}});")
            completed(receipt)
            events = native_events(root)
            assert events and any(e["kind"] == "input" and e["value"].replace("\r\n", "\n") == text for e in events), events[-8:]
            return {"unicode_and_multiline": True, "receipt": receipt["id"]}
        if not check("native_unicode_input", input_text):
            raise RuntimeError("Input outcome reconciled from fixture log; dependent writes stopped")

        def submit():
            receipt = mcp.script(bind + "const q=(await a.query({role:'Button',name:'Submit once'})).filter(e=>e.element_token&&e.display_only!==true);"
                "if(q.length!==1)throw new Error('submit is absent or ambiguous');"
                "return await a.click(q[0].element_token);")
            completed(receipt)
            events = [e for e in native_events(root) if e["kind"] == "submit"]
            assert len(events) == 1 and events[0]["value"].replace("\r\n", "\n") == text, events
            original = mcp.exec(session="windows-live", operation="result", id=receipt["id"])
            assert original["id"] == receipt["id"], original
            assert len([e for e in native_events(root) if e["kind"] == "submit"]) == 1
            return {"submissions": 1, "original_receipt_query_no_duplicate": True}
        check("native_semantic_submit", submit)

        def save():
            receipt = mcp.script(bind + "return await a.pressKey('ctrl+s',undefined,{delivery_mode:'foreground'});")
            completed(receipt)
            assert (root / "saved.txt").read_text(encoding="utf-8") == text
            return {"saved_sha256": hashlib.sha256((root / "saved.txt").read_bytes()).hexdigest()}
        check("native_ctrl_s_disk_readback", save)

        def pixel():
            receipt = mcp.script(bind + "const s=await a.getState({tree_format:'elements',max_elements:80,"
                "include_screenshot:true,max_image_dimension:1000});"
                "const q=(s.elements||[]).filter(e=>e.label==='Pixel target');"
                "if(q.length!==1||!s.capture_id||s.screenshot_error)throw new Error('pixel target not observed');"
                "return {capture_id:s.capture_id,frame:q[0].screenshot_frame,width:s.screenshot_width,height:s.screenshot_height};")
            value = completed(receipt)
            mcp.image(receipt, "native-pixel.png")
            frame = value["frame"]
            assert frame["w"] > 0 and frame["h"] > 0, frame
            point = [frame["x"] + frame["w"] / 2, frame["y"] + frame["h"] / 2]
            script = bind + f"return await a.click({json.dumps(point)},{{capture_id:{json.dumps(value['capture_id'])},delivery_mode:'foreground'}});"
            clicked = mcp.script(script)
            completed(clicked)
            assert len([e for e in native_events(root) if e["kind"] == "pixel"]) == 1
            reused = mcp.script(script)
            assert reused["status"] == "failed" and "capture_not_found" in json.dumps(reused), reused
            assert len([e for e in native_events(root) if e["kind"] == "pixel"]) == 1
            return {"point": point, "frame": frame, "one_use_capture": True}
        if alignment_ok:
            check("native_pixel_and_capture_reuse_refusal", pixel)
        else:
            report["checks"].append({"name":"native_pixel_and_capture_reuse_refusal","status":"SKIP",
                "reason":"Window screenshot pixels do not match native geometry; no pixel input attempted"})

        def checkbox():
            receipt = mcp.script(bind + "const q=(await a.query({role:'CheckBox',name:'Test checkbox'})).filter(e=>e.element_token&&e.display_only!==true);"
                "if(q.length!==1)throw new Error('checkbox is absent or ambiguous');"
                "return await a.click(q[0].element_token);")
            completed(receipt)
            events = [e for e in native_events(root) if e["kind"] == "check"]
            assert len(events) == 1 and events[0]["value"] == "True", events
            return events
        check("native_checkbox", checkbox)

        def stability():
            for index in range(args.rounds):
                marker = "稳定性 🌍 " + str(index)
                receipt = mcp.script(bind + "const q=(await a.query({role:'Edit',name:'Test input'})).filter(e=>e.element_token&&e.display_only!==true);"
                    "if(q.length!==1)throw new Error('input is absent or ambiguous');"
                    f"return await a.setValue(q[0].element_token,{json.dumps(marker)});")
                completed(receipt)
                readback = completed(mcp.script(bind + "return (await a.query({role:'Edit',name:'Test input'})).filter(e=>e.element_token&&e.display_only!==true);"))
                assert len(readback) == 1 and readback[0].get("value") == marker, readback
                # UIA ValuePattern may update the native EDIT without a managed
                # TextChanged callback. The fixture's own Ctrl+S handler reads
                # TextBox.Text and writes it independently of the UIA client.
                completed(mcp.script(bind + "return await a.pressKey('ctrl+s',undefined,{delivery_mode:'foreground'});"))
                assert (root / "saved.txt").read_text(encoding="utf-8") == marker
            return {"rounds": args.rounds, "independent_application_value_checks": args.rounds}
        check("native_repeated_set_value", stability)

        def menu():
            receipt = mcp.exec(session="windows-live", operation="menu",
                pid=fixture_process.pid, window_id=window_id, path=["File", "Record menu"])
            completed(receipt)
            observed = [e for e in native_events(root) if e["kind"] == "menu"]
            assert len(observed) == 1 and observed[0]["value"] == "MENU-OK", observed
            return {"application_callbacks": 1}
        check("native_menu_path", menu)

        def picker():
            if args.picker_mode == "pixel":
                assert alignment_ok, "No pixel picker action after screenshot geometry failure"
                observed = mcp.script(bind + "const s=await a.getState({tree_format:'elements',max_elements:80,include_screenshot:true});"
                    "const q=(s.elements||[]).filter(e=>e.label==='Open test picker');"
                    "if(q.length!==1||!s.capture_id||s.screenshot_error)throw new Error('picker frame not observed');"
                    "return {frame:q[0].screenshot_frame,capture_id:s.capture_id};")
                state = completed(observed)
                mcp.image(observed, "native-picker-before.png")
                frame = state["frame"]
                point = [frame["x"]+frame["w"]/2, frame["y"]+frame["h"]/2]
                receipt = mcp.script(bind + f"return await a.click({json.dumps(point)},{{capture_id:{json.dumps(state['capture_id'])},delivery_mode:'foreground'}});")
            else:
                receipt = mcp.script(bind + "const q=(await a.query({role:'Button',name:'Open test picker'})).filter(e=>e.element_token&&e.display_only!==true);"
                    "if(q.length!==1)throw new Error('picker button is absent or ambiguous');"
                    "return await a.click(q[0].element_token);")
            opening = receipt
            # A terminal input error can follow the real opening of a modal.
            # Reconcile its effect without replaying the opening action.
            assert opening["status"] in ("completed", "failed"), opening
            opened = [e for e in native_events(root) if e["kind"] == "picker_opened"]
            assert len(opened) == 1, {"receipt": opening, "application_events": opened}
            deadline = time.monotonic() + 5
            dialogs = []
            while time.monotonic() < deadline:
                windows = completed(mcp.script(f"return await cua.listWindows({{pid:{fixture_process.pid}}});"))
                dialogs = [window for window in windows if window.get("title") == "DTW test picker"]
                if dialogs:
                    break
                time.sleep(.1)
            assert len(dialogs) == 1, windows
            dialog_id = dialogs[0]["window_id"]
            dialog_bind = f"const a=await cua.getApp({{pid:{fixture_process.pid},windowId:{dialog_id}}});"
            receipt = mcp.script(dialog_bind + "const s=await a.getState({tree_format:'elements',max_elements:150,include_screenshot:true});return s;")
            state = completed(receipt)
            assert "picker.txt" in json.dumps(state), state
            mcp.image(receipt, "native-picker.png")
            focused = mcp.exec(session="windows-live", operation="focus", pid=fixture_process.pid, window_id=dialog_id)
            completed(focused)
            assert focus_verified(mcp.structured(focused), dialog_id), focused
            completed(mcp.script(dialog_bind + "return await a.pressKey('return',undefined,{delivery_mode:'foreground'});"))
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                observed = [e for e in native_events(root) if e["kind"] == "picker"]
                if observed:
                    break
                time.sleep(.05)
            assert len(observed) == 1 and observed[0]["value"] == "OK", observed
            report["picker_reconciliation"] = {"opening_status": opening["status"],
                "opening_id": opening["id"], "dialog_window_id": dialog_id,
                "application_result": "OK", "opening_action_replays": 0}
            assert opening["status"] == "completed", report["picker_reconciliation"]
            return {"dialog_window_id": dialog_id, "application_result": "OK", "preset_owned_file": "picker.txt"}
        if args.picker_mode == "skip":
            report["checks"].append({"name": "native_owned_file_picker", "status": "SKIP", "reason": "Not selected in this run"})
        else:
            check("native_owned_file_picker", picker)
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
        if fixture_process.poll() is None:
            fixture_process.terminate()
            fixture_process.wait(timeout=10)
        report["application_events"] = native_events(root)
        report["status"] = "FAIL" if failed else "PASS"
        (root / "summary.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        print(json.dumps({"record": str(root / "summary.json"), "status": report["status"]}), flush=True)
    if failed:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
