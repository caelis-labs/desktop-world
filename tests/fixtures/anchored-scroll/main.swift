import AppKit
import ApplicationServices
import Foundation

// This observer reads a small allowlist only. A missing lock field stays unknown.
func sessionSnapshot(_ dictionary: [String: Any]?) -> [String: Any] {
    func flag(_ key: String) -> Any {
        guard let number = dictionary?[key] as? NSNumber,
              CFGetTypeID(number) == CFBooleanGetTypeID() else { return NSNull() }
        return number.boolValue
    }
    return ["source": "CGSessionCopyCurrentDictionary", "available": dictionary != nil,
            "uptime": ProcessInfo.processInfo.systemUptime,
            "screen_locked": flag("CGSSessionScreenIsLocked"),
            "on_console": flag(kCGSessionOnConsoleKey as String),
            "login_done": flag(kCGSessionLoginDoneKey as String)]
}
func printJSON(_ value: Any) {
    let bytes = try! JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    print(String(decoding: bytes, as: UTF8.self))
}
if CommandLine.arguments.contains("--session-precondition") {
    printJSON(sessionSnapshot(CGSessionCopyCurrentDictionary() as? [String: Any])); exit(0)
}
if CommandLine.arguments.contains("--session-controls") {
    precondition(sessionSnapshot(nil)["screen_locked"] is NSNull)
    precondition(sessionSnapshot(["CGSSessionScreenIsLocked": true])["screen_locked"] as? Bool == true)
    precondition(sessionSnapshot(["CGSSessionScreenIsLocked": false])["screen_locked"] as? Bool == false)
    precondition(sessionSnapshot(["CGSSessionScreenIsLocked": 1])["screen_locked"] is NSNull)
    printJSON(["passed": true]); exit(0)
}

let output = URL(fileURLWithPath: CommandLine.arguments.last!, isDirectory: true)
let configuration = (try? Data(contentsOf: output.appendingPathComponent("fixture.json")))
    .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] } ?? [:]
