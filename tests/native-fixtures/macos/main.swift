import AppKit

// All evidence is written by the controlled app, independently of AX readback.
let args = CommandLine.arguments
func argument(_ name: String, _ fallback: String) -> String {
  guard let i = args.firstIndex(of: name), i + 1 < args.count else { return fallback }
  return args[i + 1]
}
let fixtureTitle = argument("--title", "Desktop World Native Fixture")
let logPath = argument("--log", NSTemporaryDirectory() + "desktop-world-fixture.jsonl")
func record(_ event: String, _ value: String = "") {
  let row: [String: Any] = [
    "event": event, "value": value, "pid": ProcessInfo.processInfo.processIdentifier,
    "at": ISO8601DateFormatter().string(from: Date()),
  ]
  guard var data = try? JSONSerialization.data(withJSONObject: row) else { return }
  data.append(10)
  if !FileManager.default.fileExists(atPath: logPath) {
    FileManager.default.createFile(atPath: logPath, contents: nil)
  }
  guard let file = FileHandle(forWritingAtPath: logPath) else { return }
  defer { try? file.close() }
  file.seekToEndOfFile()
  file.write(data)
}
typealias Field = NSTextField
final class Delegate: NSObject, NSApplicationDelegate, NSTextFieldDelegate {
  var window: NSWindow!
  var field = Field(frame: NSRect(x: 24, y: 210, width: 430, height: 30))
  var status = NSTextField(labelWithString: "ready")
  var count = 0
  var monitor: Any?
  func applicationDidFinishLaunching(_ notification: Notification) {
    let menu = NSMenu()
    let appItem = NSMenuItem()
    let appMenu = NSMenu()
    appMenu.addItem(
      withTitle: "退出", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
    appItem.submenu = appMenu
    menu.addItem(appItem)
    let editItem = NSMenuItem()
    let edit = NSMenu(title: "编辑")
    edit.addItem(withTitle: "剪切", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
    edit.addItem(withTitle: "复制", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
    edit.addItem(withTitle: "粘贴", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
    edit.addItem(withTitle: "全选", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
    editItem.submenu = edit
    menu.addItem(editItem)
    NSApp.mainMenu = menu
    window = NSWindow(
      contentRect: NSRect(x: 180, y: 200, width: 500, height: 310),
      styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false
    )
    window.title = fixtureTitle
    window.isReleasedWhenClosed = false
    let heading = NSTextField(labelWithString: "Desktop World · native fixture")
    heading.frame = NSRect(x: 24, y: 260, width: 440, height: 24)
    window.contentView?.addSubview(heading)
    field.setAccessibilityLabel("内容")
    field.target = self
    field.action = #selector(submit)
    field.delegate = self
    window.contentView?.addSubview(field)
    let submitButton = NSButton(title: "提交", target: self, action: #selector(submit))
    submitButton.frame = NSRect(x: 24, y: 160, width: 100, height: 32)
    window.contentView?.addSubview(submitButton)
    let replace = NSButton(title: "替换输入框", target: self, action: #selector(replaceField))
    replace.frame = NSRect(x: 140, y: 160, width: 130, height: 32)
    window.contentView?.addSubview(replace)
    let delayed = NSButton(title: "延迟状态", target: self, action: #selector(delayStatus))
    delayed.frame = NSRect(x: 290, y: 160, width: 120, height: 32)
    window.contentView?.addSubview(delayed)
    status.frame = NSRect(x: 24, y: 110, width: 440, height: 24)
    status.setAccessibilityLabel("状态")
    window.contentView?.addSubview(status)
    let popup = NSPopUpButton(frame: NSRect(x: 24, y: 52, width: 220, height: 28))
    popup.addItems(withTitles: ["第一项", "第二项", "第三项"])
    popup.setAccessibilityLabel("选择")
    window.contentView?.addSubview(popup)
    monitor = NSEvent.addLocalMonitorForEvents(matching: [
      .keyDown, .leftMouseDown, .leftMouseUp, .leftMouseDragged, .scrollWheel,
    ]) { event in
      if event.type == .keyDown {
        record("key_down", event.characters ?? "")
      } else {
        record("pointer", String(event.type.rawValue))
      }
      return event
    }
    window.makeKeyAndOrderFront(nil)
    NSApp.activate(ignoringOtherApps: true)
    record("ready", fixtureTitle)
  }
  @objc func submit() {
    count += 1
    status.stringValue = "submitted:\(count)"
    record("submit", field.stringValue)
  }
  @objc func replaceField() {
    field.removeFromSuperview()
    field = Field(frame: NSRect(x: 24, y: 210, width: 430, height: 30))
    field.setAccessibilityLabel("内容")
    field.target = self
    field.action = #selector(submit)
    field.delegate = self
    window.contentView?.addSubview(field)
    record("replaced")
  }
  @objc func delayStatus() {
    DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
      self.status.stringValue = "delayed"
      record("delayed")
    }
  }
  func controlTextDidChange(_ obj: Notification) { record("text_changed", field.stringValue) }
  func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}
let app = NSApplication.shared
let delegate = Delegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
