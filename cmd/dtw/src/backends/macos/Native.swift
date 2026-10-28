// Narrow source port from Cua Driver 0.38.0, commit
// 85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19. Copyright (c) 2025 Cua AI, Inc.
// MIT; full source mapping and retained license: docs/native-source-provenance.md.
// The worker owns this registry. No CUA ToolRegistry, focus or input fallback.
import Foundation
import ApplicationServices
import Darwin

private struct NativeFailure: Error {
    let code: String
    let message: String
    let details: [String: Any]?

    var json: [String: Any] {
        var value: [String: Any] = ["error": code, "message": message]
        if let details { value["details"] = details }
        return value
    }
}

private func failure(_ code: String, _ message: String, details: [String: Any]? = nil) -> NativeFailure {
    NativeFailure(code: code, message: message, details: details)
}

private func integer(_ value: Any?, _ field: String) throws -> UInt64 {
    guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
          let result = UInt64(number.stringValue) else {
        throw failure("invalid_request", "\(field) must be an unsigned integer")
    }
    return result
}

private struct Deadline {
    let uptime: TimeInterval

    init(_ request: [String: Any]) throws {
        let milliseconds = try integer(request["deadline_unix_ms"], "deadline_unix_ms")
        let remaining = Double(milliseconds) / 1_000 - Date().timeIntervalSince1970
        guard remaining > 0 else { throw failure("deadline_exceeded", "Request deadline elapsed") }
        // Wall time establishes the budget once; backwards clock changes cannot
        // extend an admitted operation. The parent also terminates stuck workers.
        uptime = ProcessInfo.processInfo.systemUptime + min(remaining, 30)
    }

    func check() throws {
        guard ProcessInfo.processInfo.systemUptime < uptime else {
            throw failure("deadline_exceeded", "Request deadline elapsed")
        }
    }

    func prepare(_ element: AXUIElement) throws {
        try check()
        let remaining = uptime - ProcessInfo.processInfo.systemUptime
        guard remaining > 0.001 else { throw failure("deadline_exceeded", "No AX call budget remains") }
        let result = AXUIElementSetMessagingTimeout(element, Float(min(0.2, remaining)))
        guard result == .success else {
            throw failure("ax_unavailable", "Cannot set AX messaging deadline: \(result.rawValue)")
        }
        try check()
    }
}

private struct NativeApp: Equatable, Hashable {
    let identity: String
    let pid: Int32
    let generation: UInt64

    init(identity: String, pid: Int32, generation: UInt64) {
        self.identity = identity
        self.pid = pid
        self.generation = generation
    }

    init(_ value: Any?) throws {
        guard let json = value as? [String: Any], let identity = json["identity"] as? String,
              !identity.isEmpty else { throw failure("invalid_request", "Missing app identity") }
        let pid = try integer(json["pid"], "app.pid")
        let generation = try integer(json["generation"], "app.generation")
        guard pid > 0, pid <= UInt64(Int32.max), generation > 0 else {
            throw failure("invalid_request", "Invalid app process identity")
        }
        self.init(identity: identity, pid: Int32(pid), generation: generation)
    }

    var json: [String: Any] { ["identity": identity, "pid": pid, "generation": generation] }
}

private struct NativeTarget: Equatable {
    let app: NativeApp
    let nativeID: CGWindowID
    let generation: UInt64

    init(app: NativeApp, nativeID: CGWindowID, generation: UInt64) {
        self.app = app
        self.nativeID = nativeID
        self.generation = generation
    }

    init(_ value: Any?) throws {
        guard let json = value as? [String: Any] else {
            throw failure("invalid_request", "Missing window target")
        }
        let id = try integer(json["native_id"], "target.native_id")
        let generation = try integer(json["generation"], "target.generation")
        guard id > 0, id <= UInt64(UInt32.max), generation > 0 else {
            throw failure("invalid_request", "Invalid window identity")
        }
        self.init(app: try NativeApp(json["app"]), nativeID: UInt32(id), generation: generation)
    }

    var json: [String: Any] { ["app": app.json, "native_id": nativeID, "generation": generation] }
}

private struct BundleMetadata {
    let path: String
    let identifier: String
    let name: String
    let background: Bool
}

private struct KernelApp {
    let app: NativeApp
    let metadata: BundleMetadata
    let executable: String

    var json: [String: Any] {
        ["app": app.json, "name": metadata.name, "bundle_id": metadata.identifier,
         "bundle_path": metadata.path, "executable": executable]
    }
}

// Per-request metadata cache only. No NSWorkspace runningApplications cache or
// fallback, and no persistent cached decision that a PID is still this app.
private final class RequestContext {
    let deadline: Deadline
    private var metadata: [String: BundleMetadata] = [:]
    private var unreadableBundles: Set<String> = []
    private var modalVisits = 0

    init(_ request: [String: Any]) throws { deadline = try Deadline(request) }

    func admitModalVisit() throws {
        try deadline.check()
        guard modalVisits < 128 else {
            throw failure("modal_state_unproven", "Modal verification exceeded 128 element visits in this request")
        }
        modalVisits += 1
    }

