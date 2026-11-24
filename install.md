# Install or update Desktop World for this Agent

**User prompt:** Install or update Desktop World for this Agent using https://raw.githubusercontent.com/caelis-labs/desktop-world/main/install.md

**Installer Agent:** there is one normal path: **prepare → verify in the host → confirm**. Use the current harness and profile, not a scan of other installed Agents. Use its existing store pin; for a first default installation use the harness name (`caelis`, `dsh-desktop`, `opencode`, `pi`, `grok-build`). Antigravity App and CLI use host `agy-app` or `agy-cli` and the **same** profile `agy` for their shared global slot. Continue with the original host identity for an existing transaction. Do not ask the user for a version, package ID, or a file path.

## Prepare the selected host

If resuming after restart, **skip the download entirely**: run the saved `install.executable` with `setup resume HOST PROFILE` and continue with its receipt. A pending transaction belongs to that exact host/profile, even if a newer release was published meanwhile.

Otherwise run **one** of the following blocks using the host's approved shell. Supply `HOST` and `PROFILE` for this invocation; when updating, also supply `INSTALLED_DTW`, the absolute executable from that host's actual DTW registration, **not** a `dtw` found on PATH. Run from the intended host workspace so the runtime can detect a conflicting project configuration. Stage all downloads outside that workspace and the source checkout. If overriding `DTW_STAGING_DIR`, supply an existing, host-approved absolute directory outside both. The download below is repository source pinned by exact blob and SHA-256, not an extra Release asset. Never pipe an unchecked response to a shell.

**macOS Apple Silicon / Linux x86_64 or arm64** (set the three shell variables above in this invocation; omit `INSTALLED_DTW` on a first install):

```sh
set -eu
launcher=$(mktemp "${DTW_STAGING_DIR:-/tmp}/dtw-bootstrap.XXXXXXXX")
trap 'rm -f "$launcher"' EXIT
curl --disable --fail --silent --show-error --location --proto '=https' --proto-redir '=https' -H 'Accept: application/vnd.github.raw+json' 'https://api.github.com/repos/caelis-labs/desktop-world/git/blobs/1e785113adf11ac9937a61c1f40a0326974a4cf0' -o "$launcher"
if [ "$(uname -s)" = Darwin ]; then hash=$(shasum -a 256 "$launcher"); else hash=$(sha256sum "$launcher"); fi
[ "${hash%% *}" = '3d9477fb8f9cbe825a3acba26a810bb0ed0ca21333140359c413627c844dbc1d' ] || { echo 'Bootstrap checksum mismatch' >&2; exit 1; }
set -- "$HOST" "$PROFILE"; if [ -n "${INSTALLED_DTW:-}" ]; then set -- "$@" "$INSTALLED_DTW"; fi
sh "$launcher" "$@"
```

**Windows x64 / PowerShell 5.1 or 7** (set `$DtwHost`, `$DtwProfile` and optional `$InstalledDtw`; do not use PowerShell's reserved `$Host` variable):

```powershell
$ErrorActionPreference = 'Stop'
$dir = if ($env:DTW_STAGING_DIR) { $env:DTW_STAGING_DIR } else { [Environment]::GetFolderPath('LocalApplicationData') }
$file = Join-Path $dir ('dtw-bootstrap-' + [guid]::NewGuid().ToString('N') + '.ps1')
try {
  Invoke-WebRequest -UseBasicParsing -Headers @{Accept='application/vnd.github.raw+json'} -Uri 'https://api.github.com/repos/caelis-labs/desktop-world/git/blobs/f46170e823718ecbf38ca1f6d48152d7c062079a' -OutFile $file
  if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne 'd57c350faff0b963b8c888ec65b9785c28250cb7a6e6931a28b820a704256c27') { throw 'Bootstrap checksum mismatch' }
  $argv = @($DtwHost, $DtwProfile); if ($InstalledDtw) { $argv += $InstalledDtw }
  & $file @argv
} finally { if (Test-Path -LiteralPath $file) { Remove-Item -LiteralPath $file } }
```

Linux uses Python 3 from the standard Ubuntu Desktop installation for JSON decoding; macOS uses system `plutil`. The bootstrap resolves the official latest stable release once. The authenticated release manifest names one platform archive and its complete `package_id`; the native manager verifies the inventory before using it. A verified cached/current package needs no archive download. With an existing setup-capable manager, a pending update needs no release lookup. Host registration, package bindings and activation belong to the binary, not the download scripts. If the selected release predates `setup` (including **v0.6.0**), use **that tag's** [legacy workflow](https://github.com/caelis-labs/desktop-world/blob/v0.6.0/install.md) instead; main-branch documentation does not add commands to old binaries.

## Check the installation

Act on the JSON `state`; do not probe several routes in parallel:

| State | Next action |
| --- | --- |
| `verify` | Follow the receipt's `next` for the host's reload or fresh-session step. Use the **actual host connection**. Check that the loaded Skill and running executable match `install` (for a host-managed copy, check the copy's identity). Discover `apps` and `exec`, call `apps({state:"running"})`, then use an unchanged returned ID for one authorized test window. In one `exec`, use `await app.look(); await app.capture();` and check the result/image. Do not send test input or capture a user's unrelated documents. |
| `restart_required` | Save the receipt, report **prepared, not active**, and ask for one normal restart after the user saves work. Resume this transaction; do not redownload, bind again or kill shared processes. |
| `host_action_required` | The returned `next` identifies the specific registration, scope, compatibility or authorization boundary. Follow that one step using the matching host guide; keep any pending pin. After the host step, use the saved receipt to resume or perform the specified live check. Do not ask the user to edit a config that the Agent is authorized to edit. A fresh session can refresh its tool snapshot, not force a parent plugin to reload. |

After successful **live** verification, invoke the receipt's `install.executable` once: `setup confirm HOST PROFILE PACKAGE_ID`, using the exact returned ID. This confirms that package, not whichever update happens to be pending later. Inspect the returned result, including collection errors; no routine `package status`, `gc`, standalone MCP handshake or second host verifier. Report the version and actual result. On failure, leave pending for diagnosis; do not claim installation succeeded or start a second plugin registration.

## Host-specific or older installations

[Caelis](docs/install-caelis.md) · [DSH Desktop](docs/install-dsh.md) · [Antigravity App / CLI](docs/install-agy.md) · [OpenCode](docs/install-opencode.md) · [Pi](docs/install-pi.md) · [Grok Build](docs/install-grok-build.md). These describe the real host integration points, version gates and supported reloads. An unsupported profile, a platform permission prompt or host trust decision cannot be silently bypassed. On macOS, Accessibility and Screen Recording must apply to the launching host. Linux desktop operations require a supported unlocked native X11 session; see [platform limits](docs/product-matrix.md).

## Select and extract a package

For older runtimes or manual recovery, use the [selected release](https://github.com/caelis-labs/desktop-world/releases/latest)'s **own** installation instructions, bundled manifests and [package lifecycle](docs/package-management.md). Never combine runtime and Skill from different packages. Other standard MCP clients can register the verified `install.executable` with argument `mcp` and the same package's Skill; use one route only.

### Windows x64: download and verify once

The [published v0.6.0 Windows recipe](https://github.com/caelis-labs/desktop-world/blob/v0.6.0/install.md#windows-x64-download-and-verify-once) is for the pre-setup workflow. A denied execution or store path needs normal host authorization, not an ACL or execution-policy change.
