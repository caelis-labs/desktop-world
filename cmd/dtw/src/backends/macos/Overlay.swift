// DTW cursor presentation, adapted from CUA b38360443960a6540685a9a5eb5407f10fe3d03b.
// CUA slices: MIT, Copyright (c) 2025 Cua AI, Inc.; see cua_input/LICENSE-CUA and
// docs/cursor-overlay-port.md for source blocks, hashes and deliberate changes.
import AppKit
import Foundation
import CoreGraphics
import Darwin

struct OverlayTargetState: Codable, Equatable {
    let pid: UInt32
    let birth: UInt64
    let window: UInt32
    let bounds: [Double]
    var rect: CGRect? {
        guard pid > 0, pid <= UInt32(Int32.max), birth > 0, window > 0, bounds.count == 4,
              bounds.allSatisfy({ $0.isFinite }), bounds[2] > 0, bounds[3] > 0 else { return nil }
        return CGRect(x: bounds[0], y: bounds[1], width: bounds[2], height: bounds[3])
    }
}
struct OverlayPointerEvent: Codable {
    let operation: UInt64
    let sequence: UInt64
    let press_sequence: UInt64
    let target: OverlayTargetState
    let point: [Double]
    let route: String
    let kind: String
    var position: CGPoint? {
        guard point.count == 2, point.allSatisfy({ $0.isFinite }),
              ["background", "foreground"].contains(route),
              ["move", "press", "release", "scroll"].contains(kind),
              sequence > 0, press_sequence <= sequence,
              let bounds = target.rect else { return nil }
        let position = CGPoint(x: point[0], y: point[1])
        return bounds.contains(position) ? position : nil
    }
}
struct OverlayControl: Decodable {
    let command: String
    let operation: UInt64?
}

/// Main-thread-owned, bounded state: one operation and its latest marker.
/// A canceled generation cannot restart, even if a delayed event was queued.
struct OverlayPresentationState {
    private(set) var generation: UInt64 = 0
    private(set) var active = false
    private(set) var closed = false
    private(set) var sequence: UInt64 = 0
    private(set) var pressSequence: UInt64 = 0
    private(set) var marker: OverlayPointerEvent?
    mutating func begin(_ value: UInt64) -> Bool {
        guard !closed, value > generation else { return false }
        generation = value; active = true; sequence = 0; pressSequence = 0; marker = nil
        return true
    }
    mutating func hide(_ value: UInt64) {
        guard value == generation else { return }
        active = false; marker = nil
    }
    mutating func close() { closed = true; active = false; marker = nil }
    mutating func hideStaleTarget() { active = false; marker = nil }
    mutating func accept(_ event: OverlayPointerEvent) -> Bool {
        guard !closed, active, event.operation == generation, event.sequence > sequence,
              event.position != nil else { return false }
        sequence = event.sequence
        pressSequence = max(pressSequence, event.press_sequence)
        marker = event
        return true
    }
}

/// Click feedback is a bounded pulse, not a claim about current button state.
/// Release feedback is best effort; a missing release must never look held.
struct OverlayClickPulse {
    private(set) var expiresAt: TimeInterval = 0
    mutating func observe(pressSequence: UInt64, previousPressSequence: UInt64, at now: TimeInterval) {
        if pressSequence > previousPressSequence { expiresAt = now + 0.2 }
    }
    mutating func reset() { expiresAt = 0 }
    func isVisible(at now: TimeInterval) -> Bool { now < expiresAt }
}

/// A one-slot mailbox. Main-thread control commands bypass this queue. There
/// can be one scheduled drain and one latest event, never a per-motion history.
final class OverlayLatestEvent {
    private let lock = NSLock()
    private var latest: OverlayPointerEvent?
    private var scheduled = false
    func put(_ event: OverlayPointerEvent) -> Bool {
        lock.lock(); defer { lock.unlock() }
        // Out-of-order datagrams must not replace a more recent operation/event.
        if let value = latest, (event.operation < value.operation ||
            (event.operation == value.operation && event.sequence <= value.sequence)) { return false }
        latest = event
        guard !scheduled else { return false }
        scheduled = true
        return true
    }
    func take() -> OverlayPointerEvent? {
        lock.lock(); defer { lock.unlock() }
        let value = latest; latest = nil; scheduled = false
        return value
    }
}