    private func startStamp(_ pid: Int32) throws -> UInt64? {
        try deadline.check()
        var info = proc_bsdinfo()
        let size = Int32(MemoryLayout<proc_bsdinfo>.size)
        guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size) == size,
              info.pbi_pid == UInt32(pid), info.pbi_start_tvusec < 1_000_000 else { return nil }
        let (seconds, overflow) = info.pbi_start_tvsec.multipliedReportingOverflow(by: 1_000_000)
        let (stamp, addedOverflow) = seconds.addingReportingOverflow(info.pbi_start_tvusec)
        return overflow || addedOverflow || stamp == 0 ? nil : stamp
    }

    private func executable(_ pid: Int32) throws -> String? {
        try deadline.check()
        var buffer = [UInt8](repeating: 0, count: 4_096)
        let count = buffer.withUnsafeMutableBytes { proc_pidpath(pid, $0.baseAddress, UInt32($0.count)) }
        guard count > 0, count < buffer.count,
              let zero = buffer.firstIndex(of: 0),
              let path = String(bytes: buffer[..<zero], encoding: .utf8), !path.isEmpty else { return nil }
        return URL(fileURLWithPath: path).resolvingSymlinksInPath().standardizedFileURL.path
    }

    private func bundle(_ executable: String) throws -> BundleMetadata? {
        // Use the innermost bundle: Chrome Helper's own LSUIElement flag must
        // classify the helper, not the outer Chrome application's metadata.
        guard let marker = executable.range(of: ".app/Contents/MacOS/", options: .backwards) else { return nil }
        let end = executable.index(marker.lowerBound, offsetBy: 4)
        let path = String(executable[..<end])
        if let cached = metadata[path] { return cached }
        if unreadableBundles.contains(path) { return nil }
        try deadline.check()
        let plistURL = URL(fileURLWithPath: path).appendingPathComponent("Contents/Info.plist")
        guard let size = try? plistURL.resourceValues(forKeys: [.fileSizeKey]).fileSize,
              size <= 2 * 1_024 * 1_024,
              let data = try? Data(contentsOf: plistURL),
              data.count <= 2 * 1_024 * 1_024,
              let plist = (try? PropertyListSerialization.propertyList(from: data, format: nil)) as? [String: Any],
              let identifier = plist["CFBundleIdentifier"] as? String, !identifier.isEmpty else {
            unreadableBundles.insert(path)
            return nil
        }
        func flag(_ key: String) -> Bool {
            if let number = plist[key] as? NSNumber { return number.boolValue }
            if let text = plist[key] as? String { return ["true", "yes", "1"].contains(text.lowercased()) }
            return false
        }
        let name = (plist["CFBundleDisplayName"] as? String) ?? (plist["CFBundleName"] as? String)
            ?? URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent
        let result = BundleMetadata(path: path, identifier: identifier, name: name,
                                    background: flag("LSUIElement") || flag("LSBackgroundOnly"))
        metadata[path] = result
        return result
    }

    func app(_ pid: Int32) throws -> KernelApp? {
        guard let before = try startStamp(pid), let path = try executable(pid),
              let bundle = try bundle(path), !bundle.background,
              try startStamp(pid) == before else { return nil }
        let identityData = try JSONSerialization.data(withJSONObject: [bundle.path, bundle.identifier, path])
        let identity = "macos:" + String(decoding: identityData, as: UTF8.self)
        return KernelApp(app: NativeApp(identity: identity, pid: pid, generation: before),
                         metadata: bundle, executable: path)
    }

    func verify(_ expected: NativeApp) throws {
        guard try app(expected.pid)?.app == expected else {
            throw failure("app_changed", "Process birth stamp, executable or bundle identity changed")
        }
    }

    func catalog() throws -> [String: Any] {
        try deadline.check()
        let count = proc_listallpids(nil, 0)
        guard count > 0, count < 1_000_000 else {
            throw failure("catalog_unavailable", "Kernel process table could not be enumerated")
        }
        var pids = [Int32](repeating: 0, count: Int(count) + 64)
        let written = pids.withUnsafeMutableBytes { proc_listallpids($0.baseAddress, Int32($0.count)) }
        guard written > 0, written < pids.count else {
            throw failure("catalog_unavailable", "Kernel process table changed beyond enumeration capacity")
        }
        // Public CG metadata is used only to classify nonstandard bundle roots;
        // catalog never enters AX, requests permission or captures pixels.
        let windows = try cgWindows(deadline)
        let windowed = Set(windows.map(\.pid))
        let roots = ["/Applications/", "/System/Applications/", "/Users/Shared/Applications/",
                     "/opt/homebrew-cask/Caskroom/", NSHomeDirectory() + "/Applications/"]
        var apps: [KernelApp] = []
        for pid in Set(pids.prefix(Int(written))).sorted() where pid > 0 {
            try deadline.check()
            guard let candidate = try app(pid) else { continue }
            let knownRoot = roots.contains { candidate.metadata.path.hasPrefix($0) }
            if knownRoot || candidate.metadata.identifier == "com.apple.finder" || windowed.contains(pid) {
                // Keep every proven process instance. Do not select a lowest PID
                // or infer an app parent when more than one process qualifies.
                apps.append(candidate)
            }
        }
        try deadline.check()
        apps.sort { ($0.metadata.name, $0.app.pid) < ($1.metadata.name, $1.app.pid) }
        return ["apps": apps.map(\.json), "coverage": [
            "scope": "kernel_running_user_app_bundles", "complete": false,
            "excludes": ["unreadable_processes", "background_bundles", "non_bundle_executables"],
            "identity": "canonical_bundle_identifier_executable_and_kernel_birth_stamp"
        ]]
    }
}

