# Release readiness

Historical CUA-based release contract. This page records the old product and its
acceptance boundaries; it is not the next release gate. The next release contains
the rewrite only. See [releasing](releasing.md) and the [product matrix](product-matrix.md).
The old candidate workflows have been retired; references below describe their
historical behavior.

The first stable release is `v0.1.0`. It uses CUA source `d74a9cab3fb2e80ab7f0cef47025ec828586ea75` for both the SDK and worker. Windows upstream limitations are non-blocking and tracked in [#36](https://github.com/caelis-labs/desktop-world/issues/36); affected Windows flows remain limited support.

## Distribution contract

Each platform ships a standalone executable, a compressed copy of that executable, a separate slim standard Plugin archive, SHA-256 checksums, and `BUILD-<platform>.json`. The slim runtime is built from the same source without the embedded self-extracting payload. Keep its adjacent worker, Skill, manifests and licensing inventory together. The standalone CLI still supports self-contained installation.

macOS builds explicitly target 14.0. The release workflow signs the worker before embedding, signs both standalone and slim runtimes, notarizes all three final executables, and checks signatures, online tickets and extracted-worker byte identity. Standalone Mach-O tickets cannot be stapled. Deployment metadata is a compatibility target, not proof of native acceptance on every OS version. The available local desktop is macOS 27.0.1; CI runs macOS 26. Clean macOS 14 first-run permissions remain a separate unverified environment.

Windows x64 executables are unsigned and require the x64 Microsoft Visual C++ runtime. RC.9 had real Windows WinForms, Chrome, Unicode input, screenshot and independent readback evidence; those historical results do not certify the new artifact. Current CI checks package integrity, protocol, isolation and bounded stop. DPI-unaware 125% geometry, modal-provider stalls and post-effect foreground errors remain open in #36. Elevated apps, secure desktop and mixed-DPI input are not advertised as verified.

## Runtime and installation guarantees

- Cancel ends the private worker within a bounded supervisor poll and permanently stops new execution in that Runtime. The original receipt becomes `unknown` when input may have been dispatched. Restart and observe before further input; never replay automatically.
- EOF, SIGTERM and parent death terminate the worker; native helpers own the scene lease for their entire process lifetime and monitor parent death and cancellation.
- Payload retention is bounded by 64 MiB, 64 completed results and 15 minutes. Expired payloads retain lightweight receipt status as `result_expired`; 4096 receipts stop new admission until restart.
- Standard exports restore canonical manifests, relative paths and checksums; host registrations and ownership markers do not travel. Installed host copies can re-export.
- Claude supports user scope only; project scope is explicitly rejected. Existing marketplaces must match the owned local source. Other documented host scopes remain supported.

## Promotion

Run source/unit CI and signed candidate builds on the exact release commit. Download both candidate artifacts and verify checksums, BUILD revision, standalone and slim package contracts. Run native desktop and real-host checks on the downloaded macOS candidate. Publish those same bytes without rebuilding after acceptance. Keep actual acceptance scope in the release notes, including unavailable clean-machine and Windows native coverage. Core/Bot pins must distinguish standalone, slim archive, runtime, worker and BUILD digests.

See [installation](install.md), [embedding](embedding.md), and historical [RC.9 acceptance](rc9-acceptance.md).
