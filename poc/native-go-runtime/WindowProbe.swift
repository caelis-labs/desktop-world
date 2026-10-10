// Read-only comparison of CGWindow and AXWindows for one owned test PID.
// Prints only counts and exact-title matches, never unrelated window names.
import AppKit
import ApplicationServices

guard CommandLine.arguments.count == 3,
      let pid = Int32(CommandLine.arguments[1]) else {
  fputs("usage: WindowProbe PID EXACT_OWNED_TITLE\n", stderr)
  exit(2)
}
let title = CommandLine.arguments[2]
let all = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] ?? []
let owned = all.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == pid }
let cgMatches = owned.filter { ($0[kCGWindowName as String] as? String) == title }
let app = AXUIElementCreateApplication(pid)
AXUIElementSetMessagingTimeout(app, 2)
var raw: CFTypeRef?
let code = AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &raw)
let rawType = raw.map { CFGetTypeID($0) } ?? 0
let arrayType = CFArrayGetTypeID()
let elementType = AXUIElementGetTypeID()
let windows = raw as? [AXUIElement] ?? []
var axMatches = 0
var axTitles: [String] = []
var axRoles: [String] = []
var axSameAsApp: [Bool] = []
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
}
let result: [String: Any] = [
  "pid": pid, "cg_windows": owned.count, "cg_exact_matches": cgMatches.count,
  "ax_status": code.rawValue, "ax_windows": windows.count, "ax_exact_matches": axMatches,
  "ax_raw_type": rawType, "cf_array_type": arrayType, "ax_element_type": elementType,
  "ax_owned_titles": axTitles,
  "ax_roles": axRoles,
  "ax_same_as_app": axSameAsApp,
  "cg_owned": owned.map { ["name": $0[kCGWindowName as String] as? String ?? "", "layer": String(describing:$0[kCGWindowLayer as String] ?? ""), "number": String(describing:$0[kCGWindowNumber as String] ?? "")] },
]
let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
print(String(decoding: data, as: UTF8.self))
