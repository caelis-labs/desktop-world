import AppKit

// Build without EVENT_PROBE for the plain control. Build with EVENT_PROBE to
// change only the window's sendEvent instrumentation; both show one text box.
#if EVENT_PROBE || DEFERRED_EVENT_PROBE
#if DEFERRED_EVENT_PROBE
let fixtureTitle = "DTW Deferred Event Input POC"
let fixtureVersion = "minimal-input-20261010-e-deferred"
#else
let fixtureTitle = "DTW Event Input POC"
let fixtureVersion = "minimal-input-20261010-b-event"
#endif
final class EventInputWindow: NSWindow {
  var eventSink: (([String: Any]) -> Void)?
  override func sendEvent(_ event: NSEvent) {
    var metadata: [String: Any]? = nil
    if [.keyDown, .keyUp, .leftMouseDown, .leftMouseUp].contains(event.type) {
      metadata = ["event_type": event.type.rawValue, "event_time": event.timestamp]
      if [.keyDown, .keyUp].contains(event.type) {
        metadata?["key_code"] = Int(event.keyCode)
      } else {
        metadata?["event_number"] = event.eventNumber
      }
    }
    #if DEFERRED_EVENT_PROBE
    super.sendEvent(event)
    if let metadata {
      DispatchQueue.main.async { [weak self] in self?.eventSink?(metadata) }
    }
    #else
    if let metadata { eventSink?(metadata) }
    super.sendEvent(event)
    #endif
  }
}
#elseif SUBCLASS_ONLY
let fixtureTitle = "DTW Subclass Input POC"
let fixtureVersion = "minimal-input-20261010-c-subclass"
final class BareInputWindow: NSWindow {}
#elseif OVERRIDE_ONLY
let fixtureTitle = "DTW Override Input POC"
let fixtureVersion = "minimal-input-20261010-d-override"
final class ForwardInputWindow: NSWindow {
  override func sendEvent(_ event: NSEvent) { super.sendEvent(event) }
}
#else
let fixtureTitle = "DTW Minimal Input POC"
let fixtureVersion = "minimal-input-20261010-b-plain"
#endif

final class MinimalInputFixture: NSObject, NSApplicationDelegate, NSTextFieldDelegate {
  var window: NSWindow!
  var field: NSTextField!
  var sequence = 0
  let logPath = "/private/tmp/dtw-minimal-input-\(ProcessInfo.processInfo.processIdentifier).jsonl"

  func record(_ event: String, _ extra: [String: Any] = [:]) {
    sequence += 1
    var row: [String: Any] = [
      "event": event,
      "seq": sequence,
      "time": Date().timeIntervalSince1970,
      "pid": ProcessInfo.processInfo.processIdentifier,
      "title": window?.title ?? "",
      "front_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0,
      "active": NSApp.isActive,
      "key": window?.isKeyWindow ?? false,
      "first_responder": window?.firstResponder.map { String(describing: type(of: $0)) } ?? "none",
      "field_length": field?.stringValue.count ?? 0,
      "field_editable": field?.isEditable ?? false,
      "field_enabled": field?.isEnabled ?? false,
      "bundle_path": Bundle.main.bundlePath,
      "version": fixtureVersion,
    ]
    for (key, value) in extra { row[key] = value }
    guard var bytes = try? JSONSerialization.data(withJSONObject: row) else { return }
    bytes.append(10)
    if !FileManager.default.fileExists(atPath: logPath) {
      FileManager.default.createFile(atPath: logPath, contents: nil)
    }
    guard let file = FileHandle(forWritingAtPath: logPath) else { return }
    file.seekToEndOfFile()
    file.write(bytes)
    try? file.close()
  }

  func applicationDidFinishLaunching(_ notification: Notification) {
    let visible = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
    let rect = NSRect(x: visible.midX - 240, y: visible.midY - 70, width: 480, height: 140)
    #if EVENT_PROBE || DEFERRED_EVENT_PROBE
    window = EventInputWindow(contentRect: rect, styleMask: [.titled, .closable], backing: .buffered, defer: false)
    (window as? EventInputWindow)?.eventSink = { [weak self] metadata in
      self?.record("window_event", metadata)
    }
    #elseif SUBCLASS_ONLY
    window = BareInputWindow(contentRect: rect, styleMask: [.titled, .closable], backing: .buffered, defer: false)
    #elseif OVERRIDE_ONLY
    window = ForwardInputWindow(contentRect: rect, styleMask: [.titled, .closable], backing: .buffered, defer: false)
    #else
    window = NSWindow(contentRect: rect, styleMask: [.titled, .closable], backing: .buffered, defer: false)
    #endif
    window.title = fixtureTitle
    window.isReleasedWhenClosed = false
    field = NSTextField(frame: NSRect(x: 20, y: 55, width: 440, height: 32))
    field.setAccessibilityLabel("Minimal input")
    field.delegate = self
    window.contentView!.addSubview(field)
    for (name, label) in [
      (NSWindow.didBecomeKeyNotification, "became_key"),
      (NSWindow.didResignKeyNotification, "resigned_key"),
    ] {
      NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in
        self?.record(label)
      }
    }
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    window.makeFirstResponder(field)
    record("ready")
  }

  func controlTextDidChange(_ notification: Notification) { record("text_change") }
  func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

let app = NSApplication.shared
let delegate = MinimalInputFixture()
app.setActivationPolicy(.regular)
app.delegate = delegate
app.run()
