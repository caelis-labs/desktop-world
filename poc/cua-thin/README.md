# Thin CUA adapter POC

Read [design](../../docs/cua-kernel-poc.md) and [evidence](../../docs/cua-kernel-poc-evidence.md) before treating this as an installer. This directory is a local macOS prototype, not the current DTW product path.

`Cargo.toml` points to the local CUA SDK checkout. `build-package.sh` verifies the full CUA source commit `ac967b9006aafa10870aadb5a645b038cba9721a`, builds `dtw` and the upstream private worker, and writes a new package directory with one Skill, one MCP `exec` server, and MIT/MPL notices. For example:

```sh
DTW_CUA_TARGET_DIR=/private/tmp/dtw-cua-build \
  sh poc/cua-thin/build-package.sh /private/tmp/dtw-cua-package
python3 poc/cua-thin/probe.py /private/tmp/dtw-cua-package/bin/dtw
python3 poc/cua-thin/contract_probe.py /private/tmp/dtw-cua-package/bin/dtw
```

An already built private worker can be supplied with `CUA_DRIVER_BIN`; the script then requires `CUA_DRIVER_BIN_SHA256` and refuses a hash mismatch. This POC's final local package used that path because a fresh offline worker build lacked the cached `anstream 1.0.0` crate. The worker binary hash and source pin are recorded in the evidence document.

To make an isolated Antigravity project plugin, run `python3 poc/cua-thin/adapt_antigravity.py PACKAGE PROJECT/.agents/plugins/desktop-world`, then `agy plugin validate PROJECT/.agents/plugins/desktop-world`. This copies into only the chosen project. The packaged `bin/dtw` also runs directly as an MCP stdio CLI; Node is not needed. Live fixture scripts launch temporary Chrome processes and require the actual host's macOS permissions. `accept_agent_chrome.py`, `accept_agent_wps.py`, and `evaluate_agent_usability.py` use a fresh Antigravity session; any permission rule for that test must be scoped to `mcp(desktop-world_dtw/exec)` and restored afterward. `probe_modal_focus.py` targets only a new temporary Obsidian process and closes it after checking chooser focus metadata.

The public `session` is an adapter label mapped to a CUA trusted Session; it partitions adapter JSON state and receipts. CUA's native element tokens can cross labels inside one worker, and Agents operating the same desktop can affect one another. A separate MCP process per independent Agent host gives each its own private worker. Reobserve before actions after another Agent may have changed a window. The evidence document records the remaining usability and native-dialog limits.
