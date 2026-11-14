#!/usr/bin/env python3
"""Bounded Linux evidence probe. Real DISPLAY: metadata only; input only fresh Xvfb.

Run with --isolated --binary /absolute/path/to/candidate/dtw for actual backend
checks. No unlock, real-display fixture, real-display DTW, or root capture exists.
Owned process groups and scratch data are cleaned; PNG evidence is retained.
Requires Python 3,
wish, Xvfb, kwin_x11, dbus-run-session, xprop, xdpyinfo, and loginctl.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
import struct
import tempfile
import threading
import time
import uuid


def emit(kind, **values):
    print(json.dumps({"kind": kind, **values}, ensure_ascii=False), flush=True)


def command(args, env=None, timeout=10):
    p = subprocess.run(args, env=env, capture_output=True, text=True, timeout=timeout)
    return {"argv": args, "status": p.returncode, "stdout": p.stdout, "stderr": p.stderr}


def lock_state(session):
    result = command(["loginctl", "show-session", session, "-p", "Id", "-p", "Type", "-p", "Display", "-p", "LockedHint"])
    emit("real_session_lock", **result)
    return result["status"] == 0 and "LockedHint=yes" in result["stdout"].splitlines()


def lines(pipe):
    q = queue.Queue()
    def read():
        for line in iter(pipe.readline, ""):
            q.put(line)
        q.put(None)
    threading.Thread(target=read, daemon=True).start()
    return q


def next_line(q, timeout=15):
    value = q.get(timeout=timeout)
    if value is None:
        raise RuntimeError("child stdout closed")
    return value


class Children:
    def __init__(self):
        self.owned = []

    def start(self, args, env, **kwargs):
        p = subprocess.Popen(args, env=env, start_new_session=True, **kwargs)
        self.owned.append(p)
        return p

    def close(self):
        # Each group was created above, never selected by executable/name.
        for p in reversed(self.owned):
            for sig in (signal.SIGTERM, signal.SIGKILL):
                try:
                    os.killpg(p.pid, sig)
                except ProcessLookupError:
                    break
                try:
                    p.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    continue
                # Kill any owned helper descendants remaining in this group.
                if sig == signal.SIGTERM:
                    try:
                        os.killpg(p.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                break
            p.wait(timeout=3)
        emit("cleanup", owned_leaders=[p.pid for p in self.owned], statuses=[p.returncode for p in self.owned])


class MCP:
    def __init__(self, p, artifacts, toolset):
        self.p, self.serial, self.output = p, 0, lines(p.stdout)
        self.artifacts, self.toolset = artifacts, toolset
        self.last_images = []

    def send(self, obj):
        self.p.stdin.write(json.dumps(obj) + "\n")
        self.p.stdin.flush()

    def rpc(self, method, params):
        self.serial += 1
        self.send({"jsonrpc": "2.0", "id": self.serial, "method": method, "params": params})
        deadline = time.monotonic() + 35
        while time.monotonic() < deadline:
            row = json.loads(next_line(self.output, max(.1, deadline-time.monotonic())))
            if row.get("id") == self.serial:
                return row
        raise TimeoutError(method)

    def tool(self, name, args):
        reply = self.rpc("tools/call", {"name": name, "arguments": args})
        result = reply.get("result", {})
        self.last_images = []
        for index, item in enumerate(result.get("content", [])):
            if item.get("type") != "image":
                continue
            if item.get("mimeType") != "image/png":
                raise RuntimeError("expected PNG evidence")
            data = base64.b64decode(item["data"], validate=True)
            if data[:8] != b"\x89PNG\r\n\x1a\n":
                raise RuntimeError("invalid PNG signature")
            width, height = struct.unpack(">II", data[16:24])
            path = self.artifacts / f"{self.toolset}-{self.serial:02d}-{name}-{index}.png"
            path.write_bytes(data)
            self.last_images.append({"path": str(path), "sha256": hashlib.sha256(data).hexdigest(), "width": width, "height": height})
        emit("isolated_mcp", toolset=self.toolset, call_id=self.serial, tool=name, args=args, structured=result.get("structuredContent"), is_error=result.get("isError"), error=reply.get("error"), images=self.last_images)
        if reply.get("error") or result.get("isError") or not result:
            raise RuntimeError(f"MCP call {self.serial} failed; no input replay")
        return result


def fixture_state(path, toolset, stage):
    rows = path.read_text().splitlines()
    if len(rows) != 3:
        raise RuntimeError("incomplete owned fixture state")
    state = {"counter": int(rows[0]), "entry": rows[1], "ack": rows[2]}
    emit("isolated_fixture_state", toolset=toolset, stage=stage, **state)
    return state


def pixel_point(result, images, geometry, widget, toolset):
    if len(images) != 1:
        raise RuntimeError("capture must return exactly one PNG")
    image = images[0]
    view_image = result.get("structuredContent", {}).get("view", {}).get("image", {})
    if (view_image.get("width"), view_image.get("height")) != (image["width"], image["height"]):
        raise RuntimeError("PNG and coordinate canvas dimensions differ")
    if view_image.get("bounds") != [geometry["x"], geometry["y"], geometry["width"], geometry["height"]]:
        raise RuntimeError("capture bounds do not match fixture client geometry")
    x, y, width, height = geometry[widget]
    point = {"x": (x + width / 2) * image["width"] / geometry["width"],
             "y": (y + height / 2) * image["height"] / geometry["height"]}
    emit("isolated_pixel_mapping", toolset=toolset, widget=widget, geometry=geometry, image=image, point=point)
    return point


def fixture_round(binary, root, env, children, log, toolset, artifacts):
    fixture_class = "DtwLockedProbe" + uuid.uuid4().hex
    executable = root / fixture_class
    shutil.copy2(shutil.which("wish"), executable)
    # wish derives its root WM_CLASS from the script basename.
    tcl = root / (fixture_class + ".tcl")
    state_path = root / (toolset + "-state.txt")
    tcl.write_text("set state_path " + json.dumps(str(state_path)) + "\n" + r'''wm title . "DTW owned Linux fixture"
wm geometry . 460x240+50+50
set counter 0
set ack ""
proc save_state {} {
    global state_path counter ack
    set f [open "$state_path.tmp" w]
    puts $f $counter
    puts $f [.entry get]
    puts $f $ack
    close $f
    file rename -force "$state_path.tmp" $state_path
}
label .label -text "Owned DTW fixture: isolated Xvfb only"
entry .entry -width 40
button .button -text "Increment once" -command {
    incr counter
    .count configure -text "Count=$counter"
    save_state
    puts "BUTTON=$counter"; flush stdout
}
label .count -text "Count=0"
label .ack -text "ACK pending"
pack .label .entry .button .count .ack -padx 12 -pady 6
bind .entry <Return> {
    set ack [.entry get]
    .ack configure -text "ACK=$ack"
    save_state
    puts "ACK=$ack"; flush stdout
}
after 500 {
    save_state
    puts "READY=[pid],[winfo class .],[wm frame .]"
    puts [format {GEOMETRY={"x":%d,"y":%d,"width":%d,"height":%d,"button":[%d,%d,%d,%d],"entry":[%d,%d,%d,%d]}} [winfo rootx .] [winfo rooty .] [winfo width .] [winfo height .] [winfo x .button] [winfo y .button] [winfo width .button] [winfo height .button] [winfo x .entry] [winfo y .entry] [winfo width .entry] [winfo height .entry]]
    flush stdout
}
''')
    fixture_env = dict(env)
    # Supply the matching distribution libraries explicitly for copied wish.
    # Startup failures remain errors; no alternate launch/input is retried.
    wish_name = Path(shutil.which("wish")).resolve().name
    version = wish_name.removeprefix("wish")
    for variable, prefix, marker in (("TCL_LIBRARY", "tcl", "init.tcl"), ("TK_LIBRARY", "tk", "tk.tcl")):
        library = Path("/usr/share") / (prefix + version)
        if (library / marker).is_file():
            fixture_env[variable] = str(library)
    emit("isolated_fixture_libraries", toolset=toolset, tcl=fixture_env.get("TCL_LIBRARY"), tk=fixture_env.get("TK_LIBRARY"))
    fixture = children.start([str(executable), str(tcl)], fixture_env, stdout=subprocess.PIPE, stderr=log(toolset + "-fixture-errors.log"), text=True)
    server = None
    try:
        fixture_output = lines(fixture.stdout)
        ready = next_line(fixture_output, 10).strip()
        if not ready.startswith("READY=" + str(fixture.pid) + "," + (fixture_class + ".tcl").capitalize() + ","):
            raise RuntimeError("fixture identity mismatch: " + ready)
        emit("isolated_fixture", toolset=toolset, pid=fixture.pid, executable=str(executable), identity=ready)
        geometry_line = next_line(fixture_output, 5).strip()
        if not geometry_line.startswith("GEOMETRY="):
            raise RuntimeError("missing fixture geometry")
        geometry = json.loads(geometry_line.removeprefix("GEOMETRY="))
        server = children.start([str(binary), "mcp", "--toolset", toolset], env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log(toolset + "-mcp-errors.log"), text=True, bufsize=1)
        client = MCP(server, artifacts, toolset)
        emit("isolated_initialize", toolset=toolset, reply=client.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "linux-locked-probe", "version": "1"}}))
        client.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        emit("isolated_tools", toolset=toolset, reply=client.rpc("tools/list", {}))
        apps = client.tool("apps", {"query": fixture_class, "state": "running"}).get("structuredContent", {}).get("apps", [])
        if len(apps) != 1 or apps[0].get("app") != str(executable):
            raise RuntimeError("scoped fixture discovery failed: " + repr(apps))
        app = apps[0]["app"]
        token = "dtw_probe_" + toolset + "_731"

        def call(method, args=None):
            args = args or {}
            if toolset == "script":
                public = {"type_text": "typeText"}.get(method, method)
                value = args["text"] if method == "type_text" else "ENTER" if method == "keypress" else args
                source = "return await app." + public + "(" + (json.dumps(value) if args else "") + ");"
                return client.tool("exec", {"app": app, "source": source})
            return client.tool(method, {"app": app, **args})

        initial = fixture_state(state_path, toolset, "initial")
        if initial != {"counter": 0, "entry": "", "ack": ""}:
            raise RuntimeError("fixture did not start empty")
        call("look")
        before = call("capture")
        before_hash = client.last_images[0]["sha256"]
        point = pixel_point(before, client.last_images, geometry, "button", toolset)
        call("click", point)
        button = next_line(fixture_output, 5).strip()
        if button != "BUTTON=1":
            raise RuntimeError("expected exactly one button increment: " + button)
        call("look")
        call("capture")
        if client.last_images[0]["sha256"] == before_hash:
            raise RuntimeError("button effect did not change captured image")
        after_button = fixture_state(state_path, toolset, "after_button")
        if after_button != {"counter": 1, "entry": "", "ack": ""}:
            raise RuntimeError("button readback mismatch")

        entry_capture = call("capture")
        entry_hash = client.last_images[0]["sha256"]
        point = pixel_point(entry_capture, client.last_images, geometry, "entry", toolset)
        call("click", point)
        call("type_text", {"text": token})
        call("keypress", {"keys": ["ENTER"]})
        ack = next_line(fixture_output, 5).strip()
        if ack != "ACK=" + token:
            raise RuntimeError("owned fixture acknowledgement mismatch: " + ack)
        call("look")
        call("capture")
        changed = client.last_images[0]["sha256"] != entry_hash
        final = fixture_state(state_path, toolset, "final")
        verified = final == {"counter": 1, "entry": token, "ack": token} and changed
        emit("isolated_effect", toolset=toolset, verified=verified, acknowledgement=ack, button_event=button, image_changed=changed, no_replay=True)
        if not verified:
            raise RuntimeError("fixture final state/image mismatch")
        server.stdin.close()
        server.wait(timeout=10)
        emit("isolated_mcp_exit", toolset=toolset, status=server.returncode)
        if server.returncode != 0:
            raise RuntimeError("isolated MCP server exited with status " + str(server.returncode))
    finally:
        if state_path.is_file():
            # Preserve independent state even when a call fails; never retry input.
            try:
                final_state = fixture_state(state_path, toolset, "round_end")
                (artifacts / (toolset + "-fixture-state.json")).write_text(json.dumps(final_state) + "\n")
            except Exception as exc:
                emit("isolated_fixture_readback_error", toolset=toolset, message=str(exc))
        # End this fixture before the independent second round. Group cleanup
        # below also covers timed-out MCP workers and private WM descendants.
        fixture.terminate()
        fixture.wait(timeout=3)
        if server is not None and server.poll() is None:
            server.terminate()
            server.wait(timeout=3)


def isolated(binary, toolsets, artifacts):
    if not binary.is_file():
        raise RuntimeError("candidate binary missing: " + str(binary))
    children = Children()
    with tempfile.TemporaryDirectory(prefix="dtw-locked-probe-") as temp:
        root = Path(temp)
        # Private configuration, runtime directory and D-Bus prevent KWin from
        # sharing the user's desktop session. Xvfb chooses a free display.
        env = dict(os.environ)
        for key in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "SESSION_MANAGER", "XAUTHORITY"):
            env.pop(key, None)
        env.update(XDG_SESSION_TYPE="x11", XDG_CONFIG_HOME=str(root / "config"), XDG_CACHE_HOME=str(root / "cache"), XDG_DATA_HOME=str(root / "data"), XDG_RUNTIME_DIR=str(root / "runtime"), QT_QPA_PLATFORM="xcb", KWIN_COMPOSE="N")
        (root / "runtime").mkdir(mode=0o700)
        logs = []
        def log(name):
            f = open(root / name, "w+")
            logs.append((name, f))
            return f
        try:
            read_fd, write_fd = os.pipe()
            try:
                children.start(["Xvfb", "-displayfd", str(write_fd), "-screen", "0", "1024x768x24", "-nolisten", "tcp"], env, pass_fds=(write_fd,), stdout=log("xvfb.log"), stderr=subprocess.STDOUT)
            finally:
                os.close(write_fd)
            with os.fdopen(read_fd) as pipe:
                display_number = next_line(lines(pipe), 10).strip()
            if not display_number.isdecimal():
                raise RuntimeError("invalid isolated display")
            env["DISPLAY"] = ":" + display_number
            if env["DISPLAY"] == os.environ.get("DISPLAY", ":0") or env["DISPLAY"] == ":0":
                raise RuntimeError("refusing real display")
            emit("isolated_display", display=env["DISPLAY"], xauthority="fresh server default; TCP disabled", backend="actual X11, not mock")
            children.start(["dbus-run-session", "--", "kwin_x11"], env, stdout=log("kwin.log"), stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 12
            while True:
                wm = command(["xprop", "-root", "_NET_SUPPORTING_WM_CHECK"], env)
                if wm["status"] == 0 and "window id #" in wm["stdout"]:
                    emit("isolated_wm", **wm)
                    break
                if time.monotonic() >= deadline:
                    raise RuntimeError("isolated KWin EWMH startup timeout: " + wm["stdout"])
                time.sleep(.15)
            emit("binary_version", **command([str(binary), "--version"], env))
            emit("binary_sha256", sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
            failures = []
            for toolset in toolsets:
                try:
                    fixture_round(binary, root, env, children, log, toolset, artifacts)
                except Exception as exc:
                    failures.append(toolset)
                    emit("isolated_round_error", toolset=toolset, type=type(exc).__name__, message=str(exc), replayed=False)
            if failures:
                raise RuntimeError("isolated rounds failed: " + ", ".join(failures))
        finally:
            children.close()
            for name, f in logs:
                f.flush()
                f.seek(0)
                text = f.read()
                (artifacts / name).write_text(text)
                emit("isolated_child_log", name=name, text=text[:16000])
                f.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--session", default=os.environ.get("XDG_SESSION_ID", "7"))
    parser.add_argument("--isolated", action="store_true")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--artifacts", type=Path, help="new directory for retained PNGs and child logs")
    parser.add_argument("--toolset", choices=("script", "atomic", "both"), default="both")
    args = parser.parse_args()
    emit("environment", display=os.environ.get("DISPLAY"), session_type=os.environ.get("XDG_SESSION_TYPE"), session_id=args.session, timestamp=time.strftime("%Y-%m-%dT%H:%M:%S%z"))
    before = lock_state(args.session)
    status = 0
    try:
        if not before:
            raise RuntimeError("real session is not proven locked; stopping")
        result = command(["xdpyinfo", "-display", ":0", "-queryExtensions"])
        # Server/extension metadata only; no client properties/titles/pixels.
        result["stdout"] = "\n".join(line for line in result["stdout"].splitlines() if any(s in line for s in ("name of display", "version number", "vendor string", "vendor release", "XTEST", "X-Resource", "XInputExtension", "dimensions:")))
        emit("real_display_readonly_extensions", **result)
        emit("real_display_boundary", fixture_mapped=False, discovery=False, capture=False, input=False, reason="ordinary mapping safety under this locker not proven; no locked-session app operation claimed")
        if args.isolated:
            if args.binary is None:
                raise RuntimeError("--isolated requires --binary")
            artifacts = args.artifacts.resolve() if args.artifacts else Path(tempfile.mkdtemp(prefix="dtw-direct-evidence-"))
            if args.artifacts:
                artifacts.mkdir(parents=True, exist_ok=False)
            emit("evidence_directory", path=str(artifacts))
            isolated(args.binary.resolve(), ("script", "atomic") if args.toolset == "both" else (args.toolset,), artifacts)
    except Exception as exc:
        status = 1
        emit("error", type=type(exc).__name__, message=str(exc))
    finally:
        after = lock_state(args.session)
        emit("lock_check", before=before, after=after)
        if not after:
            status = 1
    return status


if __name__ == "__main__":
    raise SystemExit(main())
