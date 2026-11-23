# Linux X11 acceptance

DTW discovers running applications with an attributable X11 window. It reads
window metadata and unobstructed client pixels, and uses XTest for foreground
focus, coordinate click, hover, same-window content drag, wheel steps and mapped
keyboard input. Semantic text reading requires a provider with native element
support. Linux X11 currently exposes window metadata and PNGs for inspection.

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
to the archive. The `apps` tool reports coverage with `installed:false` on X11.

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