// CUA theme.rs SESSION_CURSOR_FILLS + stable_session_index, without its theme/font runtime.
func overlayColorIndex(_ session: String, count: Int = 9) -> Int {
    guard count > 0 else { return 0 }
    let suffix = session.lastIndex(where: { "-_.".contains($0) }).map { String(session[session.index(after: $0)...]) } ?? session
    if let number = UInt64(suffix), number > 0 { return Int((number - 1) % UInt64(count)) }
    if suffix.utf8.count == 1, let value = suffix.utf8.first {
        if value >= 65 && value <= 90 { return Int(value - 65) % count }
        if value >= 97 && value <= 122 { return Int(value - 97) % count }
    }
    var hash: UInt32 = 2_166_136_261
    for scalar in session.unicodeScalars { hash = (hash ^ scalar.value) &* 16_777_619 }
    return Int(hash) % count
}
private let overlayPalette: [[CGFloat]] = [
    [178,132,255], [247,132,170], [96,218,174], [244,178,66], [76,204,224],
    [221,113,236], [232,82,98], [184,220,54], [80,126,236]
]

/// Global CG coordinates use the primary display's top-left. AppKit has a
/// bottom-left axis. The anchor is the actual primary display, NEVER mainScreen
/// (which follows the focused app). Coordinates remain logical points; AppKit
/// handles backing scale on the screen containing the small overlay window.
func overlayAppKitPoint(_ point: CGPoint, primaryFrame: CGRect) -> CGPoint {
    CGPoint(x: point.x, y: primaryFrame.maxY - point.y)
}
func overlayTargetMatches(_ target: OverlayTargetState, pid: UInt32?, birth: UInt64?,
                          bounds: CGRect?, onScreen: Bool) -> Bool {
    guard onScreen, pid == target.pid, birth == target.birth,
          let actual = bounds, let expected = target.rect else { return false }
    return actual == expected
}
private func overlayBirth(_ pid: Int32) -> UInt64? {
    var info = proc_bsdinfo()
    let size = Int32(MemoryLayout<proc_bsdinfo>.size)
    guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size) == size,
          info.pbi_pid == UInt32(pid), info.pbi_start_tvusec < 1_000_000 else { return nil }
    let (seconds, overflow) = info.pbi_start_tvsec.multipliedReportingOverflow(by: 1_000_000)
    let (stamp, overflow2) = seconds.addingReportingOverflow(info.pbi_start_tvusec)
    return overflow || overflow2 || stamp == 0 ? nil : stamp
}

private final class OverlayWindow: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

private final class OverlayCursorView: NSView {
    override var isFlipped: Bool { true }
    let color: NSColor
    var pulse = OverlayClickPulse()
    let hotspot = CGPoint(x: 14, y: 14)
    init(session: String) {
        let rgb = overlayPalette[overlayColorIndex(session)]
        color = NSColor(srgbRed: rgb[0] / 255, green: rgb[1] / 255, blue: rgb[2] / 255, alpha: 1)
        super.init(frame: CGRect(x: 0, y: 0, width: 88, height: 64))
    }
    required init?(coder: NSCoder) { nil }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.clear.setFill(); dirtyRect.fill(using: .copy)
        if pulse.isVisible(at: ProcessInfo.processInfo.systemUptime) {
            color.withAlphaComponent(0.28).setFill()
            NSBezierPath(ovalIn: CGRect(x: 3, y: 3, width: 22, height: 22)).fill()
        }
        let arrow = overlayArrowPath(hotspot: hotspot, scale: 0.42)
        NSColor.white.setStroke(); arrow.lineWidth = 3; arrow.lineJoinStyle = .round; arrow.stroke()
        color.setFill(); arrow.fill()

    }
}
// CUA build_default_theme.py DART_OUTLINE, HOTSPOT and UPRIGHT_DEGREES.
// Geometric source only: no embedded Lottie renderer, theme assets or Inter font.
func overlayArrowPath(hotspot: CGPoint, scale: CGFloat) -> NSBezierPath {
    let segments: [(CGPoint, CGPoint, CGPoint)] = [
        (.init(x:55,y:30), .zero, .init(x:-7,y:-2)),
        (.init(x:43,y:41), .init(x:-1,y:-8), .zero),
        (.init(x:64,y:98), .zero, .init(x:3,y:8)),
        (.init(x:77,y:99), .init(x:-4,y:7), .zero),
        (.init(x:86,y:79), .zero, .init(x:2,y:-4)),
        (.init(x:95,y:70), .init(x:-4,y:2), .zero),
        (.init(x:108,y:63), .zero, .init(x:7,y:-4)),
        (.init(x:107,y:50), .init(x:7,y:3), .zero)
    ]
    let angle = CGFloat.pi / 9
    func transform(_ point: CGPoint) -> CGPoint {
        let x = point.x - 55, y = point.y - 30
        return CGPoint(x: hotspot.x + (x * cos(angle) - y * sin(angle)) * scale,
                       y: hotspot.y + (x * sin(angle) + y * cos(angle)) * scale)
    }
    func add(_ point: CGPoint, _ tangent: CGPoint) -> CGPoint {
        CGPoint(x: point.x + tangent.x, y: point.y + tangent.y)
    }
    let path = NSBezierPath(); path.move(to: transform(segments[0].0))
    for index in segments.indices {
        let a = segments[index], b = segments[(index + 1) % segments.count]
        path.curve(to: transform(b.0), controlPoint1: transform(add(a.0, a.2)), controlPoint2: transform(add(b.0, b.1)))
    }
    path.close(); return path
}

