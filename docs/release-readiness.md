# Release readiness

The current prerelease is `v0.1.0-rc.8`. Each platform's user-facing artifact is **one executable**: `dtw-darwin-arm64` or `dtw-windows-amd64.exe`, accompanied by its `.sha256` checksum and build-evidence JSON. The CI package directory is an intermediate validation artifact; users do not download a DMG or a Plugin ZIP.

The macOS release gate builds the private worker, signs it with Developer ID and hardened runtime, embeds those signed bytes in DTW, signs the final DTW executable, and submits both final Mach-O files in one ZIP to Apple's notary service. The ZIP is only a submission container. The gate then exports a Plugin from the standalone CLI, byte-compares the extracted worker with the signed worker, verifies both signatures, and checks each executable's online notarization ticket with `codesign --check-notarization`. Apple's [notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow) says tickets for standalone executables cannot be stapled directly. Apple recommends [a `codesign` notarization check for other code](https://developer.apple.com/forums/topics/code-signing-topic?sortBy=oldest); `spctl --type exec` can reject a valid command-line tool as “not an app.” Fresh-machine launch and permission acceptance remain a separate release check.

Windows builds a standalone `.exe`, exports the Plugin from an isolated copy, and checks its stdio MCP contract. The candidate is unsigned. Windows native GUI behavior remains unpassed until tested on a real Windows machine.

## Acceptance scope

- The release's `BUILD-*.json` and `.sha256` files identify its exact source tag, pinned CUA revision, platform, and executable hashes.
- The macOS candidate workflow requires Developer ID signatures, hardened runtime, accepted notarization, an exact extracted-worker match, online ticket checks, and a complete standalone Plugin export with licensing inventory. The published executable must also pass an independent download and signature check.
- A fresh Antigravity CLI session using the same source code and a local signed single-file candidate discovered the Skill and MCP server, then completed isolated Chrome, Obsidian, and WPS tasks with independent readback; see [acceptance](acceptance.md). A fresh Mac download, Gatekeeper launch, and desktop permission flow remain separate acceptance.
- Claude CLI installation and update were tested in an isolated project. Fresh-session Claude Skill and MCP use, Caelis Core loading, and Caelis Bot bundle integration remain separate host acceptance.
- The Windows candidate workflow checks the package and headless MCP contract. Real Windows GUI behavior for this prerelease remains unpassed.

## Known runtime limits

- macOS Accessibility and Screen Recording authorization belongs to the actual launching Agent or terminal app.
- On macOS with multiple displays, the pinned CUA worker draws its virtual cursor in a single main-display overlay. A target window on a secondary display can receive an AX action while the cursor is drawn on the other display or clipped. Do not use the cursor's visible position as proof of the target; secondary-display pixel input requires a live retest after an upstream overlay fix.
- On Windows, an x64 Visual C++ runtime may be needed; elevated applications still follow Windows integrity restrictions.
- Some DPI-unaware Windows windows at 125% scaling can expose a mismatch between screenshot pixels and UIA geometry. Inspect the target again before pixel input.
- Modal file dialogs can cause uncertain completion or focus changes. Reconcile the original execution receipt and visible UI before another action.
- Foreground actions share focus across Agents. Check the exact target window before typing or using global shortcuts.

See [Windows acceptance evidence](windows-acceptance.md), [acceptance matrix](acceptance.md), and [installation](install.md).
