import AppKit

// A single standard window and input field, launched without app activation.
// No event tap, sendEvent override, simulated input, or text-value logging.
let args = CommandLine.arguments
func option(_ name: String, _ fallback: String) -> String {
  guard let index = args.firstIndex(of: name), index + 1 < args.count else { return fallback }
  return args[index + 1]
}
let title = option("--title", "DTW AX Minimal POC")
let logPath = option("--log", "/private/tmp/dtw-ax-minimal-\(ProcessInfo.processInfo.processIdentifier).jsonl")
let controlPath = option("--control-file", "")
#if AX_OVERRIDE
let axCallPath = option("--ax-call-log", "/private/tmp/dtw-ax-call-\(ProcessInfo.processInfo.processIdentifier).jsonl")
@objc(AXDiagnosticApplication)
final class AXDiagnosticApplication: NSApplication {
  override func accessibilityWindows() -> [Any]? {
    let values = super.accessibilityWindows()
    let row: [String: Any] = ["method": "accessibilityWindows", "time": Date().timeIntervalSince1970,
      "pid": ProcessInfo.processInfo.processIdentifier, "main_thread": Thread.isMainThread,
      "count": values?.count ?? 0, "window_numbers": (values ?? []).compactMap { ($0 as? NSWindow)?.windowNumber }]
    if var bytes = try? JSONSerialization.data(withJSONObject: row) {
      bytes.append(10)
      if !FileManager.default.fileExists(atPath: axCallPath) {
        FileManager.default.createFile(atPath: axCallPath, contents: nil)
      }
      if let file = FileHandle(forWritingAtPath: axCallPath) {
        file.seekToEndOfFile()
        file.write(bytes)
        try? file.close()
      }
    }
    return values
  }
}
#endif

final class Fixture: NSObject, NSApplicationDelegate {
  var window: NSWindow!
  var field: NSTextField!
  var timer: Timer?
  var sequence = 0
  var ticks = 0

  func record(_ event: String, _ extra: [String: Any] = [:]) {
    sequence += 1
    var row: [String: Any] = [
      "event": event, "seq": sequence, "time": Date().timeIntervalSince1970,
      "pid": ProcessInfo.processInfo.processIdentifier,
      "main_thread": Thread.isMainThread,
      "app_running": NSApp.isRunning, "app_active": NSApp.isActive,
      "app_class": String(describing: type(of: NSApp!)),
      "activation_policy": NSApp.activationPolicy().rawValue,
      "app_window_count": NSApp.windows.count,
      "window_number": window?.windowNumber ?? 0,
      "window_visible": window?.isVisible ?? false,
      "window_key": window?.isKeyWindow ?? false,
      "window_main": window?.isMainWindow ?? false,
      "field_editable": field?.isEditable ?? false,
      "field_length": field?.stringValue.count ?? 0,
      "bundle_path": Bundle.main.bundlePath,
      "executable_path": Bundle.main.executablePath ?? "",
      "version": "ax-minimal-20261010-e-quiet-prime",
    ]
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
    let frame = NSRect(x: 30, y: 180, width: 360, height: 160)
    window = NSWindow(contentRect: frame, styleMask: [.titled, .closable],
      backing: .buffered, defer: false)
    window.title = title
    window.isReleasedWhenClosed = false
    field = NSTextField(frame: NSRect(x: 20, y: 80, width: 320, height: 30))
    field.setAccessibilityLabel("Minimal input")
    window.contentView?.addSubview(field)
    window.orderFront(nil)
    record("ready", ["title": title])
    timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
      guard let self else { return }
      self.ticks += 1
      if self.ticks % 10 == 0 { self.record("runloop_tick", ["ticks": self.ticks]) }
      guard !controlPath.isEmpty,
        let command = try? String(contentsOfFile: controlPath, encoding: .utf8) else { return }
      try? FileManager.default.removeItem(atPath: controlPath)
      switch command.trimmingCharacters(in: .whitespacesAndNewlines) {
      case "inspect":
        let accessible = NSApp.accessibilityWindows() ?? []
        let native = NSApp.windows
        self.record("self_accessibility_inspect", [
          "accessibility_window_count": accessible.count,
          "accessibility_window_native_matches": accessible.filter { item in
            guard let candidate = item as? NSWindow else { return false }
            return candidate === self.window
          }.count,
          "accessibility_window_types": accessible.map { String(describing: type(of: $0)) },
          "native_window_numbers": native.map(\.windowNumber),
          "accessibility_main_is_window": (NSApp.accessibilityMainWindow() as? NSWindow) === self.window,
          "window_ax_role": self.window.accessibilityRole()?.rawValue ?? "unavailable",
          "window_is_ax_element": self.window.isAccessibilityElement(),
        ])
      case "order_back":
        self.window.orderBack(nil)
        self.record("ordered_back")
      case "order_front":
        self.window.orderFront(nil)
        self.record("ordered_front")
      case "make_main":
        self.window.makeMain()
        self.record("made_main")
      case "prime_once":
        let kinds: [CGEventType] = [.keyDown, .leftMouseDown, .rightMouseDown,
          .otherMouseDown, .mouseMoved, .scrollWheel]
        func activityAge() -> Double {
          kinds.map { CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: $0) }
            .filter { $0 >= 0 }.min() ?? 1e9
        }
        let age = activityAge()
        guard age >= 0.8 else {
          self.record("prime_refused_user_active", ["activity_age_ms": Int(age * 1000)])
          break
        }
        let prior = NSWorkspace.shared.frontmostApplication
        let began = ProcessInfo.processInfo.systemUptime
        self.window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        self.record("prime_requested", ["activity_age_ms": Int(age * 1000)])
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
          let current = NSWorkspace.shared.frontmostApplication
          let selfFront = current?.processIdentifier == ProcessInfo.processInfo.processIdentifier
          let userActive = activityAge() < 0.15
          self.window.orderBack(nil)
          let restoreRequested = selfFront && (prior?.activate(options: []) ?? false)
          self.record("prime_finished", ["self_front_at_end": selfFront,
            "user_active_during_borrow": userActive,
            "restore_requested": restoreRequested,
            "elapsed_ms": Int((ProcessInfo.processInfo.systemUptime - began) * 1000)])
        }
      default:
        self.record("unknown_control")
      }
    }
  }
}

#if AX_OVERRIDE
let app = AXDiagnosticApplication.shared
#else
let app = NSApplication.shared
#endif
let delegate = Fixture()
app.setActivationPolicy(.regular)
app.delegate = delegate
app.run()
