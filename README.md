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
https://github.com/caelis-labs/desktop-world/blob/main/docs/install.md
Choose the rewrite package for my OS, verify it, and install its desktop tools
and bundled Skill through this Agent's standard extension points. Preserve my
existing configuration. Verify the connection by listing open app windows.
If no rewrite package is available, ask me for the candidate package rather
than installing an old release.
```

**v0.2.0 is currently an unpublished candidate.** Use the supplied rewrite package;
historical v0.1.x releases are a different edition. You do not need a development
toolchain to run the package. See [installation details](docs/install.md) for
manual setup and upgrades.

## Try it

Open the application you want to use, then ask your Agent:

> Use Desktop World to inspect the open windows and show me a screenshot of Chrome.

> Use Desktop World to update this document, save it under a new name, then reopen
> it and check the changes.

The bundled Skill teaches the Agent the available operations. Your Agent handles
approvals. On macOS, relevant operations require Accessibility and Screen Recording
permissions. Keep the desktop unlocked; some actions bring a window to the front.

## Current availability

**macOS arm64** has real-app testing; the final release package still needs
acceptance. **Windows x64** has a candidate for independent desktop acceptance.
Linux is experimental. Lock-screen use and remote control are not supported.

[Installation](docs/install.md) · [Capabilities and limits](docs/product-matrix.md) ·
[MIT license](LICENSE)
