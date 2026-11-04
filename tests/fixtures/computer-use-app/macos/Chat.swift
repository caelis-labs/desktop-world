import AppKit

final class ChatCanvas: NSView {
    weak var owner: ComputerUseDelegate?
    override var isFlipped: Bool { true }
    let rowHeight: CGFloat = 84
    var scrollOffset: CGFloat = 134 * 84
    var selectedID = ""
    var editingID = ""
    var editedMessages: [String: String] = [:]
    var popup: NSWindow?
    var popupCanvas: ChatPopup?
    var pinned = ""
    var edited = ""
    var replies: [String] = []
    var sends = 0
    var rightClicks = 0
    var edits = 0
    var pinCount = 0
    var replyCount = 0
    var selectedAction = ""
    var replyTo = ""
    let compose = NSTextField()
    let send = NSButton(title: "Send", target: nil, action: nil)
    let editor = NSTextField()
    let apply = NSButton(title: "Apply edit", target: nil, action: nil)
    var feedRect: NSRect { NSRect(x: 230, y: 80, width: max(260, bounds.width - 250), height: max(180, bounds.height - 218)) }
    var messageRect: NSRect { rect(for: 137) }
    var firstVisible: Int { Int(scrollOffset / rowHeight) + 1 }
    var messageCount: Int { 240 + replies.count }
    var maximumOffset: CGFloat { max(0, CGFloat(messageCount) * rowHeight - feedRect.height) }
    func rect(for index: Int) -> NSRect { NSRect(x: feedRect.minX + 8, y: feedRect.minY + CGFloat(index - 1) * rowHeight - scrollOffset + 4, width: feedRect.width - 28, height: rowHeight - 8) }
    func id(for index: Int) -> String { "message-\(index)" }
    func originalText(_ index: Int) -> String {
        if index > 240 { return replies[index - 241] }
        if index == 137 { return "Release check 137" }
        let topics = ["Artifact review completed", "Canary metrics look healthy", "Waiting for approval", "Deployment checklist updated", "Trace samples attached", "Regional rollout discussion", "Please verify the release notes", "Build queue status"]
        return "\(topics[index % topics.count]) · item \(index)"
    }
    func messageText(_ index: Int) -> String { editedMessages[id(for: index)] ?? originalText(index) }
    override init(frame: NSRect) {
        super.init(frame: frame)
        setAccessibilityElement(false)
        compose.placeholderString = "Write a message"; compose.setAccessibilityLabel("Message input")
        editor.placeholderString = "Edit message"; editor.setAccessibilityLabel("Edit message"); editor.isHidden = true
        apply.isHidden = true; apply.target = self; apply.action = #selector(applyEdit)
        send.target = self; send.action = #selector(sendMessage)
        [compose, send, editor, apply].forEach { addSubview($0) }
    }
    required init?(coder: NSCoder) { fatalError("programmatic fixture") }
    override func layout() {
        super.layout()
        compose.frame = NSRect(x: 238, y: bounds.height - 48, width: bounds.width - 354, height: 28)
        send.frame = NSRect(x: bounds.width - 108, y: bounds.height - 50, width: 88, height: 32)
        editor.frame = NSRect(x: 238, y: bounds.height - 86, width: bounds.width - 374, height: 28)
        apply.frame = NSRect(x: bounds.width - 130, y: bounds.height - 88, width: 112, height: 32)
    }
    func text(_ value: String, _ rect: NSRect, _ size: CGFloat = 15, _ color: NSColor = .labelColor) {
        (value as NSString).draw(in: rect, withAttributes: [.font: NSFont.systemFont(ofSize: size), .foregroundColor: color])
    }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.windowBackgroundColor.setFill(); bounds.fill()
        NSColor.controlBackgroundColor.setFill(); NSRect(x: 0, y: 0, width: 218, height: bounds.height).fill()
        text("WORKSPACE / 30 rooms", NSRect(x: 16, y: 16, width: 200, height: 24), 13, .secondaryLabelColor)
        let roomNames = ["Release room", "Engineering", "Incident desk", "Build pipeline", "Design review", "Operations", "Regional rollout", "Customer signals", "QA triage", "Platform"]
        for index in 0..<30 {
            let y = 50 + CGFloat(index) * 20
            if index == 0 { NSColor.selectedContentBackgroundColor.withAlphaComponent(0.2).setFill(); NSRect(x: 8, y: y - 2, width: 202, height: 20).fill() }
            text("# \(roomNames[index % 10])\(index >= 10 ? " \(index / 10 + 1)" : "")", NSRect(x: 16, y: y, width: 155, height: 19), 12)
            text(index % 4 == 0 ? "●" : "\(index * 7 % 23)", NSRect(x: 181, y: y, width: 30, height: 19), 11, .secondaryLabelColor)
        }
        text("Project release room", NSRect(x: 238, y: 14, width: 600, height: 30), 23)
        text("128 members · 17 online · notifications muted · last sync 09:43", NSRect(x: 238, y: 48, width: bounds.width - 260, height: 22), 12, .secondaryLabelColor)
        NSGraphicsContext.saveGraphicsState(); NSBezierPath(rect: feedRect).addClip()
        let last = min(messageCount, firstVisible + Int(ceil(feedRect.height / rowHeight)))
        if firstVisible <= last {
            for index in firstVisible...last {
                let row = rect(for: index)
                (index % 2 == 0 ? NSColor.controlBackgroundColor : NSColor.textBackgroundColor).setFill()
                NSBezierPath(roundedRect: row, xRadius: 9, yRadius: 9).fill()
                let names = ["Mira", "Alex", "Rui", "Sam", "Build bot", "Nora"]
                text("\(names[index % names.count]) · 09:\(String(format: "%02d", index % 60)) · #\(index) · delivered ✓✓", NSRect(x: row.minX + 12, y: row.minY + 8, width: row.width - 24, height: 20), 11, .secondaryLabelColor)
                text(messageText(index), NSRect(x: row.minX + 12, y: row.minY + 30, width: row.width - 24, height: 28), index == 137 ? 18 : 14)
                text("\(index % 5) reactions · \(index % 3) replies\(pinned == id(for: index) ? " · PINNED" : "")", NSRect(x: row.minX + 12, y: row.minY + 57, width: row.width - 24, height: 17), 10, .secondaryLabelColor)
            }
        }
        NSGraphicsContext.restoreGraphicsState()
        let track = NSRect(x: feedRect.maxX - 7, y: feedRect.minY, width: 4, height: feedRect.height)
        NSColor.separatorColor.setFill(); track.fill()
        let thumbHeight = max(20, feedRect.height * feedRect.height / (CGFloat(messageCount) * rowHeight))
        let thumbY = track.minY + (track.height - thumbHeight) * (maximumOffset > 0 ? scrollOffset / maximumOffset : 0)
        NSColor.secondaryLabelColor.setFill(); NSBezierPath(roundedRect: NSRect(x: track.minX, y: thumbY, width: 4, height: thumbHeight), xRadius: 2, yRadius: 2).fill()
        text(replyTo.isEmpty ? "\(messageCount) messages · right-click for actions" : "Replying to \(replyTo)", NSRect(x: 238, y: bounds.height - 126, width: bounds.width - 260, height: 22), 12, .secondaryLabelColor)
    }
    override func scrollWheel(with event: NSEvent) {
        guard feedRect.contains(convert(event.locationInWindow, from: nil)) else { return }
        closePopup()
        let delta = event.scrollingDeltaY * (event.hasPreciseScrollingDeltas ? 1 : 20)
        scrollOffset = min(maximumOffset, max(0, scrollOffset - delta))
        needsDisplay = true; owner?.event("chat-scroll", ["scroll_offset": scrollOffset, "first_visible": firstVisible]); owner?.publish()
    }
    func hitMessage(_ point: NSPoint) -> Int? {
        guard feedRect.contains(point) else { return nil }
        let index = Int((point.y - feedRect.minY + scrollOffset) / rowHeight) + 1
        guard (1...messageCount).contains(index), rect(for: index).contains(point) else { return nil }
        return index
    }
    override func rightMouseDown(with event: NSEvent) {
        guard let index = hitMessage(convert(event.locationInWindow, from: nil)), let owner, let window else { return }
        selectedID = id(for: index); rightClicks += 1; closePopup()
        let row = rect(for: index)
        let local = NSRect(x: min(bounds.width - 162, row.minX + 100), y: min(bounds.height - 138, max(feedRect.minY, row.minY + 14)), width: 150, height: 126)
        let screen = window.convertToScreen(convert(local, to: nil))
        let panel = NSWindow(contentRect: screen, styleMask: .borderless, backing: .buffered, defer: false)
        panel.title = "Chat actions"; panel.isReleasedWhenClosed = false; panel.level = .popUpMenu
        let canvas = ChatPopup(frame: NSRect(origin: .zero, size: screen.size)); canvas.chat = self
        panel.contentView = canvas; popup = panel; popupCanvas = canvas
        window.addChildWindow(panel, ordered: .above); panel.orderFront(nil)
        owner.event("chat-menu", ["message_id": selectedID]); owner.publish()
    }
    func closePopup() { if let popup { popup.parent?.removeChildWindow(popup); popup.close() }; popup = nil; popupCanvas = nil }
    func choose(_ action: String) {
        guard !selectedID.isEmpty else { return }
        selectedAction = action
        switch action {
        case "Pin": pinned = selectedID; pinCount += 1
        case "Edit":
            editingID = selectedID
            if let index = Int(selectedID.replacingOccurrences(of: "message-", with: "")) { editor.stringValue = messageText(index) }
            editor.isHidden = false; apply.isHidden = false; window?.makeFirstResponder(editor)
        case "Reply": replyTo = selectedID; replyCount += 1; window?.makeFirstResponder(compose)
        default: return
        }
        closePopup(); needsDisplay = true; owner?.event("chat-action", ["action": action, "message_id": selectedID]); owner?.publish()
    }
    @objc func applyEdit() {
        edited = editor.currentEditor()?.string ?? editor.stringValue; editedMessages[editingID] = edited; edits += 1
        editor.isHidden = true; apply.isHidden = true; needsDisplay = true
        owner?.event("chat-edit", ["text": edited, "message_id": editingID]); owner?.publish()
    }
    @objc func sendMessage() {
        let value = compose.currentEditor()?.string ?? compose.stringValue; replies.append(value); sends += 1; compose.stringValue = ""
        scrollOffset = maximumOffset; needsDisplay = true; owner?.event("chat-send", ["text": value, "reply_to": replyTo]); owner?.publish()
    }
    var state: [String: Any] {
        ["pinned": pinned, "edited": edited, "edited_messages": editedMessages, "edits": edits, "pin_count": pinCount, "right_clicks": rightClicks, "reply_count": replyCount, "reply_to": replyTo, "replies": replies, "sends": sends, "compose": compose.currentEditor()?.string ?? compose.stringValue, "menu_open": popup != nil, "selected_action": selectedAction, "selected_id": selectedID, "editing_id": editingID, "scroll_offset": scrollOffset, "first_visible": firstVisible, "message_count": messageCount, "room_count": 30]
    }
}

final class ChatPopup: NSView {
    weak var chat: ChatCanvas?
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override var isFlipped: Bool { true }
    override init(frame: NSRect) { super.init(frame: frame); setAccessibilityElement(false); setAccessibilityChildren([]) }
    required init?(coder: NSCoder) { fatalError("programmatic fixture") }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.controlBackgroundColor.setFill(); bounds.fill()
        for (index, title) in ["Pin", "Edit", "Reply"].enumerated() {
            (title as NSString).draw(at: NSPoint(x: 18, y: 12 + index * 40), withAttributes: [.font: NSFont.systemFont(ofSize: 17), .foregroundColor: NSColor.labelColor])
        }
    }
    override func mouseDown(with event: NSEvent) { let row = Int(convert(event.locationInWindow, from: nil).y / 40); if (0..<3).contains(row) { chat?.choose(["Pin", "Edit", "Reply"][row]) } }
}
