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

In **Caelis, DSH, agy, OpenCode, Pi or Grok Build**, ask:

> Install or update Desktop World for this Agent using https://raw.githubusercontent.com/caelis-labs/desktop-world/main/install.md

The [installer guide](install.md) selects one official stable package for the current host/profile. No development toolchain, manual version selection or second MCP registration is needed. If the host cannot reload the plugin, save your work and restart that host once, then resume verification; do not reinstall. System permissions and host approvals still apply.

Source packages expose MCP server `dtw` and bundle Skill `dtw` at `skills/dtw`
(Caelis identity `desktop-world:dtw`), under plugin/install ID `desktop-world`.

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