private struct CGRecord {
    let id: CGWindowID
    let pid: Int32
    let bounds: CGRect
    let onscreen: Bool

    var coordinates: [Double] { [bounds.minX, bounds.minY, bounds.width, bounds.height] }
}

// Fixed counters contain only facts about the requested PID. No titles, raw AX
// values or other applications enter the failure message. Error samples are capped.
private struct WindowInventoryDiagnostics {
    var counts = Dictionary(uniqueKeysWithValues: [
        "cg_app_raw", "window_server_count", "cg_non_layer_zero", "cg_invalid_metadata",
        "ax_window_count", "excluded_scope", "ax_pid_unproven", "ax_role_unproven",
        "ax_subrole_unproven", "exact_id_unproven", "exact_id_zero", "cg_owner_unmatched",
        "duplicate_id", "supported_count", "excluded_non_window_role", "excluded_non_standard_subrole",
        "excluded_open_save_panel", "excluded_non_layer_zero"
    ].map { ($0, 0) })
    var axErrors: [Int32] = []

    var coverage: [String: Any] {
        ["scope": "same_pid_layer_zero_standard_ax_windows", "complete": false,
         "window_server_count": counts["window_server_count"]!, "supported_count": counts["supported_count"]!,
         "diagnostics": counts, "ax_id_errors": axErrors,
         "excludes": ["helpers", "sheets", "dialogs", "menus", "unproven_ax_windows"],
         "identity": "retained_ax_window_equal_and_exact_cg_owner"]
    }

    mutating func increment(_ key: String) { counts[key]! += 1 }
    mutating func exactIDFailure(_ result: AXError) {
        increment(result == .success ? "exact_id_zero" : "exact_id_unproven")
        if result != .success && axErrors.count < 8 { axErrors.append(result.rawValue) }
    }
    func requireProven() throws {
        let unresolved = ["cg_invalid_metadata", "ax_pid_unproven", "ax_role_unproven",
                          "ax_subrole_unproven", "exact_id_unproven", "exact_id_zero",
                          "cg_owner_unmatched", "duplicate_id"].contains { counts[$0]! > 0 }
        let missingAX = counts["ax_window_count"] == 0 && counts["window_server_count"]! > 0
        guard !unresolved && !missingAX else {
            let fields = counts.keys.sorted().map { "\($0)=\(counts[$0]!)" }.joined(separator: ",")
            throw failure("window_inventory_unproven",
                          "Window inventory unproven: \(fields); ax_id_errors=\(axErrors)",
                          details: ["inventory": coverage])
        }
    }
}

private func cgWindowRows(_ deadline: Deadline) throws -> [[String: Any]] {
    try deadline.check()
    guard let rows = CGWindowListCopyWindowInfo([.excludeDesktopElements], 0) as? [[String: Any]] else {
        throw failure("windows_unavailable", "WindowServer metadata unavailable")
    }
    try deadline.check()
    return rows
}

private func cgRecord(_ row: [String: Any]) -> CGRecord? {
    guard let id = row[kCGWindowNumber as String] as? UInt32, id != 0,
          let pid = row[kCGWindowOwnerPID as String] as? Int32, pid > 0,
          (row[kCGWindowLayer as String] as? Int) == 0,
          let dictionary = row[kCGWindowBounds as String] as? NSDictionary,
          let bounds = CGRect(dictionaryRepresentation: dictionary),
          bounds.minX.isFinite, bounds.minY.isFinite, bounds.width.isFinite, bounds.height.isFinite,
          bounds.width > 0, bounds.height > 0 else { return nil }
    return CGRecord(id: id, pid: pid, bounds: bounds,
                    onscreen: (row[kCGWindowIsOnscreen as String] as? Bool) ?? false)
}

private func cgWindows(_ deadline: Deadline) throws -> [CGRecord] {
    try cgWindowRows(deadline).compactMap(cgRecord)
}

// Keep explicitly excluded layer IDs so a mapped AX window on another layer is
// excluded, rather than being mislabeled as an unproven owner. Extra CG surfaces
// alone do not establish a missing AX window (one AX window may have several).
private func appCGInventory(_ rows: [[String: Any]], pid: Int32,
                            diagnostics: inout WindowInventoryDiagnostics) -> ([CGRecord], Set<CGWindowID>) {
    var records: [CGRecord] = []
    var excluded: Set<CGWindowID> = []
    for row in rows where row[kCGWindowOwnerPID as String] as? Int32 == pid {
        diagnostics.increment("cg_app_raw")
        guard let layer = row[kCGWindowLayer as String] as? Int else {
            diagnostics.increment("cg_invalid_metadata"); continue
        }
        if layer != 0 {
            diagnostics.increment("cg_non_layer_zero")
            if let id = row[kCGWindowNumber as String] as? UInt32, id > 0 { excluded.insert(id) }
        } else if let record = cgRecord(row) {
            records.append(record)
            diagnostics.increment("window_server_count")
        } else { diagnostics.increment("cg_invalid_metadata") }
    }
    return (records, excluded)
}

