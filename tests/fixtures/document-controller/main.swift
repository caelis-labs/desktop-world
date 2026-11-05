// A real NSDocumentController Open panel, including its NSRemoteView service.
// It starts in an empty owned directory and never saves a user document.
import AppKit

let probeRoot = Bundle.main.bundleURL.deletingLastPathComponent()

final class ProbeDocumentController: NSDocumentController {
    override func beginOpenPanel(_ panel: NSOpenPanel, forTypes types: [String]?,
                                 completionHandler: @escaping (Int) -> Void) {
        let directory = probeRoot.appendingPathComponent("documents", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        panel.directoryURL = directory
        super.beginOpenPanel(panel, forTypes: types, completionHandler: completionHandler)
    }
}

@objc(DTWProbeDocument) final class ProbeDocument: NSDocument {
    override class var autosavesInPlace: Bool { false }
    override func makeWindowControllers() {
        let window = NSWindow(contentRect: NSRect(x: 180, y: 250, width: 600, height: 360),
                              styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "Owned new document"
        let text = NSTextView(frame: window.contentView!.bounds)
        text.isRichText = false
        window.contentView!.addSubview(text)
        addWindowController(NSWindowController(window: window))
        (NSApp.delegate as? ProbeDelegate)?.created += 1
    }
    override func data(ofType typeName: String) throws -> Data { Data() }
    override func read(from data: Data, ofType typeName: String) throws {}
}

final class ProbeDelegate: NSObject, NSApplicationDelegate {
    var created = 0
    var panelReady = false
    var timer: Timer?
    var monitor: Any?
    var events: [[String: Any]] = []

    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool { false }

    func applicationDidFinishLaunching(_ note: Notification) {
        let menu = NSMenu(), appItem = NSMenuItem(), appMenu = NSMenu()
        appMenu.addItem(withTitle: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        menu.addItem(appItem)
        let fileItem = NSMenuItem(), file = NSMenu(title: "File")
        file.addItem(withTitle: "New", action: #selector(NSDocumentController.newDocument(_:)), keyEquivalent: "n")
        file.addItem(withTitle: "Open", action: #selector(NSDocumentController.openDocument(_:)), keyEquivalent: "o")
        fileItem.submenu = file
        menu.addItem(fileItem)
        NSApp.mainMenu = menu
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseUp, .keyDown]) { [weak self] event in
            var cursor = event.window?.contentView?.hitTest(event.locationInWindow)
            var chain: [String] = []
            while let view = cursor {
                chain.append(NSStringFromClass(type(of: view)))
                cursor = view.superview
            }
            self?.events.append(["type": event.type.rawValue, "window": event.windowNumber,
                                 "x": event.locationInWindow.x, "y": event.locationInWindow.y,
                                 "active": NSApp.isActive, "key_window": NSApp.keyWindow?.windowNumber ?? 0,
                                 "event_window_key": event.window?.isKeyWindow ?? false, "hit_chain": chain])
            return event
        }
        let timer = Timer(timeInterval: 0.05, repeats: true) { [weak self] _ in self?.tick() }
        self.timer = timer
        for mode in [RunLoop.Mode.common, .modalPanel, .eventTracking] { RunLoop.main.add(timer, forMode: mode) }
        tick()
        // Let LaunchServices finish its startup events before opening our panel.
        // Any startup document belongs solely to this fresh owned instance.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [self] in
            for document in NSDocumentController.shared.documents { document.close() }
            panelReady = true
            NSDocumentController.shared.openDocument(nil)
        }
    }

    func tick() {
        let state: [String: Any] = ["pid": ProcessInfo.processInfo.processIdentifier, "created": created,
            "panel_ready": panelReady,
            "frontmost": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0,
            "windows": NSApp.windows.filter { $0.isVisible }.map {
                ["id": $0.windowNumber, "title": $0.title, "open_panel": $0 is NSOpenPanel,
                 "x": $0.frame.minX, "y": $0.frame.minY,
                 "width": $0.frame.width, "height": $0.frame.height]
            }, "events": events]
        do {
            try JSONSerialization.data(withJSONObject: state, options: [.sortedKeys])
                .write(to: probeRoot.appendingPathComponent("state.json"), options: .atomic)
        } catch { fputs("state: \(error)\n", stderr) }
        if FileManager.default.fileExists(atPath: probeRoot.appendingPathComponent("stop").path) { NSApp.terminate(nil) }
    }
}

let app = NSApplication.shared
let activity = ProcessInfo.processInfo.beginActivity(options: .userInitiatedAllowingIdleSystemSleep, reason: "Owned GUI acceptance state publisher")
let documentController = ProbeDocumentController()
let delegate = ProbeDelegate()
app.setActivationPolicy(.regular)
app.delegate = delegate
withExtendedLifetime((delegate, documentController, activity)) { app.run() }
