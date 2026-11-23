#!/usr/bin/env python3
"""Real GTK 3 AT-SPI and MCP test in an owned X11 display and D-Bus session."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

import linux_x11_probe as x11


def fixture(state_file):
    import gi
    gi.require_version("Gtk", "3.0")
    from gi.repository import GLib, Gtk

    title = "DTW owned AT-SPI fixture"
    window = Gtk.Window(title=title, default_width=470, default_height=280)
    layout = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
    entry = Gtk.Entry()
    entry.set_text("A😀BC")
    entry.get_accessible().set_name("Fixture entry")
    password = Gtk.Entry(visibility=False)
    password.set_text("owned-test-password")
    password.get_accessible().set_name("Fixture password")
    increment = Gtk.Button(label="Increment fixture")
    other_window = Gtk.Button(label="Open another fixture window")
    state = {"clicks": 0, "multi_window": False}
    second = None

    def save(*_):
        temp = state_file.with_suffix(".tmp")
        temp.write_text(json.dumps({**state, "value": entry.get_text()}))
        temp.replace(state_file)

    def click(*_):
        state["clicks"] += 1
        save()

    def open_window(*_):
        nonlocal second
        second = Gtk.Window(title="DTW owned AT-SPI peer", default_width=180, default_height=100)
        second.move(20, 40)
        second.show_all()
        state["multi_window"] = True
        save()

    for widget in (entry, password, increment, other_window):
        layout.pack_start(widget, False, False, 0)
    window.add(layout)
    entry.connect("changed", save)
    increment.connect("clicked", click)
    other_window.connect("clicked", open_window)
    window.show_all()
    save()
    GLib.MainLoop().run()


def poll(read, predicate, description):
    until = time.monotonic() + 8
    while time.monotonic() < until:
        value = read()
        if predicate(value):
            return value
        time.sleep(.1)
    raise RuntimeError(f"Timed out waiting for {description}")


def dbus(env, address, destination, path, method, *arguments):
    connection = ["--address", address] if address else ["--session"]
    return x11.command(["gdbus", "call", *connection, "-d", destination, "-o", path,
                        "-m", method, *arguments], env, timeout=5)


def acceptance(binary, output, root):
    env = os.environ.copy()
    owned = x11.Children()
    logs = []

    def log(name):
        stream = (output / name).open("w")
        logs.append(stream)
        return stream

    try:
        env["DISPLAY"] = x11.start_xvfb(owned, env, log("xvfb.log"))
        x11.emit("display", display=env["DISPLAY"], wm="KWin X11", toolkit="GTK 3")
        owned.start(["kwin_x11"], env, stdout=log("kwin.log"), stderr=subprocess.STDOUT)
        poll(lambda: x11.command(["xprop", "-root", "_NET_SUPPORTING_WM_CHECK"], env),
             lambda data: data["status"] == 0 and "window id #" in data["stdout"], "EWMH window manager")
        address_reply = dbus(env, None, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus.GetAddress")
        if address_reply["status"] != 0:
            raise RuntimeError(address_reply)
        address = address_reply["stdout"].split("'")[1]
        if not address.startswith(f"unix:path={env['XDG_RUNTIME_DIR']}/"):
            raise RuntimeError("accessibility bus is outside the owned runtime")
        env["AT_SPI_BUS_ADDRESS"] = address
        # Fedora's accessibility bus expects systemd activation for the registry.
        # Run the real registry on the already-created private bus instead.
        registry = shutil.which("at-spi2-registryd", path="/usr/libexec:/usr/lib/at-spi2-core:/usr/libexec/at-spi2-core")
        if not registry:
            raise RuntimeError("installed at-spi2-registryd not found")
        owned.start([registry], env, stdout=log("registry.log"), stderr=subprocess.STDOUT)
        poll(lambda: dbus(env, address, "org.freedesktop.DBus", "/org/freedesktop/DBus",
                          "org.freedesktop.DBus.NameHasOwner", "org.a11y.atspi.Registry"),
             lambda data: data["status"] == 0 and "true" in data["stdout"], "native registry")
        executable = root / ("DtwGtkFixture" + uuid.uuid4().hex)
        shutil.copy2(sys.executable, executable)
        state = output / "fixture.json"
        process = owned.start([str(executable), str(Path(__file__).resolve()), "--fixture", str(state)], env,
                              stdout=log("fixture.log"), stderr=subprocess.STDOUT)
        poll(lambda: state.exists(), bool, "GTK fixture")
        poll(lambda: x11.command(["xprop", "-root", "_NET_CLIENT_LIST"], env),
             lambda data: data["status"] == 0 and "0x" in data["stdout"], "managed GTK window")
        poll(lambda: dbus(env, address, "org.a11y.atspi.Registry", "/org/a11y/atspi/accessible/root",
                          "org.a11y.atspi.Accessible.GetChildren"),
             lambda data: data["status"] == 0 and "objectpath" in data["stdout"], "AT-SPI registration")
        x11.emit("fixture", pid=process.pid, binary=str(executable))
        client = x11.connect(binary, env, owned, log, output, "atspi")
        apps = client.tool("apps", {"query": executable.name, "state": "running"})["structuredContent"]["apps"]
        if [app["app"] for app in apps] != [str(executable)]:
            raise RuntimeError("owned executable not uniquely discovered")

        def execute(source, **kwargs):
            return client.tool("exec", {"app": str(executable), "source": source}, **kwargs)["structuredContent"]

        view = execute("await app.look({full:true});")["view"]
        if view["semantics"]["complete"] is not True:
            raise RuntimeError(f"AT-SPI window identity or discovery unproven: {view}")
        elements = {row.get("name"): row for row in view["elements"]}
        if elements["Fixture entry"]["role"] != "text_field" or "setValue" not in elements["Fixture entry"]["actions"]:
            raise RuntimeError(f"editable semantic receiver unproven: {elements}")
        if "press" not in elements["Increment fixture"]["actions"]:
            raise RuntimeError(f"native press receiver unproven: {elements}")

        entry = json.dumps(elements["Fixture entry"]["id"])
        protected = json.dumps(elements["Fixture password"]["id"])
        pages = execute(f"const a=await app.readText({{target:{entry},limit:2}});"
                        f"const b=await app.readText({{target:{entry},offset:a.next_offset,limit:2}});"
                        f"const c=await app.readText({{target:{entry},offset:b.next_offset,limit:2}});"
                        "return [a,b,c];")["result"]
        if [(p["text"], p["offset"], p["next_offset"], p["total_length"], p["unit"]) for p in pages] != [
            ("A", 0, 1, 5, "utf16"), ("😀", 1, 3, 5, "utf16"), ("BC", 3, None, 5, "utf16")]:
            raise RuntimeError(f"unexpected UTF-16 pages: {pages}")
        execute(f"await app.readText({{target:{entry},offset:2,limit:2}});", error_code="invalid_text_range")
        execute(f"await app.readText({{target:{protected}}});", error_code="unsupported_capability")
        setter = execute("await app.setValue({selector:{role:'text_field',text:'Fixture entry'},value:'Native SET 😀'});")
        if not any(ack["confirmed"] == "element_value_set" for ack in setter["effects"].get("acknowledgements", [])):
            raise RuntimeError(f"native setValue readback did not confirm: {setter}")
        execute("await app.click({selector:{role:'button',text:'Increment fixture'}});")
        actual = poll(lambda: json.loads(state.read_text()),
                      lambda value: value == {"clicks": 1, "value": "Native SET 😀", "multi_window": False},
                      "independent press and text effects")
        x11.emit("verified_effects", **actual)
        execute("await app.click({selector:{role:'button',text:'Open another fixture window'}});")
        poll(lambda: json.loads(state.read_text()), lambda value: value["multi_window"], "second owned window")
        poll(lambda: x11.command(["xprop", "-root", "_NET_CLIENT_LIST"], env),
             lambda data: data["status"] == 0 and data["stdout"].count("0x") == 2,
             "second managed GTK window")
        view = execute("await app.look({full:true});")["view"]
        if len(view["windows"]) != 2 or view["elements"] or view["semantics"]["reason"] != "unsupported_capability":
            raise RuntimeError(f"multi-window semantics did not fail closed: {view}")
        x11.finish(client)
        x11.emit("pass", multi_window_semantics="unsupported_capability", password_text="unsupported_capability")
    finally:
        owned.close()
        for stream in logs:
            stream.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--private", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.fixture:
        fixture(args.fixture)
        return 0
    if not args.binary or not args.output:
        parser.error("--binary and --output are required")
    binary, output = args.binary.resolve(strict=True), args.output.resolve()
    if args.private:
        if (os.environ.get("XDG_RUNTIME_DIR") != str(args.private / "runtime")
                or "DISPLAY" in os.environ or "WAYLAND_DISPLAY" in os.environ
                or not os.environ.get("DBUS_SESSION_BUS_ADDRESS")):
            parser.error("private mode requires the isolated launcher")
        acceptance(binary, output, args.private)
        return 0
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    with binary.open("rb") as stream:
        x11.emit("candidate", sha256=hashlib.file_digest(stream, "sha256").hexdigest(), path=str(binary))
    with tempfile.TemporaryDirectory(prefix="dtw-atspi-") as directory:
        root = Path(directory)
        (root / "runtime").mkdir(mode=0o700)
        env = os.environ.copy()
        for key in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "AT_SPI_BUS_ADDRESS", "XAUTHORITY", "SESSION_MANAGER"):
            env.pop(key, None)
        env.update(XDG_RUNTIME_DIR=str(root / "runtime"), XDG_CONFIG_HOME=str(root / "config"),
                   XDG_CACHE_HOME=str(root / "cache"), XDG_DATA_HOME=str(root / "data"),
                   XDG_SESSION_TYPE="x11", GDK_BACKEND="x11", GTK_USE_PORTAL="0", GTK_A11Y="atspi",
                   GSETTINGS_BACKEND="memory", GIO_USE_VFS="local", QT_QPA_PLATFORM="xcb", KWIN_COMPOSE="N")
        return subprocess.run(["dbus-run-session", "--", sys.executable, str(Path(__file__).resolve()),
                               "--private", directory, "--binary", str(binary), "--output", str(output)],
                              env=env, timeout=120).returncode


if __name__ == "__main__":
    raise SystemExit(main())
