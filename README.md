# Desktop World

Desktop World (DTW) lets an Agent inspect and operate one authorized desktop app through a versioned MCP `exec` tool. DTW owns app identity, observation bindings, execution budgets and action receipts; the Harness owns user approval.

This repository contains the rewrite only. The former CUA SDK/worker runtime, unrestricted script facade, plugin exporter and Agent installers have been removed. The rewrite has no release package or automatic installation path yet. Existing [releases](https://github.com/caelis-labs/desktop-world/releases) are historical products and do not install this implementation.

## Development entry

Build with Rust 1.98.0; native macOS builds also require Xcode and its Swift toolchain:

```sh
cargo +1.98.0 build --manifest-path cmd/dtw/Cargo.toml --locked --bin dtw
cmd/dtw/target/debug/dtw --help
cmd/dtw/target/debug/dtw app catalog
```

If `CARGO_TARGET_DIR` is set, use the executable from that directory instead. Bare `dtw` and `dtw --help` display help; they do not start an MCP session.

`app catalog` discovers running app instances. Discovery does not authorize access to their windows. After approval, the trusted Host starts a context with its own grant file:

```sh
/absolute/path/to/dtw app serve --grant /absolute/host-grant.json
```

The Host grants one exact app instance and selected capabilities. Agent tool arguments cannot create or extend that grant. See [Host integration](docs/embedding.md) and the [app protocol](docs/app-protocol-v1.md).

## Current capability scope

The macOS rewrite completes the C5 core: app authority and identity, typed window inventory, AX observation and advertised AXPress, precise-window screenshots and pixel clicks, bounded app scripts, grant expiry and input cleanup. The five script methods are `app.windows`, `app.observe`, `app.capture`, `app.press` and `app.click`. `exec.describe` publishes their schemas and reference rules; [TypeScript declarations](docs/app-script-api.d.ts) describe the same API.

The [C5 acceptance report](docs/c5-completion-acceptance.md) records real GUI, TextEdit window/Space boundaries, and local Harness results. Inventory distinguishes an empty supported scope from unproven coverage. Windows native support, text/keyboard operations, Browser/DOM control and new distribution packages remain future work. A failed or uncertain action is never automatically replayed. C6 can extend capability groups on this baseline; the [product matrix](docs/product-matrix.md) records the verified scope.

Caelis has a local, unpublished Host integration with actual approval and GUI evidence. It is not an installed Caelis/Bot feature or a general Agent plugin release. Hosts must validate their own startup, permissions and lifecycle boundary.

## Development and release

[Build instructions](docs/building.md) separate lightweight checks from product builds and real GUI acceptance. [Implementation checkpoints](docs/implementation-checkpoints.md) record evidence and next steps. CI does not establish complete application or release readiness.

The next release will contain only the usable rewrite. Its package and installation contract will be defined after selecting and validating supported workflows; see [releasing](docs/releasing.md) and [installation status](docs/install.md).

DTW is [MIT licensed](LICENSE). Adapted source and dependencies retain their own terms and attribution in [third-party notices](THIRD_PARTY_NOTICES.md).
