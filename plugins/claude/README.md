# Desktop World

Inspect and operate running desktop applications through standard MCP tools and an app-scoped JavaScript API.

This is a local development plugin staged from a source build. It includes Agent Plugins 1.0, Claude Code, Antigravity and DSH Cordis entries around one executable and the Desktop World Skill. Choose one route in each host; do not register both plugin and manual MCP/Skill copies. Follow [the installation guide](install.md) and the [Caelis](docs/install-caelis.md), [DSH](docs/install-dsh.md) or [Antigravity App/CLI](docs/install-agy.md) instructions. Claude Code can load this directory with `claude --plugin-dir /absolute/path/to/plugin`.

Start by calling `apps`, then pass a returned application ID such as `com.apple.TextEdit` to `exec` together with a small awaited script as `source`. These are its only two inputs; DTW generates execution identity internally. The compact `apps` index lists windows, observe, capture, press, click, focus, typeText, keypress, scroll and setValue. Ask `api` for only the unfamiliar methods’ schemas. Verify application effects independently after actions. `result` reads an original receipt without repeating an operation.

The Harness owns tool approval. macOS 14.2+ arm64 operations need the corresponding Accessibility or Screen Recording permissions. Windows x64 uses native UIA and input delivery, with foreground access managed by DTW when needed. This development stage still needs acceptance on its exact bytes; prior release acceptance does not certify a rebuilt binary. Browser DOM and locked-session input remain outside the runtime contract. Switching apps retains each app’s committed state and still-valid references. References can expire or be consumed by actions; rediscover them when needed.

This local stage is not a signed or notarized release package. Build identity and retained source/license notices are included alongside the executable; public distribution requires separate release validation.
