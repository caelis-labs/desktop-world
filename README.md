# Desktop World

**Let your Agent use your desktop apps.** Desktop World (DTW) connects Caelis,
DeepSeek Harness (DSH), agy, OpenCode, Pi and Grok Build to applications such as
Chrome, WPS Office and Obsidian—so you can describe a task and let the Agent
operate the interface.

## What can it do?

- Search running and installed apps; inspect compact UI changes and app screenshots.
- On macOS, start an installed app automatically and request normal quit.
- Click, type, use keyboard shortcuts, scroll, and edit text.
- Work with forms, file dialogs and menus, then read back the result.
- Work across several apps within the same conversation.

## Install with your Agent

Copy this prompt into **Caelis, DSH, agy, OpenCode, Pi or Grok Build**.
Pi requires a package containing its native adapter; see the
[Pi package availability](docs/install-pi.md#package-feature-gate) before installing.

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

The MCP interface exposes `apps` and app-scoped `exec({app, source})`. The bundled
Skill documents the [app methods](skills/dtw/API.md#methods). For example,
`await app.getByRole('button',{name:'Save',exact:true}).click()` resolves a unique
native control before sending input. Verify the save separately from the click receipt.
DTW owns window routing and observation state.
The Agent observes when needed and batches known actions, using semantic targets
and pixel fallback. DTW prepares receivers and returns observations after input.
[Capabilities and limits](docs/product-matrix.md) describes current support and
acceptance boundaries. The bundled Skill teaches the Agent the available operations. Your Agent handles
approvals. On macOS, relevant operations require Accessibility and Screen Recording
permissions. Keep the desktop unlocked. When foreground interaction is needed,
DTW prepares the verified target directly. macOS prefers working background
capabilities; Windows uses foreground interaction by default. Foreground, focus
and real pointer changes remain in place after the call. Switching away stops
further input from the current execution; held keys and buttons are released.

## Current availability

The release matrix includes **macOS arm64**, **Windows x64**, **Linux x86_64** and
**Linux arm64**. Linux packages target glibc 2.39 or later and require a local native
X11 session for desktop operations. Linux supports bounded AT-SPI semantics and
launch of eligible desktop entries. Wayland and XWayland are unsupported. See the
[platform matrix](docs/product-matrix.md) for capability and validation coverage.

[Installation](install.md) · [Capabilities and limits](docs/product-matrix.md) ·
[Computer Use acceptance app](tests/fixtures/computer-use-app/README.md) · [MIT license](LICENSE)
