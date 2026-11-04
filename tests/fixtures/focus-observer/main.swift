import AppKit
import Foundation
import CoreFoundation
import Darwin

// Read-only console observer: no NSApplication, windows, activation or input.
final class FocusObserver {
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
        let observedPID = NSWorkspace.shared.frontmostApplication?.processIdentifier
        let pid = activation?.processIdentifier ?? observedPID
        var row: [String: Any] = ["unix_ms": Date().timeIntervalSince1970 * 1_000, "kind": kind, "frontmost_pid": pid as Any? ?? NSNull()]
        if kind == "activation" { row["observed_frontmost_pid"] = observedPID as Any? ?? NSNull() }
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
