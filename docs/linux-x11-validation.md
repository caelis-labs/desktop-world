# Linux X11 acceptance

DTW discovers running applications with an attributable X11 window and a subset
of installed XDG desktop entries. It reads window metadata and unobstructed client
pixels, and uses XTest for foreground focus, coordinate click, hover, same-window
content drag, wheel steps and mapped keyboard input. With a proven single window,
AT-SPI supports semantic inspection, UTF-16 text pages, native button activation
and editable text replacement.

The release matrix builds Linux x86_64 and arm64 natively on Ubuntu 24.04 with
glibc 2.39. A local Fedora candidate records its own distro, libc and ELF
requirements. An ELF architecture check or cross-build does not establish GUI
coverage. A local X11 server with EWMH, XRes 1.2, XInput 2 and XTest is required.
Wayland and XWayland are unsupported.

## Build, extract and import

Use a clean checkout on a native host. This example builds the x86_64 candidate,
verifies and extracts the final archive, imports it into a new package store and
runs the headless `apps`/`exec`, worker and retired-flag controls on the managed
executable. For an arm64 host, select `aarch64-unknown-linux-gnu`.

```sh
run=$(mktemp -d)
python3 scripts/build-rewrite-candidate.py \
  --target x86_64-unknown-linux-gnu --output "$run/candidate"
python3 scripts/check-linux-candidate.py \
  --candidate "$run/candidate" --output "$run/installed"
```

Use `package import`'s returned `executable` and the matching Skill from its
package directory. The candidate reader and `SHA256SUMS` bind the bytes tested
to the archive. Linux discovery reports partial coverage and falls back to running
windows when there are no eligible entries.

## Desktop fixture

The fixture runner needs Python 3, Tcl/Tk `wish` and `loginctl` for the real
session check. The isolated run additionally needs Xvfb, `kwin_x11`,
`dbus-run-session` and `xprop`. The real run checks the selected logind session
and uses the active foreground on an unlocked local X11 desktop. Run it during
a quiet interval. DTW does not restore the prior focus or pointer. Only the newly
created fixture is selected, and only its client pixels are retained.

```sh
binary=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["executable"])' "$run/installed/import.json")
sha=$(sha256sum "$binary" | cut -d ' ' -f 1)
loginctl list-sessions
session=$XDG_SESSION_ID
python3 tests/acceptance/linux_x11_probe.py \
  --real-session "$session" --binary "$binary" --sha256 "$sha" \
  --artifacts "$run/real" > "$run/real.jsonl"
python3 tests/acceptance/linux_x11_probe.py \
  --isolated --binary "$binary" --sha256 "$sha" \
  --artifacts "$run/xvfb" > "$run/xvfb.jsonl"
```

Set `session` to the matching active session ID. For a physical display, run the
probe in the same PID namespace as the X server. A mismatched namespace fails
XRes identity proof even when `xdpyinfo` can read the display.

Both runs check the authorized window title and screenshot geometry, reject
invalid coordinates, read back button, entry, wheel, hover and drag events from
Tk, and require clean MCP EOF. Initial and final PNG hashes record the visual
change. Xvfb exercises the actual X11 protocol on a private virtual display.
It does not establish physical desktop, lock-screen, Wayland, or other
architecture behavior. No ordinary CI step injects desktop input.

## Semantics and application startup

Run the opt-in probes on the managed executable. AT-SPI uses GTK 3, Python GI,
D-Bus and the installed registry. Startup compiles a small native X11 fixture
and registers it in a private XDG directory. Both use private Xvfb/KWin displays.

```sh
python3 tests/acceptance/linux_atspi_probe.py \
  --binary "$binary" --output "$run/atspi" > "$run/atspi.jsonl"
python3 tests/acceptance/linux_lifecycle_probe.py \
  --binary "$binary" --artifacts "$run/lifecycle" > "$run/lifecycle.jsonl"
```

The AT-SPI probe checks native reads, editable text readback and button events.
It exercises split-surrogate offsets, protected fields and multiple-window
ambiguity. The lifecycle probe verifies stopped discovery through launch and
running state, duplicate avoidance, registration loss and unsupported quit.
Running applications stay available when desktop-entry discovery is partial.
