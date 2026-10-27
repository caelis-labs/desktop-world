# Building Desktop World from source

The next release targets the rewrite only. Its current scope and remaining work
are in the [product matrix](product-matrix.md) and
[implementation checkpoints](implementation-checkpoints.md).

## Fast development checks

With Rust 1.98.0 (including rustfmt) and Python 3.11+:

```sh
sh scripts/check-source.sh
```

This runs version/format checks, std-only Core/IPC tests and Python acceptance
negative controls. It needs no CUA checkout, Node, Cargo dependency build, package
or desktop. macOS contributors with Xcode can also run the pure Swift controls:

```sh
python3 tests/ci/run_native_inventory_tests.py
```

The macOS CI job runs this separately; non-macOS local runs explicitly skip it.

## Current rewrite development build

For changes to the protocol, script engine or Rust native bridge, build and test
the relevant feature locally. macOS native builds also require Xcode/Swift:

```sh
cargo +1.98.0 test --manifest-path cmd/dtw/Cargo.toml --locked --features app-protocol --lib
cargo +1.98.0 build --manifest-path cmd/dtw/Cargo.toml --locked --features app-protocol --bin dtw
```

The current `dtw app` entry uses the rewritten runtime. The default legacy entry
and unconditional CUA SDK dependency still remain in the source tree, so these
Cargo commands can still compile upstream dependencies. Removing that coupling
is the next separate cleanup checkpoint; lightweight CI deliberately does not
run these full builds. Use a persistent `CARGO_TARGET_DIR` for local iteration.

Feature tests do not replace the [app protocol](app-protocol-acceptance.md),
[app script](app-script-acceptance.md), or
[window inventory](window-inventory-acceptance.md) acceptance steps.
Run the relevant real GUI checks when changing native behavior, on an owned test
desktop and with independent effect readback.

## Historical package scripts

`scripts/build-plugin.sh` and `scripts/build-plugin.ps1` still build the old
CUA-based packages. They are pending removal/replacement and are no longer CI or
next-release inputs. The historical source and packaging procedure is retained
in Git history at the #57 merge, `882f9f1eb76e2303e2bdc0370d73984e0196e876`.
Do not use those old packages as rewritten-product acceptance evidence.
See [releasing](releasing.md) for the new release boundary.
