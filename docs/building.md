# Building Desktop World from source

The source tree contains the rewrite only. Its supported slices and remaining work
are recorded in the [product matrix](product-matrix.md) and
[implementation checkpoints](implementation-checkpoints.md).

## Fast development checks

With Rust 1.98.0 (including rustfmt) and Python 3.11+:

```sh
sh scripts/check-source.sh
```

This checks the Cargo/lockfile/version.txt versions, formatting, std-only Core/IPC
and Python acceptance negative controls. It needs no CUA checkout, Node, Cargo
dependency build, package or desktop. macOS contributors with Xcode can also run
the pure Swift inventory controls:

```sh
python3 tests/ci/run_native_inventory_tests.py
```

The macOS CI job runs this separately; non-macOS local runs explicitly skip it.
These controls do not compile the complete protocol, script engine or Rust native
bridge. Relevant changes also require the local build/tests below.

## Rewrite development build

No feature flag selects the rewrite. Build and test the normal package:

```sh
cargo +1.98.0 test --manifest-path cmd/dtw/Cargo.toml --locked --lib
cargo +1.98.0 build --manifest-path cmd/dtw/Cargo.toml --locked --bin dtw
cmd/dtw/target/debug/dtw --help
```

Cargo uses the locked DTW dependencies, including rquickjs; it does not fetch or
build the CUA SDK or upstream worker. Use a persistent `CARGO_TARGET_DIR` for local
iteration and adjust the executable path above when setting it. Native macOS
builds require Xcode/Swift. Other platforms do not yet have a native execution
backend; a successful build does not establish desktop support.

The public commands are `dtw app catalog` and `dtw app serve --grant /absolute/host-grant.json`.
The Host prepares the grant after approval; see [Host integration](embedding.md).
Bare `dtw` and `--help` display help instead of starting an old MCP executor.

Library tests do not replace the [app protocol](app-protocol-acceptance.md),
[app script](app-script-acceptance.md), or
[window inventory](window-inventory-acceptance.md) acceptance steps.
Run relevant GUI checks when changing native behavior, on an owned test desktop
and with independent effect readback. Previous evidence applies to its recorded
source and binary, not automatically to a newly built executable.

## Distribution status

There is no rewritten release package, plugin exporter, installer, signing or
candidate workflow yet. The old CUA package scripts and plugin payload have been
removed. Their historical procedure remains in Git at
[`c553486`](https://github.com/caelis-labs/desktop-world/tree/c55348610071e2b26e10ce490744065b0cdd08b7).
Do not use those old packages as rewrite acceptance evidence.
See [releasing](releasing.md) for the remaining release work.
