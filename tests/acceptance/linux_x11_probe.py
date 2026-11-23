#!/usr/bin/env python3
"""Exercise one owned Tk window through the shipped Linux apps/exec path."""
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


def lines(pipe, transcript=None):
    q = queue.Queue()
    def read():
        for line in iter(pipe.readline, ""):
            if transcript is not None:
                with transcript.open("a") as output:
                    output.write(line)
            q.put(line)
        q.put(None)
    threading.Thread(target=read, daemon=True).start()
    return q


def next_line(q, timeout=15):
    value = q.get(timeout=timeout)
    if value is None:
        raise RuntimeError("child stdout closed")
    return value


def start_xvfb(children, env, log):
    # A headless runner can have :0 free. Pick a nonzero display and wait
    # for this exact server to confirm it owns the socket before using it.
    display = str(100 + uuid.uuid4().int % 10000)
    read_fd, write_fd = os.pipe()
    try:
        with os.fdopen(read_fd) as pipe:
            children.start(["Xvfb", ":" + display, "-displayfd", str(write_fd),
                            "-screen", "0", "1024x768x24", "-nolisten", "tcp", "-noreset", "-nopn"],
                           env, pass_fds=(write_fd,), stdout=log, stderr=subprocess.STDOUT)
            if next_line(lines(pipe), 10).strip() != display:
                raise RuntimeError("private Xvfb did not confirm its display")
    finally:
        os.close(write_fd)
    return ":" + display


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
    def __init__(self, p, artifacts, mode):
        self.p, self.serial, self.output = p, 0, lines(p.stdout, artifacts / (mode + "-mcp-stdout.jsonl"))
        self.artifacts, self.mode = artifacts, mode
        self.last_images = []

    def send(self, obj):
        self.p.stdin.write(json.dumps(obj) + "\n")
        self.p.stdin.flush()

    def rpc(self, method, params):
        self.serial += 1
        request = {"jsonrpc": "2.0", "id": self.serial, "method": method, "params": params}
        (self.artifacts / f"{self.mode}-{self.serial:02d}-request.json").write_text(json.dumps(request) + "\n")
        self.send(request)
        deadline = time.monotonic() + 35
        while time.monotonic() < deadline:
            row = json.loads(next_line(self.output, max(.1, deadline-time.monotonic())))
            if row.get("id") == self.serial:
                return row
        raise TimeoutError(method)

    def tool(self, name, args, *, error_code=None):
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
            path = self.artifacts / f"{self.mode}-{self.serial:02d}-{name}-{index}.png"
            path.write_bytes(data)
            self.last_images.append({"path": str(path), "sha256": hashlib.sha256(data).hexdigest(), "width": width, "height": height})
        emit("mcp", mode=self.mode, call_id=self.serial, tool=name, args=args, structured=result.get("structuredContent"), is_error=result.get("isError"), error=reply.get("error"), images=self.last_images)
        if error_code is not None:
            actual = result.get("structuredContent", {}).get("error", {}).get("code")
            if not result.get("isError") or actual != error_code:
                raise RuntimeError(f"expected {error_code}, got {actual or reply.get('error')}")
        elif reply.get("error") or result.get("isError") or not result:
            raise RuntimeError(f"MCP call {self.serial} failed; no input replay")
        return result


def fraction(geometry, x, y):
    return {"x": x / geometry["width"], "y": y / geometry["height"], "unit": "fraction"}


def point(geometry, name):
    x, y, width, height = geometry[name]
    return fraction(geometry, x + width / 2, y + height / 2)


