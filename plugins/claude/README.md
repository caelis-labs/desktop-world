# Desktop World

Inspect and operate running macOS applications through standard MCP tools and an app-scoped JavaScript API.

This is a local development plugin staged from the rewrite's source build. Load its directory with `claude --plugin-dir /absolute/path/to/plugin`. The plugin registers its bundled `bin/dtw mcp` server and the Desktop World Skill. No separate server registration or custom Host adapter is required.

Start by calling `apps`, then pass a returned application ID such as `com.apple.TextEdit` to `exec` together with a small awaited script as `source`. These are its only two inputs; DTW generates execution identity internally. The compact `apps` index lists windows, observe, capture, press, click, focus, typeText, keypress, scroll and setValue. Ask `api` for only the unfamiliar methods’ schemas. Verify application effects independently after actions. `result` reads an original receipt without repeating an operation.

The Harness owns tool approval. Native operations need the corresponding macOS Accessibility or Screen Recording permissions. macOS 14.2+ has separate WindowServer surfaces and bounded AX semantics, plus common foreground input. Windows native source awaits independent desktop acceptance; Linux X11 is experimental. Browser DOM, independent background pointers and locked-session input remain deferred. Switching apps retains each app’s committed state and still-valid references. References can expire or be consumed by actions; rediscover them when needed.

This local stage is not a signed or notarized release package. Build identity and retained source/license notices are included alongside the executable; public distribution requires separate release validation.
