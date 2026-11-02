#!/usr/bin/env python3
"""Opt-in macOS lifecycle/Session acceptance using only disposable owned apps.

Uses the three advertised MCP tools; never calls the legacy result alias.
Run with an available test desktop. Writes independent fixture state and MCP
evidence; it does not install client configuration or exercise a live LLM host.
"""
import argparse
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import time
import uuid

from standard_agent_probe import Mcp, require, wait_until, write_json


def prepare(root, source, bundle_id, *, ui_element=False, compiled=None):
    bundle = root / "Lifecycle.app"
    executable = bundle / "Contents/MacOS/Lifecycle"
    executable.parent.mkdir(parents=True)
    metadata = {"CFBundleExecutable": executable.name, "CFBundleIdentifier": bundle_id,
                "CFBundleName": "DTW Lifecycle Fixture", "CFBundlePackageType": "APPL",
                "CFBundleVersion": "1", "LSUIElement": ui_element}
    with (bundle / "Contents/Info.plist").open("wb") as stream:
        plistlib.dump(metadata, stream)
    if compiled:
        shutil.copy2(compiled, executable)
    else:
        with (root / "build.log").open("w") as log:
            subprocess.run(["xcrun", "swiftc", "-module-cache-path", str(root / "swift-cache"),
                            str(source), "-o", str(executable)], stdout=log,
                           stderr=subprocess.STDOUT, check=True, timeout=90)
    return bundle, executable


