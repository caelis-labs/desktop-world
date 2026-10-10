// Read-only comparison of CGWindow and AXWindows for one owned test PID.
// Prints only counts and exact-title matches, never unrelated window names.
import AppKit
import ApplicationServices
import Darwin

guard CommandLine.arguments.count == 3,
      let pid = Int32(CommandLine.arguments[1]) else {
  fputs("usage: WindowProbe PID EXACT_OWNED_TITLE\n", stderr)
  exit(2)
}
let title = CommandLine.arguments[2]
var processInfo = proc_bsdinfo()
let processInfoBytes = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0,
  &processInfo, Int32(MemoryLayout<proc_bsdinfo>.size))
let all = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
let owned = all.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == pid }
let onScreen = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
let ownedOnScreen = onScreen.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == pid }
let cgMatches = owned.filter { ($0[kCGWindowName as String] as? String) == title }
let app = AXUIElementCreateApplication(pid)
AXUIElementSetMessagingTimeout(app, 2)
var raw: CFTypeRef?
let code = AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &raw)
let rawType = raw.map { CFGetTypeID($0) } ?? 0
let arrayType = CFArrayGetTypeID()
let elementType = AXUIElementGetTypeID()
let windows = raw as? [AXUIElement] ?? []
// Existing cooperative backend uses this macOS SPI. This read-only probe
// checks whether an AX window can be joined to an owned CGWindow by number;
// absence or error is an unresolved identity, never a title-based fallback.
typealias AXWindowNumber = @convention(c) (AXUIElement, UnsafeMutablePointer<CGWindowID>) -> AXError
let axWindowSymbol = dlsym(dlopen(nil, RTLD_NOW), "_AXUIElementGetWindow")
let axWindowNumber = axWindowSymbol.map { unsafeBitCast($0, to: AXWindowNumber.self) }
func elementMetadata(_ element: AXUIElement?) -> [String: Any] {
  guard let element else { return ["present": false] }
  var role: CFTypeRef?
  let roleStatus = AXUIElementCopyAttributeValue(element, kAXRoleAttribute as CFString, &role)
  var owner: pid_t = 0
  let pidStatus = AXUIElementGetPid(element, &owner)
  var number: CGWindowID = 0
  let nativeStatus = axWindowNumber?(element, &number).rawValue ?? -1
  return ["present": true, "role": role as? String ?? "<unavailable>",
    "role_status": roleStatus.rawValue, "pid_status": pidStatus.rawValue,
    "owner_is_target": owner == pid, "native_status": nativeStatus,
    "native_number": number]
}
func appAttribute(_ key: CFString) -> [String: Any] {
  var value: CFTypeRef?
  let status = AXUIElementCopyAttributeValue(app, key, &value)
  let element = value.flatMap { CFGetTypeID($0) == AXUIElementGetTypeID() ? ($0 as! AXUIElement) : nil }
  var result = elementMetadata(element)
  result["attribute_status"] = status.rawValue
  return result
}
func appChildren(_ key: CFString) -> [String: Any] {
  var value: CFTypeRef?
  let status = AXUIElementCopyAttributeValue(app, key, &value)
  let children = value as? [AXUIElement] ?? []
  return ["attribute_status": status.rawValue, "count": children.count,
    "elements": children.prefix(32).map { elementMetadata($0) }]
}
var hit: AXUIElement?
var hitStatus = AXError.failure
if let bounds = cgMatches.first?[kCGWindowBounds as String] as? [String: CGFloat],
   let x = bounds["X"], let y = bounds["Y"], let width = bounds["Width"], let height = bounds["Height"] {
  hitStatus = AXUIElementCopyElementAtPosition(app, Float(x + width / 2), Float(y + height / 2), &hit)
}
var axMatches = 0
var axTitles: [String] = []
var axRoles: [String] = []
var axSameAsApp: [Bool] = []
var axNative: [[String: Any]] = []
for window in windows {
  axSameAsApp.append(CFEqual(window, app))
  var name: CFTypeRef?
  if AXUIElementCopyAttributeValue(window, kAXTitleAttribute as CFString, &name) == .success {
    let value = name as? String ?? "<non-string>"
    axTitles.append(value)
    if value == title { axMatches += 1 }
  } else { axTitles.append("<unavailable>") }
  var role: CFTypeRef?
  if AXUIElementCopyAttributeValue(window, kAXRoleAttribute as CFString, &role) == .success {
    axRoles.append(role as? String ?? "<non-string>")
  } else { axRoles.append("<unavailable>") }
  var number: CGWindowID = 0
  let status = axWindowNumber?(window, &number).rawValue ?? -1
  let matches = owned.filter { ($0[kCGWindowNumber as String] as? CGWindowID) == number }
  axNative.append(["status": status, "number": number,
    "owned_cg_matches": matches.count, "owner_pid_matches": matches.count == 1])
}
let result: [String: Any] = [
  "pid": pid, "process_start_sec": processInfoBytes == MemoryLayout<proc_bsdinfo>.size ? processInfo.pbi_start_tvsec : 0,
  "process_start_usec": processInfoBytes == MemoryLayout<proc_bsdinfo>.size ? processInfo.pbi_start_tvusec : 0,
  "caller_ax_trusted": AXIsProcessTrusted(),
  "main_window": appAttribute(kAXMainWindowAttribute as CFString),
  "focused_window": appAttribute(kAXFocusedWindowAttribute as CFString),
  "children": appChildren(kAXChildrenAttribute as CFString),
  "app_hit_status": hitStatus.rawValue,
  "app_hit": elementMetadata(hit),
  "cg_windows": owned.count, "cg_exact_matches": cgMatches.count,
  "cg_on_screen_owned": ownedOnScreen.count,
  "cg_on_screen_exact_matches": ownedOnScreen.filter { ($0[kCGWindowName as String] as? String) == title }.count,
  "ax_status": code.rawValue, "ax_windows": windows.count, "ax_exact_matches": axMatches,
  "ax_raw_type": rawType, "cf_array_type": arrayType, "ax_element_type": elementType,
  "ax_owned_titles": axTitles,
  "ax_roles": axRoles,
  "ax_same_as_app": axSameAsApp,
  "ax_native": axNative,
  "cg_owned": owned.map { ["name": $0[kCGWindowName as String] as? String ?? "", "layer": String(describing:$0[kCGWindowLayer as String] ?? ""), "number": String(describing:$0[kCGWindowNumber as String] ?? "")] },
]
let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
print(String(decoding: data, as: UTF8.self))