private typealias AXGetWindow = @convention(c) (AXUIElement, UnsafeMutablePointer<CGWindowID>) -> AXError
private final class ExactWindowSymbol: @unchecked Sendable {
    // Keep the framework loaded for the lifetime of the function pointer.
    private let handle: UnsafeMutableRawPointer?
    let function: AXGetWindow?

    init() {
        handle = dlopen("/System/Library/Frameworks/ApplicationServices.framework/Frameworks/HIServices.framework/HIServices", RTLD_LAZY)
        function = handle.flatMap { dlsym($0, "_AXUIElementGetWindow") }
            .map { unsafeBitCast($0, to: AXGetWindow.self) }
    }
}

private let exactWindowSymbol = ExactWindowSymbol()

private func axAttribute(_ element: AXUIElement, _ name: String, _ deadline: Deadline) throws -> CFTypeRef? {
    try deadline.prepare(element)
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    try deadline.check()
    if result == .attributeUnsupported || result == .noValue { return nil }
    guard result == .success else {
        throw failure("ax_unavailable", "AX attribute \(name) failed: \(result.rawValue)")
    }
    return value
}

private func axElement(_ value: CFTypeRef?) -> AXUIElement? {
    guard let value, CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
    return (value as! AXUIElement)
}

private func axPID(_ element: AXUIElement, _ deadline: Deadline) throws -> Int32 {
    try deadline.prepare(element)
    var pid: pid_t = 0
    let result = AXUIElementGetPid(element, &pid)
    try deadline.check()
    guard result == .success, pid > 0 else { throw failure("ax_unavailable", "AX process identity unavailable") }
    return pid
}

private func axWindowIdentity(_ element: AXUIElement, _ deadline: Deadline) throws -> (AXError, CGWindowID) {
    guard let function = exactWindowSymbol.function else {
        throw failure("unsupported_platform", "Exact AX window identity SPI is unavailable")
    }
    try deadline.prepare(element)
    var id: CGWindowID = 0
    let result = function(element, &id)
    try deadline.check()
    return (result, id)
}

private func axWindowID(_ element: AXUIElement, _ deadline: Deadline) throws -> CGWindowID {
    let (result, id) = try axWindowIdentity(element, deadline)
    guard result == .success, id > 0 else {
        throw failure("window_unproven", "AX exact window identity unresolved: AXError=\(result.rawValue), id=\(id)")
    }
    return id
}

private func axList(_ element: AXUIElement, _ attribute: String, limit: Int,
                    _ deadline: Deadline, required: Bool = false) throws -> [AXUIElement] {
    try deadline.prepare(element)
    var value: CFTypeRef?
    // Keep the donor's single-value array read. The indexed-values API can
    // reject index zero for an empty array; an illegal argument is not proof
    // that the node has no children. Only the copied prefix is retained here.
    let result = AXUIElementCopyAttributeValue(element, attribute as CFString, &value)
    try deadline.check()
    if result == .attributeUnsupported || result == .noValue {
        if required { throw failure("modal_state_unproven", "Required AX inventory \(attribute) is unavailable") }
        return []
    }
    guard result == .success, let value, CFGetTypeID(value) == CFArrayGetTypeID() else {
        throw failure("ax_unavailable", "AX array \(attribute) failed: \(result.rawValue)")
    }
    let array = value as! CFArray
    var elements: [AXUIElement] = []
    for index in 0..<min(limit, CFArrayGetCount(array)) {
        guard let pointer = CFArrayGetValueAtIndex(array, index) else {
            throw failure("ax_unavailable", "AX array contained a null element")
        }
        let item = Unmanaged<AnyObject>.fromOpaque(pointer).takeUnretainedValue()
        guard CFGetTypeID(item) == AXUIElementGetTypeID() else {
            throw failure("ax_unavailable", "AX array contained a non-element")
        }
        elements.append(item as! AXUIElement)
    }
    // Swift CF bridging keeps strong AX references, equivalent to the donor's
    // copy_ax_windows CFRetain loop and AxSnapshot/RetainedElement ownership.
    return elements
}

private func actions(_ element: AXUIElement, _ deadline: Deadline) throws -> [String] {
    try deadline.prepare(element)
    var names: CFArray?
    let result = AXUIElementCopyActionNames(element, &names)
    try deadline.check()
    if result == .actionUnsupported || result == .noValue { return [] }
    guard result == .success else { throw failure("ax_unavailable", "AX action names failed: \(result.rawValue)") }
    return names as? [String] ?? []
}

private enum WindowClassification: String {
    case supported, excluded = "excluded_scope"
    case pidUnproven = "ax_pid_unproven", roleUnproven = "ax_role_unproven", subroleUnproven = "ax_subrole_unproven"
}

// Closures preserve the existing short-circuit AX reads and allow pure negative
// controls. Missing/malformed metadata differs from an explicitly unsupported role.
private func classifyWindow(actualPID: Int32, expectedPID: Int32,
                            role: () throws -> String?, subrole: () throws -> String?,
                            identifier: () throws -> String?,
                            excluded: (String) -> Void = { _ in }) throws -> WindowClassification {
    guard actualPID == expectedPID else { return .pidUnproven }
    guard let role = try role(), !role.isEmpty else { return .roleUnproven }
    guard role == kAXWindowRole else { excluded("excluded_non_window_role"); return .excluded }
    guard let subrole = try subrole(), !subrole.isEmpty else { return .subroleUnproven }
    guard subrole == kAXStandardWindowSubrole else { excluded("excluded_non_standard_subrole"); return .excluded }
    let identifier = try identifier()
    if identifier == "open-panel" || identifier == "save-panel" {
        excluded("excluded_open_save_panel"); return .excluded
    }
    return .supported
}

