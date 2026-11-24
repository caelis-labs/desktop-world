# Install DTW in OpenCode

Start with the [common setup entry](../install.md). Use this guide only for
host-specific configuration, reload, or recovery steps. Use the receipt's
`install.package_path` for registration.

## Configure the installed host

The OpenCode **2.3.0** integration uses `~/.config/opencode/opencode.jsonc`,
merging MCP server `dtw` and Skill path `skills/dtw` from the same managed
package:

```jsonc
{
  "mcp": {
    "servers": {
      "dtw": {
        "type": "local",
        "command": ["/absolute/managed/package/bin/dtw", "mcp"]
      }
    }
  },
  "skills": ["/absolute/managed/package/skills/dtw"]
}
```

Edit only DTW's entries after making a private backup, retaining comments,
model/provider settings, credentials, permissions, other MCP servers, and
existing Skill paths. Preserve existing JSONC formatting rather than
reformatting through a plain JSON serializer, and never print private file
contents. Do not copy the Skill into another discovery directory or add a
second DTW server.

**Version and fork boundary:** other OpenCode versions or derived Agents may use
different configuration paths or schemas; inspect the host's actual schema
before editing manually. OpenCode's JavaScript `plugin` array is for OpenCode JS
plugins, not DTW plugin manifest directories.

## Reload and verify

After `dtw setup` updates `~/.config/opencode/opencode.jsonc` (or after manual
editing), reload OpenCode and inspect its live connection:

```sh
opencode reload
opencode mcp list
```

Expect `Configuration reloaded` and `dtw connected`, then start a new session so
the new tools and Skill snapshot load cleanly. Confirm live `apps` and `exec`
tools, confirm Skill `dtw` resolves to the same managed package, and complete the
[first-execution check](../install.md#check-the-installation) including a
read-only observation and screenshot of an authorized test window. On macOS,
follow the common Accessibility and Screen Recording guidance.

After those live checks pass,
[confirm the setup receipt](../install.md#use-the-setup-receipt).

## Upgrade or remove

Finish active DTW operations and rerun setup for the same OpenCode profile.
For manual registration, update only DTW's command executable and `skills/dtw`
paths in `opencode.jsonc`. Reload, start a fresh session, verify live tools,
Skill, and GUI observation, then confirm the setup receipt.

For removal, remove only DTW's `mcp.servers.dtw` entry and matching `skills`
path and run `opencode reload`. After the connection exits,
[unbind the receipt's profile in its original store](package-management.md#remove-a-host-installation).
