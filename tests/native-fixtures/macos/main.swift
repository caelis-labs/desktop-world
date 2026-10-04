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
final class SlowField: NSTextField {
  override func accessibilityValue() -> String? {
    if argument("--trace-reads", "0") == "1" { record("value_read", accessibilityLabel() ?? "slow") }
    Thread.sleep(forTimeInterval: 0.025)
    return super.accessibilityValue()
  }
}
final class Disclosure: NSView {
  var expanded = false
  let details = NSTextField(labelWithString: "Shipping details ready")
  override func isAccessibilityElement() -> Bool { true }
  override func accessibilityRole() -> NSAccessibility.Role? { .disclosureTriangle }
  override func accessibilityLabel() -> String? { "Shipping details" }
  override func isAccessibilityExpanded() -> Bool { expanded }
  override func setAccessibilityExpanded(_ value: Bool) {
    expanded = value
    details.isHidden = !value
    record("expanded", value ? "true" : "false")
    record("details_visible", details.isHidden ? "false" : "true")
    NSAccessibility.post(element: self, notification: .valueChanged)
  }
  override func isAccessibilitySelectorAllowed(_ selector: Selector) -> Bool {
    if selector == #selector(setAccessibilityExpanded(_:)) { return true }
    return super.isAccessibilitySelectorAllowed(selector)
  }
}
typealias Field = NSTextField
final class Delegate: NSObject, NSApplicationDelegate, NSTextFieldDelegate {
  var window: NSWindow!
  var field = Field(frame: NSRect(x: 24, y: 210, width: 430, height: 30))
  var status = NSTextField(labelWithString: "ready")
  var count = 0
  var checkbox: NSButton!
  var mixed: NSButton!
  var slider: NSSlider!
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
      contentRect: NSRect(x: 180, y: 200, width: 500, height: 530),
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
    checkbox = NSButton(checkboxWithTitle: "Value checkbox", target: self, action: #selector(toggleValue))
    checkbox.frame = NSRect(x: 24, y: 320, width: 210, height: 28)
    window.contentView?.addSubview(checkbox)
    mixed = NSButton(checkboxWithTitle: "Mixed checkbox", target: self, action: #selector(toggleMixed))
    mixed.allowsMixedState = true
    mixed.state = .mixed
    mixed.frame = NSRect(x: 250, y: 320, width: 210, height: 28)
    window.contentView?.addSubview(mixed)
    slider = NSSlider(value: 37, minValue: 0, maxValue: 100, target: nil, action: nil)
    slider.setAccessibilityLabel("Numeric slider")
    slider.frame = NSRect(x: 24, y: 360, width: 200, height: 28)
    window.contentView?.addSubview(slider)
    let labelOnly = NSButton(title: "Label only", target: self, action: #selector(labelAction))
    labelOnly.frame = NSRect(x: 250, y: 360, width: 160, height: 28)
    window.contentView?.addSubview(labelOnly)
    let secret = NSSecureTextField(frame: NSRect(x: 24, y: 400, width: 200, height: 28))
    secret.setAccessibilityLabel("Protected field")
    secret.stringValue = "fixture-secret"
    window.contentView?.addSubview(secret)
    let boundary = NSTextField(labelWithString: String(repeating: "x", count: 383) + "🌍tail")
    boundary.frame = NSRect(x: 250, y: 400, width: 200, height: 28)
    boundary.setAccessibilityLabel("Unicode preview boundary")
    window.contentView?.addSubview(boundary)
    let disclosure = Disclosure(frame: NSRect(x: 24, y: 470, width: 420, height: 28))
    disclosure.details.frame = NSRect(x: 24, y: 440, width: 420, height: 24)
    disclosure.details.isHidden = true
    window.contentView?.addSubview(disclosure)
    window.contentView?.addSubview(disclosure.details)
    let slowCount = Int(argument("--slow-count", "0")) ?? 0
    for i in 0..<slowCount {
      let slow = SlowField(labelWithString: "Slow row \(i)")
      slow.frame = NSRect(x: 24, y: 10, width: 200, height: 20)
      slow.setAccessibilityLabel("Slow row \(i)")
      window.contentView?.addSubview(slow)
    }
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
    if argument("--background", "0") == "1" {
      window.orderBack(nil)
    } else {
      window.makeKeyAndOrderFront(nil)
      NSApp.activate(ignoringOtherApps: true)
    }
    record("ready", fixtureTitle)
  }
  @objc func toggleValue() { record("checkbox", String(checkbox.state.rawValue)) }
  @objc func toggleMixed() { record("mixed", String(mixed.state.rawValue)) }
  @objc func labelAction() { record("label_action") }
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
