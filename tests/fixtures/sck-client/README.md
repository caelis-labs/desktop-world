# Minimal SCK multi-instance diagnostic

This read-only diagnostic isolates ScreenCaptureKit from CUA and DTW. Compile
with `xcrun swiftc main.swift -o /private/tmp/sck-client`. Use a window ID from an
authorized application observation. Run `/private/tmp/sck-client WINDOW_ID hold`,
then run the same executable with the same ID in another terminal while the first
is still alive. Stop the first or let its 60-second hold expire, then repeat.

On the tested macOS 27.0.1 host, the second process reached `shareable_callback`
but timed out waiting for `image_callback`; removing the live peer restored
capture. Different process names and servicing the main dispatch queue did not
resolve the reproduced lifetime interference. This is local reproduction
evidence, not a claim about all macOS versions or Apple's internal cause.

The DTW regression is `tests/acceptance/multi_runtime_scene_probe.py`. It keeps
two identical MCP Runtimes alive while alternating and overlapping app reads.
The adapter's short-lived native child plus parent-held lease must keep this
working without changing CUA, sharing Agent state or retrying input.
