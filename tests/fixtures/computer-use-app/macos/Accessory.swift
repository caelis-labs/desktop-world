import AppKit

/// Owned windowless status item. State and commands stay inside its fixture root.
final class ComputerUseAccessory: NSObject, NSApplicationDelegate {
    let root: URL
    var statusItem: NSStatusItem?
    var timer: Timer?
    var journal: FileHandle?
    var counter = 0
    var sequence = 0
    var writeError = ""
    var caseName = "accessory"

    init(root: URL = Bundle.main.bundleURL.deletingLastPathComponent()) {
        self.root = root
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        do {
            let config = try JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("config.json"))) as? [String: Any]
            caseName = config?["case"] as? String ?? "accessory"
            let path = root.appendingPathComponent("native-events.jsonl")
            if !FileManager.default.fileExists(atPath: path.path) {
                guard FileManager.default.createFile(atPath: path.path, contents: nil) else { throw CocoaError(.fileWriteUnknown) }
            }
            journal = try FileHandle(forWritingTo: path)
            try journal?.seekToEnd()
        } catch { writeError = "accessory setup: \(error.localizedDescription)" }
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        item.button?.title = "DTW Accessory"
        item.button?.setAccessibilityLabel("DTW owned accessory \(caseName)")
        let menu = NSMenu(title: "DTW Accessory")
        let increment = NSMenuItem(title: "Increment accessory counter", action: #selector(incrementCounter), keyEquivalent: "")
        increment.target = self
        menu.addItem(increment)
        item.menu = menu
        statusItem = item
        event("launch")
        publish()
        let publisher = Timer(timeInterval: 0.1, repeats: true) { [weak self] _ in self?.tick() }
        timer = publisher
        for mode in [RunLoop.Mode.common, .modalPanel, .eventTracking] { RunLoop.main.add(publisher, forMode: mode) }
    }

    @objc func incrementCounter() {
        counter += 1
        event("accessory-increment")
        publish()
    }

    func event(_ kind: String) {
        sequence += 1
        do {
            var data = try JSONSerialization.data(withJSONObject: ["kind": kind, "sequence": sequence, "counter": counter, "pid": ProcessInfo.processInfo.processIdentifier], options: [.sortedKeys])
            data.append(0x0a)
            try journal?.write(contentsOf: data)
        } catch { writeError = "accessory journal: \(error.localizedDescription)" }
    }

    func publish() {
        let state: [String: Any] = ["schema_version": 1, "case": caseName,
            "pid": ProcessInfo.processInfo.processIdentifier, "ready": statusItem?.button != nil,
            "mode": "accessory", "windows": [], "anchors": [:], "write_error": writeError,
            "pressed_mouse_buttons": NSEvent.pressedMouseButtons,
            "environment": ["frontmost_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier as Any? ?? NSNull()],
            "accessory": ["counter": counter, "status_item": statusItem?.button != nil, "window_count": NSApp.windows.filter { $0.isVisible && $0 !== statusItem?.button?.window }.count]]
        do {
            let data = try JSONSerialization.data(withJSONObject: state, options: [.sortedKeys])
            try data.write(to: root.appendingPathComponent("native-state.json"), options: .atomic)
        } catch { writeError = "accessory state: \(error.localizedDescription)" }
    }

    func tick() {
        if let command = try? String(contentsOf: root.appendingPathComponent("command"), encoding: .utf8),
           command.trimmingCharacters(in: .whitespacesAndNewlines) == "quit" {
            NSApp.terminate(nil)
            return
        }
        publish()
    }

    func applicationWillTerminate(_ notification: Notification) {
        timer?.invalidate()
        if let statusItem { NSStatusBar.system.removeStatusItem(statusItem) }
        event("quit")
        do { try journal?.close() } catch { writeError = "accessory close: \(error.localizedDescription)" }
    }
}
