import AppKit

final class FlippedFixtureView: NSView { override var isFlipped: Bool { true } }
final class GestureStatusLabel: NSTextField {
    override func hitTest(_ point: NSPoint) -> NSView? { nil }
}
final class GestureFixtureView: NSView {
    weak var owner: ComputerUseDelegate?
    var tracking: NSTrackingArea?
    let statusLabel = GestureStatusLabel(labelWithString: "")
    var gestureStatus: String {
        let counts = owner?.gestures ?? [:]
        return "Hover: \(counts["hover", default: 0])  Click: \(counts["click", default: 0])  Double-click: \(counts["double_click", default: 0])  Right: \(counts["right_click", default: 0])  Drag: \(counts["drag", default: 0])  Up: \(counts["up", default: 0])"
    }
    override func isAccessibilityElement() -> Bool { true }
    override func accessibilityRole() -> NSAccessibility.Role? { .group }
    override func accessibilityLabel() -> String? { "Gesture desk" }
    override func updateTrackingAreas() {
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.activeAlways, .inVisibleRect, .mouseEnteredAndExited], owner: self, userInfo: nil)
        tracking = area; addTrackingArea(area); super.updateTrackingAreas()
    }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.systemTeal.withAlphaComponent(0.25).setFill(); bounds.fill()
        let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 14), .foregroundColor: NSColor.labelColor]
        ("Gesture desk — hover, double-click or right-click" as NSString).draw(at: NSPoint(x: 12, y: 34), withAttributes: attributes)
    }
    override func mouseEntered(with event: NSEvent) { owner?.gesture("hover", event) }
    override func mouseDown(with event: NSEvent) { owner?.gesture(event.clickCount == 2 ? "double_click" : "click", event) }
    override func mouseDragged(with event: NSEvent) { owner?.gesture("drag", event) }
    override func mouseUp(with event: NSEvent) { owner?.gesture("up", event) }
    override func rightMouseDown(with event: NSEvent) { owner?.gesture("right_click", event) }
}

