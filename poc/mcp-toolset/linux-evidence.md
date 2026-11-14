# Linux locked-session and isolated X11 evidence

Final result: real session lock metadata passed without any locked-session
application operation. The frozen candidate passed actual script and atomic
X11 backend checks on owned fixtures in isolated Xvfb, including PNG capture
and independent text acknowledgements. Earlier harness failures are preserved
separately below.

## Safety boundary and real locked-session result

The real X11 session remained locked. No unlock/bypass, input, focus, quit,
XTest injection, screenshot, root/locker capture, client-title read, locker
unmap/restack, or security-setting change was performed on `:0`. No private
application was operated. Ordinary fixture mapping could not be proven safe
against the active locker and window manager, so **no fixture was mapped on
`:0` and no locked-session application operation was actually validated**.

Observed on 2026-11-14 09:31:27 +0800:

- `DISPLAY=:0`, `XDG_SESSION_TYPE=x11`, `XDG_SESSION_ID=7`.
- `loginctl show-session 7 -p Id -p Type -p Display -p LockedHint` returned
  `Id=7`, `Type=x11`, `Display=:0`, `LockedHint=yes` both before and after.
- `loginctl show-session self ...` failed: `Caller does not belong to any known
  session.` The explicit environment session ID succeeded; no lock inference
  was made from the failed `self` lookup.
- `xdpyinfo -display :0 -queryExtensions` succeeded. Server: X11 11.0,
  The X.Org Foundation, vendor release `12101021`, `1920x1080` pixels.
  Advertised extensions: X-Resource opcode 150, XInputExtension opcode 131,
  XTEST opcode 132. Advertisement is not proof of XRes 1.2 ownership or actual
  input/capture functionality. No extension input request was sent.

Reproduce the bounded metadata-only probe from the repository root:

```sh
python3 poc/mcp-toolset/linux_locked_probe.py --session 7
```

This command exited 0 with `lock_check: {before: true, after: true}`. It does
not invoke DTW against the real display and does not enumerate private clients.
`loginctl` observations are before/after evidence, not continuous monitoring.

## Backend inspection

Inspection used the source on branch `poc/mcp-atomic-toolset`:

- `cmd/dtw/src/backends/linux/native.rs`: `Store::new` checks native X11,
  negotiates XRes 1.2, XInput 2 and XTest, and proves its own connection PID.
  Version queries are not input injection.
- `catalog` reads `_NET_CLIENT_LIST`, each client's `WM_CLIENT_MACHINE`, XRes
  local PID ownership and `/proc` executable/start identity. It reads no titles
  or pixels. The public `apps` query is applied after catalog retrieval in
  `app_protocol/server.rs`; it is not a server-side class-only X11 scan.
- `windows` proves ownership before reading the selected app's title. It also
  writes `_DTW_WINDOW_INCARNATION_V1` on that owned window, so it is not entirely
  read-only even though its public purpose is discovery.
- `capture` validates target identity and checks viewability, geometry and
  occlusion before `XGetImage` on the target window. It has no session-lock
  guard. Its occlusion checks are insufficient authority to attempt capture
  on the real locked display under this task's constraints.
- `action` calls `focus`, which may send an EWMH `_NET_ACTIVE_WINDOW` request;
  later segments call XTest after validating focus and input state. There is
  no `LockedHint` guard. These paths were prohibited on `:0`.

## Isolated Xvfb validation

The probe supports a separate, fresh Xvfb server with TCP disabled, private
D-Bus session, private KWin configuration/cache/runtime, and one owned Tk
fixture. KWin was found at `/usr/bin/kwin_x11`; a custom WM was unnecessary.
The fixture uses a unique copied wish executable, WM class and exact child PID.
The `apps` query uses that unique fixture identity. All DTW calls in isolated
mode receive only the fixture's executable authorization identity.

```sh
python3 poc/mcp-toolset/linux_locked_probe.py --session 7 --isolated \
  --binary /home/xueyongzhi/WorkDir/caelis-labs/desktop-world/cmd/dtw/target/release/dtw
```

Use `--toolset script` or `--toolset atomic` for one independent server round;
the default `--toolset both` creates separate fixtures and MCP connections.
The script records candidate version/hash, server conditions, exact MCP tool
arguments, structured receipts, image hashes, fixture acknowledgement, child
logs, and lock status before/after. Script mode calls `apps`, then `exec` with
`look`, `capture`, and `typeText` + `keypress`; atomic mode calls `apps`, `look`,
`capture`, `type_text` and `keypress` with flat `app` arguments. Input success
requires the owned Tk entry to print the exact acknowledgement
`ACK=dtw_probe_script_731` or `ACK=dtw_probe_atomic_731`, respectively. A receipt
alone is not treated as effect verification. Unknown/failed input is never replayed.
Subprocess/MCP/startup waits are bounded; cleanup signals only process groups
created by this probe, never names or all-user processes. Scratch data lives
under a temporary directory and is removed.

