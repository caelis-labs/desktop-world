// Standalone pure controls. Append after Overlay.swift as main.swift; no GUI is opened.
func expectOverlay(_ condition: @autoclosure () -> Bool, _ description: String) {
    precondition(condition(), description)
}
let target = OverlayTargetState(pid: 7, birth: 9, window: 42, bounds: [-1600, -200, 1200, 800])
func pointer(_ operation: UInt64, _ sequence: UInt64, _ kind: String = "move", press: UInt64 = 0) -> OverlayPointerEvent {
    OverlayPointerEvent(operation: operation, sequence: sequence, press_sequence: press, target: target,
                        point: [-1400, 40], route: "background", kind: kind)
}
var state = OverlayPresentationState()
expectOverlay(state.begin(1), "first operation admitted")
expectOverlay(state.accept(pointer(1, 1)), "actual directed event shown")
state.hide(1)
expectOverlay(!state.accept(pointer(1, 2)), "canceled operation cannot resurrect")
expectOverlay(!state.begin(1), "canceled generation cannot restart")
expectOverlay(state.begin(2), "new operation keeps session but changes generation")
expectOverlay(state.accept(pointer(2, 1)), "new operation can display")
state.hide(1)
expectOverlay(state.marker != nil, "old cancellation cannot hide a newer operation")
expectOverlay(!state.accept(pointer(1, 999)), "late old event cannot replace current marker")
expectOverlay(!state.accept(pointer(2, 1)), "out-of-order or duplicate event ignored")
state.hideStaleTarget()
expectOverlay(state.marker == nil && !state.active, "target change terminates current marker generation")
expectOverlay(!state.accept(pointer(2, 2)), "late same-generation release cannot revive a stale target")
expectOverlay(!state.accept(pointer(2, 3, "press", press: 3)), "returning target geometry cannot revive old press")
expectOverlay(!state.begin(2), "target refresh requires a new native operation")
expectOverlay(state.begin(3) && state.accept(pointer(3, 1)), "new operation can show refreshed target")
state.close()
expectOverlay(!state.begin(4) && !state.accept(pointer(3, 2)), "session close is terminal")

var pulse = OverlayClickPulse()
expectOverlay(!pulse.isVisible(at: 10), "no presentation before an actual press edge")
pulse.observe(pressSequence: 1, previousPressSequence: 0, at: 10)
expectOverlay(pulse.isVisible(at: 10.1), "actual click has visible pulse")
expectOverlay(!pulse.isVisible(at: 10.21), "dropped release cannot keep a permanent pressed appearance")
pulse.observe(pressSequence: 1, previousPressSequence: 1, at: 10.15)
expectOverlay(!pulse.isVisible(at: 10.21), "move/release with same press edge cannot extend highlight")
pulse.observe(pressSequence: 4, previousPressSequence: 1, at: 11)
expectOverlay(pulse.isVisible(at: 11.1), "coalesced actual press edge still produces a pulse")
pulse.reset()
expectOverlay(!pulse.isVisible(at: 11.1), "hide or new generation clears click pulse")

let mailbox = OverlayLatestEvent()
expectOverlay(mailbox.put(pointer(3, 1, "press", press: 1)), "first event schedules one drain")
for sequence in 2...10000 {
    expectOverlay(!mailbox.put(pointer(3, UInt64(sequence), "move", press: 1)), "flood coalesces into one slot")
}
expectOverlay(!mailbox.put(pointer(2, 99999)), "old generation cannot replace queued latest")
let latest = mailbox.take()!
expectOverlay(latest.sequence == 10000 && latest.press_sequence == 1, "coalescing retains latest position and click edge")
expectOverlay(mailbox.take() == nil, "no event history is retained")
expectOverlay(mailbox.put(pointer(3, 10001)), "next event schedules next drain")

expectOverlay(overlayTargetMatches(target, pid: 7, birth: 9, bounds: target.rect, onScreen: true), "exact target accepted")
expectOverlay(!overlayTargetMatches(target, pid: 7, birth: 10, bounds: target.rect, onScreen: true), "reused pid rejected")
expectOverlay(!overlayTargetMatches(target, pid: 8, birth: 9, bounds: target.rect, onScreen: true), "different owner rejected")
expectOverlay(!overlayTargetMatches(target, pid: 7, birth: 9, bounds: target.rect, onScreen: false), "other Space/closed target hidden")
expectOverlay(!overlayTargetMatches(target, pid: 7, birth: 9, bounds: target.rect!.offsetBy(dx: 1, dy: 0), onScreen: true), "moved target hidden")
expectOverlay(!overlayTargetMatches(target, pid: 7, birth: nil, bounds: target.rect, onScreen: true), "missing process proof rejected")
let primary = CGRect(x: 0, y: 0, width: 1512, height: 982)
expectOverlay(overlayAppKitPoint(CGPoint(x: -1400, y: 40), primaryFrame: primary) == CGPoint(x: -1400, y: 942), "left screen negative origin preserved")
expectOverlay(overlayAppKitPoint(CGPoint(x: 200, y: -600), primaryFrame: primary) == CGPoint(x: 200, y: 1582), "screen above primary preserved")
expectOverlay(overlayAppKitPoint(CGPoint(x: 2500, y: 1600), primaryFrame: primary) == CGPoint(x: 2500, y: -618), "lower screen negative AppKit origin preserved")
// Coordinate conversion uses points, so moving from scale 1 to scale 2 does not
// multiply or offset the event point. Only AppKit's raster backing changes.
expectOverlay(overlayColorIndex("agent-1") == 0 && overlayColorIndex("agent-2") == 1, "donor stable palette")
expectOverlay(overlayColorIndex("abcd") == overlayColorIndex("abcd"), "stable session color")
let bad = OverlayPointerEvent(operation: 3, sequence: 1, press_sequence: 0, target: target,
                              point: [-1400, 40], route: "semantic", kind: "move")
expectOverlay(bad.position == nil, "semantic action has no fabricated pointer")
let malformed = OverlayPointerEvent(operation: 3, sequence: 1, press_sequence: 2, target: target,
                                    point: [-1400, 40], route: "background", kind: "move")
expectOverlay(malformed.position == nil, "invalid click sequence rejected")
let hotspot = CGPoint(x: 14, y: 14)
let path = overlayArrowPath(hotspot: hotspot, scale: 0.42)
var points = [CGPoint](repeating: .zero, count: 3)
let first = points.withUnsafeMutableBufferPointer { buffer in
    path.element(at: 0, associatedPoints: buffer.baseAddress!)
}
expectOverlay(first == .moveTo && points[0] == hotspot, "ported arrow hotspot remains the actual event point")

// Draw offscreen without creating an application/window or loading fonts.
// The old session badge area must stay transparent while the arrow remains visible.
let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 88, pixelsHigh: 64,
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
private let cursor = OverlayCursorView(session: "a19f")
cursor.draw(cursor.bounds)
NSGraphicsContext.restoreGraphicsState()
expectOverlay(bitmap.colorAt(x: 78, y: 45)!.alphaComponent == 0,
              "session badge background and digits are not presented")
expectOverlay((0..<64).contains { y in (0..<88).contains { x in
    bitmap.colorAt(x: x, y: y)!.alphaComponent > 0
}}, "session-colored arrow remains visible")
print("overlay contract controls passed (arrow, pulse, lifecycle and no identity badge)")
