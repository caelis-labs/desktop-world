# Desktop World

Inspect and operate running macOS applications through standard MCP tools and an app-scoped JavaScript API.

This is a local development plugin staged from the rewrite's source build. Load its directory with `claude --plugin-dir /absolute/path/to/plugin`. The plugin registers its bundled `bin/dtw mcp` server and the Desktop World Skill. No separate server registration or custom Host adapter is required.

Start by calling `apps`, then pass a returned application ID such as `com.apple.TextEdit` to `exec` together with a unique request ID and a small awaited script. Supported methods are windows, observe, capture, press and click. Verify application effects independently after actions. `result` reads an original receipt without repeating an operation.

The Harness owns tool approval. Native operations need the corresponding macOS Accessibility or Screen Recording permissions. Current support is limited to the documented standard-window subset; keyboard, text entry, browser DOM and Windows are not implemented. Switching apps closes the old execution context, so state and refs must be established again when returning.

This local stage is not a signed or notarized release package. Build identity and retained source/license notices are included alongside the executable; public distribution requires separate release validation.
