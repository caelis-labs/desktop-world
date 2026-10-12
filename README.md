# Desktop World

**Give your AI Agent the ability to operate desktop apps.** Desktop World (DTW) lets an Agent inspect windows, click controls, type, and take screenshots through one MCP tool. A companion Skill teaches it how to use those abilities.

Ask your Agent to fill in a form, operate a browser, or work with a desktop document. DTW uses CUA Driver for native computer use, preferring background actions where supported. You do not need to write scripts, install Node, or install a separate CuaDriver App.

**[Download the plugin](https://github.com/caelis-labs/desktop-world/releases/tag/v0.1.0-rc.4)** · [Installation guide](docs/install.md) · [Agent Skill](packaging/plugin/skills/desktop-world/SKILL.md)

## Quick start

### 1. Download and unpack

The current release is **v0.1.0-rc.4**, a preview release. Download the complete package for your computer:

| Your computer | Download |
| --- | --- |
| macOS, Apple Silicon | [macOS arm64 plugin](https://github.com/caelis-labs/desktop-world/releases/download/v0.1.0-rc.4/desktop-world-v0.1.0-rc.4-darwin-arm64.tar.gz) |
| Windows, Intel/AMD x64 | [Windows x64 plugin](https://github.com/caelis-labs/desktop-world/releases/download/v0.1.0-rc.4/desktop-world-v0.1.0-rc.4-windows-amd64.zip) |

Extract it to a permanent folder. Keep the **entire `desktop-world` folder**, including `bin`, `libexec`, `skills`, and `LICENSING`; moving only the executable will not work. Source archives are for developers, not installation. Checksums are available on the release page.

### 2. Connect your Agent

Choose the route your Agent supports:

- **Agent Plugin 1.0 client:** use its local-plugin installation flow to load the extracted `desktop-world` directory, which contains `plugin.json`. The exact UI or command belongs to your Agent.
- **Antigravity:** use the bundled installer shown below. It installs both MCP and the Skill in your project.
- **Other Skills + MCP clients, including Codex:** register the bundled executable as a stdio MCP server and install the companion Skill. See the [copyable setup instructions](docs/install.md#skills--mcp-clients).

For **Antigravity**, open a terminal in the extracted `desktop-world` folder and replace the project path with an existing project:

macOS:

```sh
./bin/dtw install antigravity --project "/absolute/path/to/your-project"
```

Windows PowerShell:

```powershell
.\bin\dtw.exe install antigravity --project "C:\path\to\your-project"
```

Reload your Agent or open a new session. The installer uses a new `.agents/plugins/desktop-world` directory; it will not overwrite an existing installation. [Upgrading and manual setup →](docs/install.md)

### 3. Grant access and try a task

On macOS, grant **Accessibility** and **Screen Recording** to the Agent app that launches DTW, then fully restart that app. No separate CuaDriver App is required. On Windows, install the [x64 Visual C++ Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist) if it is missing.

Start with a read-only request:

> Use Desktop World to inspect my browser window and describe the available controls. Do not click or type yet.

Then try an action on a temporary document or test page:

> Use Desktop World to type “Hello from DTW” into my open test document. Verify that the text appears, and leave the document open.

Your Agent should discover the **Desktop World Skill** and one MCP tool, **`exec`**. The Skill handles scripting, screenshots, and checking the result; you can give tasks in natural language.

## What to expect

- **One tool, scriptable actions:** the Agent can batch desktop operations instead of calling a separate MCP tool for every click.
- **Small results:** screenshots and large UI trees are available when needed, rather than always filling the conversation.
- **Background where supported:** some operations still require the foreground. Multiple Agents share the same desktop and can affect focus.
- **Native runtime:** installed packages do not need Node, Rust, Python, or build tools.

rc.4 is a preview, not a guarantee that every application works. Known Windows limitations include coordinate mismatch for some DPI-unaware windows at 125% scaling and uncertain completion around modal file dialogs. The macOS package is ad hoc signed and not notarized; Windows binaries are unsigned. See [release limitations and acceptance](docs/rc4-release.md) before relying on DTW for unattended work.

## Troubleshooting

| Problem | What to check |
| --- | --- |
| No Desktop World tool or Skill | Reload the Agent; confirm both MCP and the Skill are installed. |
| Permission denied on macOS | Authorize the actual launching Agent app, then fully restart it. |
| Missing worker or library | Restore the complete release folder; do not copy only `bin/dtw`. |
| Missing `VCRUNTIME140.dll` on Windows | Install the x64 Visual C++ Redistributable. |
| Installer says the destination exists | Follow the [upgrade instructions](docs/install.md#upgrading); it deliberately does not overwrite existing files. |
| An operation times out | Ask the Agent to inspect the UI and check the original execution result before retrying. The action may already have happened. |

## For contributors and application integrators

[Build from source](docs/building.md) · [Architecture](docs/architecture.md) · [Acceptance records](docs/acceptance.md) · [Script recipes](packaging/plugin/skills/desktop-world/RECIPES.md) · [API reference](packaging/plugin/skills/desktop-world/api.d.ts)

DTW supplies the plugin, a single execution entry, bounded results, and execution receipts. A pinned CUA native worker supplies desktop operations. Applications can bundle the complete plugin and own the operating-system permissions.

## License

DTW is [MIT licensed](LICENSE). CUA and its dependencies retain their own licenses. Complete notices and required source bundles ship in `LICENSING`; keep that directory when redistributing. Optional CUA perception and Spaces components are not included. See [third-party notices](THIRD_PARTY_NOTICES.md).