Installed package versions (`rpm -q kwin-x11 tk tcl xorg-x11-server-Xvfb python3`):
KWin X11 6.6.4-1.fc43, Tk/Tcl 9.0.2-1.fc43, Xvfb 21.1.25-2.fc43,
Python 3.14.3-2.fc43, all x86_64.

Preliminary bootstrap attempts, preserved distinctly:

| JSONL log under `/tmp` | Candidate SHA-256 | Result |
| --- | --- | --- |
| `dtw-linux-script-first.jsonl` | `63c2911b7bf08d1f33bece24250c3f37b462bf769d5fb44e33c6d4351ed712a4` | Xvfb/KWin ready; copied wish readiness timeout; no MCP calls |
| `dtw-linux-script-libraries.jsonl` | `f54c9684328d7dae78e35dd8a75331a6c2ea2aefc97e8c06077b902cfa73ff6b` | Explicit Tk library; readiness timeout; no MCP calls |
| `dtw-linux-script-libraries-fixed.jsonl` | `f54c9684328d7dae78e35dd8a75331a6c2ea2aefc97e8c06077b902cfa73ff6b` | Explicit matching Tcl/Tk libraries; readiness timeout; no MCP calls |
| `dtw-linux-script-ready.jsonl` | `3a87fc31c0a0849bda3bbf5cee942079c46f89cb32383f220e709edcda4afe21` | Script-first wish startup succeeded, exact class check rejected `READY=131,Script-fixture.tcl,0x200013`; no MCP calls |

The initial timeouts had empty fixture stderr. Explicit library selection did
not resolve them; the observed root cause was `wish` option/script ordering.
Putting the script first produced readiness, while `-class` was ignored in
that position. The final harness uses a unique script basename as the default
WM class and checks the exact PID and class. Its initial mixed-case assertion
exposed Tk's case canonicalization; the final harness checks the exact
canonical class using `capitalize()`. No failed input was replayed:
none of these four attempts reached DTW or dispatched input. Each ended with
owned child cleanup and `LockedHint=yes` before/after. The candidate binary was
being rebuilt by parent between attempts, hence each observed hash is retained.

The existing debug binary has an older CLI and was explicitly excluded as
candidate evidence. A KWin
`--help` attempt with an empty `DISPLAY` exited 134 without output; it did not
connect to `:0` and is not an isolated KWin functionality result.

## First frozen-candidate attempt: fixture assertion failure

At 2026-11-14 09:39:35 +0800, first ran against the frozen candidate:

```sh
python3 poc/mcp-toolset/linux_locked_probe.py --session 7 --isolated --toolset both \
  --binary /tmp/dtw-mcp-candidate-0hp0f5v7/dtw > /tmp/dtw-linux-final.jsonl
```

Version `dtw 0.4.2`, SHA-256:
`3f3aaf2ef5750e41e8ff8a294ad2feda9aaca124c8d33b4944de04954a5fe55d`.
This matches the frozen hash supplied by parent. Exit status: **1**.

- Xvfb selected `:1`, TCP disabled. KWin EWMH startup succeeded:
  `_NET_SUPPORTING_WM_CHECK(WINDOW): window id # 0x20000c`.
- Script fixture readiness was
  `READY=125,Dtwlockedprobe55301473f7b640008877029f4617b928.tcl,0x200013`.
- Atomic fixture readiness was
  `READY=194,Dtwlockedprobe45219008ae8f4871a4697999c9533cbc.tcl,0x200018`.
- Both exact class checks rejected the observed class because the expected
  prefix was `DtwLockedProbe` and Tk canonicalized it to `Dtwlockedprobe`.
  This was a **probe identity-assertion bug**, not a DTW backend failure. Fixture
  stderr was empty. Each round used a fresh owned fixture; no entry was reused.
- Neither round reached MCP initialization or `apps`. Consequently script
  `exec`, atomic `look`/`capture`/`type_text`/`keypress`, captures and input
  acknowledgements were **not run in this attempt**.
- No failed/unknown input was replayed; no DTW input was attempted at all.
  This attempt stopped at the identity boundary. To reproduce this historical
  harness error, change only the expected `(fixture_class + '.tcl').capitalize()`
  to `(fixture_class + '.tcl')` in a temporary copy of the probe and run that
  copy against the same candidate. Unique tokens/PIDs/XIDs will differ. The
  current probe fixes this assertion while retaining exact PID/class checks.
- Cleanup recorded owned leader PIDs `[6, 9, 125, 194]`, exit statuses
  `[0, -15, -15, -15]`. All recorded fixture/WM/server leaders were reaped;
  only probe-created process groups were signaled. Temporary files were removed.
- Final explicit `loginctl show-session 7 ...` again reported
  `LockedHint=yes`; the probe emitted `lock_check: {before: true, after: true}`.

