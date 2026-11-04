import AppKit
import WebKit
import UniformTypeIdentifiers
import Darwin

func fixtureProcessBirth() -> UInt64? {
    let pid = ProcessInfo.processInfo.processIdentifier
    var info = proc_bsdinfo()
    let size = Int32(MemoryLayout<proc_bsdinfo>.size)
    guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size) == size,
          info.pbi_pid == UInt32(pid), info.pbi_start_tvusec < 1_000_000 else { return nil }
    return info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec
}

func fixtureRoot() -> URL {
    let arguments = CommandLine.arguments
    if let index = arguments.firstIndex(of: "--fixture-root"), index + 1 < arguments.count {
        return URL(fileURLWithPath: arguments[index + 1], isDirectory: true).standardizedFileURL
    }
    return Bundle.main.bundleURL.deletingLastPathComponent()
}

final class ComputerUseDelegate: NSObject, NSApplicationDelegate, WKNavigationDelegate {
    let root = fixtureRoot()
    var caseName = "unconfigured"
    var webURL = ""
    var activateOnPresentation = false
    var configuredWindowX: CGFloat?
    var configuredWindowY: CGFloat?
    var writeError = ""
    var mainWindow: NSWindow!
    var peers: [NSWindow] = []
    var peerFields: [NSTextField] = []
    var peerCounts: [Int] = []
    var body: NSView!
    var nativeView: NSView!
    var chat: ChatCanvas!
    var webView: WKWebView!
    var mode = "native"
    var loaded = false
    var webError = ""
    var timer: Timer?
    var monitor: Any?
    var journal: FileHandle?
    var sequence = 0
    var namedViews: [String: NSView] = [:]
    let note = NSTextField()
    let multilineNote = NSTextView()
    var initialMultilineFocus = false
    let counterLabel = NSTextField(labelWithString: "Counter: 0")
    var reviewed = NSButton()
    let choice = NSPopUpButton()
    let slider = NSSlider(value: 25, minValue: 0, maxValue: 100, target: nil, action: nil)
    var leftScroll: NSScrollView!
    var rightScroll: NSScrollView!
    var counter = 0
    var saves = 0
    var savedNote = ""
    var cleanNote = "Initial native note"
    var quitConfirmation: NSAlert?
    var cleanupRequested = false
    var quitRequests = 0
    var quitCancellations = 0
    var sliderChanges = 0
    var auditIndex = -1
    var auditCount = 0
    var gestures: [String: Int] = [:]
    var panelKind = ""
    var panelResult = ""
    var openedText = ""
    var fileError = ""
    var fileSaves = 0
    var panel: NSSavePanel?

