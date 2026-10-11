# Thin CUA adapter POC

Read [final convergence and acceptance](../../docs/cua-thin-final-poc.md), [design](../../docs/cua-kernel-poc.md), and [native capability map](../../docs/cua-native-capabilities.md) before treating this as a release. This directory is a local macOS prototype, not the current DTW product path.

`Cargo.toml` points to the local CUA SDK checkout. `build-package.sh` verifies the full CUA source commit `ac967b9006aafa10870aadb5a645b038cba9721a`, builds `dtw` and the upstream private worker, and writes a new package directory with one Skill, one MCP `exec` server, and MIT/MPL notices. For example:

```sh
DTW_CUA_TARGET_DIR=/private/tmp/dtw-cua-build \
  sh poc/cua-thin/build-package.sh /private/tmp/dtw-cua-package
python3 poc/cua-thin/probe.py /private/tmp/dtw-cua-package/bin/dtw
python3 poc/cua-thin/contract_probe.py /private/tmp/dtw-cua-package/bin/dtw
python3 poc/cua-thin/native_ops_probe.py /private/tmp/dtw-cua-package/bin/dtw
```

An already built private worker can be supplied with `CUA_DRIVER_BIN`; the script then requires `CUA_DRIVER_BIN_SHA256` and refuses a hash mismatch. The checked local package used that path because a fresh offline worker build lacked the cached `anstream 1.0.0` crate. The worker binary hash and source pin are recorded in the evidence document.

To make an isolated Antigravity project plugin, run `PACKAGE/bin/dtw install antigravity --project EXISTING_PROJECT_DIR`, then `agy plugin validate EXISTING_PROJECT_DIR/.agents/plugins/desktop-world`. It refuses to replace an existing path. `adapt_antigravity.py` is retained as an older test helper. The packaged `bin/dtw` also runs directly as an MCP stdio CLI; Node is not needed. Live fixture scripts launch temporary Chrome/Obsidian/WPS processes and require the actual host's macOS permissions. `evaluate_agent_obsidian_visual.py` uses the CLI installer and a fresh Antigravity session; its temporary one-tool permission rule is restored afterward. `coordinate_probe.py` checks actual screenshot-to-click mapping in an isolated Chrome process. `evaluate_agent_bilibili.py` requires an already opened exact user Chrome PID/window ID and explicit authorization for a real like/undo; it temporarily adds only the one-tool Antigravity allow rule and restores the original config bytes when unchanged by anyone else. These probes are not routine installer steps.

The packaged Skill links its small `api.d.ts` signature reference and `RECIPES.md` examples. Scripts execute in upstream CUA QuickJS, and `operation=menu` and `operation=desktop` call two upstream atomics excluded from `run_script`; this POC adds no second JS runtime. The public `session` is an adapter label mapped to a CUA trusted Session; it partitions adapter JSON state and receipts. CUA's native element tokens can cross labels inside one worker, and Agents operating the same desktop can affect one another. A separate MCP process per independent Agent host gives each its own private worker. Reobserve before actions after another Agent may have changed a window. The final report distinguishes successful native-picker Agent work from remaining release gates.