KWin's private D-Bus logs also included portal FUSE/PipeWire/service warnings;
they did not prevent EWMH readiness. Xvfb logged nonfatal xkbcomp keysym warnings
and a failed attempt to choose the already occupied display before selecting
`:1`. These messages are not proof of DTW capability or backend failure.

## Final result: script and atomic native backend checks passed in Xvfb

After correcting only the known exact-class assertion, ran fresh independent
script and atomic fixtures at 2026-11-14 09:40:54 +0800:

```sh
python3 poc/mcp-toolset/linux_locked_probe.py --session 7 --isolated --toolset both \
  --binary /tmp/dtw-mcp-candidate-0hp0f5v7/dtw > /tmp/dtw-linux-final-class-fixed.jsonl
```

Exit status **0**. Candidate hash was again
`3f3aaf2ef5750e41e8ff8a294ad2feda9aaca124c8d33b4944de04954a5fe55d`,
version `dtw 0.4.2`. The class check uses the exact canonical unique script
basename; exact PID and unique copied executable checks remain in place.

| Check | Script server | Atomic server |
| --- | --- | --- |
| Tool listing | `apps`, `exec` | `apps` + 14 facade tools |
| Scoped `apps` | Exactly one matching owned executable, running | Exactly one matching owned executable, running |
| Observation | `exec: return await app.look()` completed | `look {app}` completed |
| Semantic coverage | `unsupported_capability`, zero elements, screenshot fallback | Same explicit limitation |
| Capture | `exec: return await app.capture()` returned PNG, 920×360 | `capture {app}` returned PNG, 920×360 |
| Typing | `exec` with `typeText('dtw_probe_script_731')`, then `keypress('ENTER')` | `type_text {app,text:'dtw_probe_atomic_731'}`, then `keypress {app,keys:['ENTER']}` |
| Dispatch | 2 dispatched actions, observed | 1 dispatched action per call, observed |
| Actual fixture acknowledgement | `ACK=dtw_probe_script_731` | `ACK=dtw_probe_atomic_731` |
| MCP exit | 0 | 0 |

Both `look` and `capture` produced actual `image/png` MCP blocks. Before input,
their base64 payload SHA-256 was
`4371e31fdc4614066aee2abe9b27cda1d303c58bebc4d13e95ffac7ab38b6a1c`.
After input, image hashes changed; exact post-input hashes are in the JSONL.
Images were generated only from the owned fixture on Xvfb `:1`. The probe
retains image hashes instead of large base64 payloads.

Script fixture PID `125`, WM class
`Dtwlockedprobe7016b54afb164723af78af07d9d672fd.tcl`, XID `0x200013`.
Atomic fixture PID `216`, WM class
`Dtwlockedprobe85674d9c8426477284a69d23b7ddd661.tcl`, XID `0x200018`.
These PIDs are as observed inside the command's PID namespace. Each fixture
started with a fresh entry; no input was retried or replayed. Fixture and MCP
stderr logs were empty. The app discovery response explicitly reported
`complete:false`, `installed:false`, `scope:running_applications`.

Cleanup recorded owned leaders `[6, 9, 125, 195, 216, 228]`, statuses
`[0, -15, -15, 0, -15, 0]`; all recorded leaders were reaped and only
probe-created process groups were signaled. Temporary data was removed.
The final explicit lock query again returned `LockedHint=yes`, and the probe
reported `lock_check: {before:true, after:true}`. This is metadata sampled before
and after the run, not continuous lock-state evidence. The Linux backend has no
AT-SPI implementation: the successful `look` used screenshot fallback and
explicitly reported unsupported native semantics.

A final read-only cleanup check found the owned temporary directory
`/tmp/dtw-locked-probe-uuczerfg` and Xvfb socket `/tmp/.X11-unix/X1` absent.
Scoped process queries for that fixture path and the frozen candidate's MCP
command matched only the checking command's own shell/sandbox ancestors, not
retained test processes. The recorded cleanup reaped every owned leader and
signaled only the groups it created; no task process was observed remaining.
These checks are within the command runtime's process namespace.

After raw success was reviewed, the reproducible script gained explicit
assertions that capture contains at least one `image/png` MCP content block and
that the MCP process exits 0. The successful raw run already satisfied both;
only a Python syntax check was rerun after adding these assertions, with no
further GUI experiment.

These are **actual DTW native X11 backend results in isolated Xvfb**, including
XRes ownership, owned-window capture and XTest typing. They do not establish
app operation under the real session's lock. Other atomic tools (read_text,
wait, wait_for_new_window, click, set_value, move, drag, scroll, focus, quit)
were listed but not individually invoked. Rust tests using controlled/mock
providers are a separate evidence class, not native-display validation.
Core/atomic Rust tests and the candidate build are owned by parent.
