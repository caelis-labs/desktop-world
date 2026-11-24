import AppKit
import ApplicationServices
import WebKit

let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let pointerModifierMask: NSEvent.ModifierFlags = [.command, .shift, .option, .control]
let sourceModifierMask: CGEventFlags = [.maskCommand, .maskShift, .maskAlternate, .maskControl]

final class FlippedView: NSView { override var isFlipped: Bool { true } }

final class WheelPane: NSScrollView {
    var record: ((NSEvent, CGFloat, CGFloat) -> Void)?

    override func scrollWheel(with event: NSEvent) {
        let before = contentView.bounds.origin.y
        let limit = (documentView?.bounds.height ?? 0) - contentView.bounds.height
        let after = min(limit, max(0, before - event.deltaY * 16))
        contentView.scroll(to: NSPoint(x: 0, y: after))
        reflectScrolledClipView(contentView)
        record?(event, before, after)
    }
}

final class ClickPad: NSView {
    var record: ((String, NSEvent) -> Void)?

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.systemTeal.setFill()
        bounds.fill()
        ("Pointer pad" as NSString).draw(at: NSPoint(x: 16, y: 14),
            withAttributes: [.foregroundColor: NSColor.black, .font: NSFont.systemFont(ofSize: 16)])
    }
    override func mouseDown(with event: NSEvent) { record?("down", event) }
    override func mouseUp(with event: NSEvent) { record?("up", event) }
    override func rightMouseDown(with event: NSEvent) { record?("down", event) }
    override func rightMouseUp(with event: NSEvent) { record?("up", event) }
    override func otherMouseDown(with event: NSEvent) { record?("down", event) }
    override func otherMouseUp(with event: NSEvent) { record?("up", event) }
}

final class ContextButton: NSButton {
    var show: (() -> Void)?

    override func accessibilityPerformShowMenu() -> Bool {
        show?()
        return true
    }
}

final class Fixture: NSObject, NSApplicationDelegate, NSMenuDelegate, WKScriptMessageHandler {
    let content = FlippedView(frame: NSRect(x: 0, y: 0, width: 780, height: 590))
    let cueOracle = CommandLine.arguments.contains("--semantic-cue-oracle")
    let addressedOracle = CommandLine.arguments.contains("--addressed-middle-oracle")
    let decoy = CommandLine.arguments.contains("--addressed-middle-decoy")
    let note = NSTextField(string: "initial")
    let noteArchive = NSTextField(string: "neighbor")
    let readyButton = NSButton(title: "Release gate", target: nil, action: nil)
    let contextButton = ContextButton(frame: .zero)
    let popup = NSPopUpButton(frame: .zero, pullsDown: false)
    let pad = ClickPad(frame: .zero)
    var window: NSWindow!
    var secondDocument: NSWindow?
    var documentScheduled = false
    var web: WKWebView!
    var panes: [String: WheelPane] = [:]
    var journal: FileHandle!
    var journalCount = 0
    var webReady = false
    var cueLayout: [String: Any] = [:]
    var menusTracking: Set<String> = []
    var eventMonitor: Any?

    func record(_ kind: String, _ fields: [String: Any] = [:]) {
        journalCount += 1
        var row = fields
        row["kind"] = kind
        row["sequence"] = journalCount
        row["uptime"] = ProcessInfo.processInfo.systemUptime
        let bytes = try! JSONSerialization.data(withJSONObject: row, options: [.sortedKeys])
        try! journal.write(contentsOf: bytes + Data("\n".utf8))
        publish()
    }

    func screenRect(_ view: NSView) -> [CGFloat] {
        screenBounds(window.convertToScreen(view.convert(view.bounds, to: nil)))
    }

    func screenBounds(_ bounds: CGRect) -> [CGFloat] {
        return [bounds.minX, CGDisplayBounds(CGMainDisplayID()).height - bounds.maxY, bounds.width, bounds.height]
    }

