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

[Use the setup receipt](../install.md#use-the-setup-receipt) for resume and
confirmation. Keep the original store, even if the host's default has changed.

- Wait for the host's supported reload or restart before confirming. External
  configuration edits, inventory refreshes, and new conversations leave a
  running parent plugin loaded. Use the supported MCP restart or reload where
  available. Otherwise keep `pending` and request a normal host restart.
- Antigravity setup can populate an empty global slot. Automatic live replacement
  on Unix requires a slot matching the previous managed pin and `"agy_live_swap": 2`
  in the verified **destination** package. An unmodified managed v0.6.1 can
  upgrade in place; restart only its DTW MCP in the App, not the entire host.
  For ineligible occupied slots, [replace the slot offline](install-agy.md#replace-offline)
  with both App and CLI stopped and the old tree backed up outside discovery
  through live verification and confirmation. Never run `agy plugin install`
  on an occupied slot. A successful swap reports `registration.backup`. Keep
  it outside discovery through live verification and confirmation. Afterward,
  you can remove it if an inspection finds no local data you need to keep. If
  the exchange is uncertain, keep the backup and avoid a second swap.
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
dtw package bind PROFILE PACKAGE_ID
dtw package activate PROFILE
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

## Remove a host installation

Remove the host's DTW plugin or MCP and Skill registration and let its
connections exit. Run `package unbind PROFILE` with the managed executable from
that installation's receipt, using `receipt.profile` and the
[original store](../install.md#use-the-setup-receipt). The command removes the
profile's store binding so later GC can collect unreferenced payloads. If the
binding may have changed, use `package status` in the original store to check
that the profile still belongs to the host and package in the receipt before
unbinding. The host owns its plugin directory.
