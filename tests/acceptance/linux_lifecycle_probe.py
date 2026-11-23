#!/usr/bin/env python3
"""Verify restricted Linux lifecycle in a private Xvfb; never use the real display."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

import linux_x11_probe as x11

FIXTURE = r'''
#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <limits.h>
int main(void) {
    char self[PATH_MAX], state[PATH_MAX + 16], stop[PATH_MAX + 16];
    ssize_t n = readlink("/proc/self/exe", self, sizeof(self)-1);
    if (n < 0) return 2;
    self[n] = 0;
    snprintf(state, sizeof(state), "%s.state", self);
    snprintf(stop, sizeof(stop), "%s.stop", self);
    FILE *f = fopen(state, "a"); if (!f) return 3;
    fprintf(f, "%d\n", getpid()); fclose(f);
    Display *d = XOpenDisplay(NULL); if (!d) return 4;
    Window w = XCreateSimpleWindow(d, DefaultRootWindow(d), 50, 50, 320, 160, 0, 0, 0xffffff);
    char host[256]; gethostname(host, sizeof(host));
    XChangeProperty(d,w,XInternAtom(d,"WM_CLIENT_MACHINE",False),XA_STRING,8,PropModeReplace,(unsigned char*)host,strlen(host));
    unsigned long pid = getpid();
    XChangeProperty(d,w,XInternAtom(d,"_NET_WM_PID",False),XA_CARDINAL,32,PropModeReplace,(unsigned char*)&pid,1);
    XStoreName(d,w,"DTW lifecycle owned application");
    XMapWindow(d,w); XFlush(d);
    while (access(stop,F_OK) != 0) { while(XPending(d)) { XEvent e; XNextEvent(d,&e); } usleep(20000); }
    XDestroyWindow(d,w); XCloseDisplay(d); return 0;
}
'''.replace('#include <stdio.h>', '#include <stdio.h>\n#include <string.h>')


def lifecycle_round(binary, root, env, children, log, mode, artifacts):
    env["XDG_DATA_DIRS"] = str(root / "empty-system-data")
    applications = Path(env["XDG_DATA_HOME"]) / "applications"
    applications.mkdir(parents=True)
    executable = root / "DtwLifecycleFixture"
    source = root / "fixture.c"
    source.write_text(FIXTURE)
    subprocess.run(["cc", "-Wall", "-Werror", str(source), "-o", str(executable), "-lX11"], check=True)
    entry = f"[Desktop Entry]\nType=Application\nName=DTW Lifecycle Fixture\nExec={executable}\nTerminal=false\n"
    (applications / "fixture.desktop").write_text(entry)
    # Every unsafe/unsupported form uses a unique name, so discovery can assert
    # none were silently interpreted as a launchable installed application.
    for name, change in {
        "Arguments": "Exec=/bin/echo hello", "FieldCode": "Exec=/bin/echo %U",
        "Shell": "Exec=/bin/sh -c touch /tmp/DTW_SHOULD_NOT_EXIST",
        "Terminal": "Terminal=true", "DBus": "DBusActivatable=true",
        "Hidden": "Hidden=true", "WorkingDirectory": "Path=/tmp",
    }.items():
        lines = entry.replace("DTW Lifecycle Fixture", "Rejected" + name).splitlines()
        key = change.split("=", 1)[0]
        lines = [line for line in lines if not line.startswith(key + "=")]
        (applications / (name + ".desktop")).write_text("\n".join(lines + [change]) + "\n")
    client = x11.connect(binary, env, children, log, artifacts, mode)
    state = Path(str(executable) + ".state")
    stop = Path(str(executable) + ".stop")
    pid = None
    try:
        apps = client.tool("apps", {"query": "DTW Lifecycle Fixture", "state": "all"})["structuredContent"]
        assert len(apps["apps"]) == 1 and apps["apps"][0]["app"] == str(executable), apps
        assert not apps["apps"][0]["running"] and apps["coverage"]["installed"] and not apps["coverage"]["complete"], apps
        rejected = client.tool("apps", {"query": "Rejected", "state": "all"})["structuredContent"]
        assert rejected["apps"] == [], rejected
        def execute(source, **kwargs):
            return client.tool("exec", {"app": str(executable), "source": source}, **kwargs)["structuredContent"]
        # Public DTW look owns startup; launch is an internal lifecycle method.
        launched = execute("return await app.look({full:true});")
        startup = launched["view"]["startup"]["outcome"]
        assert startup["dispatch"] == "dispatched" and startup["error"] is None, startup
        assert startup["evidence"] == {"kind": "confirmed", "postcondition": "application_running"}, startup
        assert launched["view"]["running"] and state.exists(), launched
        pids = state.read_text().splitlines()
        assert len(pids) == 1, pids
        pid = int(pids[0])
        assert Path(f"/proc/{pid}/exe").resolve() == executable
        running = client.tool("apps", {"query": "DTW Lifecycle Fixture", "state": "running"})["structuredContent"]
        assert [row["app"] for row in running["apps"]] == [str(executable)], running
        repeated = execute("return await app.look({full:true});")
        assert state.read_text().splitlines() == pids and "startup" not in repeated["view"], repeated
        looked = repeated
        assert "DTW lifecycle owned application" in json.dumps(looked), looked
        execute("await app.quit();", error_code="unsupported_capability")
        assert Path(f"/proc/{pid}/exe").resolve() == executable
        client.tool("exec", {"app": "/bin/sh", "source": "await app.look();"}, error_code="app_not_found")
        x11.emit("lifecycle_verified", pid=pid, executable=str(executable), launch=launched, repeated=repeated,
                 quit="unsupported; owned instance remains alive", rejected_desktop_forms=7)
        x11.finish(client)
        # The independent application must survive normal metadata/server exit.
        assert Path(f"/proc/{pid}/exe").resolve() == executable
        # No eligible entries: preserve the original running-only discovery path.
        for desktop in applications.glob("*.desktop"):
            desktop.unlink()
        client = x11.connect(binary, env, children, log, artifacts, "fallback")
        fallback = client.tool("apps", {"query": "DtwLifecycleFixture", "state": "running"})["structuredContent"]
        assert [row["app"] for row in fallback["apps"]] == [str(executable)], fallback
        assert not fallback["coverage"]["installed"], fallback
        client.tool("exec", {"app": str(executable), "source": "await app.look({full:true});"})
        applications.rmdir()
        applications.write_text("owned unreadable-as-directory catalog fixture")
        unavailable = client.tool("apps", {"query": "DtwLifecycleFixture", "state": "running"})["structuredContent"]
        assert [row["app"] for row in unavailable["apps"]] == [str(executable)], unavailable
        assert not unavailable["coverage"]["installed"] and "desktop_catalog_unreadable" in unavailable["coverage"]["read_error_codes"], unavailable
        client.tool("exec", {"app": str(executable), "source": "await app.look({full:true});"})
        x11.finish(client)
        x11.emit("running_only_fallback_verified", coverage=fallback["coverage"])
    finally:
        stop.touch()
        if pid is None and state.exists():
            pid = int(state.read_text().splitlines()[0])
        if pid is not None:
            deadline = time.monotonic() + 5
            while Path(f"/proc/{pid}/exe").exists():
                if time.monotonic() >= deadline:
                    raise RuntimeError("owned lifecycle fixture failed to exit via its cleanup request")
                time.sleep(.03)
            x11.emit("lifecycle_fixture_cleanup", pid=pid, exit="fixture cleanup request")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--artifacts", required=True, type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    artifacts = args.artifacts.resolve()
    artifacts.mkdir(mode=0o700, parents=True, exist_ok=False)
    x11.emit("candidate", binary=str(binary), sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
    x11.fixture_round = lifecycle_round
    x11.isolated(binary, artifacts)


if __name__ == "__main__":
    main()
