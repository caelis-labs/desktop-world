#!/usr/bin/env python3
"""Bounded Linux evidence probe. Real DISPLAY: metadata only; input only fresh Xvfb.

Run with --isolated --binary /absolute/path/to/candidate/dtw for actual backend
checks. No unlock, real-display fixture, real-display DTW, or root capture exists.
All owned process groups and scratch data are cleaned on exit. Requires Python 3,
wish, Xvfb, kwin_x11, dbus-run-session, xprop, xdpyinfo, and loginctl.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
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
    def __init__(self, p):
        self.p, self.serial, self.output = p, 0, lines(p.stdout)

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
        # Keep structured receipts and image hashes, never huge image payloads.
        result = reply.get("result", {})
        images = [{"mime": item.get("mimeType"), "base64_sha256": hashlib.sha256(item.get("data", "").encode()).hexdigest()} for item in result.get("content", []) if item.get("type") == "image"]
        emit("isolated_mcp", tool=name, args=args, structured=result.get("structuredContent"), is_error=result.get("isError"), error=reply.get("error"), images=images)
        return result


def fixture_round(binary, root, env, children, log, toolset):
    fixture_class = "DtwLockedProbe" + uuid.uuid4().hex
    executable = root / fixture_class
    shutil.copy2(shutil.which("wish"), executable)
    # wish derives its root WM_CLASS from the script basename.
    tcl = root / (fixture_class + ".tcl")
    tcl.write_text('''wm title . "DTW owned Linux fixture"
wm geometry . 460x180+50+50
label .label -text "Owned DTW fixture: isolated Xvfb only"
entry .entry -width 40
button .button -text "Acknowledge" -command {puts "BUTTON"; flush stdout}
pack .label .entry .button -padx 12 -pady 10
focus .entry
bind .entry <Return> {puts "ACK=[.entry get]"; flush stdout}
after 500 {puts "READY=[pid],[winfo class .],[wm frame .]"; flush stdout}
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
        server = children.start([str(binary), "mcp", "--toolset", toolset], env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log(toolset + "-mcp-errors.log"), text=True, bufsize=1)
        client = MCP(server)
        emit("isolated_initialize", toolset=toolset, reply=client.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "linux-locked-probe", "version": "1"}}))
        client.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        emit("isolated_tools", toolset=toolset, reply=client.rpc("tools/list", {}))
        apps = client.tool("apps", {"query": fixture_class, "state": "running"}).get("structuredContent", {}).get("apps", [])
        if len(apps) != 1 or apps[0].get("app") != str(executable):
            raise RuntimeError("scoped fixture discovery failed: " + repr(apps))
        app = apps[0]["app"]
        token = "dtw_probe_" + toolset + "_731"
        if toolset == "script":
            client.tool("exec", {"app": app, "source": "return await app.look();"})
            capture = client.tool("exec", {"app": app, "source": "return await app.capture();"})
        else:
            client.tool("look", {"app": app})
            capture = client.tool("capture", {"app": app})
        if capture.get("isError") or not capture:
            raise RuntimeError("isolated capture failed; see receipt")
        if not any(item.get("type") == "image" and item.get("mimeType") == "image/png" for item in capture.get("content", [])):
            raise RuntimeError("isolated capture returned no image/png content block")
        if toolset == "script":
            receipt = client.tool("exec", {"app": app, "source": "await app.typeText(" + json.dumps(token) + "); await app.keypress('ENTER'); return 'input_complete';"})
        else:
            receipt = client.tool("type_text", {"app": app, "text": token})
            if not receipt.get("isError") and receipt:
                receipt = client.tool("keypress", {"app": app, "keys": ["ENTER"]})
        if receipt.get("isError") or not receipt:
            emit("isolated_effect", toolset=toolset, verified=False, reason="input receipt failed; no replay")
            raise RuntimeError("isolated input failed; see receipt")
        ack = next_line(fixture_output, 5).strip()
        emit("isolated_effect", toolset=toolset, verified=ack == "ACK=" + token, acknowledgement=ack)
        if ack != "ACK=" + token:
            raise RuntimeError("owned fixture acknowledgement mismatch")
        server.stdin.close()
        server.wait(timeout=10)
        emit("isolated_mcp_exit", toolset=toolset, status=server.returncode)
        if server.returncode != 0:
            raise RuntimeError("isolated MCP server exited with status " + str(server.returncode))
    finally:
        # End this fixture before the independent second round. Group cleanup
        # below also covers timed-out MCP workers and private WM descendants.
        fixture.terminate()
        fixture.wait(timeout=3)
        if server is not None and server.poll() is None:
            server.terminate()
            server.wait(timeout=3)


def isolated(binary, toolsets):
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
                    fixture_round(binary, root, env, children, log, toolset)
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
                emit("isolated_child_log", name=name, text=f.read(16000))
                f.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--session", default=os.environ.get("XDG_SESSION_ID", "7"))
    parser.add_argument("--isolated", action="store_true")
    parser.add_argument("--binary", type=Path)
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
            isolated(args.binary.resolve(), ("script", "atomic") if args.toolset == "both" else (args.toolset,))
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
