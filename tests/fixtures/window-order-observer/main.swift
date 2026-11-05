import CoreGraphics
import Foundation

// Read-only WindowServer order witness. No AX, activation, screenshots or input.
// Collect only the specified owners and omit window titles/content. Rank is the
// documented front-to-back order; compare layer and overlapping bounds as well.
func snapshot(_ owners: Set<Int32>) throws -> [String: Any] {
    let started = Date().timeIntervalSince1970 * 1_000
    guard let rows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], 0)
        as? [[String: Any]] else { throw CocoaError(.fileReadUnknown) }
    let windows: [[String: Any]] = rows.enumerated().compactMap { rank, row in
        guard let pid = row[kCGWindowOwnerPID as String] as? Int32, owners.contains(pid),
              let id = row[kCGWindowNumber as String] as? UInt32,
              let layer = row[kCGWindowLayer as String] as? Int,
              let raw = row[kCGWindowBounds as String] as? NSDictionary,
              let bounds = CGRect(dictionaryRepresentation: raw),
              bounds.width > 0, bounds.height > 0 else { return nil }
        return ["owner_pid": pid, "window_id": id, "layer": layer, "rank": rank,
                "bounds": [bounds.minX, bounds.minY, bounds.width, bounds.height],
                "alpha": row[kCGWindowAlpha as String] ?? NSNull()]
    }
    return ["unix_ms": started, "finished_unix_ms": Date().timeIntervalSince1970 * 1_000,
            "windows": windows]
}

func encode(_ value: [String: Any]) throws -> Data {
    try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) + Data("\n".utf8)
}

let args = CommandLine.arguments
guard args.count == 3 else {
    FileHandle.standardError.write(Data("usage: window-order-observer OUTPUT|--current PID[,PID...]\n".utf8)); exit(2)
}
let parts = args[2].split(separator: ",", omittingEmptySubsequences: false)
let pids = parts.compactMap { Int32($0) }
guard !pids.isEmpty, pids.count <= 8, pids.count == parts.count,
      pids.allSatisfy({ $0 > 0 }) else { exit(2) }
let owners = Set(pids)
do {
    if args[1] == "--current" {
        try FileHandle.standardOutput.write(contentsOf: encode(snapshot(owners)))
    } else {
        guard FileManager.default.createFile(atPath: args[1], contents: nil) else {
            throw CocoaError(.fileWriteUnknown)
        }
        let output = try FileHandle(forWritingTo: URL(fileURLWithPath: args[1]))
        var failure: Error?
        let until = Date().addingTimeInterval(600)
        let timer = Timer(timeInterval: 0.05, repeats: true) { _ in
            do {
                try output.write(contentsOf: encode(snapshot(owners)))
                if Date() >= until { CFRunLoopStop(CFRunLoopGetMain()) }
            } catch { failure = error; CFRunLoopStop(CFRunLoopGetMain()) }
        }
        timer.tolerance = 0
        RunLoop.main.add(timer, forMode: .common)
        DispatchQueue.global(qos: .utility).async {
            _ = try? FileHandle.standardInput.read(upToCount: 1)
            DispatchQueue.main.async { CFRunLoopStop(CFRunLoopGetMain()) }
        }
        FileHandle.standardOutput.write(Data("ready\n".utf8))
        CFRunLoopRun()
        timer.invalidate()
        try output.close()
        if let failure { throw failure }
    }
} catch {
    FileHandle.standardError.write(Data("window order observer: \(error)\n".utf8)); exit(1)
}