    func publish() {
        let state: [String: Any] = [
            "pid": ProcessInfo.processInfo.processIdentifier, "ready": window.isVisible && (decoy || webReady),
            "sequence": journalCount, "note": note.stringValue, "note_archive": noteArchive.stringValue,
            "second_document_id": secondDocument?.windowNumber ?? 0,
            "note_enabled": note.isEnabled, "gate_attached": readyButton.superview != nil,
            "popup": popup.titleOfSelectedItem ?? "", "menus_tracking": menusTracking.sorted(),
            "offsets": panes.mapValues { $0.contentView.bounds.origin.y }, "pad_bounds": decoy ? [] : screenRect(pad),
            "mouse_buttons": NSEvent.pressedMouseButtons,
            "keyboard_modifiers": NSEvent.modifierFlags.intersection(pointerModifierMask).rawValue,
            "hid_modifiers": CGEventSource.flagsState(.hidSystemState).intersection(sourceModifierMask).rawValue,
            "session_modifiers": CGEventSource.flagsState(.combinedSessionState).intersection(sourceModifierMask).rawValue,
            "active": NSApp.isActive, "window_id": window.windowNumber,
            "window_bounds": screenBounds(window.frame), "cue_layout": cueLayout,
            "cue_bounds": Dictionary(uniqueKeysWithValues: content.subviews.compactMap { view -> (String, [CGFloat])? in
                if view === note { return ("Native note", screenRect(view)) }
                if let button = view as? NSButton, button.tag == 1 { return ("Save", screenRect(button)) }
                return nil
            })
        ]
        try! JSONSerialization.data(withJSONObject: state, options: [.sortedKeys])
            .write(to: output.appendingPathComponent("state.json"), options: .atomic)
    }

    func add(_ view: NSView, _ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat) {
        view.frame = NSRect(x: x, y: y, width: w, height: h)
        content.addSubview(view)
    }

    func actionButton(_ title: String, _ x: CGFloat, _ y: CGFloat, _ action: Selector, tag: Int = 0) {
        let button = NSButton(title: title, target: self, action: action)
        button.tag = tag
        add(button, x, y, 138, 30)
    }

    @objc func recordButton(_ sender: NSButton) {
        record("button", ["title": sender.title, "tag": sender.tag])
    }

    @objc func commitNote(_ sender: NSButton) {
        record("native_commit", ["value": note.stringValue, "neighbor": noteArchive.stringValue])
    }