def fixture_state(root):
    try:
        return json.loads((root / "state.json").read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def owned_cleanup(root, executable):
    """A last-resort signal requires this run's unique executable path to match."""
    state = fixture_state(root)
    if not state or not alive(state["pid"]):
        return {"forced": False}
    pid = state["pid"]
    command = subprocess.run(["ps", "-p", str(pid), "-o", "comm="],
                             text=True, capture_output=True, check=False).stdout.strip()
    require(command == str(executable), "cleanup refuses a PID whose executable changed")
    os.kill(pid, signal.SIGTERM)
    wait_until(lambda: not alive(pid), timeout=5)
    return {"forced": True, "pid": pid}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dtw", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).resolve().parents[1] / "fixtures/app-lifecycle/main.swift"
    key = uuid.uuid4().hex
    fixtures = []
    regular = prepare(output / "regular", source, f"dev.caelis.dtw.lifecycle.{key}")
    fixtures.append((output / "regular", regular[1]))
    agent_id = f"dev.caelis.dtw.agent.{key}"
    agent = prepare(output / "agent", source, agent_id, ui_element=True, compiled=regular[1])
    fixtures.append((output / "agent", agent[1]))
    helper_id = f"dev.caelis.dtw.helper.{key}"
    helper_root = output / "Outer.app/Contents/Frameworks"
    helper = prepare(helper_root, source, helper_id, ui_element=True, compiled=regular[1])
    fixtures.append((helper_root, helper[1]))
    report = {"cases": [], "cleanup": [], "tool_calls": 0}
    mcp = Mcp(args.dtw.resolve(), output)
    helper_child = None

    def execute(app, source, *, allow_error=False):
        arguments = {"source": source}
        if app is not None:
            arguments["app"] = str(app)
        started = time.monotonic()
        data, frame = mcp.tool("exec", arguments)
        report["tool_calls"] += 1
        if not allow_error:
            require(not frame.get("isError"), data)
        report["cases"].append({"execution_id": data.get("execution_id"),
            "seconds": time.monotonic()-started, "response_json_bytes": len(json.dumps(data).encode()),
            "status": data.get("status")})
        return data, frame

    def result(data):
        return data["value"]["result"]

    def receipt(execution_id):
        page, _ = execute(None, "return await session.result(" + json.dumps(execution_id) + ");")
        return result(page)

    try:
        mcp.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "dtw-lifecycle-session-owned-probe", "version": "1"}})
        mcp.notify("notifications/initialized")
        names = [tool["name"] for tool in mcp.rpc("tools/list", {})["tools"]]
        require(names == ["apps", "api", "exec"], names)
        execute(None, "print('session-ready');session.state.marker=73;")
        pure, _ = execute(None, "return {marker:session.state.marker,app:session.app};")
        require(result(pure) == {"marker": 73, "app": None}, pure)

        launch, image = execute(regular[0], """
await app.launch(); const w=await app.window({title:'DTW Lifecycle Main'});
state.window=w.window_ref; state.marker=41;
await w.press({selector:{role:'button',text:'Increment'}});
await w.wait({selector:{role:'static_text',text:'Count: 1'},state:'present'});
await w.capture(); print('count verified'); return {marker:session.state.marker};
""")
        state = wait_until(lambda: fixture_state(output / "regular"))
        require(state["clicks"] == 1 and result(launch)["marker"] == 41, (state, launch))
        require(any(item["type"] == "image" for item in image["content"]), image)
        pid = state["pid"]
        again, _ = execute(regular[0], "return await app.launch();")
        require(result(again)["outcome"]["dispatch"] == "not_dispatched", again)
        require(fixture_state(output / "regular")["pid"] == pid, "launch duplicated the app")

        close, _ = execute(regular[0], "const w=await app.window({title:'DTW Lifecycle Peer'});return await w.close();")
        require(result(close)["outcome"]["evidence"]["postcondition"] == "window_closed", close)
        require(alive(pid), "close() terminated the application")
        wait_until(lambda: not fixture_state(output / "regular")["windows"][1]["visible"])

        (output / "regular/command").write_text("block_quit")
        wait_until(lambda: fixture_state(output / "regular")["command"] == "block_quit")
        pending, _ = execute(regular[0], "return await app.quit();")
        require(result(pending)["outcome"]["dispatch"] == "dispatched", pending)
        require(result(pending)["outcome"]["evidence"]["kind"] == "unverified", pending)
        require(alive(pid) and fixture_state(output / "regular")["quit_requests"] == 1, pending)
        execute(regular[0], "const parent=await app.window({window_ref:state.window});return await parent.waitForWindow({state:'dialog'});")
        # The independent fixture controller cancels its synthetic save prompt.
        # This proves quit did not bypass it; it is not DTW dialog-input evidence.
        (output / "regular/command").write_text("cancel_quit")
        wait_until(lambda: fixture_state(output / "regular")["cancelled_quits"] == 1)
        execute(regular[0], "const parent=await app.window({window_ref:state.window});await parent.waitForWindow({state:'unblocked'});")
        report["blocked_quit"] = {"dispatch":"dispatched", "evidence":"unverified",
            "requests_before_new_work":1,"prompt_cancelled_by":"independent fixture controller"}
        exited, _ = execute(regular[0], "return await app.quit();")
        require(result(exited)["outcome"]["evidence"]["postcondition"] == "application_exited", exited)
        wait_until(lambda: not alive(pid))
        saved = receipt(launch["execution_id"])
        require(saved["execution_id"] == launch["execution_id"] and saved["value"]["result"] == {"marker": 41}, saved)
        relaunched, _ = execute(regular[0], "await app.launch();let stale=false;try{await app.capture({window_ref:state.window});}catch(e){stale=e.code==='target_changed';}return {marker:state.marker,stale};")
        require(result(relaunched) == {"marker": 41, "stale": True}, relaunched)
        execute(regular[0], "await app.quit();")

        # UIElement with a genuine window must be discoverable and operable.
        execute(agent[0], "await app.launch();const w=await app.window({title:'DTW Lifecycle Main'});await w.press({selector:{role:'button',text:'Increment'}});await w.wait({selector:{role:'static_text',text:'Count: 1'},state:'present'});")
        require(fixture_state(output / "agent")["clicks"] == 1, "UIElement input failed")
        helper_child = subprocess.Popen([str(helper[1])], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_until(lambda: fixture_state(helper_root))
        apps, _ = mcp.tool("apps", {})
        ids = [app["app"] for app in apps["apps"]]
        require(agent_id in ids and helper_id not in ids, "UIElement/helper discovery classification failed")
        report["discovery"] = {"ui_element_included": True, "nested_helper_excluded": True}
        execute(agent[0], "await app.quit();")

        # A running desktop VM does not block provider-free receipt recovery.
        old_pid = fixture_state(output / "agent")["pid"]
        rpc_id = mcp.begin("tools/call", {"name": "exec", "arguments": {
            "app": str(agent[0]), "source": "await app.launch();print('launch completed');while(true){}"}})
        wait_until(lambda: fixture_state(output / "agent") if fixture_state(output / "agent")["pid"] != old_pid else None)
        live, _ = execute(None, "return await session.result();")
        original = result(live)
        require(original["status"] == "running" and original["script"]["steps"][0]["operation"] == "launch", original)
        mcp.notify("notifications/cancelled", {"requestId": rpc_id, "reason": "owned fixture cancellation"})
        deadline = time.monotonic()+8
        while True:
            cancelled = receipt(original["execution_id"])
            if cancelled["status"] != "running":
                break
            require(time.monotonic() < deadline, "cancelled execution did not settle")
            time.sleep(.05)
        require(cancelled["status"] == "completed" and "cancel" in cancelled["error"]["code"], cancelled)
        require(cancelled["script"]["steps"][0]["receipt"]["outcome"]["dispatch"] == "dispatched", cancelled)
        execute(agent[0], "await app.quit();")
        report["passed"] = True
    finally:
        report["mcp_cleanup"] = mcp.close()
        if helper_child and helper_child.poll() is None:
            helper_child.terminate()
            helper_child.wait(timeout=5)
        for root, executable in fixtures:
            report["cleanup"].append(owned_cleanup(root, executable))
        write_json(output / "report.json", report)
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