def fixture_round(binary, root, env, children, log, mode, artifacts):
    marker = "DtwX11Probe" + uuid.uuid4().hex
    executable = root / marker
    wish = Path(shutil.which("wish")).resolve()
    shutil.copy2(wish, executable)
    state = artifacts / "fixture-state.txt"
    tcl = root / "fixture.tcl"
    tcl.write_text("set state " + json.dumps(str(state)) + "\n" + r'''wm title . "DTW owned X11 fixture"
wm geometry . 460x300+80+80
wm attributes . -topmost 1
set count 0
set ack ""
set hover 0
set wheel 0
set drag 0
set held 0
proc save {} {
    global state count ack hover wheel drag held
    set f [open "$state.tmp" w]
    foreach value [list $count [.entry get] $ack $hover $wheel $drag $held] { puts $f $value }
    close $f
    file rename -force "$state.tmp" $state
}
entry .entry -width 38
button .button -text "Increment" -command {
    .button configure -text "Count=[incr count]"
    save
}
canvas .pad -width 340 -height 130 -background #274f76 -highlightthickness 0
.pad create rectangle 18 18 145 104 -fill #537da5 -tags start
.pad create rectangle 195 18 320 104 -fill #658f4c -tags finish
pack .entry .button .pad -padx 10 -pady 8
bind .entry <Return> { set ack [.entry get]; . configure -background #adc4a0; save }
bind .pad <Motion> { set hover 1; save }
if {[package vcompare [package present Tk] 9.0] >= 0} {
    bind .pad <MouseWheel> { incr wheel; save }
} else {
    bind .pad <Button-4> { incr wheel; save }
    bind .pad <Button-5> { incr wheel; save }
}
bind .pad <ButtonPress-1> { set held 1; save }
bind .pad <ButtonRelease-1> {
    if {$held && %x > 195 && %x < 320} { incr drag; .pad configure -background #375b46 }
    set held 0
    save
}
after 500 {
    save
    puts "READY=[pid]"
    puts [format {GEOMETRY={"x":%d,"y":%d,"width":%d,"height":%d,"button":[%d,%d,%d,%d],"entry":[%d,%d,%d,%d],"pad":[%d,%d,%d,%d]}} [winfo rootx .] [winfo rooty .] [winfo width .] [winfo height .] [winfo x .button] [winfo y .button] [winfo width .button] [winfo height .button] [winfo x .entry] [winfo y .entry] [winfo width .entry] [winfo height .entry] [winfo x .pad] [winfo y .pad] [winfo width .pad] [winfo height .pad]]
    flush stdout
}
''')
    fixture_env = dict(env)
    version = wish.name.removeprefix("wish")
    for variable, prefix, marker_file in (("TCL_LIBRARY", "tcl", "init.tcl"), ("TK_LIBRARY", "tk", "tk.tcl")):
        library = Path("/usr/share") / (prefix + version)
        if (library / marker_file).is_file():
            fixture_env[variable] = str(library)
    fixture = children.start([str(executable), str(tcl)], fixture_env, stdout=subprocess.PIPE,
                             stderr=log("fixture-stderr.log"), text=True)
    output = lines(fixture.stdout, artifacts / "fixture-stdout.log")
    if next_line(output, 10).strip() != f"READY={fixture.pid}":
        raise RuntimeError("fixture identity mismatch")
    geometry = json.loads(next_line(output, 5).removeprefix("GEOMETRY="))
    emit("fixture", pid=fixture.pid, app=str(executable), geometry=geometry)

    def readback(expected):
        deadline = time.monotonic() + 2
        while True:
            actual = state.read_text().splitlines()
            if actual == expected:
                emit("fixture_effect", state=actual)
                return
            if time.monotonic() >= deadline:
                raise RuntimeError(f"fixture expected {expected}, got {actual}")
            time.sleep(.05)

    client = connect(binary, env, children, log, artifacts, mode)
    apps = client.tool("apps", {"query": marker, "state": "running"})["structuredContent"]
    if [app["app"] for app in apps["apps"] if app["running"]] != [str(executable)] or len(apps["apps"]) != 1:
        raise RuntimeError(f"expected one matching running app, got {apps}")

    def execute(source, **kwargs):
        return client.tool("exec", {"app": str(executable), "source": source}, **kwargs)["structuredContent"]

    def operation(method, args=None):
        return execute(f"await app.{method}({json.dumps(args or {})});")

    def capture():
        result = operation("capture", {"full": True})
        image = result["view"]["image"]
        expected = [geometry[k] for k in ("x", "y", "width", "height")]
        if image["bounds"] != expected or len(client.last_images) != 1:
            raise RuntimeError(f"fixture/image geometry mismatch: {image}, {expected}")
        png = client.last_images[0]
        if (png["width"], png["height"]) != (image["width"], image["height"]):
            raise RuntimeError(f"PNG and coordinate canvas size mismatch: {png}, {image}")
        return png["sha256"]

    readback(["0", "", "", "0", "0", "0", "0"])
    looked = operation("look", {"full": True})["view"]
    if ([w["title"] for w in looked["windows"]] != ["DTW owned X11 fixture"]
            or looked["elements"] or looked["semantics"].get("reason") != "unsupported_capability"):
        raise RuntimeError(f"unexpected X11 metadata: {looked}")
    initial = capture()
    execute("await app.move({x:-1,y:0});", error_code="invalid_arguments")
    execute("await app.move({x:99999,y:0});", error_code="invalid_arguments")
    readback(["0", "", "", "0", "0", "0", "0"])
    operation("focus")
    operation("click", point(geometry, "button"))
    readback(["1", "", "", "0", "0", "0", "0"])
    operation("click", point(geometry, "entry"))
    token = "DTW_X11_731"
    operation("typeText", {"text": token})
    operation("keypress", {"keys": ["ENTER"]})
    readback(["1", token, token, "0", "0", "0", "0"])
    x, y, _, _ = geometry["pad"]
    path = [fraction(geometry, x + offset, y + 60) for offset in (70, 160, 260)]
    operation("move", path[0])
    readback(["1", token, token, "1", "0", "0", "0"])
    operation("drag", {"path": [{"x": p["x"], "y": p["y"]} for p in path], "unit": "fraction"})
    readback(["1", token, token, "1", "0", "1", "0"])
    operation("scroll", {**path[0], "delta_y": 1})
    operation("scroll", {"delta_y": 1})
    readback(["1", token, token, "1", "2", "1", "0"])
    final = capture()
    if initial == final:
        raise RuntimeError("fixture changed but capture did not")
    finish(client)
    emit("effect", initial_png=initial, final_png=final, verified=True)