    func applicationDidFinishLaunching(_ notification: Notification) {
        do {
            let config = try JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("config.json"))) as? [String: Any]
            guard let name = config?["case"] as? String, let url = config?["web_url"] as? String else { throw NSError(domain: "fixture", code: 1, userInfo: [NSLocalizedDescriptionKey: "config.json requires case and web_url strings"]) }
            caseName = name; webURL = url
            activateOnPresentation = config?["activate"] as? Bool ?? false
            initialMultilineFocus = config?["initial_multiline_focus"] as? Bool ?? false
            configuredWindowX = (config?["window_x"] as? NSNumber).map { CGFloat(truncating: $0) }
            configuredWindowY = (config?["window_y"] as? NSNumber).map { CGFloat(truncating: $0) }
        } catch { writeError = "configuration: \(error.localizedDescription)" }
        let journalURL = root.appendingPathComponent("native-events.jsonl")
        do {
            if !FileManager.default.fileExists(atPath: journalURL.path) { guard FileManager.default.createFile(atPath: journalURL.path, contents: nil) else { throw CocoaError(.fileWriteUnknown) } }
            journal = try FileHandle(forWritingTo: journalURL); try journal?.seekToEnd()
        } catch { failIO("event journal", error) }
        buildMenu()
        let screen = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1280, height: 900)
        let size = NSSize(width: min(1050, screen.width - 40), height: min(800, screen.height - 50))
        mainWindow = NSWindow(contentRect: NSRect(x: screen.minX + 20, y: screen.maxY - size.height - 30, width: size.width, height: size.height), styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        if let x = configuredWindowX, let y = configuredWindowY {
            let primaryTop = NSScreen.screens.first?.frame.maxY ?? 0
            mainWindow.setFrameOrigin(NSPoint(x: x, y: primaryTop - y - mainWindow.frame.height))
        }
        mainWindow.title = "DTW Computer Use — \(caseName)"; mainWindow.isReleasedWhenClosed = false; mainWindow.acceptsMouseMovedEvents = true
        let content = FlippedFixtureView(frame: NSRect(origin: .zero, size: size)); mainWindow.contentView = content
        let items: [(String, Selector, CGFloat)] = [("Native controls", #selector(showNative), 140), ("Web workspace", #selector(showWeb), 145), ("Canvas chat", #selector(showChat), 125), ("Open file…", #selector(openFile), 110), ("Save file…", #selector(saveFile), 110), ("New peer window", #selector(newPeer), 155)]
        var x: CGFloat = 12
        for (title, action, width) in items { let control = button(title, NSRect(x: x, y: 10, width: width, height: 32), action, in: content); namedViews[title] = control; x += width + 4 }
        body = NSView(frame: NSRect(x: 0, y: 54, width: size.width, height: size.height - 54)); content.addSubview(body)
        nativeView = buildNative(body.bounds); body.addSubview(nativeView)
        chat = ChatCanvas(frame: body.bounds); chat.owner = self; chat.isHidden = true; body.addSubview(chat)
        webView = WKWebView(frame: body.bounds, configuration: WKWebViewConfiguration()); webView.navigationDelegate = self; webView.isHidden = true; body.addSubview(webView)
        if let url = URL(string: webURL), ["http", "https"].contains(url.scheme ?? "") { webView.load(URLRequest(url: url)) } else { webError = "Invalid configured web_url" }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .leftMouseUp, .rightMouseUp, .leftMouseDragged, .scrollWheel, .keyDown]) { [weak self] event in
            guard let self, event.window != nil else { return event }
            let p = event.locationInWindow
            var fields: [String: Any] = ["type": event.type.rawValue, "window": event.windowNumber, "x": self.finite(p.x), "y": self.finite(p.y)]
            if event.type == .scrollWheel { fields["delta_x"] = self.finite(event.scrollingDeltaX); fields["delta_y"] = self.finite(event.scrollingDeltaY) }
            if event.type == .leftMouseDown || event.type == .rightMouseDown { fields["click_count"] = event.clickCount }
            if event.type == .keyDown { fields["key_code"] = event.keyCode }
            fields["editing_receiver"] = event.window?.firstResponder === self.multilineNote ? "multiline" :
                (self.note.currentEditor() != nil && event.window?.firstResponder === self.note.currentEditor() ? "note" : "other")
            fields["multiline_selection"] = ["location": self.multilineNote.selectedRange().location,
                                             "length": self.multilineNote.selectedRange().length]
            self.event("input", fields); return event
        }
        present(mainWindow, firstResponder: initialMultilineFocus ? multilineNote : note)
        event("launch", [:]); publish()
        timer = Timer(timeInterval: 0.1, repeats: true) { [weak self] _ in self?.tick() }
        for mode in [RunLoop.Mode.common, .modalPanel, .eventTracking] { RunLoop.main.add(timer!, forMode: mode) }
    }
    func present(_ window: NSWindow, firstResponder: NSResponder) {
        window.makeFirstResponder(firstResponder)
        if activateOnPresentation {
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
        } else {
            window.orderFront(nil)
        }
    }
    func buildMenu() {
        let menu = NSMenu()
        let appItem = NSMenuItem(); let appMenu = NSMenu(); appMenu.addItem(withTitle: "Quit Computer Use", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"); appItem.submenu = appMenu; menu.addItem(appItem)
        let fileItem = NSMenuItem(); let fileMenu = NSMenu(title: "File")
        for (title, action, key) in [("Open file…", #selector(openFile), "o"), ("Save file…", #selector(saveFile), "s"), ("New peer window", #selector(newPeer), "n")] { let item = NSMenuItem(title: title, action: action, keyEquivalent: key); item.target = self; fileMenu.addItem(item) }
        fileMenu.addItem(withTitle: "Close Window", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        fileItem.submenu = fileMenu; menu.addItem(fileItem)
        let editItem = NSMenuItem(); let edit = NSMenu(title: "Edit")
        for (title, selector, key) in [("Cut", #selector(NSText.cut(_:)), "x"), ("Copy", #selector(NSText.copy(_:)), "c"), ("Paste", #selector(NSText.paste(_:)), "v"), ("Select All", #selector(NSText.selectAll(_:)), "a")] { edit.addItem(withTitle: title, action: selector, keyEquivalent: key) }
        editItem.submenu = edit; menu.addItem(editItem); NSApp.mainMenu = menu
    }
    func switchMode(_ next: String) { chat.closePopup(); mode = next; nativeView.isHidden = next != "native"; webView.isHidden = next != "web"; chat.isHidden = next != "chat"; event("mode", ["mode": next]); publish() }
    @objc func showNative() { switchMode("native") }
    @objc func showWeb() {
        if webView.url?.absoluteString != webURL, let url = URL(string: webURL) { webView.load(URLRequest(url: url)) }
        switchMode("web")
    }
    @objc func showChat() { switchMode("chat") }
    @objc func newPeer() {
        let index = peers.count
        let window = NSWindow(contentRect: NSRect(x: mainWindow.frame.minX + 100 + CGFloat(index * 15), y: mainWindow.frame.minY + 150, width: 440, height: 180), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "DTW Computer Use — \(caseName) — Peer \(index + 1)"; window.isReleasedWhenClosed = false
        let field = NSTextField(frame: NSRect(x: 20, y: 100, width: 390, height: 28)); field.setAccessibilityLabel("Peer note"); window.contentView!.addSubview(field)
        let b = button("Increment peer counter", NSRect(x: 20, y: 40, width: 230, height: 32), #selector(incrementPeer(_:)), in: window.contentView!); b.tag = index
        peers.append(window); peerFields.append(field); peerCounts.append(0); present(window, firstResponder: field); event("new-peer", ["index": index]); publish()
    }
    @objc func incrementPeer(_ sender: NSButton) { peerCounts[sender.tag] += 1; event("peer-increment", ["index": sender.tag, "count": peerCounts[sender.tag]]); publish() }
    func owned(_ url: URL) -> Bool { url.resolvingSymlinksInPath().deletingLastPathComponent() == root.resolvingSymlinksInPath() }
    @objc func openFile() {
        let open = NSOpenPanel(); open.directoryURL = root; open.canChooseDirectories = false; open.allowsMultipleSelection = false
        panel = open; panelKind = "open"; panelResult = "pending"; fileError = ""
        open.beginSheetModal(for: mainWindow) { [weak self] response in
            guard let self else { return }; self.panelResult = response == .OK ? "ok" : "cancel"
            if response == .OK, let url = open.url {
                do { guard self.owned(url) else { throw CocoaError(.fileReadNoPermission) }; self.openedText = try String(contentsOf: url, encoding: .utf8) } catch { self.fileError = error.localizedDescription; self.panelResult = "error" }
            }
            self.event("open-file", ["result": self.panelResult, "error": self.fileError]); self.publish()
        }; publish()
    }
    @objc func saveFile() {
        let save = NSSavePanel(); save.directoryURL = root; save.nameFieldStringValue = "owned-note.txt"; save.canCreateDirectories = false; save.allowedContentTypes = [.plainText]; save.isExtensionHidden = false
        panel = save; panelKind = "save"; panelResult = "pending"; fileError = ""
        save.beginSheetModal(for: mainWindow) { [weak self] response in
            guard let self else { return }; self.panelResult = response == .OK ? "ok" : "cancel"
            if response == .OK, let url = save.url {
                do { guard self.owned(url), url.lastPathComponent == "owned-note.txt" else { throw CocoaError(.fileWriteNoPermission) }; try self.noteText.write(to: url, atomically: true, encoding: .utf8); self.fileSaves += 1 } catch { self.fileError = error.localizedDescription; self.panelResult = "error" }
            }
            self.event("save-file", ["result": self.panelResult, "error": self.fileError]); self.publish()
        }; publish()
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { loaded = true; webError = ""; event("web-loaded", ["url": webView.url?.absoluteString ?? ""]); publish() }
    func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) { loaded = false; webError = ""; publish() }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { navigationFailed(error) }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { navigationFailed(error) }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { navigationFailed(NSError(domain: "WebKit", code: 1, userInfo: [NSLocalizedDescriptionKey: "Web content process terminated"])) }
    func navigationFailed(_ error: Error) { loaded = false; webError = error.localizedDescription; event("web-error", ["error": webError]); publish() }
    func finite(_ value: CGFloat) -> Any { value.isFinite ? value as Any : NSNull() }
    func failIO(_ operation: String, _ error: Error) { writeError = "\(operation): \(error.localizedDescription)"; FileHandle.standardError.write(Data((writeError + "\n").utf8)) }
    func event(_ kind: String, _ fields: [String: Any]) {
        sequence += 1
        var row = fields; row["sequence"] = sequence; row["kind"] = kind; row["uptime"] = ProcessInfo.processInfo.systemUptime
        row["unix_ms"] = Date().timeIntervalSince1970 * 1_000
        row["frontmost_pid"] = NSWorkspace.shared.frontmostApplication?.processIdentifier as Any? ?? NSNull()
        row["pid"] = ProcessInfo.processInfo.processIdentifier
        row["process_birth"] = fixtureProcessBirth() as Any? ?? NSNull()
        do { guard let journal else { throw CocoaError(.fileWriteUnknown) }; let data = try JSONSerialization.data(withJSONObject: row, options: [.sortedKeys]); try journal.write(contentsOf: data + Data("\n".utf8)) } catch { failIO("append event", error) }
    }
    func point(_ view: NSView, _ local: NSPoint? = nil) -> [Any]? {
        guard let window = view.window else { return nil }
        let p = window.convertPoint(toScreen: view.convert(local ?? NSPoint(x: view.bounds.midX, y: view.bounds.midY), to: nil))
        return [finite(p.x), finite((NSScreen.screens.first?.frame.maxY ?? 0) - p.y)]
    }
    func publish() {
        guard mainWindow != nil, chat != nil, webView != nil else { return }
        let top = NSScreen.screens.first?.frame.maxY ?? 0
        var anchors: [String: [Any]] = [:]
        func visit(_ view: NSView) {
            guard !view.isHidden else { return }
            if let b = view as? NSButton, !b.title.isEmpty, let p = point(b) { anchors[b.title] = p }
            for child in view.subviews { visit(child) }
        }
        visit(mainWindow.contentView!)
        for (name, view) in namedViews { anchors[name] = point(view) }
        anchors["slider-start"] = point(slider, NSPoint(x: 7, y: slider.bounds.midY)); anchors["slider-end"] = point(slider, NSPoint(x: slider.bounds.width - 7, y: slider.bounds.midY))
        let visibleMessage137 = chat.messageRect.intersection(chat.feedRect)
        if !visibleMessage137.isEmpty { anchors["chat-message-137"] = point(chat, NSPoint(x: visibleMessage137.midX, y: visibleMessage137.midY)) }
        for index in 1...chat.messageCount {
            let visible = chat.rect(for: index).intersection(chat.feedRect)
            if !visible.isEmpty { anchors["chat-message-\(index)"] = point(chat, NSPoint(x: visible.midX, y: visible.midY)) }
        }
        anchors["chat-feed"] = point(chat, NSPoint(x: chat.feedRect.midX, y: chat.feedRect.midY))
        anchors["chat-compose"] = point(chat.compose); anchors["chat-send"] = point(chat.send)
        anchors["web-origin"] = point(webView, NSPoint(x: webView.bounds.minX, y: webView.isFlipped ? webView.bounds.minY : webView.bounds.maxY))
        anchors["chat-edit-field"] = point(chat.editor); anchors["chat-apply-edit"] = point(chat.apply)
        if let popup = chat.popupCanvas { for (index, name) in ["chat-pin", "chat-edit", "chat-reply"].enumerated() { anchors[name] = point(popup, NSPoint(x: 75, y: 20 + index * 40)) } }
        for (index, peer) in peers.enumerated() where peer.isVisible {
            if let close = peer.standardWindowButton(.closeButton) { anchors["peer-close-\(index)"] = point(close) }
        }
        var windows = [mainWindow!] + peers
        if let popup = chat.popup { windows.append(popup) }; windows += windows.compactMap { $0.attachedSheet }
        let rows: [[String: Any]] = windows.filter { $0.isVisible }.map { ["title": $0.title, "id": $0.windowNumber, "bounds": [finite($0.frame.minX), finite(top - $0.frame.maxY), finite($0.frame.width), finite($0.frame.height)]] }
        let pointer = CGEvent(source: nil)?.location
        let noteSelection = (note.currentEditor() as? NSTextView)?.selectedRange()
        let native: [String: Any] = ["counter": counter, "multiline_note": multilineNote.string, "multiline_focused": mainWindow.firstResponder === multilineNote, "note": noteText, "dirty": noteText != cleanNote, "quit": ["visible": quitConfirmation != nil, "requests": quitRequests, "cancellations": quitCancellations], "note_selection": noteSelection.map { ["location": $0.location, "length": $0.length, "unit": "utf16"] as [String: Any] } as Any? ?? NSNull(), "saved_note": savedNote, "saves": saves, "reviewed": reviewed.state == .on, "selection": choice.titleOfSelectedItem ?? "", "slider": slider.doubleValue, "slider_changes": sliderChanges, "audit_index": auditIndex, "audit_count": auditCount, "gestures": gestures, "left_scroll_y": leftScroll.contentView.bounds.origin.y, "right_scroll_y": rightScroll.contentView.bounds.origin.y, "peers": peers.enumerated().map { index, window in ["title": window.title, "counter": peerCounts[index], "note": peerFields[index].currentEditor()?.string ?? peerFields[index].stringValue] as [String: Any] }, "panel": ["kind": panelKind, "result": panelResult, "visible": panel?.isVisible ?? false, "opened_text": openedText, "error": fileError, "file_saves": fileSaves]]
        var chatState = chat.state; chatState["pins"] = chat.pinCount
        let state: [String: Any] = ["schema_version": 1, "case": caseName, "pid": ProcessInfo.processInfo.processIdentifier, "process_birth": fixtureProcessBirth() as Any? ?? NSNull(), "pressed_mouse_buttons": NSEvent.pressedMouseButtons, "mode": mode, "ready": mainWindow.isVisible, "write_error": writeError, "environment": ["pointer_screen": pointer.map { [finite($0.x), finite($0.y)] } as Any? ?? NSNull(), "frontmost_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier as Any? ?? NSNull()], "windows": rows, "anchors": anchors, "native": native, "chat": chatState, "web": ["url": webView.url?.absoluteString ?? webURL, "loaded": loaded, "error": webError]]
        do { let data = try JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]); try data.write(to: root.appendingPathComponent("native-state.json"), options: .atomic) } catch { failIO("publish state", error) }
    }
    func tick() { if let command = try? String(contentsOf: root.appendingPathComponent("command"), encoding: .utf8), command.trimmingCharacters(in: .whitespacesAndNewlines) == "quit" { cleanupRequested = true; NSApp.terminate(nil); return }; publish() }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if cleanupRequested { return .terminateNow }
        guard noteText != cleanNote else { return .terminateNow }
        if quitConfirmation != nil { return .terminateLater }
        quitRequests += 1
        let alert = NSAlert()
        alert.messageText = "Quit with unsaved note?"
        alert.informativeText = "The edited native note has not been committed."
        alert.addButton(withTitle: "Keep Open")
        alert.addButton(withTitle: "Discard and Quit")
        quitConfirmation = alert
        event("quit-confirmation", ["note": noteText]); publish()
        alert.beginSheetModal(for: mainWindow) { [weak self] response in
            guard let self else { return }
            self.quitConfirmation = nil
            let discard = response == .alertSecondButtonReturn
            if !discard { self.quitCancellations += 1 }
            self.event("quit-decision", ["discard": discard, "note": self.noteText]); self.publish()
            sender.reply(toApplicationShouldTerminate: discard)
        }
        return .terminateLater
    }
    func applicationWillTerminate(_ notification: Notification) { timer?.invalidate(); if let monitor { NSEvent.removeMonitor(monitor) }; chat?.closePopup(); event("quit", [:]); do { try journal?.close() } catch { failIO("close journal", error) } }
}
let app = NSApplication.shared
let routingConfiguration = (try? Data(contentsOf: fixtureRoot().appendingPathComponent("config.json")))
    .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
let role = routingConfiguration?["role"] as? String ?? "target"
let delegate: NSObject & NSApplicationDelegate
switch role {
case "witness": delegate = ForegroundWitness()
case "accessory": delegate = ComputerUseAccessory(root: fixtureRoot())
default: delegate = ComputerUseDelegate()
}
app.delegate = delegate
app.setActivationPolicy(role == "accessory" ? .accessory : .regular)
app.run()
