# Managed package lifecycle

The installer owns this CLI. Agent desktop tasks still use only `apps` and
`exec({app,source})`; no package/session management appears in those tools.

Packages expose MCP server `dtw` and bundle Skill `dtw` at `skills/dtw`, under
plugin/install ID `desktop-world` (Caelis Skill identity `desktop-world:dtw`),
with `store_schema: 1`. Update one host/profile at a time through the standard
`dtw setup` transaction below; never edit or rename files inside an immutable
store package.

## Installer Agent update workflow

Use the [single Agent entry](../install.md). The native `setup` command
coordinates the immutable store and **one** host/profile:

- `setup prepare HOST PROFILE PACKAGE_ID [SOURCE]` reconciles/imports the
  selected verified package and registers or stages the host update.
- `setup resume HOST PROFILE` settles an active pending registration (for
  example after a required host restart or Antigravity reload) without network
  access.
- `setup confirm HOST PROFILE PACKAGE_ID` promotes `pending → current`,
  retains `rollback`, and runs owned store GC after live verification passes.

Key operational boundaries:

- Reuse the target host/profile pin without altering other hosts or profiles.
- Wait for the host's supported reload/restart boundary before confirming:
  external config edits (such as DSH `cordis.patch.yml`), UI inventory refreshes,
  and new conversations do not reload an already-running parent plugin. Where live
  MCP restart or reload is supported (e.g. OpenCode `opencode reload`, Pi
  `/reload`, Antigravity App slot swap + single MCP restart/toggle), apply that
  one step; otherwise keep `pending` and request one normal host restart.
- Inside Antigravity (`agy`), `dtw setup` populates an empty global slot
  automatically and, on Unix, atomically swaps a slot matching the previously
  managed `agy` pin in place (preserving unpinned/modified slots and leaving
  occupied Windows slots untouched for manual replacement); Antigravity owns
  removal of its plugin directory copy on uninstall.
- After host reload, a running installation conversation with a stale tool
  snapshot can invoke one fresh verifier context (e.g., `self` subagent in
  Antigravity App) to verify live `apps`, `exec`, `look()`, `capture()`, and the
  matching `dtw` Skill before confirming.

## Store verification, paths, and low-level package commands

The low-level `package` subcommands below document the store internals; installers
use `dtw setup` instead.

A distribution package is immutable and addressed by SHA-256 of its complete
`SHA256SUMS`. Import verifies every member, `BUILD.json` binary/Skill identity,
and all plugin versions before publishing a staged copy. Links/reparse points,
unsafe paths, case collisions, unlisted files, and unsupported store formats are
rejected. Import never executes imported content and does not edit host
configurations.

Default stores are `~/Library/Application Support/DesktopWorld` on macOS,
`%LOCALAPPDATA%\DesktopWorld` on Windows, and
`$XDG_DATA_HOME/desktop-world` on Linux (default `~/.local/share/desktop-world`).
`--store` accepts an absolute dedicated directory; existing nonempty unmanaged
directories are refused. Paths may contain spaces or Unicode; use the
returned encoded `cordis_url`, never a hand-built file URL. See
[DSH Windows compatibility](install-dsh.md#windows-host-execution-and-storage)
for external staging and authorization requirements.

`dtw package --help` (also `-h`, or no package subcommand) returns JSON with
`usage`, `manager_version`, and `store_schema` without opening the store.
Invalid syntax, host/profile names, and package IDs also fail before store
initialization with `package_error` on stderr (exit code 2).

```sh
dtw package import "/absolute/verified/package"
dtw package bind dsh-desktop <package_id>
dtw package activate dsh-desktop
```

Host/profile names use lowercase ASCII letters, digits, dot, dash, or
underscore (Windows reserved device names rejected). Activation promotes
`pending → current`, retains the previous `current` as one `rollback`
reference, and collects unpinned packages older than seven days. `package status`
re-verifies every managed package on disk and is for diagnostics only.

```sh
dtw package gc             # preview
dtw package gc --apply     # collect only owned, unreferenced, old packages
```

Collection retains `current`, `pending`, and `rollback` references for every
host and uses cross-process locks (`leases/`) to retain live managed runtimes and
their native workers, including a worker that outlives its supervisor. It never
kills a consumer; locked removals (such as active Windows executables) are
reported as `blocked` and preserved. Owned stale staging/trash entries are
collected after seven days; unknown entries are left alone.

To remove a host installation, stop its connections and uninstall its DTW plugin
or MCP/Skill registration first, then run `dtw package unbind <host-profile>`
so later GC can collect unreferenced payloads. The store never adopts, migrates,
or deletes standalone installations, source checkouts, or other hosts' plugin
directories.