private func windowClassification(_ element: AXUIElement, pid: Int32, deadline: Deadline,
                                  excluded: (String) -> Void = { _ in }) throws -> WindowClassification {
    try classifyWindow(actualPID: axPID(element, deadline), expectedPID: pid,
                       role: { try axAttribute(element, kAXRoleAttribute, deadline) as? String },
                       subrole: { try axAttribute(element, kAXSubroleAttribute, deadline) as? String },
                       identifier: { try axAttribute(element, kAXIdentifierAttribute, deadline) as? String }, excluded: excluded)
}

private func ordinaryWindow(_ element: AXUIElement, pid: Int32, deadline: Deadline) throws -> Bool {
    try windowClassification(element, pid: pid, deadline: deadline) == .supported
}

private func requireNonModalWindow(_ element: AXUIElement, _ deadline: Deadline) throws {
    // AXModal is required for every AXWindow by AXAttributeConstants.h. Missing
    // or malformed state is not evidence that an underlying control is usable.
    guard let value = try axAttribute(element, kAXModalAttribute, deadline),
          CFGetTypeID(value) == CFBooleanGetTypeID(), let modal = value as? Bool else {
        throw failure("modal_state_unproven", "A window does not expose a Boolean AXModal state")
    }
    if modal { throw failure("unsupported_modal", "A modal window blocks this application scope") }
}

private func rejectModalSurface(_ element: AXUIElement, _ context: RequestContext) throws {
    try context.admitModalVisit()
    let deadline = context.deadline
    guard let role = try axAttribute(element, kAXRoleAttribute, deadline) as? String else {
        throw failure("modal_state_unproven", "An AX inventory element has no readable role")
    }
    if role == kAXSheetRole {
        throw failure("unsupported_modal", "A sheet is present; sheet/modal routing is not supported")
    }
    if role == kAXWindowRole { try requireNonModalWindow(element, deadline) }
}

private func requireAppWithoutModal(_ application: AXUIElement, windows: [AXUIElement],
                                    context: RequestContext) throws {
    // AXWindows alone need not contain a top-level consent sheet (the donor's
    // window_scope explicitly inspects application children for that case).
    let children = try axList(application, kAXChildrenAttribute, limit: 129, context.deadline, required: true)
    guard children.count <= 128 else {
        throw failure("modal_state_unproven", "Application AX child inventory exceeds the supported limit")
    }
    var seen: [AXUIElement] = []
    for element in windows + children {
        guard !seen.contains(where: { CFEqual($0, element) }) else { continue }
        seen.append(element)
        try rejectModalSurface(element, context)
    }
}

private func requireWindowWithoutSheet(_ element: AXUIElement, _ context: RequestContext) throws {
    try context.admitModalVisit()
    let deadline = context.deadline
    try requireNonModalWindow(element, deadline)
    // Some providers expose AXSheets, but it is not a public SDK constant.
    // Its absence is inconclusive: the standard AXChildren check is mandatory.
    if let sheets = try axAttribute(element, "AXSheets", deadline) {
        guard CFGetTypeID(sheets) == CFArrayGetTypeID() else {
            throw failure("modal_state_unproven", "AXSheets is not an array")
        }
        if CFArrayGetCount((sheets as! CFArray)) > 0 {
            throw failure("unsupported_modal", "The target window has an attached sheet")
        }
    }
    let children = try axList(element, kAXChildrenAttribute, limit: 129, deadline, required: true)
    guard children.count <= 128 else {
        throw failure("modal_state_unproven", "Window AX child inventory exceeds the supported limit")
    }
    for child in children { try rejectModalSurface(child, context) }
}

private struct WindowSlot: Hashable {
    let app: NativeApp
    let id: CGWindowID
}

private struct RegisteredWindow {
    let target: NativeTarget
    let element: AXUIElement
}

private struct CandidateWindow {
    let cg: CGRecord
    let element: AXUIElement
    let title: String
}

// Roles have a small platform-independent vocabulary. The legacy display label
// retains native text where no normalized role is available.
private func semanticRole(_ role: String) -> String {
    switch role {
    case kAXButtonRole: return "button"
    case kAXCheckBoxRole: return "checkbox"
    case kAXRadioButtonRole: return "radio_button"
    case kAXTextFieldRole: return "text_field"
    case kAXTextAreaRole: return "text_area"
    case kAXStaticTextRole: return "static_text"
    case kAXWindowRole: return "window"
    case kAXGroupRole: return "group"
    case kAXScrollAreaRole: return "scroll_area"
    case kAXImageRole: return "image"
    case kAXToolbarRole: return "toolbar"
    default: return "unknown"
    }
}

private struct NativeElement {
    let id: UInt64
    let element: AXUIElement
    let label: String
    let role: String
    let name: String?
    let value: String?
    let enabled: Bool?
    let canPress: Bool

    var json: [String: Any] {
        ["id": id, "label": label, "role": role,
         "name": name as Any? ?? NSNull(), "value": value as Any? ?? NSNull(),
         "enabled": enabled as Any? ?? NSNull(), "actions": canPress ? ["press"] : [], "can_press": canPress]
    }
}