let fixtureApplication = NSApplication.shared
fixtureApplication.setActivationPolicy(.regular)
final class Document: NSView { override var isFlipped: Bool { true } }
final class Region: NSScrollView {
    var name = ""
    var wheel: ((Region, NSEvent, Double, Double) -> Void)?
    override func scrollWheel(with event: NSEvent) {
        // Deliberately deterministic owned business behavior: one native line
        // moves 16 points. Routing, line count and location remain real events.
        let before = contentView.bounds.origin.y
        let maximum = max(0, (documentView?.bounds.height ?? 0) - contentView.bounds.height)
        let after = min(maximum, max(0, before - event.deltaY * 16))
        contentView.scroll(to: NSPoint(x: 0, y: after)); reflectScrolledClipView(contentView)
        wheel?(self, event, before, after)
    }
}
final class Delegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    var regions: [String: Region] = [:]
    var anchors: [String: NSTextField] = [:]
    var timer: Timer?
    var command = ""
    var events: [[String: Any]] = []
    var eventsFile: FileHandle!
    var generation = 1
    func region(_ name: String, frame: NSRect, parent: NSView) -> Region {
        let scroll = Region(frame: frame)
        scroll.name = name; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = false
        scroll.borderType = .bezelBorder; scroll.drawsBackground = true
        scroll.backgroundColor = name == "left" ? .systemTeal : name == "right" ? .systemOrange : .systemPurple
        scroll.setAccessibilityLabel(name.capitalized + " scroll region")
        let document = Document(frame: NSRect(x: 0, y: 0, width: frame.width - 18, height: 2400))
        scroll.documentView = document; parent.addSubview(scroll)
        for index in 0..<60 {
            let label = NSTextField(labelWithString: name + " record " + String(index))
            label.frame = NSRect(x: 24, y: CGFloat(index) * 36 + 80, width: 220, height: 24)
            document.addSubview(label)
        }
        regions[name] = scroll; addAnchor(name)
        scroll.wheel = { [weak self] region, event, before, after in self?.record(region, event, before, after) }
        return scroll
    }
    func addAnchor(_ name: String) {
        let label = NSTextField(labelWithString: name.capitalized + " anchor")
        label.frame = NSRect(x: 24, y: 26, width: 220, height: 28)
        label.setAccessibilityLabel(name.capitalized + " anchor")
        label.font = .boldSystemFont(ofSize: 19)
        regions[name]?.documentView?.addSubview(label); anchors[name] = label
    }
    func screenRect(_ view: NSView) -> [Double] {
        let frame = window.convertToScreen(view.convert(view.bounds, to: nil))
        return [frame.minX, CGDisplayBounds(CGMainDisplayID()).height - frame.maxY, frame.width, frame.height]
    }
    func record(_ region: Region, _ event: NSEvent, _ before: Double, _ after: Double) {
        let point = window.convertPoint(toScreen: event.locationInWindow)
        let height = CGDisplayBounds(CGMainDisplayID()).height
        let row: [String: Any] = ["sequence": events.count + 1, "region": region.name,
            "uptime": ProcessInfo.processInfo.systemUptime, "timestamp": event.timestamp,
            "delta_y": event.deltaY, "scrolling_delta_y": event.scrollingDeltaY,
            "precise": event.hasPreciseScrollingDeltas, "window_id": event.windowNumber,
            "screen_point": [point.x, height - point.y], "before": before, "after": after,
            "generation": generation, "source": "NSScrollView.scrollWheel native NSEvent"]
        events.append(row)
        let data = try! JSONSerialization.data(withJSONObject: row, options: [.sortedKeys])
        try? eventsFile.write(contentsOf: data + Data("\n".utf8)); publish()
    }
    func publish() {
        guard window != nil else { return }
        let height = CGDisplayBounds(CGMainDisplayID()).height, frame = window.frame
        let cursor = NSEvent.mouseLocation
        let state: [String: Any] = ["ready": true, "pid": ProcessInfo.processInfo.processIdentifier,
            "command": command, "generation": generation, "events_count": events.count,
            "events": Array(events.suffix(128)), "published_uptime": ProcessInfo.processInfo.systemUptime,
            "active": fixtureApplication.isActive, "key_window": fixtureApplication.keyWindow?.windowNumber ?? 0,
            "frontmost_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0,
            "pointer_screen": [cursor.x, height - cursor.y],
            "window_bounds": [frame.minX, height - frame.maxY, frame.width, frame.height],
            "windows": [["id": window.windowNumber, "title": window.title, "visible": window.isVisible]],
            "offsets": regions.mapValues { $0.contentView.bounds.origin.y },
            "regions": regions.mapValues { screenRect($0.contentView) },
            "anchors": anchors.mapValues { screenRect($0) }]
        try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys])
            .write(to: output.appendingPathComponent("state.json"), options: .atomic)
    }
    func handle(_ raw: String) {
        command = raw
        switch raw.trimmingCharacters(in: .whitespacesAndNewlines) {
        case "reset":
            for scroll in regions.values { scroll.contentView.scroll(to: .zero); scroll.reflectScrolledClipView(scroll.contentView) }
        case "replace-left":
            anchors["left"]?.removeFromSuperview(); anchors.removeValue(forKey: "left")
            generation += 1; addAnchor("left")
        case "resign": window.resignKey()
        case "front": window.makeKeyAndOrderFront(nil); fixtureApplication.activate(ignoringOtherApps: true)
        case "quit": fixtureApplication.terminate(nil)
        default: break
        }
        publish()
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        let path = output.appendingPathComponent("events.jsonl")
        _ = FileManager.default.createFile(atPath: path.path, contents: nil)
        eventsFile = try! FileHandle(forWritingTo: path)
        let x = configuration["x"] as? Double ?? 80
        window = NSWindow(contentRect: NSRect(x: x, y: 180, width: 800, height: 550),
                          styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = configuration["title"] as? String ?? "DTW Anchored Scroll"
        window.isReleasedWhenClosed = false
        let content = Document(frame: NSRect(x: 0, y: 0, width: 800, height: 550)); window.contentView = content
        _ = region("left", frame: NSRect(x: 18, y: 18, width: 442, height: 512), parent: content)
        let right = region("right", frame: NSRect(x: 480, y: 18, width: 300, height: 512), parent: content)
        _ = region("nested", frame: NSRect(x: 20, y: 120, width: 250, height: 200), parent: right.documentView!)
        window.makeKeyAndOrderFront(nil); fixtureApplication.activate(ignoringOtherApps: true)
        timer = Timer.scheduledTimer(withTimeInterval: 0.02, repeats: true) { [weak self] _ in
            guard let self else { return }
            if let next = try? String(contentsOf: output.appendingPathComponent("command"), encoding: .utf8), next != self.command {
                self.handle(next)
            }
            self.publish()
        }
        publish()
    }
}
let delegate = Delegate(); fixtureApplication.delegate = delegate; fixtureApplication.run()
