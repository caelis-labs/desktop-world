import AppKit

// Two owned, nonactivating AppKit windows for window-content occlusion proof.
// No input monitor, simulated events, user text, or unrelated app data.
let args = CommandLine.arguments
func option(_ name: String, _ fallback: String) -> String {
  guard let i = args.firstIndex(of: name), i + 1 < args.count else { return fallback }
  return args[i + 1]
}
let targetTitle = option("--title", "DTW Own Occlusion POC")
let logPath = option("--log", "/private/tmp/dtw-own-occlusion.jsonl")
let controlPath = option("--control-file", "/private/tmp/dtw-own-occlusion-control.txt")

final class Fixture: NSObject, NSApplicationDelegate {
  var target: NSWindow!
  var cover: NSWindow!
  var timer: Timer?
  var sequence = 0

  func record(_ event: String) {
    sequence += 1
    let ordered = NSApp.orderedWindows.map(\.windowNumber)
    let row: [String: Any] = [
      "event": event, "seq": sequence, "time": Date().timeIntervalSince1970,
      "pid": ProcessInfo.processInfo.processIdentifier,
      "active": NSApp.isActive, "target_key": target?.isKeyWindow ?? false,
      "cover_key": cover?.isKeyWindow ?? false,
      "target_window": target?.windowNumber ?? 0,
      "cover_window": cover?.windowNumber ?? 0,
      "target_visible": target?.isVisible ?? false,
      "cover_visible": cover?.isVisible ?? false,
      "own_window_order": ordered,
      "cover_above_target": (ordered.firstIndex(of: cover?.windowNumber ?? 0) ?? Int.max)
        < (ordered.firstIndex(of: target?.windowNumber ?? 0) ?? Int.max),
    ]
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
    let frame = NSRect(x: visible.minX + 30, y: visible.minY + 180,
      width: 360, height: 160)
    target = NSWindow(contentRect: frame, styleMask: [.titled, .closable],
      backing: .buffered, defer: false)
    target.title = targetTitle
    target.isReleasedWhenClosed = false
    target.contentView?.wantsLayer = true
    target.contentView?.layer?.backgroundColor = NSColor.systemRed.cgColor

    cover = NSWindow(contentRect: frame, styleMask: [.titled, .closable],
      backing: .buffered, defer: false)
    cover.title = "DTW Own Occluder POC"
    cover.isReleasedWhenClosed = false
    cover.contentView?.wantsLayer = true
    cover.contentView?.layer?.backgroundColor = NSColor.systemBlue.cgColor

    target.orderFront(nil)
    record("ready")
    timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
      guard let self,
        let command = try? String(contentsOfFile: controlPath, encoding: .utf8) else { return }
      try? FileManager.default.removeItem(atPath: controlPath)
      switch command.trimmingCharacters(in: .whitespacesAndNewlines) {
      case "cover":
        self.cover.orderFront(nil)
        self.record("covered")
      case "uncover":
        self.cover.orderOut(nil)
        self.record("uncovered")
      case "close":
        self.cover.close()
        self.target.close()
        self.record("closed")
      default:
        self.record("unknown_control")
      }
    }
  }
}

let app = NSApplication.shared
let delegate = Fixture()
app.setActivationPolicy(.accessory)
app.delegate = delegate
app.run()
