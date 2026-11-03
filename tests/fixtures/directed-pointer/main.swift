// Owned-fixture experiment only. Not a DTW route, fallback or public API.
// Target-local activation adapted from the MIT CUA port documented in
// docs/background-input-port.md; no front-process or global HID operation.
import ApplicationServices
import Darwin
import Foundation

let args = CommandLine.arguments
guard args.count == 12, ["public", "skylight"].contains(args[11]),
      let pid = Int32(args[2]), let window = UInt32(args[3]),
      let left = Double(args[4]), let top = Double(args[5]),
      let x = Double(args[6]), let y = Double(args[7]),
      let endX = Double(args[8]), let endY = Double(args[9]), let count = Int(args[10]),
      pid > 0, window > 0, (1...2).contains(count),
      [left, top, x, y, endX, endY].allSatisfy({ $0.isFinite }),
      let rows = CGWindowListCopyWindowInfo(.optionIncludingWindow, window) as? [[String: Any]],
      rows.contains(where: { ($0[kCGWindowOwnerPID as String] as? Int32) == pid }),
      let library = dlopen("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics", RTLD_LAZY),
      let symbol = dlsym(library, "CGEventSetWindowLocation"),
      let source = CGEventSource(stateID: .hidSystemState) else { fatalError("invalid owned experiment") }
typealias SetWindowPoint = @convention(c) (OpaquePointer, Double, Double) -> Void
let windowPoint = unsafeBitCast(symbol, to: SetWindowPoint.self)
// The same target-only AppKit activation record as the pinned CUA primitive.
// This changes app-local activation; it never makes the process frontmost.
let sky = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY)!
func resolveSymbol<T>(_ name: String, _ type: T.Type) -> T {
    guard let raw = dlsym(sky, name) ?? dlsym(library, name) else { fatalError("missing \(name)") }
    return unsafeBitCast(raw, to: type)
}
let connection = resolveSymbol("CGSMainConnectionID", (@convention(c) () -> UInt32).self)
let ownerOf = resolveSymbol("SLSGetWindowOwner", (@convention(c) (UInt32, UInt32, UnsafeMutablePointer<UInt32>) -> Int32).self)
let psnOf = resolveSymbol("SLSGetConnectionPSN", (@convention(c) (UInt32, UnsafeMutableRawPointer) -> Int32).self)
let postRecord = resolveSymbol("SLPSPostEventRecordTo", (@convention(c) (UnsafeRawPointer, UnsafeRawPointer) -> Int32).self)
let postToPid = resolveSymbol("SLEventPostToPid", (@convention(c) (Int32, OpaquePointer) -> Void).self)
var owner: UInt32 = 0
var psn = [UInt8](repeating: 0, count: 8)
guard ownerOf(connection(), window, &owner) == 0, owner != 0,
      psn.withUnsafeMutableBytes({ psnOf(owner, $0.baseAddress!) }) == 0 else { fatalError("owner proof") }
var record = [UInt8](repeating: 0, count: 0xF8)
record[4] = 0xF8; record[8] = 0x0D; record[0x8A] = 1
withUnsafeBytes(of: window.littleEndian) { record.replaceSubrange(0x3C..<0x40, with: $0) }
guard psn.withUnsafeBytes({ p in record.withUnsafeBytes { postRecord(p.baseAddress!, $0.baseAddress!) } }) == 0 else {
    fatalError("target-local preparation")
}
Thread.sleep(forTimeInterval: 0.05)
func post(_ type: CGEventType, _ point: CGPoint, _ previous: CGPoint, _ click: Int = 1) {
    guard let event = CGEvent(mouseEventSource: source, mouseType: type,
                              mouseCursorPosition: point, mouseButton: .left) else { fatalError("allocation") }
    event.flags = []
    event.setIntegerValueField(.mouseEventClickState, value: type == .mouseMoved ? 0 : Int64(click))
    event.setIntegerValueField(.mouseEventDeltaX, value: Int64((point.x - previous.x).rounded()))
    event.setIntegerValueField(.mouseEventDeltaY, value: Int64((point.y - previous.y).rounded()))
    for field: UInt32 in [51, 91, 92] {
        event.setIntegerValueField(CGEventField(rawValue: field)!, value: Int64(window))
    }
    event.setIntegerValueField(CGEventField(rawValue: 40)!, value: Int64(pid))
    event.setIntegerValueField(CGEventField(rawValue: 7)!, value: type == .leftMouseDragged ? 0 : 3)
    windowPoint(OpaquePointer(Unmanaged.passUnretained(event).toOpaque()), point.x - left, point.y - top)
    if args[11] == "public" { event.postToPid(pid) }
    else { postToPid(pid, OpaquePointer(Unmanaged.passUnretained(event).toOpaque())) }
}
let first = CGPoint(x: x, y: y)
let end = CGPoint(x: endX, y: endY)
let previous = CGEvent(source: nil)?.location ?? first
switch args[1] {
case "move": post(.mouseMoved, first, previous)
case "click":
    post(.mouseMoved, first, first)
    for index in 1...count {
        post(.leftMouseDown, first, first, index)
        Thread.sleep(forTimeInterval: 0.028)
        post(.leftMouseUp, first, first, index)
        Thread.sleep(forTimeInterval: 0.08)
    }
case "drag":
    post(.mouseMoved, first, first)
    post(.leftMouseDown, first, first)
    var last = first
    defer { post(.leftMouseUp, last, last) }
    for index in 1...40 {
        let t = Double(index) / 40
        let next = CGPoint(x: first.x + (end.x - first.x) * t, y: first.y + (end.y - first.y) * t)
        post(.leftMouseDragged, next, last)
        last = next
        Thread.sleep(forTimeInterval: 0.01)
    }
default: fatalError("unknown operation")
}
Thread.sleep(forTimeInterval: 0.2)
