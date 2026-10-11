# Desktop World

Desktop World (DTW) gives an Agent a compact App → Window → Element desktop API through one MCP tool, `exec`. It runs scripts in Cua Driver's native QuickJS `run_script`. DTW keeps packaging, bounded results, execution receipts, and a small per-Session JSON `state`; CUA performs desktop discovery, screenshots, keyboard and pointer input, and native permission checks. Node and a separate CuaDriver App are not needed at runtime.

This source tree is the thin CUA implementation. The former Go/QuickJS/helper runtime is gone; scripts use `cua`, not the former `dtw.app` signatures. macOS and Windows have live fixture evidence; [Windows acceptance](docs/windows-acceptance.md) retains a blocking screenshot/geometry failure for DPI-unaware windows at 125% scaling. The [acceptance record](docs/acceptance.md) separates prior POC, current build, and unverified paths.

## Build and install

Building requires Rust 1.98.0, Python 3.11+ for dependency-notice generation, and access to the pinned [Cua Driver source](https://github.com/trycua/cua/tree/ac967b9006aafa10870aadb5a645b038cba9721a). macOS additionally needs Xcode with its Swift toolchain; Windows needs the MSVC Rust target, Visual Studio C++ build tools, a Windows SDK, and the matching **MSVC x64/x86 Spectre-mitigated libraries** (`Microsoft.VisualStudio.Component.VC.Runtimes.x86.x64.Spectre`). The pinned dependency `msvc_spectre_libs` rejects a build without that optional component. The platform build script verifies the CUA commit and builds the DTW CLI plus CUA's private worker. Choose a new destination:

```sh
./scripts/build-plugin.sh /private/tmp/dtw-plugin
/private/tmp/dtw-plugin/bin/dtw --version
```

On Windows, run PowerShell 7 and select a new directory:

```powershell
pwsh -File ./scripts/build-plugin.ps1 C:\Temp\dtw-plugin
C:\Temp\dtw-plugin\bin\dtw.exe --version
```

To use an existing clean CUA checkout at the exact pinned commit, set `CUA_SOURCE_ROOT` to that path. The scripts read that checkout without editing it. Build output defaults to a temporary directory; `DTW_BUILD_DIR` selects another. CI uses a stable `DTW_CUA_SOURCE_DIR` and caches the Rust build and downloads across runs. For an offline build, set `DTW_CARGO_OFFLINE=1` with dependencies cached. A previously built exact-source worker can be supplied with both `CUA_DRIVER_BIN` and `CUA_DRIVER_BIN_SHA256` when rebuilding only the adapter. The macOS script ad hoc signs a local test package; the Windows script produces an unsigned test package. Release signing remains separate.

Install the package into a new Agent Plugin directory:

```sh
/private/tmp/dtw-plugin/bin/dtw install plugin --to /path/to/new-plugin-dir
```

For an isolated Antigravity project, install into its `.agents/plugins/desktop-world` directory:

```sh
/private/tmp/dtw-plugin/bin/dtw install antigravity --project /path/to/existing-project
agy plugin validate /path/to/existing-project/.agents/plugins/desktop-world
```

The installer refuses an existing destination and does not change global Agent configuration. Other MCP hosts can launch `bin/dtw` over stdio and expose `skills/desktop-world/SKILL.md` to the Agent. The package advertises only `exec`; its `status`, `result`, and `cancel` operations share that entry. Actual desktop permission belongs to the host process, so the host needs macOS Accessibility and Screen Recording authorization. Agents working on the same desktop can affect one another's focus; an independent host should launch its own DTW MCP process and private CUA worker.

On Windows, use `bin/dtw.exe`; the packaged `mcp.json` and project installer also name `.exe`. Windows UI Automation and foreground input remain subject to Windows integrity/UIPI rules. The unsigned local package does not grant elevated or UIAccess input.

PowerShell 7 is required for the Windows source packaging script, not for the installed plugin. The MCP host launches `dtw.exe` and its packaged private worker directly; Rust, Visual Studio build tools, Python, PowerShell 7, and Node are not plugin runtime requirements. Python is used by source packaging and the acceptance harnesses; Pillow is only needed for Windows screenshot/geometry checks. The pinned worker's integrity-level diagnostic uses the built-in Windows `powershell.exe` (5.1); that is separate from PowerShell 7. The current MSVC Windows build dynamically links the Visual C++ runtime (`VCRUNTIME140.dll`); the target machine needs a compatible [x64 Visual C++ Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist), at least as recent as the build tools. The local package does not bundle that redistributable.

## Script example

Read the packaged [Skill](packaging/plugin/skills/desktop-world/SKILL.md), [recipes](packaging/plugin/skills/desktop-world/RECIPES.md), and [API reference](packaging/plugin/skills/desktop-world/api.d.ts) before authoring actions. Replace the PID and window ID with values observed for the authorized target.

```json
{"session":"task","script":"const app=await cua.getApp({pid:123,windowId:456}); const view=await app.getState({query:'Save',max_elements:80,include_screenshot:false}); return {window_id:app.windowId,tree:view.tree_markdown,truncated:view.truncated};"}
```

If `exec` returns `status:"running"`, use the same `session` and original `execution_id` with `operation:"result"`. Default results are compact; `part:"structured"`, `part:"text"`, and `part:"image"` retrieve retained detail. A native action receipt reports delivery, so observe the UI effect before a further action. See the Skill for native file pickers, desktop screenshots, image coordinates, large browser trees, and stale or unknown receipts.

## Source layout

```text
cmd/dtw/                    Rust MCP server, CUA SDK adapter, installer
packaging/plugin/           Agent Plugin manifests and one compact Skill
scripts/build-plugin.*      pinned-source package builds for macOS/Windows
tests/acceptance/           isolated protocol and real GUI acceptance harnesses
docs/                      architecture and acceptance boundaries
```

## License

The current DTW implementation is [MIT licensed](LICENSE). The packaged upstream CUA SDK and private worker are pinned to `ac967b9006aafa10870aadb5a645b038cba9721a`; their MIT notice and [third-party attributions](THIRD_PARTY_NOTICES.md) are preserved in the package. Optional CUA perception and Spaces components are not included. Removed historical DTW revisions retain their original notices, as explained in [NOTICE](NOTICE). See the [rc.4 release gate](docs/rc4-release.md) for dependency attribution and known upstream Windows exclusions.
