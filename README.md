# Desktop World

**Let your Agent use your desktop apps.** Desktop World (DTW) connects Caelis,
DeepSeek Harness (DSH), and agy to applications such as Chrome, WPS Office,
and Obsidian—so you can describe a task and let the Agent operate the interface.

## What can it do?

- Search running and installed apps; inspect compact UI changes and app screenshots.
- On macOS, start an installed app automatically and request normal quit.
- Click, type, use keyboard shortcuts, scroll, and edit text.
- Work with forms, file dialogs and menus, then read back the result.
- Work across several apps within the same conversation.

## Install with your Agent

Copy this prompt into **Caelis, DSH, or agy**:

```text
Install or update Desktop World for this Agent using:
https://raw.githubusercontent.com/caelis-labs/desktop-world/main/install.md
Complete the guide's Agent-managed workflow for the latest stable OS package:
verify bytes, switch the complete plugin/Skill/MCP registration, refresh the
connection, verify actual tools, and clean owned old packages automatically.
Preserve other Agents and settings. Do not ask me to choose versions or paths.
Stop at a required host restart; resume verification afterward without reinstalling.
```

Download the **latest stable** package from [Releases](https://github.com/caelis-labs/desktop-world/releases/latest).
Version PR merges automatically run CI, build and verify both platform packages,
and publish the complete release; see [the release process](docs/releasing.md).
You do not need a development toolchain to run the package. See [installation details](install.md) for
the installer workflow and host guides. Installation uses a host-approved
execution path outside the session workspace; denied access requires the host's
normal authorization process, never a sandbox-policy or ACL change. See
[DSH Windows compatibility](docs/install-dsh.md#windows-host-execution-and-storage).

Current source packages expose MCP server `dtw` and bundle Skill `dtw` at
`skills/dtw` (Caelis identity `desktop-world:dtw`). The plugin/install ID remains
`desktop-world`. Published schema 1 packages keep their original names and
bytes; follow the selected package's layout as described in [installation](install.md).

## Try it

Open the application you want to use, then ask your Agent:

> Use Desktop World to inspect the open windows and show me a screenshot of Chrome.

> Use Desktop World to update this document, save it under a new name, then reopen
> it and check the changes.

The default MCP interface is `apps` plus app-scoped `exec({app, source})`; the bundled Skill documents
the app script methods, including filtered observations and explicit text reading.
Opt in to flat app-operation tools with `dtw mcp --atomic`; their names and object
arguments match the script methods. Discover the connected tools before use. See
[MCP toolsets](docs/mcp-toolsets.md) for configuration and compatibility. DTW owns window routing and observation state.
The Agent observes when needed and batches known actions, using semantic targets
and pixel fallback. DTW prepares receivers and returns observations after input.
[Capabilities and limits](docs/product-matrix.md) describes current support and
acceptance boundaries. The bundled Skill teaches the Agent the available operations. Your Agent handles
approvals. On macOS, relevant operations require Accessibility and Screen Recording
permissions. Keep the desktop unlocked. When foreground interaction is needed,
DTW brings the verified target forward for the script and restores the previous
foreground when it can still prove ownership. Switching away stops further input
from the current script.

## Current availability

**macOS arm64** and **Windows x64** are the supported packages. Windows testing
covers Windows 11, ordinary user access and a primary display at 125% scaling.
Linux is experimental. Lock-screen use and remote control are not supported.

[Installation](install.md) · [Capabilities and limits](docs/product-matrix.md) ·
[Computer Use acceptance app](tests/fixtures/computer-use-app/README.md) · [MIT license](LICENSE)