extension ComputerUseDelegate {
    func button(_ title: String, _ frame: NSRect, _ action: Selector, in parent: NSView) -> NSButton {
        let value = NSButton(title: title, target: self, action: action); value.frame = frame; parent.addSubview(value); return value
    }
    func buildNative(_ frame: NSRect) -> NSView {
        let surface = FlippedFixtureView(frame: frame)
        _ = button("Increment counter", NSRect(x: 20, y: 16, width: 170, height: 32), #selector(increment), in: surface)
        counterLabel.frame = NSRect(x: 200, y: 21, width: 180, height: 24); surface.addSubview(counterLabel)
        note.frame = NSRect(x: 20, y: 62, width: 430, height: 28); note.stringValue = "Initial native note"; note.setAccessibilityLabel("Native note"); surface.addSubview(note)
        _ = button("Commit note", NSRect(x: 460, y: 60, width: 130, height: 32), #selector(commitNote), in: surface)
        reviewed = NSButton(checkboxWithTitle: "Reviewed", target: self, action: #selector(changeReviewed)); reviewed.frame = NSRect(x: 20, y: 105, width: 130, height: 28); surface.addSubview(reviewed)
        choice.frame = NSRect(x: 160, y: 103, width: 160, height: 30); choice.addItems(withTitles: ["Draft", "Review", "Approved"]); choice.setAccessibilityLabel("Workflow status"); choice.target = self; choice.action = #selector(changeChoice); surface.addSubview(choice)
        slider.frame = NSRect(x: 345, y: 104, width: 300, height: 28); slider.setAccessibilityLabel("Priority slider"); slider.target = self; slider.action = #selector(changeSlider); slider.isContinuous = true; surface.addSubview(slider)
        let pad = GestureFixtureView(frame: NSRect(x: 20, y: 148, width: 625, height: 60)); pad.owner = self; surface.addSubview(pad); namedViews["gesture-desk"] = pad
        pad.statusLabel.frame = NSRect(x: 12, y: 6, width: 601, height: 22)
        pad.statusLabel.font = NSFont.systemFont(ofSize: 14)
        pad.statusLabel.stringValue = pad.gestureStatus
        pad.statusLabel.setAccessibilityLabel("Gesture status")
        pad.addSubview(pad.statusLabel)
        let multilineScroll = NSScrollView(frame: NSRect(x: 665, y: 62, width: max(260, frame.width - 685), height: 146))
        multilineScroll.borderType = .bezelBorder; multilineScroll.hasVerticalScroller = true
        multilineNote.frame = NSRect(origin: .zero, size: multilineScroll.contentSize)
        multilineNote.isRichText = false; multilineNote.isEditable = true; multilineNote.isSelectable = true
        multilineNote.font = NSFont.systemFont(ofSize: 14)
        multilineNote.string = "Initial multiline note\nSecond line"
        multilineNote.setAccessibilityLabel("Multiline note")
        multilineNote.textContainer?.widthTracksTextView = true; multilineNote.isVerticallyResizable = true
        multilineScroll.documentView = multilineNote; surface.addSubview(multilineScroll)
        namedViews["native-multiline-note"] = multilineNote
        let left = NSScrollView(frame: NSRect(x: 20, y: 224, width: 420, height: max(240, frame.height - 244)))
        left.hasVerticalScroller = true; left.borderType = .bezelBorder; left.setAccessibilityLabel("Audit controls")
        let document = FlippedFixtureView(frame: NSRect(x: 0, y: 0, width: 395, height: 190 * 34 + 36))
        for index in 0..<190 {
            let title = index == 137 ? "Audit checkpoint 137" : "Audit checkpoint"
            let control = button(title, NSRect(x: 12, y: 12 + index * 34, width: 300, height: 28), #selector(audit(_:)), in: document)
            control.tag = index
        }
        left.documentView = document; surface.addSubview(left); leftScroll = left
        let right = NSScrollView(frame: NSRect(x: 456, y: 224, width: max(260, frame.width - 476), height: max(240, frame.height - 244)))
        right.hasVerticalScroller = true; right.borderType = .bezelBorder
        let text = NSTextView(frame: NSRect(x: 0, y: 0, width: right.contentSize.width, height: 6000))
        text.isEditable = false; text.isSelectable = true; text.font = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
        text.string = (1...180).map { "Reference section \($0): Review the deployment evidence, validate the owned release artifacts, and record the outcome.\n" }.joined() + "NATIVE-END-137-中文🙂"
        text.setAccessibilityLabel("Long reference"); text.textContainer?.widthTracksTextView = true; text.isVerticallyResizable = true
        do { try text.string.write(to: root.appendingPathComponent("native-reference.txt"), atomically: true, encoding: .utf8) }
        catch { failIO("publish reference oracle", error) }
        right.documentView = text; surface.addSubview(right); rightScroll = right
        namedViews["native-note"] = note; namedViews["native-left-scroll"] = left; namedViews["native-right-scroll"] = right
        return surface
    }
    @objc func increment() { counter += 1; counterLabel.stringValue = "Counter: \(counter)"; event("increment", ["counter": counter]); publish() }
    var noteText: String { note.currentEditor()?.string ?? note.stringValue }
    @objc func commitNote() { savedNote = noteText; cleanNote = savedNote; saves += 1; event("commit-note", ["note": savedNote, "saves": saves]); publish() }
    @objc func changeReviewed() { event("reviewed", ["value": reviewed.state == .on]); publish() }
    @objc func changeChoice() { event("workflow", ["value": choice.titleOfSelectedItem ?? ""]); publish() }
    @objc func changeSlider() { sliderChanges += 1; event("slider", ["value": slider.doubleValue]); publish() }
    @objc func audit(_ sender: NSButton) { auditIndex = sender.tag; auditCount += 1; event("audit", ["index": sender.tag, "count": auditCount]); publish() }
    func gesture(_ kind: String, _ input: NSEvent) {
        gestures[kind, default: 0] += 1
        if let pad = namedViews["gesture-desk"] as? GestureFixtureView {
            pad.statusLabel.stringValue = pad.gestureStatus
            NSAccessibility.post(element: pad.statusLabel, notification: .valueChanged)
        }
        var fields: [String: Any] = ["phase": kind, "click_count": input.clickCount]
        if let window = input.window {
            let screen = window.convertPoint(toScreen: input.locationInWindow)
            fields["screen_x"] = finite(screen.x)
            fields["screen_y"] = finite((NSScreen.screens.first?.frame.maxY ?? 0) - screen.y)
        }
        event("gesture", fields); publish()
    }
}