private final class OverlayController {
    var state = OverlayPresentationState()
    let window: OverlayWindow
    let cursor: OverlayCursorView
    private var timer: Timer?
    private var displayGeometry: [String] = []
    init(session: String) {
        cursor = OverlayCursorView(session: session)
        window = OverlayWindow(contentRect: cursor.frame, styleMask: [.borderless, .nonactivatingPanel],
                               backing: .buffered, defer: false)
        // Port of CUA overlay.rs run_appkit configuration. No activation call,
        // no synthetic mouse event, no cursor warp, no always-on-top window.
        window.isOpaque = false; window.backgroundColor = .clear; window.hasShadow = false
        window.ignoresMouseEvents = true; window.sharingType = .readOnly
        window.level = .normal; window.isFloatingPanel = false
        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        window.isReleasedWhenClosed = false; window.hidesOnDeactivate = false
        window.contentView = cursor
        let timer = Timer(timeInterval: 0.05, repeats: true) { [weak self] _ in self?.checkTarget() }
        self.timer = timer
        RunLoop.main.add(timer, forMode: .common)
    }
    func identity() -> [String: Any]? {
        guard !state.closed, let birth = overlayBirth(getpid()), window.windowNumber > 0 else { return nil }
        return ["pid": getpid(), "birth": birth, "windows": [window.windowNumber]]
    }
    func control(_ value: OverlayControl) -> Bool {
        switch value.command {
        case "begin":
            guard let operation = value.operation, state.begin(operation) else { return false }
            window.orderOut(nil); cursor.pulse.reset()
        case "hide":
            guard let operation = value.operation else { return false }
            state.hide(operation)
            if state.marker == nil { window.orderOut(nil); cursor.pulse.reset() }
        default: return false
        }
        return true
    }
    func stop() {
        guard !state.closed else { return }
        state.close(); timer?.invalidate(); timer = nil
        window.orderOut(nil); window.close()
        NSApp.stop(nil)
        // Wake NSApplication's event loop even if it was idle on EOF.
        if let event = NSEvent.otherEvent(with: .applicationDefined, location: .zero, modifierFlags: [],
            timestamp: 0, windowNumber: 0, context: nil, subtype: 0, data1: 0, data2: 0) {
            NSApp.postEvent(event, atStart: true)
        }
    }
    func receive(_ event: OverlayPointerEvent) {
        let previousPress = state.pressSequence
        guard state.accept(event) else { return }
        guard targetIsCurrent(event.target), let position = event.position,
              let primary = NSScreen.screens.first(where: {
                  ($0.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value == CGMainDisplayID()
              }), NSScreen.screens.contains(where: { screen in
                  guard let id = (screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value else { return false }
                  return CGDisplayBounds(id).contains(position)
              }) else { hideStale(); return }
        displayGeometry = currentDisplayGeometry()
        let point = overlayAppKitPoint(position, primaryFrame: primary.frame)
        let size = cursor.frame.size
        window.setFrameOrigin(CGPoint(x: point.x - cursor.hotspot.x,
                                     y: point.y - (size.height - cursor.hotspot.y)))
        cursor.pulse.observe(pressSequence: event.press_sequence, previousPressSequence: previousPress,
                             at: ProcessInfo.processInfo.systemUptime)
        cursor.needsDisplay = true
        // CUA z_order.rs + overlay.rs dispatch_pin_above. If relative ordering
        // cannot be established, do not escalate to a globally topmost overlay.
        window.order(.above, relativeTo: Int(event.target.window))
        if !targetIsCurrent(event.target) || displayGeometry != currentDisplayGeometry() { hideStale() }
    }
    private func targetIsCurrent(_ target: OverlayTargetState) -> Bool {
        guard let rows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
                as? [[String: Any]], let row = rows.first(where: {
                    ($0[kCGWindowNumber as String] as? NSNumber)?.uint32Value == target.window
                }), let rawBounds = row[kCGWindowBounds as String] as? [String: Any],
              let bounds = CGRect(dictionaryRepresentation: rawBounds as CFDictionary) else { return false }
        return overlayTargetMatches(target,
            pid: (row[kCGWindowOwnerPID as String] as? NSNumber)?.uint32Value,
            birth: overlayBirth(Int32(target.pid)), bounds: bounds,
            onScreen: (row[kCGWindowIsOnscreen as String] as? NSNumber)?.boolValue == true)
    }
    private func currentDisplayGeometry() -> [String] {
        NSScreen.screens.compactMap { screen in
            guard let id = (screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value else { return nil }
            return "\(id):\(CGDisplayBounds(id)):\(screen.frame):\(screen.backingScaleFactor)"
        }.sorted()
    }
    private func hideStale() { state.hideStaleTarget(); window.orderOut(nil); cursor.pulse.reset() }
    private func checkTarget() {
        guard let marker = state.marker else { return }
        guard targetIsCurrent(marker.target), displayGeometry == currentDisplayGeometry() else { hideStale(); return }
        window.order(.above, relativeTo: Int(marker.target.window))
        cursor.needsDisplay = true
    }
}

private var overlayController: OverlayController?
private let overlayMailbox = OverlayLatestEvent()
@_cdecl("dtw_overlay_run")
func dtwOverlayRun(_ session: UnsafePointer<CChar>?,
                   _ publishIdentity: @convention(c) (UnsafePointer<CChar>) -> Int32) -> Int32 {
    guard Thread.isMainThread, let session = session else { return 1 }
    let app = NSApplication.shared
    app.setActivationPolicy(.accessory); app.finishLaunching()
    let controller = OverlayController(session: String(cString: session))
    overlayController = controller
    guard let identity = controller.identity() else { controller.stop(); return 1 }
    let ready: [String: Any] = ["ready": true, "identity": identity]
    guard let data = try? JSONSerialization.data(withJSONObject: ready),
          let line = String(data: data, encoding: .utf8) else { controller.stop(); return 1 }
    guard line.withCString({ publishIdentity($0) }) == 1 else { controller.stop(); return 1 }
    FileHandle.standardOutput.write(Data((line + "\n").utf8))
    app.run()
    overlayController = nil
    return 0
}
@_cdecl("dtw_overlay_control")
func dtwOverlayControl(_ raw: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    autoreleasepool {
        guard let raw = raw, let data = String(cString: raw).data(using: .utf8),
              let command = try? JSONDecoder().decode(OverlayControl.self, from: data) else { return strdup("{\"ok\":false}") }
        var accepted = false
        DispatchQueue.main.sync { accepted = overlayController?.control(command) == true }
        return strdup(accepted ? "{\"ok\":true}" : "{\"ok\":false}")
    }
}
@_cdecl("dtw_overlay_event")
func dtwOverlayEvent(_ raw: UnsafePointer<CChar>?) {
    autoreleasepool {
        guard let raw = raw, let data = String(cString: raw).data(using: .utf8),
              let event = try? JSONDecoder().decode(OverlayPointerEvent.self, from: data),
              overlayMailbox.put(event) else { return }
        DispatchQueue.main.async {
            if let latest = overlayMailbox.take() { overlayController?.receive(latest) }
        }
    }
}
@_cdecl("dtw_overlay_stop")
func dtwOverlayStop() { DispatchQueue.main.async { overlayController?.stop() } }
@_cdecl("dtw_overlay_free")
func dtwOverlayFree(_ value: UnsafeMutablePointer<CChar>?) { free(value) }
