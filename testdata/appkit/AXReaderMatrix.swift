import AppKit
import ApplicationServices
import Darwin

// Read-only, one explicitly owned PID. Compare the exact AX APIs used by the
// native scanner with the direct-value probe at several per-element timeouts.
guard CommandLine.arguments.count == 2,
  let pid = Int32(CommandLine.arguments[1]), pid > 0 else {
  fputs("usage: AXReaderMatrix OWNED_PID\n", stderr)
  exit(2)
}
typealias WindowID = @convention(c) (AXUIElement, UnsafeMutablePointer<CGWindowID>) -> AXError
let symbol = dlsym(dlopen(nil, RTLD_NOW), "_AXUIElementGetWindow")
let getWindow = symbol.map { unsafeBitCast($0, to: WindowID.self) }
func describe(_ values: [AXUIElement], _ app: AXUIElement) -> [[String: Any]] {
  values.prefix(8).map { value in
    var role: CFTypeRef?
    let roleStatus = AXUIElementCopyAttributeValue(value, kAXRoleAttribute as CFString, &role)
    var owner: pid_t = 0
    let pidStatus = AXUIElementGetPid(value, &owner)
    var number: CGWindowID = 0
    let numberStatus = getWindow?(value, &number).rawValue ?? -1
    return ["role": role as? String ?? "<unavailable>",
      "role_status": roleStatus.rawValue, "pid_status": pidStatus.rawValue,
      "owned_pid": owner == pid, "same_as_app": CFEqual(value, app),
      "native_status": numberStatus, "native_number": number]
  }
}
var trials: [[String: Any]] = []
for timeout: Float in [0.05, 0.25, 2, 5] {
  let app = AXUIElementCreateApplication(pid)
  let setStatus = AXUIElementSetMessagingTimeout(app, timeout)
  let start = ProcessInfo.processInfo.systemUptime
  var raw: CFTypeRef?
  let directStatus = AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &raw)
  let direct = raw as? [AXUIElement] ?? []
  var count: CFIndex = 0
  let countStatus = AXUIElementGetAttributeValueCount(app, kAXWindowsAttribute as CFString, &count)
  var listedRaw: CFArray?
  let listedStatus = count > 0 ? AXUIElementCopyAttributeValues(app,
    kAXWindowsAttribute as CFString, 0, count, &listedRaw) : AXError.noValue
  let listed = listedRaw as? [AXUIElement] ?? []
  trials.append(["timeout": timeout, "set_status": setStatus.rawValue,
    "direct_status": directStatus.rawValue, "direct": describe(direct, app),
    "count_status": countStatus.rawValue, "count": count,
    "listed_status": listedStatus.rawValue, "listed": describe(listed, app),
    "elapsed_ms": Int((ProcessInfo.processInfo.systemUptime - start) * 1000)])
}
let result: [String: Any] = ["pid": pid, "reader_ax_trusted": AXIsProcessTrusted(),
  "reader_executable": Bundle.main.executablePath ?? "", "trials": trials]
let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
print(String(decoding: data, as: UTF8.self))
