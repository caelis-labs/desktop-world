import AppKit

let args = CommandLine.arguments
func option(_ key: String, _ fallback: String) -> String {
  guard let i = args.firstIndex(of: key), i + 1 < args.count else { return fallback }
  return args[i + 1]
}
let title = option("--title", "DTW Focus POC")
let logPath = option("--log", "/tmp/dtw-focus-poc.jsonl")
let isHuman = option("--human", "0") == "1"
let selfTest = option("--self-test", "0") == "1"
let fixtureVersion = "focus-event-chain-20261010-a"

final class FixtureWindow: NSWindow {
  var eventSink: ((NSEvent) -> Void)?
  override func sendEvent(_ event: NSEvent) {
    if [.keyDown, .keyUp, .leftMouseDown, .leftMouseUp].contains(event.type) {
      eventSink?(event)
    }
    super.sendEvent(event)
  }
}

final class Fixture: NSObject, NSApplicationDelegate, NSTextFieldDelegate {
  var window: FixtureWindow!
  var field: NSTextField!
  var sequence = 0
  var timer: Timer?
  var localMonitor: Any?

  func record(_ event: String, _ extra: [String: Any] = [:]) {
    sequence += 1
    var row: [String: Any] = [
      "event": event, "seq": sequence, "time": Date().timeIntervalSince1970,
      "pid": ProcessInfo.processInfo.processIdentifier,
      "front_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0,
      "active": NSApp.isActive, "key": window?.isKeyWindow ?? false,
      "first_responder": window?.firstResponder.map { String(describing: type(of: $0)) } ?? "none",
      "role": isHuman ? "human" : "target",
      "window_number": window?.windowNumber ?? 0,
    ]
    // Either window may receive accidental private typing. Keep only timing,
    // length, and whether the target still equals the controlled test value.
    let value = field?.stringValue ?? ""
    row["field_length"] = value.count
    if !isHuman { row["matches_expected"] = value == "POC-中文🙂" }
    for (key, value) in extra { row[key] = value }
    guard var data = try? JSONSerialization.data(withJSONObject: row) else { return }
    data.append(10)
    if !FileManager.default.fileExists(atPath: logPath) {
      FileManager.default.createFile(atPath: logPath, contents: nil)
    }
    guard let file = FileHandle(forWritingAtPath: logPath) else { return }
    file.seekToEndOfFile()
    file.write(data)
    try? file.close()
  }

  func applicationDidFinishLaunching(_ notification: Notification) {
    let menu = NSMenu()
    let root = NSMenuItem()
    let appMenu = NSMenu()
    appMenu.addItem(withTitle: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
    root.submenu = appMenu
    menu.addItem(root)
    NSApp.mainMenu = menu
    let visible = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
    let separatedHorizontally = visible.width >= 1000
    let x = isHuman && separatedHorizontally ? visible.maxX - 480 : visible.minX + 20
    let y = isHuman && !separatedHorizontally ? visible.maxY - 260 : visible.minY + 70
    window = FixtureWindow(contentRect: NSRect(x: x, y: y, width: 460, height: 220), styleMask: [.titled, .closable], backing: .buffered, defer: false)
    window.title = title
    window.isReleasedWhenClosed = false
    let instruction = NSTextField(labelWithString: isHuman ? "Click below and type non-sensitive test text" : "DTW-owned target window")
    instruction.frame = NSRect(x: 20, y: 180, width: 420, height: 24)
    window.contentView!.addSubview(instruction)
    field = NSTextField(frame: NSRect(x: 20, y: 145, width: 420, height: 30))
    field.setAccessibilityLabel("POC text")
    field.delegate = self
    window.contentView!.addSubview(field)
    let submit = NSButton(title: "POC submit", target: self, action: #selector(submit))
    submit.frame = NSRect(x: 20, y: 70, width: 140, height: 32)
    window.contentView!.addSubview(submit)
    for (name, label) in [(NSWindow.didBecomeKeyNotification, "became_key"), (NSWindow.didResignKeyNotification, "resigned_key"), (NSWindow.didBecomeMainNotification, "became_main"), (NSWindow.didResignMainNotification, "resigned_main")] {
      NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in self?.record(label) }
    }
    window.eventSink = { [weak self] event in
      let keyCode = [.keyDown, .keyUp].contains(event.type) ? Int(event.keyCode) : -1
      self?.record("window_input", ["type": event.type.rawValue, "key_code": keyCode,
        "event_time": event.timestamp, "event_number": event.eventNumber,
        "x": event.locationInWindow.x, "y": event.locationInWindow.y])
    }
    localMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .leftMouseDown, .leftMouseUp]) { [weak self] event in
      self?.record("local_input", ["type": event.type.rawValue, "event_window_number": event.windowNumber, "event_number": event.eventNumber])
      return event
    }
    timer = Timer.scheduledTimer(withTimeInterval: 0.025, repeats: true) { [weak self] _ in self?.record("seat_sample") }
    if option("--background", isHuman ? "0" : "1") == "1" { window.orderBack(nil) }
    else {
      window.makeKeyAndOrderFront(nil)
      NSApp.activate(ignoringOtherApps: true)
      if isHuman { window.makeFirstResponder(field) }
      // Launch Services can finish registering a freshly built test bundle
      // after didFinishLaunching. Retry once on the running AppKit loop.
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
        guard let self else { return }
        if !NSApp.isActive || !self.window.isKeyWindow {
          NSApp.activate(ignoringOtherApps: true)
          self.window.makeKeyAndOrderFront(nil)
          if isHuman { self.window.makeFirstResponder(self.field) }
          self.record("activation_retry")
        }
      }
    }
    record("ready", ["title": title, "version": fixtureVersion,
      "bundle_path": Bundle.main.bundlePath, "executable_path": Bundle.main.executablePath ?? "",
      "activation_policy": NSApp.activationPolicy().rawValue,
      "window_frame": NSStringFromRect(window.frame), "visible_frame": NSStringFromRect(visible),
      "window_visible": window.isVisible, "field_enabled": field.isEnabled,
      "field_editable": field.isEditable, "field_selectable": field.isSelectable,
      "editor_present": field.currentEditor() != nil,
      "window_class": String(describing: type(of: window)),
      "event_sink_present": window.eventSink != nil])
    if selfTest {
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
        guard let self else { return }
        // The event is queued only inside this process and names only its own
        // window. It cannot move the pointer or type into another application.
        let point = NSPoint(x: 400, y: 30)
        guard let event = NSEvent.mouseEvent(with: .leftMouseDown, location: point,
          modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
          windowNumber: self.window.windowNumber, context: nil, eventNumber: 704201,
          clickCount: 1, pressure: 1) else {
          self.record("self_post_failed")
          return
        }
        self.record("self_post", ["event_number": event.eventNumber])
        NSApp.postEvent(event, atStart: false)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { [weak self] in
          guard let self else { return }
          self.record("self_direct_send", ["event_number": event.eventNumber])
          self.window.sendEvent(event)
        }
      }
    }
  }

  func controlTextDidBeginEditing(_ notification: Notification) { record("begin_edit") }
  func controlTextDidChange(_ notification: Notification) {
    let editor = window.firstResponder as? NSTextView
    let marked = editor?.markedRange() ?? NSRange(location: NSNotFound, length: 0)
    record("text_change", ["marked_location": marked.location, "marked_length": marked.length])
  }
  func controlTextDidEndEditing(_ notification: Notification) { record("end_edit") }
  @objc func submit() { record("submit") }
}

let app = NSApplication.shared
let delegate = Fixture()
app.setActivationPolicy(.regular)
app.delegate = delegate
app.run()
