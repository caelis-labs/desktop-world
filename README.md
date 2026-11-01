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
https://github.com/caelis-labs/desktop-world/blob/main/install.md
Choose the v0.2.0 package for my OS, verify it, and install its desktop tools
and bundled Skill through this Agent's standard extension points. Preserve my
existing configuration. Verify the connection by listing open app windows.
Do not substitute historical v0.1.x packages.
```

Download the **v0.2.0** package from [Releases](https://github.com/caelis-labs/desktop-world/releases/tag/v0.2.0).
Historical v0.1.x releases are a different edition. You do not need a development
toolchain to run the package. See [installation details](install.md) for
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
