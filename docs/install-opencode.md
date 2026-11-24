# Install DTW in OpenCode

Start with the [common setup entry](../install.md). Use this guide only for its host-specific registration or recovery step. For a prepared package, use the returned `install.package_path`; do not import or bind it again. Finish with `setup confirm` and the exact receipt ID after live verification. The low-level `package activate` examples below are for older managers.

For the older manual route, [download, verify and import the DTW package](../install.md#select-and-extract-a-package).
Use import's immutable `package_path` for the paths below, not the extraction
directory. Bind a distinct host/profile pin (for example `opencode`) before
changing registration; [activate it](package-management.md) only after checking
the new live MCP connection, matching bundled Skill and GUI observation.
All `/absolute/managed/package` paths below are placeholders for that returned
package path.

## Configure the installed host

The verified OpenCode **2.3.0** installation uses the global configuration file
`~/.config/opencode/opencode.jsonc`, with `mcp.servers` and a `skills` path array.
Merge these entries into the existing JSONC document:

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

This is a configuration fragment, not a replacement file. Make a private backup
and edit only DTW's entries, retaining comments, model/provider settings,
credentials, permissions, other MCP servers and existing Skill paths. Preserve
existing JSONC formatting where possible; do not rewrite the file through a
plain JSON serializer or print its private contents. Both paths must refer to
the same immutable package. Do not copy the Skill into another discovery path
or add a second DTW server.

**Version and fork boundary:** this configuration was verified on 2.3.0. Older
versions, including v1, can use different MCP and Skill configuration shapes.
Inspect the actual installed host's schema and configuration scope before editing.
A derived Agent may use different paths, schemas or extension capabilities;
choose its supported integration instead of assuming either OpenCode layout.
OpenCode's JavaScript `plugin` array is for OpenCode JS plugins, not a list of
DTW plugin manifest directories. Do not put the DTW package there as a substitute
for its MCP and Skill configuration.

The example uses BUILD `package_store_schema: 2` names: MCP server `dtw` and
Skill `dtw` at `skills/dtw`. An older package retains the names recorded in its
own BUILD and manifests; do not rename its contents to fit this example.

## Reload and verify

For the tested 2.3.0 host, the supported configuration reload succeeded:

```sh
opencode reload
opencode mcp list
```

The observed results were `Configuration reloaded` and `dtw connected`. Start a
new session after the reload so that the session loads the new tools and Skill.
Confirm that live `apps` and `exec` tools are available, that Skill `dtw` comes
from the same managed package, and that the actual server process uses that
package's executable/version. Complete the
[first-execution check](../install.md#check-the-installation), including a
read-only observation and screenshot of an authorized test window. On macOS,
follow the common Accessibility and Screen Recording permission guidance.
A connection listing alone does not establish GUI readiness.

Only after those checks pass, use the imported executable to activate this
host/profile's pending pin:

```sh
"/absolute/managed/package/bin/dtw" package activate opencode
```

**Evidence scope:** OpenCode 2.3.0 was installed with the verified official
v0.5.0 macOS arm64 package. JSONC preservation, reload and connected status were
checked, and subsequent fresh-session verification completed GUI interaction.
The [acceptance record](https://github.com/caelis-labs/desktop-world/blob/main/docs/agent-install-acceptance-2026-11-17.md) includes live
Skill loading, Unicode typing, native button effects and screenshots. These
results cover this host/package/platform combination; other platforms and
derived Agents require their own verification.

## Upgrade or remove

Finish active DTW operations, reuse this host/profile pin, and replace only its
DTW executable and Skill paths with the newly imported complete package. Reload,
start a fresh session, verify the live connection, bundled Skill and GUI, then
activate. Preserve unrelated configuration and other hosts' pins. Do not modify
the immutable old or new package.

For removal, remove only this host's DTW MCP entry and matching Skill path,
apply its reload boundary, and then run `dtw package unbind opencode` with the
managed executable. Follow the [package lifecycle](package-management.md) for
owned cleanup and rollback; do not remove a package still referenced by another
host or live consumer.
