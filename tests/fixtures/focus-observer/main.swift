import AppKit
import Foundation
import CoreFoundation
import Darwin

// Same read-only PSN/PID/PSN proof as DTW's skylight::front_process_pid.
// NSWorkspace.frontmostApplication can still contain the previous app while
// didActivate callbacks run; it is not a live oracle for these transitions.
private final class WindowServerForegroundReader {
    typealias Front = @convention(c) (UnsafeMutableRawPointer) -> Int32
    typealias PID = @convention(c) (UnsafeRawPointer, UnsafeMutablePointer<Int32>) -> Int32
    typealias PSN = @convention(c) (Int32, UnsafeMutableRawPointer) -> Int32
    let front: Front?
    let pid: PID?
    let psn: PSN?
    init() {
        _ = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY | RTLD_GLOBAL)
        func symbol<T>(_ name: String, _ type: T.Type) -> T? {
            guard let pointer = dlsym(UnsafeMutableRawPointer(bitPattern: -2), name) else { return nil }
            return unsafeBitCast(pointer, to: type)
        }
        front = symbol("_SLPSGetFrontProcess", Front.self)
        pid = symbol("GetProcessPID", PID.self)
        psn = symbol("GetProcessForPID", PSN.self)
    }
    func current() -> Int32? {
        guard let front, let pid, let psn else { return nil }
        var before: [UInt32] = [0, 0], after: [UInt32] = [0, 0], resolved: [UInt32] = [0, 0]
        var process: Int32 = 0
        guard before.withUnsafeMutableBytes({ front($0.baseAddress!) }) == 0, before != [0, 0],
              before.withUnsafeBytes({ pid($0.baseAddress!, &process) }) == 0, process > 0,
              resolved.withUnsafeMutableBytes({ psn(process, $0.baseAddress!) }) == 0,
              after.withUnsafeMutableBytes({ front($0.baseAddress!) }) == 0,
              before == after, before == resolved else { return nil }
        return process
    }
}

// Read-only console observer: no NSApplication, windows, activation or input.
final class FocusObserver {
    private let foreground = WindowServerForegroundReader()
    let output: FileHandle
    var timer: Timer?
    var activationObserver: NSObjectProtocol?
    var exitCode: Int32 = 0
    var stopped = false

    init(path: String) throws {
        if !FileManager.default.fileExists(atPath: path), !FileManager.default.createFile(atPath: path, contents: nil) { throw CocoaError(.fileWriteUnknown) }
        output = try FileHandle(forWritingTo: URL(fileURLWithPath: path))
        try output.seekToEnd()
    }
    func record(_ kind: String, activation: NSRunningApplication? = nil) {
        guard !stopped else { return }
        let observedPID = foreground.current()
        let pid = activation?.processIdentifier ?? observedPID
        var row: [String: Any] = ["unix_ms": Date().timeIntervalSince1970 * 1_000, "kind": kind, "focus_source": "window_server_psn", "frontmost_pid": observedPID as Any? ?? NSNull()]
        if kind == "activation" { row["observed_frontmost_pid"] = observedPID as Any? ?? NSNull(); row["activation_pid"] = pid as Any? ?? NSNull() }
        if observedPID == nil { row["error"] = "frontmost_application_unavailable" }
        else if kind == "activation", activation == nil { row["error"] = "activation_application_unavailable" }
        do { try output.write(contentsOf: JSONSerialization.data(withJSONObject: row, options: [.sortedKeys]) + Data("\n".utf8)) }
        catch { fail(error) }
    }
    func fail(_ error: Error) {
        FileHandle.standardError.write(Data(("focus observer: \(error.localizedDescription)\n").utf8))
        exitCode = 1; stop()
    }
    func start() {
        activationObserver = NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main) { [weak self] notification in
            self?.record("activation", activation: notification.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication)
        }
        record("sample")
        guard !stopped else { return }
        let samplingTimer = Timer(timeInterval: 0.002, repeats: true) { [weak self] _ in self?.record("sample") }
        samplingTimer.tolerance = 0
        timer = samplingTimer; RunLoop.main.add(samplingTimer, forMode: .common)
        FileHandle.standardOutput.write(Data("ready\n".utf8))
        // Keep the main run loop free to deliver workspace notifications/timers.
        DispatchQueue.global(qos: .utility).async { [weak self] in
            do {
                _ = try FileHandle.standardInput.read(upToCount: 1)
                DispatchQueue.main.async { self?.stop() }
            } catch {
                DispatchQueue.main.async { self?.fail(error) }
            }
        }
    }
    func stop() {
        guard !stopped else { return }
        stopped = true; timer?.invalidate()
        if let activationObserver { NSWorkspace.shared.notificationCenter.removeObserver(activationObserver) }
        CFRunLoopStop(CFRunLoopGetMain())
    }
    func close() {
        do { try output.close() } catch { fail(error) }
    }
}

if CommandLine.arguments.count == 2, CommandLine.arguments[1] == "--current" {
    guard let pid = WindowServerForegroundReader().current() else { exit(1) }
    print(pid)
    exit(0)
}

guard CommandLine.arguments.count == 2 else {
    FileHandle.standardError.write(Data("usage: focus-observer <output.jsonl>\n".utf8)); exit(2)
}
do {
    let observer = try FocusObserver(path: CommandLine.arguments[1])
    observer.start()
    if !observer.stopped { CFRunLoopRun() }
    observer.close()
    exit(observer.exitCode)
} catch {
    FileHandle.standardError.write(Data(("focus observer: \(error.localizedDescription)\n").utf8)); exit(1)
}