def connect(binary, env, children, log, artifacts, label):
    server = children.start([str(binary), "mcp"], env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=log(label + "-stderr.log"), text=True, bufsize=1)
    client = MCP(server, artifacts, label)
    reply = client.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                    "clientInfo": {"name": "linux-x11-acceptance", "version": "1"}})
    if reply.get("result", {}).get("serverInfo", {}).get("name") != "dtw":
        raise RuntimeError(f"unexpected initialize reply: {reply}")
    emit("initialize", **reply)
    client.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    listing = client.rpc("tools/list", {})
    actual = sorted(tool["name"] for tool in listing.get("result", {}).get("tools", []))
    if actual != ["apps", "exec"]:
        raise RuntimeError(f"unexpected public tools: {actual}")
    emit("tools", names=actual)
    return client


def finish(client):
    client.p.stdin.close()
    if client.p.wait(timeout=10) != 0:
        raise RuntimeError(f"MCP server exited with {client.p.returncode}")
    emit("mcp_exit", label=client.mode, status=client.p.returncode)


def isolated(binary, artifacts):
    if not binary.is_file():
        raise RuntimeError("candidate binary missing: " + str(binary))
    children = Children()
    with tempfile.TemporaryDirectory(prefix="dtw-locked-probe-") as temp:
        root = Path(temp)
        # Private configuration, runtime directory and D-Bus prevent KWin from
        # sharing the user's desktop session.
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
            env["DISPLAY"] = start_xvfb(children, env, log("xvfb.log"))
            if env["DISPLAY"] == os.environ.get("DISPLAY"):
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
            fixture_round(binary, root, env, children, log, "xvfb", artifacts)
        finally:
            children.close()
            for name, f in logs:
                f.flush()
                f.seek(0)
                text = f.read()
                (artifacts / name).write_text(text)
                emit("isolated_child_log", name=name, text=text[:16000])
                f.close()


def real_session(binary, session, artifacts):
    env = dict(os.environ)
    session_info = command(["loginctl", "show-session", session, "-p", "Display", "-p", "Type",
                            "-p", "Active", "-p", "LockedHint"])
    required = {"Type=x11", "Active=yes", "LockedHint=no", f"Display={env.get('DISPLAY')}"}
    if session_info["status"] != 0 or not required.issubset(session_info["stdout"].splitlines()):
        raise RuntimeError(f"expected an active unlocked X11 desktop matching DISPLAY: {session_info}")
    emit("real_session", **session_info)
    children = Children()
    with tempfile.TemporaryDirectory(prefix="dtw-real-x11-") as temp:
        logs = []
        def log(name):
            file = (artifacts / name).open("w")
            logs.append(file)
            return file
        try:
            fixture_round(binary, Path(temp), env, children, log, "real", artifacts)
        finally:
            children.close()
            for file in logs:
                file.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    display = parser.add_mutually_exclusive_group(required=True)
    display.add_argument("--real-session", metavar="ID", help="run against this unlocked logind X11 session")
    display.add_argument("--isolated", action="store_true", help="start a private Xvfb and KWin")
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True, help="new private evidence directory")
    parser.add_argument("--sha256", required=True, help="expected frozen executable SHA-256")
    args = parser.parse_args()
    binary = args.binary.resolve()
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    if digest != args.sha256.lower():
        parser.error("frozen executable SHA-256 mismatch")
    artifacts = args.artifacts.resolve()
    artifacts.mkdir(mode=0o700, parents=True, exist_ok=False)
    emit("environment", executable=str(binary), sha256=digest, machine=os.uname().machine,
         display=os.environ.get("DISPLAY"), session_type=os.environ.get("XDG_SESSION_TYPE"))
    try:
        if args.real_session:
            real_session(binary, args.real_session, artifacts)
        else:
            isolated(binary, artifacts)
    except Exception as exc:
        emit("error", type=type(exc).__name__, message=str(exc))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
