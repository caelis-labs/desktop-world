import AppKit
import Foundation

// An independently bundled owned application used to witness foreground input.
final class ForegroundWitness: NSObject, NSApplicationDelegate {
    let root = Bundle.main.bundleURL.deletingLastPathComponent()
    let field = NSTextField()
    var window: NSWindow!
    var caseName = "unconfigured"
    var writeError = ""
    var keyEvents = 0
    var flagsEvents = 0
    var sequence = 0
    var timer: Timer?
    var monitor: Any?
    var journal: FileHandle?

    func fail(_ operation: String, _ error: Error) {
        writeError = "\(operation): \(error.localizedDescription)"
        FileHandle.standardError.write(Data((writeError + "\n").utf8))
    }
    func event(_ kind: String, _ fields: [String: Any] = [:]) {
        sequence += 1
        var row = fields
        row["kind"] = kind; row["sequence"] = sequence
        row["unix_ms"] = Date().timeIntervalSince1970 * 1_000
        row["uptime"] = ProcessInfo.processInfo.systemUptime
        do {
            guard let journal else { throw CocoaError(.fileWriteUnknown) }
            try journal.write(contentsOf: JSONSerialization.data(withJSONObject: row, options: [.sortedKeys]) + Data("\n".utf8))
        } catch { fail("append witness event", error) }
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        var activate = false
        do {
            guard let config = try JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("config.json"))) as? [String: Any], let name = config["case"] as? String else {
                throw NSError(domain: "fixture", code: 1, userInfo: [NSLocalizedDescriptionKey: "Witness config requires case"])
            }
            caseName = name; activate = config["activate"] as? Bool ?? false
        } catch { fail("witness configuration", error) }
        do {
            let url = root.appendingPathComponent("native-events.jsonl")
            if !FileManager.default.fileExists(atPath: url.path), !FileManager.default.createFile(atPath: url.path, contents: nil) { throw CocoaError(.fileWriteUnknown) }
            journal = try FileHandle(forWritingTo: url); try journal?.seekToEnd()
        } catch { fail("open witness journal", error) }
        let screen = NSScreen.screens.first?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1280, height: 900)
        window = NSWindow(contentRect: NSRect(x: screen.maxX - 340, y: screen.maxY - 210, width: 320, height: 180), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Foreground witness"; window.isReleasedWhenClosed = false
        let label = NSTextField(labelWithString: "Foreground witness")
        label.frame = NSRect(x: 20, y: 118, width: 280, height: 28); label.font = .systemFont(ofSize: 20)
        window.contentView!.addSubview(label)
        field.frame = NSRect(x: 20, y: 68, width: 280, height: 28)
        field.stringValue = "witness sentinel"; field.setAccessibilityLabel("Witness sentinel")
        window.contentView!.addSubview(field)
        let status = NSTextField(labelWithString: "Owned focus and keyboard observer")
        status.frame = NSRect(x: 20, y: 24, width: 285, height: 22)
        window.contentView!.addSubview(status)
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { [weak self] input in
            guard let self else { return input }
            if input.type == .keyDown { self.keyEvents += 1 } else { self.flagsEvents += 1 }
            var row: [String: Any] = ["key_code": input.keyCode, "flags": input.modifierFlags.rawValue, "window": input.windowNumber]
            if input.type == .keyDown { row["characters"] = input.characters ?? "" }
            self.event(input.type == .keyDown ? "keyDown" : "flagsChanged", row)
            return input
        }
        window.makeFirstResponder(field)
        if activate {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
        } else {
            window.orderFront(nil)
        }
        event("launch", ["activate": activate]); publish()
        timer = Timer(timeInterval: 0.02, repeats: true) { [weak self] _ in self?.tick() }
        for mode in [RunLoop.Mode.common, .modalPanel, .eventTracking] { RunLoop.main.add(timer!, forMode: mode) }
    }
    func finite(_ value: CGFloat) -> Any { value.isFinite ? value as Any : NSNull() }
    func publish() {
        guard window != nil else { return }
        let pointer = CGEvent(source: nil)?.location
        let state: [String: Any] = [
            "schema_version": 1, "case": caseName, "pid": ProcessInfo.processInfo.processIdentifier,
            "ready": window.isVisible, "write_error": writeError,
            "environment": ["frontmost_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier as Any? ?? NSNull(), "pointer_screen": pointer.map { [finite($0.x), finite($0.y)] } as Any? ?? NSNull()],
            "witness": ["text": field.currentEditor()?.string ?? field.stringValue, "key_events": keyEvents, "flags_events": flagsEvents]
        ]
        do { try JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]).write(to: root.appendingPathComponent("native-state.json"), options: .atomic) }
        catch { fail("publish witness state", error) }
    }
    func tick() {
        if let command = try? String(contentsOf: root.appendingPathComponent("command"), encoding: .utf8), command.trimmingCharacters(in: .whitespacesAndNewlines) == "quit" { NSApp.terminate(nil); return }
        publish()
    }
    func applicationWillTerminate(_ notification: Notification) {
        timer?.invalidate()
        if let monitor { NSEvent.removeMonitor(monitor) }
        event("quit"); publish()
        do { try journal?.close() } catch { fail("close witness journal", error) }
    }
}