    @objc func scheduleEnable(_ sender: NSButton) {
        record("gate_scheduled")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [self] in
            note.isEnabled = true
            record("gate_enabled")
        }
    }

    @objc func scheduleDocument(_ sender: NSButton) {
        documentScheduled = true
        record("document_scheduled")
    }

    func showScheduledDocument() {
        guard documentScheduled, secondDocument == nil,
              FileManager.default.fileExists(atPath: output.appendingPathComponent("release-document").path) else { return }
        let document = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 420, height: 225),
                                styleMask: [.titled, .closable], backing: .buffered, defer: false)
        document.title = "DTW Owned Second Document"
        document.isReleasedWhenClosed = false
        let label = NSTextField(labelWithString: "Second document body")
        label.frame = NSRect(x: 20, y: 170, width: 360, height: 25)
        document.contentView?.addSubview(label)
        secondDocument = document
        document.center()
        document.makeKeyAndOrderFront(nil)
        record("document_opened", ["title": document.title, "window_id": document.windowNumber,
                                   "pid": ProcessInfo.processInfo.processIdentifier])
    }

    @objc func releaseGate(_ sender: NSButton) {
        sender.removeFromSuperview()
        record("gate_removed")
    }

    @objc func menuAction(_ sender: NSMenuItem) {
        record("menu_action", ["title": sender.title, "tag": sender.tag])
    }

    @objc func selectPopup(_ sender: NSPopUpButton) {
        record("popup_selection", ["title": sender.titleOfSelectedItem ?? "", "index": sender.indexOfSelectedItem])
    }

    func makeMenu(_ title: String) -> NSMenu {
        let menu = NSMenu(title: title)
        menu.autoenablesItems = false
        menu.delegate = self
        return menu
    }

    func menuItem(_ title: String, tag: Int, enabled: Bool = true) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: #selector(menuAction(_:)), keyEquivalent: "")
        item.target = self
        item.tag = tag
        item.isEnabled = enabled
        return item
    }

    func addSubmenu(_ title: String, to parent: NSMenu) -> NSMenu {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        let menu = makeMenu(title)
        parent.addItem(item)
        parent.setSubmenu(menu, for: item)
        return menu
    }

    func menuWillOpen(_ menu: NSMenu) {
        menusTracking.insert(menu.title)
        record("menu_open", ["title": menu.title])
    }

    func menuDidClose(_ menu: NSMenu) {
        menusTracking.remove(menu.title)
        record("menu_closed", ["title": menu.title])
    }

    func installMenus() {
        let bar = NSMenu()
        let application = addSubmenu("DTW Owned GUI", to: bar)
        application.addItem(withTitle: "Quit DTW Owned GUI", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let actions = addSubmenu("Probe Actions", to: bar)
        let primary = addSubmenu("Primary Branch", to: actions)
        let sibling = addSubmenu("Sibling Branch", to: actions)
        primary.addItem(menuItem("Record", tag: 11))
        sibling.addItem(menuItem("Record", tag: 21))
        primary.addItem(menuItem("Disabled Record", tag: 12, enabled: false))
        primary.addItem(menuItem("Twin Record", tag: 13))
        primary.addItem(menuItem("Twin Record", tag: 14))
        NSApp.mainMenu = bar

        contextButton.title = "Context owner"
        contextButton.bezelStyle = .rounded
        contextButton.show = { [weak self] in
            guard let self else { return }
            record("context_request")
            DispatchQueue.main.async {
                let menu = self.makeMenu("Owned Context")
                menu.addItem(self.menuItem("Context choice", tag: 31))
                let timer = Timer(timeInterval: 0.45, repeats: false) { [weak menu] _ in menu?.cancelTracking() }
                RunLoop.main.add(timer, forMode: .eventTracking)
                menu.popUp(positioning: nil, at: NSPoint(x: 5, y: 0), in: self.contextButton)
                timer.invalidate()
            }
        }
        add(contextButton, 468, 49, 143, 30)

        let choices = makeMenu("Owned Options")
        for (index, title) in ["Operations", "Finance", "Finance Archive", "Duplicated", "Duplicated", "Locked"].enumerated() {
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.tag = index
            item.isEnabled = title != "Locked"
            choices.addItem(item)
        }
        popup.menu = choices
        popup.selectItem(at: 0)
        popup.target = self
        popup.action = #selector(selectPopup(_:))
        popup.setAccessibilityLabel("Department choices")
        add(popup, 620, 49, 143, 30)
    }

    func makePane(_ title: String, _ x: CGFloat) {
        let pane = WheelPane(frame: .zero)
        pane.hasVerticalScroller = true
        pane.borderType = .bezelBorder
        let document = FlippedView(frame: NSRect(x: 0, y: 0, width: 200, height: 600))
        for index in 0..<12 {
            let text = index == 0 ? "\(title) anchor" : "\(title) record \(index)"
            let label = NSTextField(labelWithString: text)
            label.frame = NSRect(x: 16, y: CGFloat(index * 40 + 18), width: 163, height: 22)
            label.setAccessibilityLabel(text)
            document.addSubview(label)
        }
        pane.documentView = document
        pane.record = { [weak self] event, before, after in
            self?.record("wheel", ["pane": title, "delta_y": event.deltaY, "before": before, "after": after])
        }
        panes[title] = pane
        add(pane, x, 97, 221, 263)
    }

    func installWeb() {
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.userContentController.add(self, name: "probe")
        web = WKWebView(frame: .zero, configuration: config)
        add(web, 16, 377, 748, 195)
        web.loadHTMLString("""
            <!doctype html><meta charset="utf-8"><title>Owned receipt form</title>
            <style>body{font:16px system-ui;padding:10px} input,button{font:inherit;margin:6px}</style>
            <h2>Owned receipt form</h2>
            <input aria-label="Web memo" id="memo" value="original">
            <input aria-label="Web memo archive" id="neighbor" value="web neighbor">
            <button id="commit">Commit web memo</button>
            <select aria-label="Web department"><option>Operations</option><option>Finance</option></select>
            <output id="saved"></output>
            <script>
            const send = data => window.webkit.messageHandlers.probe.postMessage(data);
            document.querySelector('#commit').onclick = () => {
              const value = document.querySelector('#memo').value;
              const neighbor = document.querySelector('#neighbor').value;
              document.querySelector('#saved').textContent = 'Saved web memo';
              send({kind:'web_commit', value, neighbor});
            };
            send({kind:'web_ready'});
            </script>
            """, baseURL: nil)
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let fields = message.body as? [String: Any], let kind = fields["kind"] as? String else { return }
        if kind == "web_ready" { webReady = true }
        record(kind, fields)
    }

    func applicationDidBecomeActive(_ notification: Notification) {
        if (addressedOracle || decoy), journal != nil { record("became_active") }
    }

    func applicationDidResignActive(_ notification: Notification) {
        if (addressedOracle || decoy), journal != nil { record("resigned_active") }
    }

    func startDecoy() {
        let screen = NSScreen.main!.visibleFrame
        let bounds = CGRect(x: screen.minX + 24, y: screen.maxY - 170, width: 220, height: 115)
        window = NSWindow(contentRect: bounds, styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "DTW Owned Foreground Decoy"
        let label = NSTextField(labelWithString: "Owned foreground witness")
        label.frame = CGRect(x: 18, y: 48, width: 185, height: 24)
        window.contentView!.addSubview(label)
        eventMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseUp,
            .rightMouseDown, .rightMouseUp, .otherMouseDown, .otherMouseUp]) { [weak self] event in
            self?.record("raw_pointer", ["type": event.type.rawValue])
            return event
        }
        showWindow()
    }

    // Only place our own ordinary window. A frontmost application can still
    // be under a utility surface; a positive visual oracle needs clear points.
    func prepareCueLayout() {
        guard let save = content.subviews.compactMap({ $0 as? NSButton }).first(where: { $0.tag == 1 }) else { return }
        let centers = [save, note].map { view in
            let bounds = window.convertToScreen(view.convert(view.bounds, to: nil))
            return CGPoint(x: bounds.midX, y: bounds.midY)
        }
        let original = window.frame
        func covers() -> [CGRect]? {
            guard let rows = CGWindowListCopyWindowInfo([.optionOnScreenAboveWindow, .excludeDesktopElements],
                                                        CGWindowID(window.windowNumber)) as? [[String: Any]] else { return nil }
            return rows.compactMap { row in
                guard (row[kCGWindowAlpha as String] as? Double ?? 1) > 0,
                      let bounds = row[kCGWindowBounds as String] as? NSDictionary else { return nil }
                return CGRect(dictionaryRepresentation: bounds)
            }
        }
        func clear(_ frame: CGRect, _ covers: [CGRect]) -> Bool {
            centers.allSatisfy { center in
                let point = CGPoint(x: center.x + frame.minX - original.minX,
                    y: CGDisplayBounds(CGMainDisplayID()).height - (center.y + frame.minY - original.minY))
                return !covers.contains { $0.insetBy(dx: -12, dy: -12).contains(point) }
            }
        }
        cueLayout = ["clear": false]
        guard let above = covers() else { return }
        cueLayout["original_clear"] = clear(original, above)
        var frames = [original]
        for screen in NSScreen.screens {
            let room = screen.visibleFrame
            guard room.width >= original.width, room.height >= original.height else { continue }
            for x: CGFloat in [0.5, 0, 1] {
                for y: CGFloat in [0.5, 0, 1] {
                    frames.append(CGRect(origin: CGPoint(x: (room.minX + x * (room.width - original.width)).rounded(),
                        y: (room.minY + y * (room.height - original.height)).rounded()), size: original.size))
                }
            }
        }
        guard let position = frames.first(where: { frame in
            NSScreen.screens.contains { $0.visibleFrame.contains(frame) } && clear(frame, above)
        }) else { return }
        if position != original { window.setFrameOrigin(position.origin) }
        cueLayout["clear"] = covers().map { clear(window.frame, $0) } ?? false
    }

    func showWindow() {
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        let timer = Timer(timeInterval: 0.05, repeats: true) { [weak self] _ in
            if FileManager.default.fileExists(atPath: output.appendingPathComponent("quit").path) {
                NSApp.terminate(nil)
            }
            if let self, self.cueOracle, self.cueLayout.isEmpty,
               FileManager.default.fileExists(atPath: output.appendingPathComponent("prepare-cue-layout").path) {
                self.prepareCueLayout()
            }
            if let self, self.cueOracle, self.readyButton.superview != nil,
               FileManager.default.fileExists(atPath: output.appendingPathComponent("detach-gate").path) {
                self.releaseGate(self.readyButton)
            }
            self?.showScheduledDocument()
            self?.publish()
        }
        RunLoop.main.add(timer, forMode: .common)
        publish()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let journalPath = output.appendingPathComponent("events.jsonl")
        precondition(FileManager.default.createFile(atPath: journalPath.path, contents: nil))
        journal = try! FileHandle(forWritingTo: journalPath)
        if decoy { startDecoy(); return }
        window = NSWindow(contentRect: content.bounds, styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "DTW Owned GUI Acceptance"
        window.contentView = content
        window.center()
        window.initialFirstResponder = content
        actionButton("Save", 16, 11, #selector(recordButton(_:)), tag: 1)
        actionButton("Save As", 165, 11, #selector(recordButton(_:)), tag: 2)
        actionButton("Same caption", 314, 11, #selector(recordButton(_:)), tag: 3)
        actionButton("Same caption", 463, 11, #selector(recordButton(_:)), tag: 4)
        actionButton("Schedule document", 620, 11, #selector(scheduleDocument(_:)))
        actionButton("Commit native note", 16, 49, #selector(commitNote(_:)))
        actionButton("Schedule enable", 165, 49, #selector(scheduleEnable(_:)))
        readyButton.target = self
        readyButton.action = #selector(releaseGate(_:))
        add(readyButton, 314, 49, 138, 30)
        note.setAccessibilityLabel("Native note")
        note.isEnabled = cueOracle
        noteArchive.setAccessibilityLabel("Native note archive")
        add(note, 490, 100, 273, 26)
        add(noteArchive, 490, 135, 273, 26)
        add(pad, 490, 187, 273, 53)
        makePane("Left", 16)
        makePane("Right", 249)
        pad.record = { [weak self] direction, event in
            guard let self else { return }
            var fields: [String: Any] = ["phase": direction, "button": event.buttonNumber,
                "flags": event.modifierFlags.intersection(pointerModifierMask).rawValue,
                "count": event.clickCount]
            if self.addressedOracle {
                let point = self.window.convertPoint(toScreen: event.locationInWindow)
                fields["point"] = [point.x, CGDisplayBounds(CGMainDisplayID()).height - point.y]
                fields["event_type"] = event.type.rawValue
                fields["flags"] = event.modifierFlags.intersection(.deviceIndependentFlagsMask).rawValue
                fields["window_id"] = event.windowNumber
            }
            self.record("pointer", fields)
        }
        eventMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { [weak self] event in
            self?.record("key", ["type": event.type.rawValue, "code": event.keyCode])
            return event
        }
        if cueOracle {
            NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseUp, .rightMouseDown,
                .rightMouseUp, .otherMouseDown, .otherMouseUp, .leftMouseDragged, .rightMouseDragged,
                .otherMouseDragged, .mouseMoved, .scrollWheel]) { [weak self] event in
                self?.record("raw_pointer", ["type": event.type.rawValue])
                return event
            }
        }
        installMenus()
        installWeb()
        showWindow()
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let fixture = Fixture()
app.delegate = fixture
app.run()
