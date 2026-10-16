# Screen sharing mechanisms and what DTW should reuse

Research checked 2026-10-16. The useful abstraction is application capture, not a
fixed main-window crop. These are primary-source findings, not an assertion that
all proprietary meeting clients implement identical internals.

| Mechanism | What it includes | Implication for an Agent |
| --- | --- | --- |
| Desktop-independent single window | One selected surface; legacy behavior excludes separate popups/children | Good isolated document snapshot, insufficient app context |
| Display + explicit window inclusion | Only listed windows, in display coordinates | Reliable per-surface action frames; refresh membership for every observation |
| Display + application inclusion | Application windows, including newly appearing child/pop-up windows | Best compositor-level coverage source; filter before crop |
| Display capture + crop | Everything inside the rectangle | Unrelated windows can leak in and occlusion changes meaning; avoid automatic fallback |

Apple explicitly demonstrates new/child windows joining an application-inclusion
stream automatically, while an explicit window list needs updating. It also
explains `contentRect`, content scale and display scale metadata for correct
geometry. The thin POC selects native window IDs and requests stock CUA window screenshots;
it does not implement a persistent application-inclusion stream.
[Apple WWDC22, Take ScreenCaptureKit to the next level](https://developer.apple.com/videos/play/wwdc2022/10155/)

Newer APIs expose child-window capture directly. Apple documents
`SCScreenshotConfiguration.includeChildWindows` as including alerts, popovers and
sheets by default. API availability and actual remote-service behavior need an OS
matrix before adopting that default in CUA; it must not be confused with older
single-window behavior or silently change a click frame's bounds.
[Apple includeChildWindows](https://developer.apple.com/documentation/screencapturekit/scscreenshotconfiguration/includechildwindows)

OBS exposes a distinct Application Capture mode that captures all visible windows
of the chosen application using ScreenCaptureKit. This is a useful open product
reference for the requested behavior.
[OBS macOS Screen Capture](https://obsproject.com/kb/macos-screen-capture-source)

WebRTC's macOS capture implementation uses ScreenCaptureKit, stream output and
native sharing-picker observer interfaces. This provides an inspectable conferencing
stack reference; it does not prove the internals of Zoom/Teams.
[WebRTC macOS ScreenCaptureKit implementation](https://webrtc.googlesource.com/src.git/+/refs/heads/main/modules/desktop_capture/mac/screen_capturer_sck.mm)

Possible upstream CUA direction (not implemented by the DTW extension):

1. Retain a capture session scoped to the selected running application. Use the
   OS application filter (and supported child-window behavior) to produce an overview
   per display, with capture-source changes invalidating the previous action frame.
2. Reconcile the overview with native window IDs, AX proxies and lifecycle signals.
   If the compositor shows content absent from the attributed window graph, report
   `coverage_incomplete`; do not silently claim a main-window capture is complete.
3. Return cropped app-only overview/active-surface images with exact metadata.
   Preserve display origin, content rect, scale, native owner and capture generation.
   Cropping must follow filtering, not substitute for it.
4. Prefer semantic actions where available. Use screenshot-bound foreground pixels
   only after fresh identity/geometry/hit checks, then independently read the effect.
   The meeting stack transports pixels; it does not solve action authority or races.

The live POC additionally exposed a remote-file-panel identity detail: system AX
can return a private service backing-window ID while app AX exposes the visible
client proxy. The popup menu belongs to that service, not the application's PID.
Thus neither same-PID collection nor rectangular overlap alone establishes all
associations. The DTW metadata adapter uses system client attribution and backing-owner
identity as a best-effort heuristic. It does not prove exhaustive association.
Application-filtered capture would require an upstream CUA primitive; reproducing
a compositor pipeline inside DTW would defeat the thin-extension boundary.
