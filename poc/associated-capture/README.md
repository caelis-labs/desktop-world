# Application observation extension

This worktree now implements the runtime-owned session and application-observation
contract described in [architecture](../../docs/architecture.md), including DTW
ScreenCaptureKit fallback and guarded pixel input. CUA remains the unmodified
0.35.4 official prerelease at `d74a9cab3fb2e80ab7f0cef47025ec828586ea75`.

Use `exec` without a `session` parameter. `operation:app` takes the exact target
PID, enumerates all attributed visible windows, and captures the upper two by
default (`max_windows:1..8`). `operation:click` takes its `observation_id`,
`image_index`, and PNG-pixel `x/y`. See the packaged Skill and API reference.

[Runtime acceptance](../../docs/runtime-acceptance.md) records the current gates.
The earlier [acceptance](acceptance.md) and its JSON are historical top2-only POC
evidence, not the current implementation or binary. The new extension removes
private AX symbols and does not depend on CUA internal types. Public-API
attribution remains best effort; top2 is only the presentation limit.

Reproducible GUI fixture:

```sh
tests/fixtures/scene/build-run.sh /private/tmp/NEW_FIXTURE_DIR
python3 tests/acceptance/scene_fixture_probe.py /path/to/dtw /private/tmp/NEW_FIXTURE_DIR /private/tmp/NEW_EVIDENCE_DIR
```

The fixture verifies independent UI effects and refusal after movement, closure,
reuse, expiry, another script or an unrelated opaque window. It does not replace
real WPS/Obsidian, multi-display or other-platform acceptance.
