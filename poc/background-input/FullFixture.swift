import AppKit
import WebKit

let arguments = CommandLine.arguments
func option(_ key: String, _ fallback: String) -> String {
  guard let i = arguments.firstIndex(of: key), i + 1 < arguments.count else { return fallback }
  return arguments[i + 1]
}
let title = option("--title", "DTW Full POC")
let log = option("--log", "/tmp/dtw-full.jsonl")
func record(_ event: String, _ value: String = "") {
  var bytes = try! JSONSerialization.data(withJSONObject: [
    "event": event, "value": value, "pid": ProcessInfo.processInfo.processIdentifier,
    "time": Date().timeIntervalSince1970,
  ])
  bytes.append(10)
  if !FileManager.default.fileExists(atPath: log) {
    FileManager.default.createFile(atPath: log, contents: nil)
  }
  let f = FileHandle(forWritingAtPath: log)!
  f.seekToEndOfFile()
  f.write(bytes)
  try? f.close()
}
final class Canvas: NSView {
  var origin = NSPoint.zero, position = NSPoint(x: 40, y: 80)
  override var acceptsFirstResponder: Bool { true }
  override func isAccessibilityElement() -> Bool { true }
  override func accessibilityRole() -> NSAccessibility.Role? { .image }
  override func accessibilityLabel() -> String? { "POC Canvas" }
  override func updateTrackingAreas() {
    super.updateTrackingAreas()
    trackingAreas.forEach { removeTrackingArea($0) }
    addTrackingArea(
      NSTrackingArea(rect: bounds, options: [.mouseMoved, .activeAlways], owner: self))
  }
  override func draw(_ rect: NSRect) {
    NSBezierPath(rect: bounds).addClip()
    NSColor.darkGray.setFill()
    bounds.fill()
    NSColor.systemGreen.setFill()
    NSBezierPath(ovalIn: NSRect(x: position.x - 12, y: position.y - 12, width: 24, height: 24))
      .fill()
  }
  override func mouseMoved(with e: NSEvent) { record("move") }
  override func mouseDown(with e: NSEvent) {
    window?.makeFirstResponder(self)
    origin = convert(e.locationInWindow, from: nil)
    record("click", String(e.clickCount))
    if e.clickCount == 2 { record("double") }
  }
  override func mouseDragged(with e: NSEvent) {
    position = convert(e.locationInWindow, from: nil)
    record("drag")
    needsDisplay = true
  }
  override func mouseUp(with e: NSEvent) {
    let p = convert(e.locationInWindow, from: nil)
    record("drop", String(Int(p.x - origin.x)) + "," + String(Int(p.y - origin.y)))
  }
  override func otherMouseDown(with e: NSEvent) { record("middle", String(e.buttonNumber)) }
  override func rightMouseDown(with e: NSEvent) {
    record("right")
    let m = NSMenu()
    let item = NSMenuItem(title: "POC menu commit", action: #selector(commit), keyEquivalent: "")
    item.target = self
    m.addItem(item)
    NSMenu.popUpContextMenu(m, with: e, for: self)
  }
  @objc func commit() { record("menu_commit") }
  override func scrollWheel(with e: NSEvent) {
    record("scroll", String(Double(e.scrollingDeltaX)) + "," + String(Double(e.scrollingDeltaY)))
    position.y += e.scrollingDeltaY
    needsDisplay = true
  }
  override func keyDown(with e: NSEvent) { record("canvas_key", e.characters ?? "") }
}
final class Delegate: NSObject, NSApplicationDelegate, NSTextViewDelegate, NSTextFieldDelegate {
  var window: NSWindow!, timer: Timer?, field = NSTextField(), text = NSTextView(), sheet: NSWindow?
  func applicationDidFinishLaunching(_ n: Notification) {
    let menu = NSMenu()
    let root = NSMenuItem()
    let app = NSMenu()
    app.addItem(
      withTitle: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
    root.submenu = app
    menu.addItem(root)
    let editRoot = NSMenuItem()
    let edit = NSMenu(title: "Edit")
    edit.addItem(
      withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
    editRoot.submenu = edit
    menu.addItem(editRoot)
    NSApp.mainMenu = menu
    window = NSWindow(
      contentRect: NSRect(x: 180, y: 200, width: 500, height: 530),
      styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
    window.title = title
    window.isReleasedWhenClosed = false
    window.acceptsMouseMovedEvents = true
    field.frame = NSRect(x: 24, y: 450, width: 430, height: 30)
    field.setAccessibilityLabel("POC text")
    field.delegate = self
    window.contentView!.addSubview(field)
    text.frame = NSRect(x: 24, y: 350, width: 430, height: 85)
    text.setAccessibilityLabel("POC multiline")
    text.delegate = self
    window.contentView!.addSubview(text)
    let canvas = Canvas(frame: NSRect(x: 24, y: 100, width: 430, height: 210))
    window.contentView!.addSubview(canvas)
    let submit = NSButton(title: "POC submit", target: self, action: #selector(save))
    submit.frame = NSRect(x: 24, y: 40, width: 130, height: 32)
    window.contentView!.addSubview(submit)
    let dialog = NSButton(title: "POC dialog", target: self, action: #selector(openDialog))
    dialog.frame = NSRect(x: 200, y: 40, width: 130, height: 32)
    window.contentView!.addSubview(dialog)
    if option("--background", "0") == "1" {
      window.orderBack(nil)
    } else {
      window.makeKeyAndOrderFront(nil)
      NSApp.activate(ignoringOtherApps: true)
    }
    if option("--human", "0") == "1" {
      timer = Timer.scheduledTimer(withTimeInterval: 0.02, repeats: true) { [weak self] _ in
        guard let w = self?.window else { return }
        let p = CGEvent(source: nil)?.location ?? .zero
        let data = try! JSONSerialization.data(withJSONObject: [
          "front_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0,
          "active": NSApp.isActive, "key": w.isKeyWindow, "x": p.x, "y": p.y,
        ])
        record("seat", String(data: data, encoding: .utf8)!)
      }
    }
    record("ready", title)
  }
  func controlTextDidChange(_ n: Notification) { record("text", field.stringValue) }
  func textDidChange(_ n: Notification) { record("multiline", text.string) }
  @objc func save() { record("submit", field.stringValue) }
  @objc func openDialog() {
    let s = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 240, height: 140), styleMask: [.titled],
      backing: .buffered, defer: false)
    s.title = title + " Dialog"
    s.isReleasedWhenClosed = false
    let f = NSTextField(frame: NSRect(x: 20, y: 90, width: 200, height: 28))
    f.setAccessibilityLabel("POC dialog text")
    s.contentView!.addSubview(f)
    let b = NSButton(title: "POC confirm", target: self, action: #selector(confirm))
    b.frame = NSRect(x: 40, y: 40, width: 160, height: 36)
    s.contentView!.addSubview(b)
    sheet = s
    window.beginSheet(s)
    record("dialog_open")
  }
  @objc func confirm() {
    if let s = sheet {
      if let field = s.contentView?.subviews.first(where: { $0 is NSTextField }) as? NSTextField {
        record("dialog_text", field.stringValue)
      }
      window.endSheet(s)
      s.orderOut(nil)
      sheet = nil
    }
    record("dialog_confirm")
  }
}
let app = NSApplication.shared
let delegate = Delegate()
app.setActivationPolicy(.regular)
app.delegate = delegate
app.run()
