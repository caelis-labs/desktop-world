import AppKit

// A disposable, independently observed application for the ordinary MCP probe.
// LaunchServices supplies no custom argv: every bundle writes beside itself.
final class Delegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    let root = Bundle.main.bundleURL.deletingLastPathComponent()
    let counter = NSTextField(labelWithString: "Count: 0")
    var windows: [NSWindow] = []
    var clicks = 0
    var quitRequests = 0
    var cancelledQuits = 0
    var blockQuit = false
    var command = ""
    var timer: Timer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        for title in ["DTW Lifecycle Main", "DTW Lifecycle Peer"] {
            let window = NSWindow(contentRect: NSRect(x: 160 + windows.count * 380, y: 240,
                width: 350, height: 220), styleMask: [.titled, .closable, .miniaturizable],
                backing: .buffered, defer: false)
            window.title = title
            window.delegate = self
            window.isReleasedWhenClosed = false
            if windows.isEmpty {
                let button = NSButton(title: "Increment", target: self, action: #selector(increment))
                button.frame = NSRect(x: 20, y: 90, width: 130, height: 35)
                counter.frame = NSRect(x: 20, y: 145, width: 250, height: 30)
                window.contentView!.addSubview(button)
                window.contentView!.addSubview(counter)
            }
            windows.append(window)
            window.makeKeyAndOrderFront(nil)
        }
        windows.first?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        writeState()
        timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            guard let self else { return }
            if let next = try? String(contentsOf: root.appendingPathComponent("command"), encoding: .utf8), next != command {
                command = next
                if next == "block_quit" { blockQuit = true }
                if next == "allow_quit" { blockQuit = false }
                if next == "cancel_quit" {
                    blockQuit = false
                    if let sheet = windows.first?.attachedSheet {
                        windows[0].endSheet(sheet)
                        sheet.orderOut(nil)
                    }
                }
            }
            writeState()
        }
        // Termination/save prompts use AppKit's modal mode. The independent
        // fixture controller must still run while that mode is active.
        RunLoop.main.add(timer!, forMode: .common)
        RunLoop.main.add(timer!, forMode: .modalPanel)
        RunLoop.main.add(timer!, forMode: .eventTracking)
    }
    @objc func increment() { clicks += 1; counter.stringValue = "Count: \(clicks)"; writeState() }
    func windowWillClose(_ notification: Notification) {
        DispatchQueue.main.async { [weak self] in self?.writeState() }
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        quitRequests += 1
        writeState()
        guard blockQuit, let parent = windows.first else { return .terminateNow }
        let alert = NSAlert()
        alert.messageText = "Unsaved fixture document"
        alert.informativeText = "This document exists only in this disposable acceptance fixture."
        alert.addButton(withTitle: "Keep Open")
        alert.beginSheetModal(for: parent) { [weak self] _ in
            self?.cancelledQuits += 1
            self?.writeState()
            NSApp.reply(toApplicationShouldTerminate: false)
        }
        return .terminateLater
    }
    func writeState() {
        let value: [String: Any] = ["pid": ProcessInfo.processInfo.processIdentifier,
            "clicks": clicks, "quit_requests": quitRequests, "cancelled_quits": cancelledQuits, "command": command,
            "windows": windows.map { ["title": $0.title, "visible": $0.isVisible,
                "number": $0.windowNumber] as [String: Any] }]
        if let bytes = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) {
            try? bytes.write(to: root.appendingPathComponent("state.json"), options: .atomic)
        }
    }
}
let delegate = Delegate()
let application = NSApplication.shared
application.delegate = delegate
application.setActivationPolicy(Bundle.main.infoDictionary?["LSUIElement"] as? Bool == true ? .accessory : .regular)
application.run()
