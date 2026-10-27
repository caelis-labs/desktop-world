# Installation status

The rewrite is currently a source development build. No rewritten downloadable
package, Agent installer or plugin export is provided. `dtw install` and
`dtw uninstall` are not commands in this implementation. Historical release
assets use the former runtime and do not install the rewrite.

For local development, follow [building](building.md). The public entry points are:

```sh
/absolute/path/to/dtw --help
/absolute/path/to/dtw app catalog
/absolute/path/to/dtw app serve --grant /absolute/host-grant.json
```

Bare `dtw` displays help. `app serve` starts the single-app MCP server only with a
valid grant supplied by the trusted Host after approval; it does not register
itself in an Agent or manufacture user authorization. See [Host integration](embedding.md)
for startup, grant and shutdown responsibilities.

## Platforms and permissions

Native execution is currently implemented for macOS with the limited scope in
the [product matrix](product-matrix.md). Windows native support is not implemented.
The OS permissions needed for the chosen operation, such as Accessibility and
Screen Recording on macOS, are separate from the Host grant. Validate permission
behavior using the actual launching Host and binary. Development fixture evidence
does not establish clean-machine installation, signing, notarization or TCC
behavior for a future package.

The actual Caelis Host integration has local acceptance evidence but remains
unpublished. Current Caelis/Bot releases and generic Agent plugin installers must
not be assumed to provide it.

## Future installation and existing versions

The new package, upgrade and removal contract is still to be defined and validated;
see [releasing](releasing.md). No command in this source tree migrates or removes
an existing legacy installation. Existing users should use the instructions that
accompanied their installed version.

Historical installation instructions are retained in
[`c553486`](https://github.com/caelis-labs/desktop-world/blob/c55348610071e2b26e10ce490744065b0cdd08b7/docs/install.md)
and in release tags. They are not instructions for the current executable.
