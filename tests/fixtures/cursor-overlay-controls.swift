// Added only to the owned native-input fixture by cursor_overlay_probe.py.
// The production fixture is not modified. These commands change only its own window.
extension Delegate {
    func cursorOverlayProbePadBounds() -> [CGFloat] {
        let frame = window.convertToScreen(pad.convert(pad.bounds, to: nil))
        return [frame.minX, CGDisplayBounds(CGMainDisplayID()).height - frame.maxY, frame.width, frame.height]
    }
    func cursorOverlayProbeCommand(_ name: String) {
        switch name {
        case "overlay_move": window.setFrameOrigin(NSPoint(x: window.frame.minX + 37, y: window.frame.minY + 29))
        case "overlay_close": window.close()
        default: break
        }
    }
}