private struct NativeObservation {
    let target: NativeTarget
    let elements: [NativeElement]
}

private final class NativeStore: @unchecked Sendable {
    let lock = NSLock()
    private var windows: [WindowSlot: RegisteredWindow] = [:]
    private var observations: [String: NativeObservation] = [:]
    // Avoid deterministic generation-one reuse after worker restart. Runtime's
    // separate provider stamp is the authoritative cross-provider ref boundary.
    private var generation = UInt64.random(in: 1...UInt64(UInt32.max)) << 32

    private func nextGeneration() throws -> UInt64 {
        let (next, overflow) = generation.addingReportingOverflow(1)
        guard !overflow else { throw failure("registry_exhausted", "Window generations exhausted") }
        generation = next
        return next
    }

    private func candidates(_ app: NativeApp, _ context: RequestContext) throws -> ([CandidateWindow], WindowInventoryDiagnostics) {
        try context.verify(app)
        guard AXIsProcessTrusted() else {
            throw failure("permission_denied", "Accessibility permission is required; no prompt was requested")
        }
        guard exactWindowSymbol.function != nil else {
            throw failure("unsupported_platform", "Exact AX window identity SPI is unavailable")
        }
        var diagnostics = WindowInventoryDiagnostics()
        let (cg, excludedCG) = appCGInventory(try cgWindowRows(context.deadline), pid: app.pid,
                                            diagnostics: &diagnostics)
        try context.deadline.check()
        let application = AXUIElementCreateApplication(app.pid)
        let elements = try axList(application, kAXWindowsAttribute, limit: 129, context.deadline, required: true)
        guard elements.count <= 128 else { throw failure("window_limit", "More than 128 AX windows; narrow scope required") }
        try requireAppWithoutModal(application, windows: elements, context: context)
        diagnostics.counts["ax_window_count"] = elements.count
        var candidates: [CandidateWindow] = []
        var seen: Set<CGWindowID> = []
        for element in elements {
            let classification = try windowClassification(element, pid: app.pid, deadline: context.deadline,
                                                        excluded: { diagnostics.increment($0) })
            guard classification == .supported else {
                diagnostics.increment(classification.rawValue)
                continue
            }
            try requireWindowWithoutSheet(element, context)
            let (result, id) = try axWindowIdentity(element, context.deadline)
            guard result == .success, id > 0 else {
                diagnostics.exactIDFailure(result)
                continue
            }
            if excludedCG.contains(id) {
                diagnostics.increment("excluded_non_layer_zero")
                diagnostics.increment("excluded_scope")
                continue
            }
            guard let record = cg.first(where: { $0.id == id }) else {
                diagnostics.increment("cg_owner_unmatched")
                continue
            }
            guard seen.insert(id).inserted else {
                diagnostics.increment("duplicate_id")
                continue
            }
            diagnostics.increment("supported_count")
            let title = try axAttribute(element, kAXTitleAttribute, context.deadline) as? String ?? ""
            candidates.append(CandidateWindow(cg: record, element: element, title: String(title.prefix(512))))
        }
        try context.verify(app)
        try diagnostics.requireProven()
        return (candidates, diagnostics)
    }

    private func listWindows(_ app: NativeApp, _ context: RequestContext) throws -> [String: Any] {
        let (candidates, diagnostics) = try candidates(app, context)
        let active = Set(candidates.map { WindowSlot(app: app, id: $0.cg.id) })
        windows = windows.filter {
            $0.key.app.pid != app.pid || ($0.key.app == app && active.contains($0.key))
        }
        var rows: [[String: Any]] = []
        for candidate in candidates {
            let slot = WindowSlot(app: app, id: candidate.cg.id)
            let target: NativeTarget
            if let existing = windows[slot], CFEqual(existing.element, candidate.element) {
                target = existing.target
            } else {
                guard windows.count < 512 || windows[slot] != nil else {
                    throw failure("registry_exhausted", "Window registry limit reached; start a new worker")
                }
                target = NativeTarget(app: app, nativeID: candidate.cg.id, generation: try nextGeneration())
            }
            windows[slot] = RegisteredWindow(target: target, element: candidate.element)
            rows.append(["target": target.json, "title": candidate.title,
                         "bounds": candidate.cg.coordinates, "onscreen": candidate.cg.onscreen])
        }
        return ["windows": rows, "coverage": diagnostics.coverage]
    }

