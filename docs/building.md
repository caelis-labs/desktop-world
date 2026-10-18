# Building Desktop World from source

For contributors and application packagers. To install the released executable, see the [user quick start](../README.md#get-started).

Building requires Rust 1.98.0, Python 3.11+ for dependency-notice generation, and access to the pinned [Cua Driver source](https://github.com/trycua/cua/tree/d74a9cab3fb2e80ab7f0cef47025ec828586ea75). macOS additionally needs Xcode with its Swift toolchain; Windows needs the MSVC Rust target, Visual Studio C++ build tools, a Windows SDK, and the matching **MSVC x64/x86 Spectre-mitigated libraries** (`Microsoft.VisualStudio.Component.VC.Runtimes.x86.x64.Spectre`). The pinned dependency `msvc_spectre_libs` rejects a build without that optional component. The platform build script verifies the CUA commit and builds the DTW CLI plus CUA's private worker. This POC branch evaluates CUA component release **0.35.4**, pinned to its exact tag commit. The app top2 extension uses the unmodified worker; see [the POC](../poc/associated-capture/README.md). Choose a new destination:

```sh
./scripts/build-plugin.sh /private/tmp/dtw-plugin
/private/tmp/dtw-plugin/bin/dtw --version
"/private/tmp/dtw-plugin/bin/dtw" install agy --project "/path/to/test-project"
```

On Windows, run PowerShell 7 and select a new directory:

```powershell
pwsh -File ./scripts/build-plugin.ps1 C:\Temp\dtw-plugin
C:\Temp\dtw-plugin\bin\dtw.exe --version
& C:\Temp\dtw-plugin\bin\dtw.exe install agy --project "C:\path\to\test-project"
```

To use an existing clean CUA checkout at the exact pinned commit, set `CUA_SOURCE_ROOT` to that path. The scripts read that checkout without editing it. Build output defaults to a temporary directory; `DTW_BUILD_DIR` selects another. CI uses a stable `DTW_CUA_SOURCE_DIR` and caches the Rust build and downloads across runs. For an offline build, set `DTW_CARGO_OFFLINE=1` with dependencies cached. A previously built exact-source worker can be supplied with both `CUA_DRIVER_BIN` and `CUA_DRIVER_BIN_SHA256` when rebuilding only the adapter. The build emits a standalone package at the destination and a slim Plugin at `<destination>-slim`. The latter runtime omits the embedded bundle. Both are release inputs; do not mix their adapter digests. The macOS script ad hoc signs local test files; the release workflow signs the worker before embedding and notarizes the final worker, standalone CLI and slim runtime. Windows candidates are unsigned.


See [architecture](architecture.md), [acceptance](acceptance.md), and [release readiness](release-readiness.md) for implementation and release validation.

The DTW application-observation extension statically compiles
`cmd/dtw/src/app_view/Scene.swift` with Xcode's Swift compiler. The resulting macOS candidate
requires macOS 14+; there is no Node runtime or additional helper installation.
After a SDK/worker upgrade, run the implicit-session contract tests and the live
application/point gates in `docs/rc9-acceptance.md` before promotion. The same executable also serves as
a short-lived native extension child; no additional distributed helper is needed.
