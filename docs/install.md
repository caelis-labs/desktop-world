# Install Desktop World

Download a platform package from the [rc.4 release](https://github.com/caelis-labs/desktop-world/releases/tag/v0.1.0-rc.4), extract it to a permanent location, and keep the entire folder. You do not need to clone or compile the repository.

## Agent Plugin clients

The extracted directory is an Agent Plugin 1.0 package with `plugin.json`, `mcp.json`, and `skills/`. Load that directory using your client's local-plugin flow. A client-specific plugin format may require an adapter; support for MCP alone does not imply support for Agent Plugin manifests.

If you need a separate copy for a plugin manager, run from the extracted folder:

```sh
./bin/dtw install plugin --to "/absolute/path/to/new-plugin-directory"
```

On Windows, use `.\bin\dtw.exe` instead of `./bin/dtw`. The destination must not already exist.

## Antigravity

Run the command in the [quick start](../README.md#2-connect-your-agent). It creates `<project>/.agents/plugins/desktop-world` with Antigravity's manifest, MCP configuration, and the Skill. Open that project in Antigravity and reload or start a fresh session. It does not modify global Agent configuration.

## Skills + MCP clients

For Codex or another client without a compatible local-plugin loader, configure **both** components:

1. Add a stdio MCP server launching the absolute path to the extracted `bin/dtw` (`bin/dtw.exe` on Windows). No arguments are needed.
2. Copy the complete `skills/desktop-world` directory into the client's documented Skills location, or register that directory if it supports external Skill paths.
3. Reload the client and confirm it exposes the Skill and one tool, `exec`.

For clients using JSON MCP settings, merge this entry with existing settings. Do not replace other servers:

```json
{
  "mcpServers": {
    "desktop-world": {
      "command": "/absolute/path/to/desktop-world/bin/dtw"
    }
  }
}
```

On Windows, a JSON command path looks like `C:\\Tools\\desktop-world\\bin\\dtw.exe`.

Codex CLI can register the MCP server with:

```sh
codex mcp add desktop-world -- "/absolute/path/to/desktop-world/bin/dtw"
```

Install the complete companion Skill in your Codex user Skills directory, `~/.agents/skills/desktop-world`, and start a fresh session. If that directory already exists, use the upgrade procedure below. rc.4 does **not** include the former `dtw install codex`, `install claude`, `install gemini`, or `install cursor` commands.

## Permissions

On macOS, the responsible host app needs Accessibility and Screen Recording. When running a CLI Agent in a terminal, the responsible app may be the terminal. Grant access to the application macOS identifies, then fully quit and restart it. Do not install a second CuaDriver App to work around permission attribution.

Windows needs a compatible x64 Visual C++ Runtime. Input into elevated applications remains subject to Windows integrity rules; installing DTW does not grant elevated access.

## Upgrading

Stop the Agent's DTW connection before changing its files. Download and unpack the new **complete** release into a new permanent directory. For a direct MCP registration, update the executable path; replace the companion Skill as a whole. For a plugin-manager installation, use that manager's replacement flow.

The Antigravity and generic copy installers refuse existing destinations. Back up the old plugin directory outside the Agent's discovery directory, then install into the now-absent destination. Reload the Agent and check the new version before removing the backup. Avoid leaving two copies discoverable at once.

Do not mix a new `dtw` executable with an old worker or replace the packaged worker with an unrelated upstream CUA release.

## First use

See the [read-only request and test task](../README.md#3-grant-access-and-try-a-task). Let the Agent read the Skill; users do not need to learn the script API.
