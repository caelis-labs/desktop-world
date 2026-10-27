// Manual, owned-app diagnostic control. Not linked into DTW or run by CI.
// Use a newly launched TextEdit instance and a saved disposable document only.
import AppKit
import ApplicationServices
import Foundation

func emit(_ value: Any) {
    let data = try! JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
}
func attribute(_ element: AXUIElement, _ name: String) -> (AXError, CFTypeRef?) {
    var value: CFTypeRef?
    let error = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    return (error, value)
}
typealias ExactID = @convention(c) (AXUIElement, UnsafeMutablePointer<CGWindowID>) -> AXError
let framework = dlopen("/System/Library/Frameworks/ApplicationServices.framework/Frameworks/HIServices.framework/HIServices", RTLD_LAZY)
let exactID = framework.flatMap { dlsym($0, "_AXUIElementGetWindow") }.map { unsafeBitCast($0, to: ExactID.self) }
let args = CommandLine.arguments
if args.count == 2 && args[1] == "list" {
    emit(NSWorkspace.shared.runningApplications.filter { $0.bundleIdentifier == "com.apple.TextEdit" }.map {
        ["pid": $0.processIdentifier, "launch": $0.launchDate?.timeIntervalSince1970 ?? 0] as [String: Any]
    })
    exit(0)
}
guard args.count >= 4, let pid = Int32(args[1]), let launch = Double(args[2]),
      let app = NSRunningApplication(processIdentifier: pid), app.bundleIdentifier == "com.apple.TextEdit",
      abs((app.launchDate?.timeIntervalSince1970 ?? 0) - launch) < 0.001 else {
    fputs("Expected owned TextEdit PID, launch timestamp, operation and optional window ID\n", stderr)
    exit(2)
}
let operation = args[3]
if operation == "activate" { emit(["accepted": app.activate(options: [])]); exit(0) }
if operation == "hide" { emit(["accepted": app.hide()]); exit(0) }
if operation == "unhide" { emit(["accepted": app.unhide()]); exit(0) }
if operation == "terminate" { emit(["accepted": app.terminate()]); exit(0) }
guard let allRows = CGWindowListCopyWindowInfo([.excludeDesktopElements], 0) as? [[String: Any]] else {
    emit(["pid": pid, "cg_query_succeeded": false]); exit(6)
}
let rows = allRows
    .filter { $0[kCGWindowOwnerPID as String] as? Int32 == pid }
    .map { row -> [String: Any] in
        var value: [String: Any] = [:]
        for key in [kCGWindowNumber, kCGWindowLayer, kCGWindowIsOnscreen, kCGWindowBounds] {
            if let item = row[key as String] { value[key as String] = item }
        }
        return value
    }
var result: [String: Any] = ["pid": pid, "launch": launch, "operation": operation,
    "at_unix_ms": Int64(Date().timeIntervalSince1970 * 1000),
    "hidden": app.isHidden, "active": app.isActive, "cg_query_succeeded": true, "cg": rows]
if operation == "cg" { emit(result); exit(0) } // Deliberately no AX request before the cold DTW read.
let application = AXUIElementCreateApplication(pid)
AXUIElementSetMessagingTimeout(application, 2)
let started = ProcessInfo.processInfo.systemUptime
let (windowsError, value) = attribute(application, kAXWindowsAttribute)
let windows = value as? [AXUIElement] ?? []
result["ax_windows_error"] = windowsError.rawValue
result["ax_array_valid"] = value as? [AXUIElement] != nil
result["ax_elapsed_ms"] = (ProcessInfo.processInfo.systemUptime - started) * 1000
var selected: AXUIElement?
result["ax"] = windows.map { window -> [String: Any] in
    var id: CGWindowID = 0
    let error = exactID?(window, &id)
    if args.count == 5 && String(id) == args[4] && error == .success { selected = window }
    var row: [String: Any] = ["id": id, "id_error": error?.rawValue ?? -1]
    for name in [kAXRoleAttribute, kAXSubroleAttribute, kAXMinimizedAttribute, "AXFullScreen"] {
        let (error, value) = attribute(window, name)
        row[name + "_error"] = error.rawValue
        if let value = value as? String { row[name] = value }
        else if let value = value as? NSNumber { row[name] = value }
    }
    return row
}
if operation != "inspect" {
    guard let window = selected else { emit(result); exit(3) }
    let error: AXError
    switch operation {
    case "minimize", "unminimize":
        error = AXUIElementSetAttributeValue(window, kAXMinimizedAttribute as CFString,
                                             operation == "minimize" ? kCFBooleanTrue : kCFBooleanFalse)
    case "fullscreen", "windowed":
        error = AXUIElementSetAttributeValue(window, "AXFullScreen" as CFString,
                                             operation == "fullscreen" ? kCFBooleanTrue : kCFBooleanFalse)
    case "close":
        let (lookup, button) = attribute(window, kAXCloseButtonAttribute)
        guard lookup == .success, let button, CFGetTypeID(button) == AXUIElementGetTypeID() else { emit(result); exit(4) }
        error = AXUIElementPerformAction(unsafeBitCast(button, to: AXUIElement.self), kAXPressAction as CFString)
    default: exit(2)
    }
    result["action_error"] = error.rawValue
    emit(result)
    exit(error == .success ? 0 : 5)
}
emit(result)
