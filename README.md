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
the installer workflow and host guides.

## Try it

Open the application you want to use, then ask your Agent:

> Use Desktop World to inspect the open windows and show me a screenshot of Chrome.

> Use Desktop World to update this document, save it under a new name, then reopen
> it and check the changes.

The MCP interface is `apps` plus app-scoped `exec`; the bundled Skill documents
the nine app script methods. DTW owns window routing and observation state.
The Agent follows look → input → look, with semantic targets and pixel fallback.
[Working protocol plan](docs/agent-protocol-convergence-plan.md) records the current
implementation and acceptance boundaries. The bundled Skill teaches the Agent the available operations. Your Agent handles
approvals. On macOS, relevant operations require Accessibility and Screen Recording
permissions. Keep the desktop unlocked. When foreground interaction is needed,
DTW brings the target window forward and leaves it there; you can switch away
when you choose.

## Current availability

**macOS arm64** and **Windows x64** are the supported packages. Windows testing
covers Windows 11, ordinary user access and a primary display at 125% scaling.
Linux is experimental. Lock-screen use and remote control are not supported.

[Installation](install.md) · [Capabilities and limits](docs/product-matrix.md) ·
[MIT license](LICENSE)