    fileprivate func validate(_ target: NativeTarget, _ context: RequestContext) throws -> CandidateWindow {
        let slot = WindowSlot(app: target.app, id: target.nativeID)
        guard let registered = windows[slot], registered.target == target else {
            throw failure("stale_window", "Target was not registered in this worker generation")
        }
        try context.verify(target.app)
        guard AXIsProcessTrusted() else { throw failure("permission_denied", "Accessibility permission is required") }
        let cg = try cgWindows(context.deadline)
        guard let record = cg.first(where: { $0.id == target.nativeID && $0.pid == target.app.pid }) else {
            windows.removeValue(forKey: slot)
            throw failure("target_changed", "WindowServer no longer proves the requested window owner")
        }
        let application = AXUIElementCreateApplication(target.app.pid)
        let current = try axList(application, kAXWindowsAttribute, limit: 129, context.deadline, required: true)
        guard current.count <= 128 else { throw failure("window_limit", "AX window inventory exceeds the supported limit") }
        try requireAppWithoutModal(application, windows: current, context: context)
        var matched: [AXUIElement] = []
        for element in current {
            do {
                if try axWindowID(element, context.deadline) == target.nativeID { matched.append(element) }
            } catch let error as NativeFailure where error.code == "window_unproven" { continue }
        }
        guard matched.count == 1, let element = matched.first,
              CFEqual(element, registered.element),
              try ordinaryWindow(element, pid: target.app.pid, deadline: context.deadline) else {
            windows.removeValue(forKey: slot)
            throw failure("target_changed", "Current AX window identity or WindowServer ownership differs")
        }
        try requireWindowWithoutSheet(element, context)
        try context.verify(target.app)
        try context.deadline.check()
        return CandidateWindow(cg: record, element: element, title: "")
    }

    private func owned(_ element: AXUIElement, by window: AXUIElement,
                       target: NativeTarget, deadline: Deadline) throws -> Bool {
        guard try axPID(element, deadline) == target.app.pid else { return false }
        if let attributed = axElement(try axAttribute(element, kAXWindowAttribute, deadline)) {
            guard try axPID(attributed, deadline) == target.app.pid && CFEqual(attributed, window)
                && axWindowID(attributed, deadline) == target.nativeID else { return false }
        }
        // Keep the donor's bounded AXParent check, but require it even with an
        // AXWindow attribute: a control reparented into a sheet/menu is outside
        // this slice. A different explicit AXWindow is never replaced by a guess.
        var cursor = element
        for _ in 0..<40 {
            guard try axPID(cursor, deadline) == target.app.pid else { return false }
            let role = try axAttribute(cursor, kAXRoleAttribute, deadline) as? String
            if role == kAXWindowRole {
                return try CFEqual(cursor, window) && axWindowID(cursor, deadline) == target.nativeID
            }
            if role == kAXApplicationRole || role == kAXSheetRole || role == kAXMenuRole || role == kAXMenuBarRole { return false }
            guard let parent = axElement(try axAttribute(cursor, kAXParentAttribute, deadline)) else { return false }
            cursor = parent
        }
        return false
    }

    private let pressRoles: Set<String> = [kAXButtonRole, kAXCheckBoxRole, kAXRadioButtonRole]

    private func pressable(_ element: AXUIElement, role: String, enabled: Bool, window: AXUIElement,
                           target: NativeTarget, deadline: Deadline) throws -> Bool {
        guard pressRoles.contains(role), enabled,
              try actions(element, deadline).contains(kAXPressAction) else { return false }
        return try owned(element, by: window, target: target, deadline: deadline)
    }

    private func observe(_ target: NativeTarget, _ context: RequestContext) throws -> [String: Any] {
        observations.removeAll()
        let window = try validate(target, context)
        var pending: [(AXUIElement, Int)] = [(window.element, 0)]
        var elements: [NativeElement] = []
        var visited: [AXUIElement] = []
        var truncated = false
        while let (element, depth) = pending.popLast() {
            try context.deadline.check()
            if elements.count >= 128 { truncated = true; break }
            guard !visited.contains(where: { CFEqual($0, element) }) else { continue }
            visited.append(element)
            guard try axPID(element, context.deadline) == target.app.pid else { continue }
            let role = try axAttribute(element, kAXRoleAttribute, context.deadline) as? String ?? ""
            if [kAXSheetRole, kAXMenuRole, kAXMenuBarRole].contains(role) { continue }
            func text(_ attribute: String) throws -> String? {
                let value = try axAttribute(element, attribute, context.deadline)
                guard let text = (value as? String) ?? (value as? NSNumber)?.stringValue else { return nil }
                return String(text.prefix(256))
            }
            let title = try text(kAXTitleAttribute)
            let value = try text(kAXValueAttribute)
            let description = try text(kAXDescriptionAttribute)
            let labels = [title, value, description].compactMap { $0 }.filter { !$0.isEmpty }
            let rawEnabled = try axAttribute(element, kAXEnabledAttribute, context.deadline)
            let enabled: Bool? = rawEnabled.flatMap {
                CFGetTypeID($0) == CFBooleanGetTypeID() ? ($0 as? Bool) : nil
            }
            let canPress = try pressable(element, role: role, enabled: enabled == true, window: window.element,
                                         target: target, deadline: context.deadline)
            elements.append(NativeElement(id: UInt64(elements.count + 1), element: element,
                                          label: labels.isEmpty ? role : labels.joined(separator: " | "),
                                          role: semanticRole(role), name: title?.isEmpty == false ? title : description,
                                          value: value, enabled: enabled, canPress: canPress))
            if depth >= 16 { truncated = true; continue }
            let remaining = max(0, 128 - elements.count - pending.count)
            let children = try axList(element, kAXChildrenAttribute, limit: remaining + 1, context.deadline)
            if children.count > remaining { truncated = true }
            pending.append(contentsOf: children.prefix(remaining).reversed().map { ($0, depth + 1) })
        }
        // Both the app and the retained window must still agree at publication.
        let final = try validate(target, context)
        guard CFEqual(final.element, window.element) else { throw failure("target_changed", "Window changed while observing") }
        try context.deadline.check()
        let binding = UUID().uuidString
        observations[binding] = NativeObservation(target: target, elements: elements)
        return ["target": target.json, "binding": binding, "elements": elements.map(\.json), "truncated": truncated]
    }

