# Desktop World

**Let your Agent use your desktop apps.** Desktop World (DTW) connects Caelis,
DeepSeek Harness (DSH), and agy to applications such as Chrome, WPS Office,
and Obsidian—so you can describe a task and let the Agent operate the interface.

## What can it do?

- Find open apps and windows, inspect controls, and take window screenshots.
- Click, type, use keyboard shortcuts, scroll, and edit text.
- Work with forms, file dialogs and menus, then read back the result.
- Work across several apps within the same conversation.

## Install with your Agent

Copy this prompt into **Caelis, DSH, or agy**:

```text
Install Desktop World for this Agent using:
https://raw.githubusercontent.com/caelis-labs/desktop-world/main/install.md
Use the guide's direct route for my Agent and latest stable OS package. Install
its desktop tools and bundled Skill as one plugin, preserving existing settings.
Verify open app windows through MCP. If this conversation needs a fresh tool
context, use a normal read-only verifier subagent or report the required next step.
```

Download the **latest stable** package from [Releases](https://github.com/caelis-labs/desktop-world/releases/latest).
You do not need a development toolchain to run the package. See [installation details](install.md) for
manual setup and upgrades.

## Try it

Open the application you want to use, then ask your Agent:

> Use Desktop World to inspect the open windows and show me a screenshot of Chrome.

> Use Desktop World to update this document, save it under a new name, then reopen
> it and check the changes.

The bundled Skill teaches the Agent the available operations. Your Agent handles
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
