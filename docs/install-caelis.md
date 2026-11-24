# Install DTW in Caelis

Start with the [common setup entry](../install.md). Use this guide only for
host-specific registration or recovery steps. Use the receipt's
`install.package_path`; do not import or bind the package manually. Finish with
`setup confirm caelis caelis <package_id>` using the exact receipt ID after live
verification.

## Install the native plugin

Each complete package includes Agent Plugins 1.0 metadata and bundles both the
`dtw` MCP server and the `dtw` Skill (`skills/dtw`), exposed under plugin ID
`desktop-world` and Skill identity `desktop-world:dtw`. Caelis supports this
layout directly through its
[plugin manager](https://github.com/caelis-labs/caelis/blob/main/docs/plugins.md):

```sh
caelis plugin install /absolute/managed/package --json
caelis plugin status desktop-world --json
```

Or enter `/plugin install /absolute/managed/package` in the Caelis TUI. Start
a new Session to load the MCP and Skill snapshot, then complete the
[first-execution check](../install.md#check-the-installation). Do not
separately register a second copy of the MCP server or standalone Skill.

**Compatibility and approval:** use a Caelis Core build that supports Agent
Plugins 1.0 identity (`desktop-world`) and `pairedSkill` routing between MCP
server `dtw` and Skill `desktop-world:dtw`. Core requests approval on first
use of each `exec.app` in a Session and reuses that approval within that
Session; a new app or new Session requires its own approval.

## Upgrade or remove

Finish active operations before changing the registration. Caelis binds a local
plugin registration to its canonical package directory; to switch packages:

```sh
caelis plugin remove desktop-world --json
caelis plugin install /absolute/managed/new-package --json
```

Preserve unrelated plugins, start a new Session, verify the live `dtw`
connection and bundled Skill, and finish with `setup confirm caelis caelis
<package_id>`. To remove DTW, remove the plugin through Caelis first, then run
`dtw package unbind caelis` with the managed executable. Managed cleanup and
rollback follow [the package lifecycle](package-management.md).

For macOS permissions, follow
[the common installation check](../install.md#check-the-installation).
