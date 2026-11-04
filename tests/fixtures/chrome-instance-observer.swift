// Read-only metadata observer: no AX, activation, input, or process control.
import AppKit
import Darwin

func birth(_ pid: Int32) -> UInt64? {
    var info = proc_bsdinfo()
    let size = Int32(MemoryLayout<proc_bsdinfo>.size)
    guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size) == size else { return nil }
    return UInt64(info.pbi_start_tvsec) * 1_000_000 + UInt64(info.pbi_start_tvusec)
}
func path(_ pid: Int32) -> String? {
    // proc_info.h defines PROC_PIDPATHINFO_MAXSIZE as 4 * MAXPATHLEN;
    // the SDK's compound macro is not imported into Swift.
    var bytes = [CChar](repeating: 0, count: 4 * Int(MAXPATHLEN))
    guard proc_pidpath(pid, &bytes, UInt32(bytes.count)) > 0 else { return nil }
    return String(cString: bytes)
}
func arguments(_ pid: Int32) -> [String]? {
    var mib = [Int32(CTL_KERN), Int32(KERN_PROCARGS2), pid]
    var size = 0
    guard sysctl(&mib, UInt32(mib.count), nil, &size, nil, 0) == 0,
          size > MemoryLayout<Int32>.size, size <= 2 * 1024 * 1024 else { return nil }
    var bytes = [UInt8](repeating: 0, count: size)
    guard sysctl(&mib, UInt32(mib.count), &bytes, &size, nil, 0) == 0 else { return nil }
    var count: Int32 = 0
    _ = withUnsafeMutableBytes(of: &count) { bytes.copyBytes(to: $0, count: MemoryLayout<Int32>.size) }
    guard count > 0, count <= 1000 else { return nil }
    var offset = MemoryLayout<Int32>.size
    // Skip the saved executable path and its NUL padding, then read argc strings.
    while offset < size && bytes[offset] != 0 { offset += 1 }
    while offset < size && bytes[offset] == 0 { offset += 1 }
    var result: [String] = []
    for _ in 0..<count {
        let start = offset
        while offset < size && bytes[offset] != 0 { offset += 1 }
        guard offset < size, let value = String(bytes: bytes[start..<offset], encoding: .utf8) else { return nil }
        result.append(value)
        offset += 1
    }
    return result
}
func canonical(_ url: URL) -> String { url.resolvingSymlinksInPath().standardizedFileURL.path }

let args = CommandLine.arguments
if args.count != 3 || Int32(args[1]) == nil { exit(64) }
let pid = Int32(args[1])!
let bundle = canonical(URL(fileURLWithPath: args[2]))
let before = birth(pid)
let executable = path(pid)
let argv = arguments(pid)
let running = NSRunningApplication(processIdentifier: pid)
let instances = NSWorkspace.shared.runningApplications.filter {
    $0.bundleURL.map(canonical) == bundle && !$0.isTerminated
}.map { Int($0.processIdentifier) }.sorted()
let after = birth(pid)
let result: [String: Any] = [
    "pid": Int(pid), "birth": before as Any? ?? NSNull(),
    "stable": before != nil && before == after,
    "executable": executable as Any? ?? NSNull(),
    "argv": argv as Any? ?? NSNull(),
    "bundle_path": running?.bundleURL.map(canonical) as Any? ?? NSNull(),
    "instances": instances
]
let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
print(String(decoding: data, as: UTF8.self))