    private func press(_ request: [String: Any]) -> [String: Any] {
        do {
            guard let binding = request["binding"] as? String,
                  let observation = observations.removeValue(forKey: binding) else {
                throw failure("stale_observation", "Native observation was retired or consumed")
            }
            let context = try RequestContext(request)
            let target = try NativeTarget(request["target"])
            let id = try integer(request["element"], "element")
            guard observation.target == target else { throw failure("target_changed", "Binding belongs to another target") }
            guard let element = observation.elements.first(where: { $0.id == id }), element.canPress else {
                throw failure("element_not_pressable", "Element is absent or outside the supported press subset")
            }
            let window = try validate(target, context)
            let role = try axAttribute(element.element, kAXRoleAttribute, context.deadline) as? String ?? ""
            let enabled = try axAttribute(element.element, kAXEnabledAttribute, context.deadline) as? Bool == true
            guard try pressable(element.element, role: role, enabled: enabled, window: window.element,
                                target: target, deadline: context.deadline) else {
                throw failure("element_not_pressable", "Current role, enabled state, ancestry or AXPress support changed")
            }
            let finalWindow = try validate(target, context)
            guard try owned(element.element, by: finalWindow.element, target: target, deadline: context.deadline) else {
                throw failure("target_changed", "Element ancestry changed before dispatch")
            }
            try context.deadline.prepare(element.element)
            // From this point onward, AX may already have delivered a side
            // effect. No error, timeout, retry or alternative route is labelled
            // not_dispatched, including AX errors whose effect is unknowable.
            guard let result = dtwWithInputAdmission({
                AXUIElementPerformAction(element.element, kAXPressAction as CFString)
            }) else { throw failure("closed", "Native input admission is closed") }
            do { try context.deadline.check() }
            catch {
                return ["dispatch": "unknown", "evidence": "unverified",
                        "error": "deadline_exceeded", "message": "AXPress returned after the request deadline"]
            }
            guard result == .success else {
                return ["dispatch": "unknown", "evidence": "unverified", "error": "ax_action_failed",
                        "message": "AXPress returned \(result.rawValue); inspect before another action"]
            }
            return ["dispatch": "dispatched", "evidence": "unverified"]
        } catch let error as NativeFailure {
            return error.json.merging(["dispatch": "not_dispatched", "evidence": "unverified"]) { _, new in new }
        } catch {
            return ["dispatch": "not_dispatched", "evidence": "unverified",
                    "error": "native_error", "message": String(describing: error)]
        }
    }

    func call(_ request: [String: Any]) -> [String: Any] {
        let operation = request["operation"] as? String ?? ""
        do {
            // Cleanup has no input side effect and must work after a deadline.
            if operation == "release" {
                guard let binding = request["binding"] as? String else {
                    throw failure("invalid_request", "Missing native binding")
                }
                return ["released": observations.removeValue(forKey: binding) != nil]
            }
            if operation == "press" { return press(request) }
            let context = try RequestContext(request)
            switch operation {
            case "catalog": return try context.catalog()
            case "windows": return try listWindows(NativeApp(request["app"]), context)
            case "observe": return try observe(NativeTarget(request["target"]), context)
            case "validate":
                let target = try NativeTarget(request["target"])
                let current = try validate(target, context)
                return ["target": target.json, "bounds": current.cg.coordinates]
            default: throw failure("invalid_request", "Unknown native operation")
            }
        } catch let error as NativeFailure {
            if operation == "press" {
                return error.json.merging(["dispatch": "not_dispatched", "evidence": "unverified"]) { _, new in new }
            }
            return error.json
        } catch { return failure("native_error", String(describing: error)).json }
    }
}

private let nativeStore = NativeStore()

// Scene.swift calls this immediately before its own admitted pixel dispatch.
// Every pixel operation requires the same typed target as the semantic provider.
func dtwValidatePixelTarget(_ request: [String: Any]) -> [String: Any]? {
    nativeStore.lock.lock()
    defer { nativeStore.lock.unlock() }
    do {
        let context = try RequestContext(request)
        _ = try nativeStore.validate(NativeTarget(request["target"]), context)
        return nil
    } catch let error as NativeFailure { return error.json }
    catch { return failure("native_error", String(describing: error)).json }
}

@_cdecl("dtw_native_call")
public func dtwNativeCall(_ input: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    autoreleasepool {
        let result: [String: Any]
        if let input, let data = String(cString: input).data(using: .utf8),
           data.count <= 1_048_576,
           let request = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] {
            nativeStore.lock.lock()
            result = nativeStore.call(request)
            nativeStore.lock.unlock()
        } else { result = failure("invalid_request", "Native request must be a JSON object under 1 MiB").json }
        guard let data = try? JSONSerialization.data(withJSONObject: result, options: [.sortedKeys]) else {
            return strdup("{\"error\":\"native_error\",\"message\":\"Native response serialization failed\"}")
        }
        return strdup(String(decoding: data, as: UTF8.self))
    }
}

@_cdecl("dtw_native_free")
public func dtwNativeFree(_ pointer: UnsafeMutablePointer<CChar>?) { free(pointer) }
