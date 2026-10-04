import AppKit
import WebKit

// All evidence is written by the controlled app, independently of AX readback.
let args = CommandLine.arguments
func argument(_ name: String, _ fallback: String) -> String {
  guard let i = args.firstIndex(of: name), i + 1 < args.count else { return fallback }
  return args[i + 1]
}
let fixtureTitle = argument("--title", "Desktop World Native Fixture")
let semanticCase = argument("--semantic-case", "")
let logPath = argument("--log", NSTemporaryDirectory() + "desktop-world-fixture.jsonl")
func record(_ event: String, _ value: String = "") {
  let row: [String: Any] = [
    "event": event, "value": value, "pid": ProcessInfo.processInfo.processIdentifier,
    "at": ISO8601DateFormatter().string(from: Date()),
    "time": Date().timeIntervalSince1970,
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
final class OrderRow: NSView {
  let order: String
  var selected = false
  init(_ name: String, y: CGFloat) {
    order = name
    super.init(frame: NSRect(x: 24, y: y, width: 430, height: 40))
    let label = NSTextField(labelWithString: name)
    label.frame = bounds.insetBy(dx: 8, dy: 8)
    addSubview(label)
  }
  required init?(coder: NSCoder) { fatalError("unused") }
  override func isAccessibilityElement() -> Bool { true }
  override func accessibilityRole() -> NSAccessibility.Role? { .row }
  override func accessibilityLabel() -> String? { order }
  override func isAccessibilitySelected() -> Bool { selected }
  override func setAccessibilitySelected(_ value: Bool) {
    selected = value
    record("selected", order + ":" + String(value))
    needsDisplay = true
    NSAccessibility.post(element: self, notification: .selectedRowsChanged)
  }
  override func isAccessibilitySelectorAllowed(_ selector: Selector) -> Bool {
    if selector == #selector(setAccessibilitySelected(_:)) { return true }
    return super.isAccessibilitySelectorAllowed(selector)
  }
  override func draw(_ rect: NSRect) {
    (selected ? NSColor.selectedContentBackgroundColor : NSColor.controlBackgroundColor).setFill()
    rect.fill()
    super.draw(rect)
  }
}
final class CaptureCanvas: NSView {
  var green = false
  override func draw(_ rect: NSRect) {
    (green ? NSColor(srgbRed: 0, green: 0.85, blue: 0, alpha: 1) : NSColor(srgbRed: 0.9, green: 0, blue: 0, alpha: 1)).setFill()
    rect.fill()
  }
}
final class Delegate: NSObject, NSApplicationDelegate, NSTextFieldDelegate, WKScriptMessageHandler, WKNavigationDelegate {
  var window: NSWindow!
  var canvasWindow: NSWindow!
  var canvas: CaptureCanvas!
  var capturePopup: NSWindow?
  var captureSheet: NSWindow?
  var field = Field(frame: NSRect(x: 24, y: 210, width: 430, height: 30))
  var status = NSTextField(labelWithString: "ready")
  var count = 0
  var checkbox: NSButton!
  var mixed: NSButton!
  var slider: NSSlider!
  var monitor: Any?
  var pocSeatTimer: Timer?
  var orders: [OrderRow] = []
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
    if !semanticCase.isEmpty { setupSemanticCase() }
    monitor = NSEvent.addLocalMonitorForEvents(matching: [
      .keyDown, .leftMouseDown, .leftMouseUp, .leftMouseDragged, .scrollWheel,
    ]) { event in
      if event.type == .keyDown {
        record("key_down", event.characters ?? "")
      } else {
        record("pointer", String(event.type.rawValue))
        if argument("--poc-log-input", "0") == "1" {
          record("pointer_location", "\(event.windowNumber):\(event.locationInWindow.x),\(event.locationInWindow.y)")
        }
      }
      return event
    }
    if argument("--background", "0") == "1" {
      window.orderBack(nil)
    } else {
      window.makeKeyAndOrderFront(nil)
      NSApp.activate(ignoringOtherApps: true)
    }
    if argument("--poc-observe-seat", "0") == "1" {
      pocSeatTimer = Timer.scheduledTimer(withTimeInterval: 0.02, repeats: true) { [weak self] _ in
        guard let self = self else { return }
        let point = CGEvent(source: nil)?.location ?? .zero
        let sample: [String: Any] = ["front_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0,
                                   "active": NSApp.isActive, "key": self.window.isKeyWindow,
                                   "x": point.x, "y": point.y]
        if let data = try? JSONSerialization.data(withJSONObject: sample), let value = String(data: data, encoding: .utf8) {
          record("seat_sample", value)
        }
      }
    }
    if semanticCase != "scroll" && semanticCase != "input-poc" { record("ready", fixtureTitle) }
  }
  @objc func toggleValue() { record("checkbox", String(checkbox.state.rawValue)) }
  @objc func toggleMixed() { record("mixed", String(mixed.state.rawValue)) }
  @objc func labelAction() { record("label_action") }
  @objc func submit() {
    if semanticCase == "selection" {
      record("ordered", orders.filter { $0.selected }.map { $0.order }.joined(separator: ","))
      return
    }
    if semanticCase == "check" {
      record("approved", checkbox.state == .on ? "order-42" : "ERROR:unchecked")
      return
    }
    count += 1
    status.stringValue = "submitted:\(count)"
    record("submit", field.stringValue)
  }
  func setupSemanticCase() {
    window.contentView?.subviews.forEach { $0.removeFromSuperview() }
    if semanticCase == "capture" { setupCapture(); return }
    if semanticCase == "selection" {
      orders = [OrderRow("Order A", y: 370), OrderRow("Order B", y: 310)]
      orders[1].selected = true // Existing unrelated choice must survive A's changes.
      orders.forEach { window.contentView?.addSubview($0) }
    } else if semanticCase == "check" {
      checkbox.title = "Approve order"
      window.contentView?.addSubview(checkbox)
      window.contentView?.addSubview(mixed)
    } else if semanticCase == "input-poc" {
      let config = WKWebViewConfiguration()
      config.userContentController.add(self, name: "fixture")
      let web = WKWebView(frame: NSRect(x: 24, y: 280, width: 430, height: 150), configuration: config)
      web.navigationDelegate = self
      window.contentView?.addSubview(web)
      web.loadHTMLString("""
        <!doctype html><meta charset="utf-8"><title>POC background form</title>
        <label>Order note <input id="note" aria-label="POC Web text" style="width:300px" oninput="send('web_input',this.value)"></label>
        <p><button aria-label="POC Web submit" onclick="send('web_submit',document.getElementById('note').value)">Save order note</button></p>
        <script>const send=(event,value)=>window.webkit.messageHandlers.fixture.postMessage({event,value});</script>
        """, baseURL: nil)
    } else if semanticCase == "scroll" {
      // Use the system WebKit provider's real AXScrollToVisible implementation.
      // The fixture supplies business callbacks, never an AX action shim.
      let config = WKWebViewConfiguration()
      config.userContentController.add(self, name: "fixture")
      let web = WKWebView(frame: NSRect(x: 24, y: 280, width: 430, height: 150), configuration: config)
      web.navigationDelegate = self
      window.contentView?.addSubview(web)
      web.loadHTMLString("""
        <!doctype html><meta charset="utf-8"><title>Orders</title>
        <p>Orders awaiting fulfillment</p><div style="height:900px"></div>
        <button id="order" onclick="finish()">Fulfill distant order</button><div style="height:100px"></div>
        <script>
        const send=(event,value)=>window.webkit.messageHandlers.fixture.postMessage({event,value});
        const visible=()=>{const r=document.getElementById('order').getBoundingClientRect();return r.bottom>0&&r.top<innerHeight};
        let logged=false;
        addEventListener('scroll',()=>{if(!logged&&visible()){logged=true;send('scrolled','true')}});
        function finish(){send('fulfilled',visible()?'order-900':'ERROR:not-visible')}
        </script>
        """, baseURL: nil)
    }
    let submitButton = NSButton(title: "提交", target: self, action: #selector(submit))
    submitButton.frame = NSRect(x: 24, y: 160, width: 100, height: 32)
    window.contentView?.addSubview(submitButton)
    let unsupported = NSButton(title: "Label only", target: self, action: #selector(labelAction))
    unsupported.frame = NSRect(x: 160, y: 160, width: 140, height: 32)
    window.contentView?.addSubview(unsupported)
  }
  func createCanvas() {
    canvasWindow = NSWindow(contentRect: NSRect(x: 180, y: 200, width: 500, height: 530), styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
    canvasWindow.title = fixtureTitle + " Canvas"
    canvasWindow.isReleasedWhenClosed = false
    canvas = CaptureCanvas(frame: canvasWindow.contentView!.bounds)
    canvas.autoresizingMask = [.width, .height]
    canvasWindow.contentView = canvas
    canvasWindow.orderBack(nil)
    record("canvas_created", String(canvasWindow.windowNumber))
  }
  func setupCapture() {
    createCanvas()
    let actions: [(String, Selector)] = [("更新画布", #selector(updateCanvas)), ("移动缩放", #selector(moveCanvas)), ("最小化画布", #selector(minimizeCanvas)), ("隐藏画布", #selector(hideCanvas)), ("恢复画布", #selector(restoreCanvas)), ("重建画布", #selector(recreateCanvas)), ("打开弹窗", #selector(openCapturePopup)), ("打开Sheet", #selector(openCaptureSheet)), ("关闭Sheet", #selector(closeCaptureSheet))]
    for (i, action) in actions.enumerated() {
      let button = NSButton(title: action.0, target: self, action: action.1)
      button.frame = NSRect(x: 24, y: 470 - i * 44, width: 190, height: 32)
      window.contentView?.addSubview(button)
    }
  }
  @objc func updateCanvas() {
    canvas.green = true; canvas.needsDisplay = true; canvas.displayIfNeeded()
    record("canvas_updated", "green")
  }
  @objc func moveCanvas() {
    canvasWindow.setFrame(NSRect(x: 220, y: 240, width: 410, height: 390), display: true)
    record("canvas_moved_resized", NSStringFromRect(canvasWindow.frame))
  }
  @objc func minimizeCanvas() { canvasWindow.miniaturize(nil); record("canvas_minimized") }
  @objc func hideCanvas() { canvasWindow.orderOut(nil); record("canvas_hidden") }
  @objc func restoreCanvas() { canvasWindow.deminiaturize(nil); canvasWindow.orderBack(nil); record("canvas_restored") }
  @objc func recreateCanvas() { canvasWindow.close(); createCanvas(); record("canvas_recreated") }
  @objc func openCapturePopup() {
    capturePopup = NSWindow(contentRect: NSRect(x: 230, y: 270, width: 160, height: 150), styleMask: [.titled], backing: .buffered, defer: false)
    capturePopup!.title = fixtureTitle + " Popup"; capturePopup!.isReleasedWhenClosed = false
    let view = NSView(); view.wantsLayer = true; view.layer?.backgroundColor = NSColor.blue.cgColor
    capturePopup!.contentView = view; capturePopup!.orderBack(nil)
    record("popup_opened", String(capturePopup!.windowNumber))
  }
  @objc func openCaptureSheet() {
    captureSheet = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 180, height: 130), styleMask: [.titled], backing: .buffered, defer: false)
    captureSheet!.title = fixtureTitle + " Sheet"; captureSheet!.isReleasedWhenClosed = false
    let view = NSView(); view.wantsLayer = true; view.layer?.backgroundColor = NSColor.blue.cgColor
    captureSheet!.contentView = view
    canvasWindow.beginSheet(captureSheet!) { _ in }
    record("sheet_opened", String(captureSheet!.windowNumber))
  }
  @objc func closeCaptureSheet() {
    if let sheet = captureSheet { canvasWindow.endSheet(sheet); sheet.orderOut(nil); captureSheet = nil; record("sheet_closed") }
  }
  func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
    guard let row = message.body as? [String: String], let event = row["event"], let value = row["value"] else { return }
    record(event, value)
  }
  func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { record("ready", fixtureTitle) }
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
