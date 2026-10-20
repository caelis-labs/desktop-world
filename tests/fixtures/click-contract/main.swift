import AppKit
import Foundation

let root = URL(fileURLWithPath: CommandLine.arguments.last!)
let app = NSApplication.shared
app.setActivationPolicy(.regular)
var events: [[String: Any]] = []
func save() {
    let data: [String: Any] = ["pid": ProcessInfo.processInfo.processIdentifier, "events": events]
    try? JSONSerialization.data(withJSONObject: data, options: [.sortedKeys])
        .write(to: root.appendingPathComponent("state.json"), options: .atomic)
}
final class Canvas: NSView {
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func draw(_ rect: NSRect) {
        NSColor.white.setFill()
        bounds.fill()
        let text = "DTW Issue 41 — isolated click fixture\nMouse events are recorded locally."
        text.draw(at: NSPoint(x: 30, y: 30), withAttributes: [
            .font: NSFont.systemFont(ofSize: 18), .foregroundColor: NSColor.black])
    }
    func record(_ event: NSEvent, _ kind: String) {
        let point = convert(event.locationInWindow, from: nil)
        events.append(["kind": kind, "ctrl": event.modifierFlags.contains(.control),
                       "shift": event.modifierFlags.contains(.shift),
                       "count": event.clickCount, "x": point.x, "y": point.y])
        save()
    }
    override func mouseDown(with event: NSEvent) { record(event, "leftDown") }
    override func mouseUp(with event: NSEvent) { record(event, "leftUp") }
    override func rightMouseDown(with event: NSEvent) { record(event, "rightDown") }
    override func rightMouseUp(with event: NSEvent) { record(event, "rightUp") }
}
final class Delegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    var timer: Timer?
    func applicationDidFinishLaunching(_ notification: Notification) {
        window = NSWindow(contentRect: NSRect(x: 300, y: 300, width: 500, height: 300),
                          styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "DTW Issue 41 Fixture"
        window.contentView = Canvas(frame: NSRect(x: 0, y: 0, width: 500, height: 300))
        window.makeKeyAndOrderFront(nil)
        app.activate(ignoringOtherApps: true)
        save()
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { _ in
            if FileManager.default.fileExists(atPath: root.appendingPathComponent("quit").path) {
                app.terminate(nil)
            }
        }
    }
}
let delegate = Delegate()
app.delegate = delegate
app.run()
