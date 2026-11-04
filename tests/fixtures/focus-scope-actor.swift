import AppKit
import Darwin

// Environment injection only. Each activation is limited to an exact owned
// process incarnation and bundle path; this helper never generates input.
func birth(_ pid: pid_t) -> UInt64? {
    var info = proc_bsdinfo()
    let size = Int32(MemoryLayout<proc_bsdinfo>.size)
    guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size) == size,
          info.pbi_pid == UInt32(pid), info.pbi_start_tvusec < 1_000_000 else { return nil }
    return info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec
}
func record(_ value: [String: Any]) throws {
    try FileHandle.standardOutput.write(contentsOf: JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) + Data("\n".utf8))
}
do {
    let args = CommandLine.arguments
    guard args.count == 4, let pid = Int32(args[1]), pid > 0,
          let expectedBirth = UInt64(args[2]), expectedBirth > 0 else {
        throw NSError(domain: "focus-scope-actor", code: 2, userInfo: [NSLocalizedDescriptionKey: "usage: focus-scope-actor PID KERNEL_BIRTH BUNDLE_PATH"])
    }
    let expected = URL(fileURLWithPath: args[3]).resolvingSymlinksInPath().standardizedFileURL
    guard let app = NSRunningApplication(processIdentifier: pid), !app.isTerminated,
          birth(pid) == expectedBirth,
          app.bundleURL?.resolvingSymlinksInPath().standardizedFileURL == expected else {
        throw NSError(domain: "focus-scope-actor", code: 3, userInfo: [NSLocalizedDescriptionKey: "owned process identity mismatch"])
    }
    let started = Date().timeIntervalSince1970 * 1_000
    guard app.activate(options: []) else {
        throw NSError(domain: "focus-scope-actor", code: 4, userInfo: [NSLocalizedDescriptionKey: "activation refused"])
    }
    try record(["kind": "environment_activation", "pid": pid, "process_birth": expectedBirth,
                "bundle": expected.path, "started_unix_ms": started,
                "ended_unix_ms": Date().timeIntervalSince1970 * 1_000])
} catch {
    FileHandle.standardError.write(Data(("focus-scope-actor: \(error)\n").utf8)); exit(1)
}
