# Release readiness

The first public release is pending. The current candidate is `v0.1.0-rc.5`. Each platform's user-facing artifact is **one executable**: `dtw-darwin-arm64` or `dtw-windows-amd64.exe`, accompanied by its `.sha256` checksum and build-evidence JSON. The CI package directory is an intermediate validation artifact; users do not download a DMG or a Plugin ZIP.

The macOS release gate builds the private worker, signs it with Developer ID and hardened runtime, embeds those signed bytes in DTW, signs the final DTW executable, and submits both final Mach-O files in one ZIP to Apple's notary service. The ZIP is only a submission container. The gate then exports a Plugin from the standalone CLI, byte-compares the extracted worker with the signed worker, verifies both signatures, and asks Gatekeeper to assess each executable. [Apple's notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow) says tickets for standalone executables are available online but cannot be stapled directly to the executable. Fresh-machine launch and permission acceptance remain a separate release check.

Windows builds a standalone `.exe`, exports the Plugin from an isolated copy, and checks its stdio MCP contract. The candidate is unsigned. Windows native GUI behavior remains unpassed until tested on a real Windows machine.

## Required evidence before publication

- Exact source tag, pinned CUA revision, platform and SHA-256 for the public executable.
- macOS Developer ID signature, hardened runtime, accepted notarization, exact extracted-worker match, and fresh-machine Gatekeeper launch.
- A standalone-file export of the complete Plugin, including `plugin.json`, `mcp.json`, Skill, worker, and licensing inventory.
- Antigravity installation and fresh-session Skill plus MCP discovery. Claude installation and fresh-session loading are separate compatibility acceptance.
- A Caelis Core standard Plugin load from the exported package. Caelis Bot application-bundle integration is a later host acceptance, not implied by the package test.
- Real Windows GUI acceptance before claiming native Windows desktop behavior beyond build and headless protocol validation.

## Known runtime limits

- macOS Accessibility and Screen Recording authorization belongs to the actual launching Agent or terminal app.
- On Windows, an x64 Visual C++ runtime may be needed; elevated applications still follow Windows integrity restrictions.
- Some DPI-unaware Windows windows at 125% scaling can expose a mismatch between screenshot pixels and UIA geometry. Inspect the target again before pixel input.
- Modal file dialogs can cause uncertain completion or focus changes. Reconcile the original execution receipt and visible UI before another action.
- Foreground actions share focus across Agents. Check the exact target window before typing or using global shortcuts.

See [Windows acceptance evidence](windows-acceptance.md), [acceptance matrix](acceptance.md), and [installation](install.md).
