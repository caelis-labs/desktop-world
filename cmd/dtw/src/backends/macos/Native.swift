// Narrow source port from Cua Driver 0.38.0, commit
// 85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19. Copyright (c) 2025 Cua AI, Inc.
// MIT; full source mapping and retained license: docs/native-source-provenance.md.
// The worker owns this registry. No CUA ToolRegistry, focus or input fallback.
import Foundation
import ApplicationServices
import AppKit
import Darwin

@_silgen_name("dtw_set_front_process")
private func dtwSetFrontProcess(_ pid: Int32, _ window: UInt32) -> Bool
@_silgen_name("dtw_front_process_matches")
private func dtwFrontProcessMatches(_ pid: Int32) -> Int32

private struct NativeFailure: Error {
    let code: String
    let message: String
    let details: [String: Any]?
    let attributeFailure: AXAttributeFailure?
    let focusAbsence: FocusAbsence?
    // Private evidence only: a fresh complete parent inventory no longer
    // contains a non-target AX object read from the previous snapshot.
    let modalSnapshotExpired: Bool

    var json: [String: Any] {
        var value: [String: Any] = ["error": code, "message": message]
        if let details { value["details"] = details }
        return value
    }
}

private struct AXAttributeFailure {
    let name: String
    let error: AXError
}

private enum FocusAbsence: Equatable {
    case application, window, frontProcess
}

private func failure(_ code: String, _ message: String, details: [String: Any]? = nil,
                     attributeFailure: AXAttributeFailure? = nil,
                     focusAbsence: FocusAbsence? = nil,
                     modalSnapshotExpired: Bool = false) -> NativeFailure {
    NativeFailure(code: code, message: message, details: details,
                  attributeFailure: attributeFailure, focusAbsence: focusAbsence,
                  modalSnapshotExpired: modalSnapshotExpired)
}

private func missingFocus(_ reason: FocusAbsence) -> NativeFailure {
    let subject: String
    switch reason {
    case .application: subject = "System-wide focused application"
    case .window: subject = "The target application's focused window"
    case .frontProcess: subject = "WindowServer foreground identity"
    }
    return failure("focus_unproven", "\(subject) is unavailable; refresh before further input", focusAbsence: reason)
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

    private init(uptime: TimeInterval) { self.uptime = uptime }

    func limited(to value: TimeInterval) -> Deadline {
        Deadline(uptime: min(uptime, value))
    }

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

private func appBundlePath(_ executable: String) -> String? {
    // Innermost bundle preserves a nested helper's own classification.
    guard let marker = executable.range(of: ".app/Contents/MacOS/", options: .backwards) else { return nil }
    return String(executable[..<executable.index(marker.lowerBound, offsetBy: 4)])
}

private func sameExecutableFile(_ left: String, _ right: String) -> Bool {
    var first = stat(), second = stat()
    guard stat(left, &first) == 0, stat(right, &second) == 0,
          first.st_mode & S_IFMT == S_IFREG, second.st_mode & S_IFMT == S_IFREG else { return false }
    return first.st_dev == second.st_dev && first.st_ino == second.st_ino
}

// proc_pidpath can expose another hard link to the same executable vnode after
// a read of macOS's code-sign clone. LS supplies only a candidate for this PID:
// accept it only when its innermost bundle agrees and both paths are one file.
// Ordinary bundles/helpers retain the kernel path and never consult AppKit.
private func canonicalAppExecutable(_ kernel: String,
                                    registered: () -> (bundle: String, executable: String)?) -> String {
    guard appBundlePath(kernel) == nil, kernel.contains("/Contents/MacOS/"),
          let candidate = registered(), appBundlePath(candidate.executable) == candidate.bundle,
          sameExecutableFile(kernel, candidate.executable) else { return kernel }
    return candidate.executable
}

// Per-request metadata cache only. Kernel PID enumeration and birth stamps are
// authoritative; registered URLs only normalize a proven same-file alias.
private final class RequestContext {
    let deadline: Deadline
    private var metadata: [String: BundleMetadata] = [:]
    private var unreadableBundles: Set<String> = []
    private var modalVisits = 0
    private let tracedPIDs: Set<Int32>

    convenience init(_ request: [String: Any]) throws {
        self.init(deadline: try Deadline(request))
    }

    init(deadline: Deadline) {
        self.deadline = deadline
        tracedPIDs = Set((ProcessInfo.processInfo.environment["DTW_CATALOG_TRACE_PIDS"] ?? "")
            .split(separator: ",").compactMap { Int32($0.trimmingCharacters(in: .whitespaces)) }
            .filter { $0 > 0 })
    }

    // Opt-in diagnostics for already-known PIDs. Observe only the original
    // reads; never print paths, bundle names, titles or NSError userInfo.
    private func traceCatalog(_ pid: Int32, _ stage: String, _ fields: [String: Any] = [:]) {
        guard tracedPIDs.contains(pid) else { return }
        var value = fields
        value["pid"] = pid
        value["stage"] = stage
        if let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) {
            FileHandle.standardError.write(Data("dtw_catalog ".utf8) + data + Data("\n".utf8))
        }
    }

    private func metadataRead<T>(_ pid: Int32, _ stage: String, _ read: () throws -> T) -> T? {
        do { return try read() }
        catch {
            let error = error as NSError
            traceCatalog(pid, stage, ["error_domain": error.domain, "error_code": error.code])
            return nil
        }
    }

    private func unavailableBundle(_ pid: Int32, _ path: String, _ stage: String,
                                   _ fields: [String: Any] = [:]) -> BundleMetadata? {
        traceCatalog(pid, stage, fields)
        unreadableBundles.insert(path)
        return nil
    }

    func admitModalVisit() throws {
        try deadline.check()
        guard modalVisits < 4_096 else {
            throw failure("modal_state_unproven", "Modal verification exceeded 4096 element visits in this request")
        }
        modalVisits += 1
    }

    func startStamp(_ pid: Int32, stage: String = "stamp") throws -> UInt64? {
        try deadline.check()
        var info = proc_bsdinfo()
        let size = Int32(MemoryLayout<proc_bsdinfo>.size)
        let count = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size)
        let readErrno = errno
        traceCatalog(pid, stage, ["return_value": count, "size": size,
                                 "errno": count == size ? 0 : readErrno,
                                 "pid_matches": info.pbi_pid == UInt32(pid),
                                 "microseconds_valid": info.pbi_start_tvusec < 1_000_000])
        guard count == size,
              info.pbi_pid == UInt32(pid), info.pbi_start_tvusec < 1_000_000 else { return nil }
        let (seconds, overflow) = info.pbi_start_tvsec.multipliedReportingOverflow(by: 1_000_000)
        let (stamp, addedOverflow) = seconds.addingReportingOverflow(info.pbi_start_tvusec)
        traceCatalog(pid, stage + "_generation", ["overflow": overflow || addedOverflow, "nonzero": stamp != 0])
        return overflow || addedOverflow || stamp == 0 ? nil : stamp
    }

    private func executable(_ pid: Int32) throws -> String? {
        try deadline.check()
        var buffer = [UInt8](repeating: 0, count: 4_096)
        let count = buffer.withUnsafeMutableBytes { proc_pidpath(pid, $0.baseAddress, UInt32($0.count)) }
        let readErrno = errno
        traceCatalog(pid, "executable", ["return_value": count, "size": buffer.count,
                                         "errno": count > 0 ? 0 : readErrno])
        guard count > 0, count < buffer.count,
              let zero = buffer.firstIndex(of: 0),
              let path = String(bytes: buffer[..<zero], encoding: .utf8), !path.isEmpty else {
            traceCatalog(pid, "executable_format", ["valid": false])
            return nil
        }
        traceCatalog(pid, "executable_format", ["valid": true])
        let kernel = URL(fileURLWithPath: path).resolvingSymlinksInPath().standardizedFileURL.path
        let canonical = canonicalAppExecutable(kernel) {
            guard let application = NSRunningApplication(processIdentifier: pid),
                  let bundle = application.bundleURL, let executable = application.executableURL else { return nil }
            return (bundle.resolvingSymlinksInPath().standardizedFileURL.path,
                    executable.resolvingSymlinksInPath().standardizedFileURL.path)
        }
        traceCatalog(pid, "executable_alias", ["normalized_same_file": canonical != kernel])
        return canonical
    }

    private func bundle(_ executable: String, pid: Int32) throws -> BundleMetadata? {
        // Use the innermost bundle: Chrome Helper's own LSUIElement flag must
        // classify the helper, not the outer Chrome application's metadata.
        guard let path = appBundlePath(executable) else {
            traceCatalog(pid, "bundle_path", ["valid": false])
            return nil
        }
        if let cached = metadata[path] {
            traceCatalog(pid, "bundle_cache", ["readable": true])
            return cached
        }
        if unreadableBundles.contains(path) {
            traceCatalog(pid, "bundle_cache", ["readable": false])
            return nil
        }
        try deadline.check()
        let plistURL = URL(fileURLWithPath: path).appendingPathComponent("Contents/Info.plist")
        guard let size = metadataRead(pid, "plist_stat", {
            try plistURL.resourceValues(forKeys: [.fileSizeKey])
        })?.fileSize else {
            return unavailableBundle(pid, path, "plist_stat_result", ["size_present": false])
        }
        guard size <= 2 * 1_024 * 1_024 else {
            return unavailableBundle(pid, path, "plist_stat_result", ["size": size, "within_limit": false])
        }
        guard let data = metadataRead(pid, "plist_read", { try Data(contentsOf: plistURL) }) else {
            return unavailableBundle(pid, path, "plist_read_result", ["readable": false])
        }
        guard data.count <= 2 * 1_024 * 1_024 else {
            return unavailableBundle(pid, path, "plist_read_result", ["size": data.count, "within_limit": false])
        }
        guard let plist = metadataRead(pid, "plist_parse", {
            try PropertyListSerialization.propertyList(from: data, format: nil)
        }) as? [String: Any] else {
            return unavailableBundle(pid, path, "plist_parse_result", ["dictionary": false])
        }
        guard let identifier = plist["CFBundleIdentifier"] as? String, !identifier.isEmpty else {
            return unavailableBundle(pid, path, "bundle_identifier", ["valid": false])
        }
        traceCatalog(pid, "bundle_metadata", ["readable": true])
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
        guard let before = try startStamp(pid, stage: "stamp_before"), let path = try executable(pid),
              let bundle = try bundle(path, pid: pid) else { return nil }
        traceCatalog(pid, "bundle_classification", ["background": bundle.background])
        guard !bundle.background else { return nil }
        let sameGeneration = try startStamp(pid, stage: "stamp_after") == before
        traceCatalog(pid, "app_generation", ["matches": sameGeneration])
        guard sameGeneration else { return nil }
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

    func hasExited(_ expected: NativeApp) throws -> Bool {
        if let current = try startStamp(expected.pid) { return current != expected.generation }
        // An unreadable/permission-denied process is not proof of death.
        return kill(expected.pid, 0) == -1 && errno == ESRCH
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
        let enumeratedPIDs = Set(pids.prefix(Int(written)))
        for pid in tracedPIDs.sorted() {
            traceCatalog(pid, "process_table", ["enumerated": enumeratedPIDs.contains(pid),
                                                 "return_value": written, "size": pids.count])
        }
        for pid in enumeratedPIDs.sorted() where pid > 0 {
            try deadline.check()
            guard let candidate = try app(pid) else { continue }
            let knownRoot = roots.contains { candidate.metadata.path.hasPrefix($0) }
            let included = knownRoot || candidate.metadata.identifier == "com.apple.finder" || windowed.contains(pid)
            traceCatalog(pid, "catalog_filter", ["known_root": knownRoot, "windowed": windowed.contains(pid),
                                                 "included": included])
            if included {
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

private func cgWindowRows(_ deadline: Deadline, only id: CGWindowID? = nil) throws -> [[String: Any]] {
    try deadline.check()
    let options: CGWindowListOption = id == nil ? .excludeDesktopElements : .optionIncludingWindow
    guard let rows = CGWindowListCopyWindowInfo(options, id ?? 0) as? [[String: Any]] else {
        throw failure("windows_unavailable", "WindowServer metadata unavailable")
    }
    try deadline.check()
    return rows
}

private func cgRecord(_ row: [String: Any]) -> CGRecord? {
    guard let id = row[kCGWindowNumber as String] as? UInt32, id != 0,
          let pid = row[kCGWindowOwnerPID as String] as? Int32, pid > 0,
          row[kCGWindowLayer as String] is Int,
          let dictionary = row[kCGWindowBounds as String] as? NSDictionary,
          let bounds = CGRect(dictionaryRepresentation: dictionary),
          bounds.minX.isFinite, bounds.minY.isFinite, bounds.width.isFinite, bounds.height.isFinite,
          bounds.size.width > 0, bounds.size.height > 0 else { return nil }
    return CGRecord(id: id, pid: pid, bounds: bounds,
                    onscreen: (row[kCGWindowIsOnscreen as String] as? Bool) ?? false)
}

private func cgWindows(_ deadline: Deadline) throws -> [CGRecord] {
    try cgWindowRows(deadline).compactMap(cgRecord)
}

// Only an empty exact query proves an unavailable backing surface. A returned
// row with missing metadata or another identity must never become an exclusion.
private func exactSurfaceRecord(_ rows: [[String: Any]], expected: CGRecord) throws -> CGRecord? {
    if rows.isEmpty { return nil }
    guard rows.count == 1, let record = cgRecord(rows[0]) else {
        throw failure("window_inventory_unproven", "Exact surface metadata is incomplete or duplicated")
    }
    guard record.id == expected.id, record.pid == expected.pid else {
        throw failure("target_changed", "Window identity changed during exact inventory lookup")
    }
    return record
}

// Pure WindowServer row classification. An AX-free inventory never treats an
// inaccessible semantic tree as evidence that system surfaces are absent.
private func surfaceInventory(_ source: [[String: Any]], pid: Int32,
                              resolve: (CGRecord) throws -> CGRecord? = { $0 }) throws -> ([(CGRecord, String)], [String: Any]) {
        var counts = ["cg_app_raw": 0, "supported_count": 0, "excluded_unavailable_surface": 0,
                      "excluded_zero_area": 0, "invalid_metadata": 0, "duplicate_id": 0]
        var candidates: [(CGRecord, String)] = []
        var seen: Set<CGWindowID> = []
        for row in source where row[kCGWindowOwnerPID as String] as? Int32 == pid {
            counts["cg_app_raw"]! += 1
            guard row[kCGWindowLayer as String] is Int else {
                counts["invalid_metadata"]! += 1; continue
            }
            if let dictionary = row[kCGWindowBounds as String] as? NSDictionary,
               let bounds = CGRect(dictionaryRepresentation: dictionary),
               bounds.minX.isFinite, bounds.minY.isFinite,
               bounds.size.width.isFinite, bounds.size.height.isFinite,
               bounds.size.width >= 0, bounds.size.height >= 0,
               bounds.size.width == 0 || bounds.size.height == 0 {
                counts["excluded_zero_area"]! += 1; continue
            }
            guard let record = cgRecord(row) else { counts["invalid_metadata"]! += 1; continue }
            guard seen.insert(record.id).inserted else { counts["duplicate_id"]! += 1; continue }
            // The all-window list also contains retained backing stores after
            // close/orderOut. Exact lookup is the same proof used by actions;
            // absence is unavailable, not evidence of a different live window.
            guard let live = try resolve(record) else {
                counts["excluded_unavailable_surface"]! += 1; continue
            }
            guard live.id == record.id, live.pid == record.pid else {
                throw failure("target_changed", "Window owner changed during inventory")
            }
            candidates.append((live, String((row[kCGWindowName as String] as? String ?? "").prefix(512))))
            counts["supported_count"]! += 1
        }
        let coverage: [String: Any] = ["scope": "same_pid_application_surfaces", "diagnostics": counts]
        guard counts["invalid_metadata"] == 0, counts["duplicate_id"] == 0 else {
            throw failure("window_inventory_unproven", "System window metadata is incomplete",
                          details: ["inventory": coverage])
        }
        return (candidates, coverage)
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

private func traceSemanticFailure(_ fields: [String: Any]) {
    guard ProcessInfo.processInfo.environment["DTW_AX_READINESS_TRACE"] == "1" else { return }
    // Only native operation/attribute names, normalized roles and error codes.
    // Never include AX values, labels or the request's text.
    if let data = try? JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys]) {
        FileHandle.standardError.write(Data("dtw_semantic_failure ".utf8) + data + Data("\n".utf8))
    }
}

private func axAttribute(_ element: AXUIElement, _ name: String, _ deadline: Deadline,
                         unsupportedIsAbsent: Bool = true) throws -> CFTypeRef? {
    try deadline.prepare(element)
    var value: CFTypeRef?
    let started = ProcessInfo.processInfo.systemUptime
    let result = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    try deadline.check()
    if result == .noValue || (unsupportedIsAbsent && result == .attributeUnsupported) { return nil }
    guard result == .success else {
        let elapsed = Int((ProcessInfo.processInfo.systemUptime - started) * 1_000)
        traceSemanticFailure(["operation":"attribute", "attribute":name, "ax_error":result.rawValue,
                              "elapsed_ms":elapsed])
        throw failure("ax_unavailable", "AX attribute \(name) failed: \(result.rawValue)",
                      details: ["attribute":name, "ax_error":result.rawValue],
                      attributeFailure: AXAttributeFailure(name: name, error: result))
    }
    return value
}

private func optionalDescription(alreadyMatched: Bool, read: () throws -> String?) throws
    -> (text: String?, incompleteMatch: Bool) {
    do { return (try read(), false) }
    catch let native as NativeFailure {
        // TextEdit exposes a valid text area/value but returns AXFailure for
        // AXDescription. Only this optional attribute/error pair is tolerated.
        guard native.code == "ax_unavailable",
              native.attributeFailure?.name == kAXDescriptionAttribute,
              native.attributeFailure?.error == .failure else { throw native }
        return (nil, !alreadyMatched)
    }
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

// Input needs a fresh system-wide focus read, not AppKit's run-loop-driven
// application snapshot. Recheck after the window query, which can itself block.
// Missing focus cannot authorize another input event. Establishment explicitly
// handles that state separately, after validating the requested target.
private func exactInputFocus(expectedPID: Int32, focusedPID: () throws -> Int32?,
                             windowMatches: () throws -> Bool) throws -> Bool {
    func readPID() throws -> Int32 {
        guard let pid = try focusedPID(), pid > 0 else {
            throw missingFocus(.application)
        }
        return pid
    }
    guard try readPID() == expectedPID else { return false }
    guard try windowMatches() else { return false }
    return try readPID() == expectedPID
}

// Keep unavailable native proof distinct from a proven different front app.
private func frontProcessMatch(_ result: Int32) throws -> Bool {
    switch result {
    case 1: return true
    case 0: return false
    case -6 ... -3:
        // Only the existing post-activation read-only poll may recover this
        // transient native absence. Pre-action snapshot checks still throw.
        throw failure("focus_unproven", "WindowServer foreground identity is unavailable",
                      details:["native_result":result], focusAbsence:.frontProcess)
    default:
        throw failure("focus_unproven", "WindowServer foreground identity cannot be queried",
                      details:["native_result":result])
    }
}
private func windowServerFrontmost(_ pid: Int32, _ deadline: Deadline) throws -> Bool {
    try deadline.check()
    let result = dtwFrontProcessMatches(pid)
    try deadline.check()
    if result < 0 { traceSemanticFailure(["operation":"front_process_read", "pid":pid, "native_result":result]) }
    return try frontProcessMatch(result)
}

// Capturing a return target may take several AX reads. The last read must
// still identify that exact window (or a positively absent focused window).
private func validateFocusSnapshot(check: () throws -> Void, read: () throws -> Bool) throws {
    try check()
    let unchanged = try read()
    try check()
    guard unchanged else {
        throw failure("focus_unproven", "Previous foreground changed while its return target was captured")
    }
}

// A caller explicitly requesting focus/input need not already have a focused
// application/window. Only typed absence permits the one requested establishment;
// actual AX, permission and identity failures still stop before any side effect.
private func focusedBeforeEstablishment(_ read: () throws -> Bool) throws -> Bool {
    do { return try read() }
    catch let error as NativeFailure where error.code == "focus_unproven"
        && (error.focusAbsence == .application || error.focusAbsence == .window) {
        return false
    }
}

// Only used after the one accepted raise/activation. These polls are read-only:
// missing focus or a busy AX provider cannot activate again or replay input.
// Preserve the unresolved typed error when the readiness budget is exhausted.
private func waitForEstablishedFocus<T>(remaining: () -> TimeInterval,
                                        check: () throws -> Void,
                                        read: () throws -> T?,
                                        wait: (TimeInterval) -> Void) throws -> T {
    var firstTransient: NativeFailure?
    while remaining() > 0 {
        try check()
        if remaining() <= 0 { break }
        var ready: T?
        do {
            ready = try read()
            firstTransient = nil // A successful read proves AX has recovered, even if focus is still false.
        } catch let error as NativeFailure {
            let missing = error.code == "focus_unproven" && error.focusAbsence != nil
            let busyAX = error.code == "ax_unavailable" && error.attributeFailure?.error == .cannotComplete
            let expiredInventory = error.modalSnapshotExpired
                && error.code == "ax_unavailable" && error.attributeFailure?.error == .invalidUIElement
            guard missing || busyAX || expiredInventory else {
                throw error
            }
            // An available AX read with missing focus has recovered from a prior
            // AX failure. Likewise, an available app with no focused window must
            // not retain the obsolete "focused application unavailable" cause.
            if firstTransient == nil || firstTransient?.focusAbsence != error.focusAbsence {
                firstTransient = error
            }
        }
        // A read may block; neither successful stale proof nor a transient error
        // may hide cancellation, an expired request, or a replaced application.
        try check()
        let available = remaining()
        if available <= 0 { break }
        if let ready { return ready }
        wait(min(0.02, available))
    }
    try check()
    if let firstTransient { throw firstTransient }
    throw failure("focus_unproven", "The exact requested window did not become the focused window")
}

// Restoration uses the same bounded read-only focus poll as establishment.
// False is an ended ownership epoch, never a successful process-only return.
private func waitForRestoredFocus(remaining: () -> TimeInterval,
                                  check: () throws -> Void,
                                  invalidated: () -> Bool,
                                  read: () throws -> Bool,
                                  wait: (TimeInterval) -> Void) throws -> Bool {
    do {
        let restored = try waitForEstablishedFocus(remaining: remaining, check: check, read: {
            if invalidated() { return false }
            let matches: Bool
            do { matches = try read() }
            catch {
                if invalidated() { return false }
                throw error
            }
            if invalidated() { return false }
            return matches ? true : nil
        }, wait: wait)
        return restored && !invalidated()
    } catch let error as NativeFailure where error.code == "focus_unproven" {
        throw failure("focus_restore_unproven", "Previous foreground restoration is unconfirmed: \(error.message)",
                      details: error.json, focusAbsence: error.focusAbsence)
    }
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
        throw failure("ax_unavailable", "AX array \(attribute) failed: \(result.rawValue)",
                      attributeFailure: AXAttributeFailure(name: attribute, error: result))
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
    guard result == .success else {
        traceSemanticFailure(["operation":"action_names", "ax_error":result.rawValue])
        throw failure("ax_unavailable", "AX action names failed: \(result.rawValue)")
    }
    return names as? [String] ?? []
}

private func supportsStringValueEditing(_ role: String) -> Bool {
    // Some providers report writable string AXValue on structural and action
    // nodes. That alone does not establish an editable-text operation.
    [kAXTextFieldRole, kAXTextAreaRole, kAXComboBoxRole].contains(role)
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

private enum FocusValidationPhase: String {
    case validation, preRaise = "pre_raise", postRaise = "post_raise"
}
private enum ModalReadScope: String {
    case appWindow = "app_window", appChild = "app_child"
    case targetRoot = "target_root", targetChild = "target_child"
}

private func modalReadFailure(_ original: NativeFailure, phase: FocusValidationPhase,
                              scope: ModalReadScope, expired: Bool = false,
                              refreshError: String? = nil) -> NativeFailure {
    var details = original.details ?? [:]
    details["validation_phase"] = phase.rawValue
    details["validation_scope"] = scope.rawValue
    if let refreshError { details["inventory_refresh_error"] = refreshError }
    var trace: [String: Any] = ["operation":"modal_validation", "phase":phase.rawValue,
        "scope":scope.rawValue, "code":original.code, "snapshot_expired":expired]
    if let attribute = original.attributeFailure {
        trace["attribute"] = attribute.name; trace["ax_error"] = attribute.error.rawValue
    }
    traceSemanticFailure(trace)
    return failure(original.code, original.message + " [" + phase.rawValue + "/" + scope.rawValue + "]",
                   details: details, attributeFailure: original.attributeFailure,
                   focusAbsence: original.focusAbsence, modalSnapshotExpired: expired)
}

// The pure policy seam cannot skip an unreadable element. It either preserves
// its error or marks the entire read snapshot expired for the existing focus
// poll to restart. No focus or input function is reachable from this helper.
private func readModalSnapshot<T>(phase: FocusValidationPhase, scope: ModalReadScope,
                                  isTarget: Bool, read: () throws -> T,
                                  checkTarget: () throws -> Void = {},
                                  parentContains: () throws -> Bool = { true }) throws -> T {
    do { return try read() }
    catch let original as NativeFailure {
        // A nested target-child error already has the most specific provenance.
        if original.details?["validation_scope"] != nil { throw original }
        guard phase == .postRaise, scope != .targetRoot, !isTarget,
              original.code == "ax_unavailable", original.attributeFailure?.error == .invalidUIElement else {
            throw modalReadFailure(original, phase: phase, scope: scope)
        }
        // Target validation is never downgraded to an auxiliary snapshot
        // error, even when its native failure has another AX status.
        try checkTarget()
        let contains: Bool
        do {
            contains = try parentContains()
        } catch let refresh as NativeFailure {
            // A deadline or actual target loss is stronger evidence than a
            // stale auxiliary object. An unreadable/incomplete parent is not.
            if ["deadline_exceeded", "native_stopping", "app_changed", "app_closed",
                "target_changed", "stale_window", "stale_observation"].contains(refresh.code) {
                throw modalReadFailure(refresh, phase: phase, scope: scope)
            }
            throw modalReadFailure(original, phase: phase, scope: scope, refreshError: refresh.code)
        }
        try checkTarget()
        throw modalReadFailure(original, phase: phase, scope: scope, expired: !contains)
    }
}

private struct FocusModalContext {
    let phase: FocusValidationPhase
    let originalTarget: AXUIElement?
    let checkTarget: () throws -> Void

    func inspect(_ element: AXUIElement, scope: ModalReadScope, context: RequestContext,
                 parentContains: () throws -> Bool) throws {
        // A missing retained target never grants non-target recovery.
        let knownTarget = originalTarget.map { CFEqual($0, element) }
        let isTarget = knownTarget ?? true
        try readModalSnapshot(phase: phase, scope: knownTarget == true ? .targetRoot : scope, isTarget: isTarget, read: {
            try rejectModalSurface(element, context)
        }, checkTarget: checkTarget, parentContains: parentContains)
    }
}

private func completeModalChildren(_ parent: AXUIElement, _ context: RequestContext) throws -> [AXUIElement] {
    // A single copied array proves absence without combining different child
    // page snapshots. Missing attributes, malformed arrays and a truncated
    // prefix are all insufficient; leave the original typed error intact.
    let children = try axList(parent, kAXChildrenAttribute, limit: 4_097, context.deadline, required: true)
    guard children.count <= 4_096 else {
        throw failure("modal_state_unproven", "Modal child inventory exceeds the supported limit")
    }
    return children
}

private func requireAppWithoutModal(_ application: AXUIElement, windows: [AXUIElement],
                                    context: RequestContext, modal: FocusModalContext) throws {
    // AXWindows alone need not contain a top-level consent sheet (the donor's
    // window_scope explicitly inspects application children for that case).
    var seen: [AXUIElement] = []
    for element in windows {
        guard !seen.contains(where: { CFEqual($0, element) }) else { continue }
        seen.append(element)
        try modal.inspect(element, scope: .appWindow, context: context) {
            let fresh = try axList(application, kAXWindowsAttribute, limit: 129, context.deadline, required: true)
            guard fresh.count <= 128 else { throw failure("window_limit", "AX window inventory exceeds the supported limit") }
            return fresh.contains { CFEqual($0, element) }
        }
    }
    try inspectModalChildren(application, context: context) { element in
        guard !seen.contains(where: { CFEqual($0, element) }) else { return }
        seen.append(element)
        try modal.inspect(element, scope: .appChild, context: context) {
            try completeModalChildren(application, context).contains { CFEqual($0, element) }
        }
    }
}

private func requireWindowWithoutSheet(_ element: AXUIElement, _ context: RequestContext,
                                      modal: FocusModalContext) throws {
    try readModalSnapshot(phase: modal.phase, scope: .targetRoot, isTarget: true, read: {
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
        try inspectModalChildren(element, context: context) { child in
            try modal.inspect(child, scope: .targetChild, context: context) {
                try completeModalChildren(element, context).contains { CFEqual($0, child) }
            }
        }
    })
}

private func inspectModalChildren(_ element: AXUIElement, context: RequestContext,
                                  inspect: (AXUIElement) throws -> Void) throws {
    let count = try axChildCount(element, context.deadline, required: true)
    guard count <= 4_096 else {
        throw failure("modal_state_unproven", "Modal child inventory exceeds the supported limit")
    }
    var index = 0
    var previous: AXUIElement?
    while index < count {
        guard try axChildCount(element, context.deadline, required: true) == count else {
            throw failure("modal_state_unproven", "Modal child inventory changed during validation")
        }
        let start = previous == nil ? index : index - 1
        var children = try axChildRange(element, index: start, count: min(16 + index - start, count - start), context.deadline)
        if let previous {
            guard CFEqual(children[0], previous) else {
                throw failure("modal_state_unproven", "Modal child inventory order changed during validation")
            }
            children.removeFirst()
        }
        guard !children.isEmpty else {
            throw failure("modal_state_unproven", "Modal child range became empty during validation")
        }
        for child in children { try inspect(child) }
        index += children.count
        previous = children.last
    }
    guard try axChildCount(element, context.deadline, required: true) == count else {
        throw failure("modal_state_unproven", "Modal child inventory changed before validation completed")
    }
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
private func semanticRole(_ role: String, subrole: String? = nil) -> String {
    if [kAXButtonRole, kAXRadioButtonRole].contains(role),
       subrole == NSAccessibility.Subrole.tabButtonSubrole.rawValue { return "tab" }
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
    case NSAccessibility.Role.link.rawValue: return "link"
    case kAXTableRole: return "table"
    case kAXRowRole: return "row"
    case kAXCellRole: return "cell"
    case kAXListRole: return "list"
    case kAXTabGroupRole: return "tab_group"
    case kAXPopUpButtonRole: return "pop_up_button"
    case kAXMenuItemRole: return "menu_item"
    case kAXHeadingRole: return "heading"
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
    let canSetValue: Bool

    var json: [String: Any] {
        ["id": id, "label": label, "role": role,
         "name": name as Any? ?? NSNull(), "value": value as Any? ?? NSNull(),
         "enabled": enabled as Any? ?? NSNull(), "actions": (canPress ? ["press"] : []) + (canSetValue ? ["setValue"] : []), "can_press": canPress]
    }
}

private struct NativeObservation {
    let target: NativeTarget
    let root: AXUIElement
    let query: NativeQuery
    var elements: [NativeElement] = []
    var pending: [DiscoveryFrame]
    var visited: [CFHashCode: [AXUIElement]] = [:]
    var visits = 0
    var edges = 0
    var buffered: NativeElement?
    var reason: String?

    mutating func note(_ value: String) { if reason == nil { reason = value } }
    mutating func enter(_ element: AXUIElement) -> Bool {
        let hash = CFHash(element)
        if visited[hash]?.contains(where: { CFEqual($0, element) }) == true { return false }
        visited[hash, default: []].append(element)
        visits += 1
        return true
    }
    mutating func deferVisit(_ element: AXUIElement) {
        let hash = CFHash(element)
        visited[hash]?.removeAll(where: { CFEqual($0, element) })
        if visited[hash]?.isEmpty == true { visited.removeValue(forKey: hash) }
        visits -= 1
    }
}

private struct NativeQuery {
    let role: String?
    let text: String?
    let limit: Int

    init(_ value: Any?) throws {
        if let value, !(value is [String: Any]) {
            throw failure("invalid_request", "Discovery query must be an object")
        }
        let object = value as? [String: Any] ?? [:]
        func optional(_ field: String) throws -> String? {
            guard let value = object[field], !(value is NSNull) else { return nil }
            guard let text = value as? String, !text.isEmpty,
                  text.unicodeScalars.count <= (field == "role" ? 64 : 256) else {
                throw failure("invalid_request", "Discovery \(field) exceeds its declared bound")
            }
            return text
        }
        role = try optional("role")
        text = try optional("text")
        let requested = try object["limit"].map { try integer($0, "query.limit") } ?? 40
        guard requested > 0, requested <= 100 else {
            throw failure("invalid_request", "Discovery limit must be in 1...100")
        }
        limit = Int(requested)
    }
}

// A child-array offset is part of the native frontier. Never materialize a
// complete AXChildren array merely to discard most of it after filtering.
private enum DiscoveryFrame {
    case node(AXUIElement, Int, Int)
    case children(AXUIElement, Int, Int, Int?, AXUIElement?)
}

private func axChildCount(_ element: AXUIElement, _ deadline: Deadline, required: Bool = false) throws -> Int {
    try deadline.prepare(element)
    var count: CFIndex = 0
    let result = AXUIElementGetAttributeValueCount(element, kAXChildrenAttribute as CFString, &count)
    try deadline.check()
    if result == .attributeUnsupported || result == .noValue {
        if required { throw failure("modal_state_unproven", "Required AXChildren inventory is unavailable") }
        return 0
    }
    guard result == .success, count >= 0 else {
        throw failure("children_unavailable", "AX child count failed: \(result.rawValue)")
    }
    return count
}

private func axChildRange(_ element: AXUIElement, index: Int, count: Int,
                          _ deadline: Deadline) throws -> [AXUIElement] {
    precondition(index >= 0 && count > 0 && count <= 17)
    try deadline.prepare(element)
    var value: CFArray?
    let result = AXUIElementCopyAttributeValues(element, kAXChildrenAttribute as CFString,
                                               index, count, &value)
    try deadline.check()
    guard result == .success, let value, CFArrayGetCount(value) > 0,
          CFArrayGetCount(value) <= count else {
        // A count/range race or illegalArgument is a coverage gap, never an
        // empty subtree. In particular, count zero never calls this function.
        throw failure("children_unavailable", "AX child range failed: \(result.rawValue)")
    }
    var children: [AXUIElement] = []
    for offset in 0..<CFArrayGetCount(value) {
        guard let pointer = CFArrayGetValueAtIndex(value, offset) else {
            throw failure("children_unavailable", "Null AX child")
        }
        let item = Unmanaged<AnyObject>.fromOpaque(pointer).takeUnretainedValue()
        guard CFGetTypeID(item) == AXUIElementGetTypeID() else {
            throw failure("children_unavailable", "Non-element AX child")
        }
        children.append(item as! AXUIElement)
    }
    return children
}

private enum SemanticReadinessDecision: String { case ready, pending, opaque, emptyContent }
private struct SemanticReadinessSample {
    var populatedWebArea = false
    var emptyWebArea = false
    var shallowUnknown = false
    var emptyContentGroup = false
    mutating func add(role: String, children: Int, depth: Int, parentRole: String) {
        if role == "AXWebArea" {
            populatedWebArea = populatedWebArea || children > 0
            emptyWebArea = emptyWebArea || children == 0
        }
        if role == kAXGroupRole, children == 0, depth == 1, parentRole == kAXWindowRole { emptyContentGroup = true }
        let containers = ["", kAXWindowRole, kAXGroupRole, kAXScrollAreaRole, "AXSplitGroup", kAXUnknownRole]
        if (role.isEmpty || role == kAXUnknownRole), children == 0,
           depth <= 4, containers.contains(parentRole) { shallowUnknown = true }
    }
    func decision(manualOptIn: Bool, elapsed: TimeInterval) -> SemanticReadinessDecision {
        if populatedWebArea { return .ready }
        // Process opt-in says nothing about this window's content. A browser's
        // native settings/dialog window need not contain an AXWebArea at all.
        if emptyWebArea { return .pending }
        if emptyContentGroup { return elapsed < 0.3 ? .pending : .emptyContent }
        if shallowUnknown { return elapsed < 0.3 ? .pending : .opaque }
        return .ready
    }
}

// One bounded observation epoch per worker, independent of request history.
// Notifications run on their own runloop: the Rust worker's main thread may be
// waiting for the next stdin frame while a script borrows foreground.
struct FocusBorrowEpoch {
    let initialPID: Int32
    let targetPID: Int32
    let targetWindow: CGWindowID?
    private(set) var reachedTarget: Bool
    private(set) var reachedWindow: Bool
    private(set) var invalidated = false
    // Only sampled reads carry a revision. First reaching a target advances it
    // even when a notification arrives before explicit establishment. Native
    // notifications retain their ordering/evidence, including leave-return.
    private(set) var sampleRevision: UInt64 = 0
    init(initialPID: Int32, targetPID: Int32, targetWindow: CGWindowID? = nil) {
        self.initialPID = initialPID; self.targetPID = targetPID; self.targetWindow = targetWindow
        reachedTarget = initialPID == targetPID
        reachedWindow = targetWindow == nil
    }
    mutating func activated(_ pid: Int32) {
        if pid == targetPID {
            if !reachedTarget { sampleRevision &+= 1 }
            reachedTarget = true
        } else if reachedTarget || pid != initialPID { invalidated = true }
    }
    mutating func focusedWindow(_ id: CGWindowID) {
        guard let targetWindow else { return }
        if id == targetWindow {
            if !reachedWindow { sampleRevision &+= 1 }
            reachedWindow = true
        } else if reachedWindow { invalidated = true }
    }
    mutating func sampled(_ pid: Int32, window: CGWindowID?, revision: UInt64) -> Bool {
        guard !invalidated, revision == sampleRevision else { return false }
        activated(pid)
        if targetPID == pid, let window { focusedWindow(window) }
        return true
    }
    mutating func established(_ pid: Int32, window: CGWindowID) {
        guard !invalidated else { return }
        // An AX sample started before exact establishment is not evidence of a
        // later external choice when its delayed result finally returns.
        sampleRevision &+= 1
        activated(pid)
        if targetPID == pid { focusedWindow(window) }
    }
    mutating func retarget(_ pid: Int32, window: CGWindowID? = nil) -> Bool {
        guard !invalidated else { return false }
        if targetPID != pid || targetWindow != window {
            let nextRevision = sampleRevision &+ 1
            self = FocusBorrowEpoch(initialPID: targetPID, targetPID: pid, targetWindow: window)
            sampleRevision = nextRevision
        }
        return true
    }
}
private final class FocusActivationObserver: @unchecked Sendable {
    private let lock = NSLock()
    private var ready = false
    private var serial: UInt64 = 0
    private var epoch: FocusBorrowEpoch?
    private var observerRunLoop: CFRunLoop?
    private var pollTimer: Timer?
    // Only the observer thread owns these AX registrations, at most one PID.
    private var axObserver: AXObserver?
    private var observedPID: Int32?
    init() {
        Thread.detachNewThread { [self] in
            let center = NSWorkspace.shared.notificationCenter
            let token = center.addObserver(forName: NSWorkspace.didActivateApplicationNotification,
                                            object: nil, queue: nil) { [weak self] notification in
                guard let app = notification.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication else { return }
                self?.activated(app.processIdentifier)
            }
            let timer = Timer(timeInterval: 0.02, repeats: true) { [weak self] timer in
                guard let self, self.isTracking else { timer.fireDate = .distantFuture; return }
                self.sampleFocus()
            }
            timer.fireDate = .distantFuture
            RunLoop.current.add(timer, forMode: .default)
            lock.lock(); observerRunLoop = CFRunLoopGetCurrent(); pollTimer = timer; ready = true; lock.unlock()
            withExtendedLifetime(token) { RunLoop.current.run() }
        }
    }
    private var isTracking: Bool {
        lock.lock(); defer { lock.unlock() }
        return epoch?.invalidated == false
    }
    private func activated(_ pid: Int32) {
        lock.lock(); defer { lock.unlock() }
        let before = epoch
        epoch?.activated(pid)
        if before?.invalidated == false && epoch?.invalidated == true {
            traceSemanticFailure(["operation":"focus_epoch_invalidated", "source":"activation_notification",
                                  "observed_pid":pid, "expected_pid":before!.targetPID])
        }
    }
    private func sampled(_ pid: Int32, window: CGWindowID?, token: UInt64, revision: UInt64,
                         source: String = "sample", sampleStarted: TimeInterval? = nil) {
        lock.lock(); defer { lock.unlock() }
        guard token == serial else { return }
        let before = epoch
        guard epoch?.sampled(pid, window: window, revision: revision) == true else {
            if before?.invalidated == false, before?.sampleRevision != revision {
                traceSemanticFailure(["operation":"focus_sample_expired", "sample_revision":revision,
                                      "current_revision":before!.sampleRevision, "source":source])
            }
            return
        }
        if before?.invalidated == false && epoch?.invalidated == true {
            var fields: [String: Any] = ["operation":"focus_epoch_invalidated", "source":source,
                                        "observed_pid":pid, "expected_pid":before!.targetPID]
            if let window { fields["observed_window"] = window }
            if let expected = before?.targetWindow { fields["expected_window"] = expected }
            if let sampleStarted { fields["sample_started_uptime"] = sampleStarted }
            fields["sample_finished_uptime"] = ProcessInfo.processInfo.systemUptime
            fields["reached_target_before_readback"] = before?.reachedTarget
            fields["reached_window_before_readback"] = before?.reachedWindow
            traceSemanticFailure(fields)
        }
    }
    private func sampleFocus() {
        let started = ProcessInfo.processInfo.systemUptime
        lock.lock(); let token = serial; let revision = epoch?.sampleRevision; lock.unlock()
        guard let revision else { return }
        guard let deadline = try? Deadline(["deadline_unix_ms": UInt64((Date().timeIntervalSince1970 + 0.04) * 1_000)]) else { return }
        do {
            // Fresh AX reads work while the worker main thread is waiting for
            // stdin; an NSWorkspace cached PID alone cannot see sibling windows.
            let system = AXUIElementCreateSystemWide()
            guard let app = axElement(try axAttribute(system, kAXFocusedApplicationAttribute, deadline)) else { return }
            let pid = try axPID(app, deadline)
            let window = axElement(try axAttribute(app, kAXFocusedWindowAttribute, deadline))
            sampled(pid, window: try window.map { try axWindowID($0, deadline) }, token: token, revision: revision,
                    sampleStarted: started)
        } catch {
            // The AX-free point path still has a bounded WindowServer sample.
            // Missing proof never establishes a new window identity.
            guard let pid = NSWorkspace.shared.frontmostApplication?.processIdentifier,
                  (try? windowServerFrontmost(pid, deadline)) == true else { return }
            let window = try? cgWindows(deadline).first { $0.pid == pid && $0.onscreen }?.id
            sampled(pid, window: window, token: token, revision: revision,
                    source: "sample_cg_fallback", sampleStarted: started)
        }
    }
    private func windowNotification(_ element: AXUIElement) {
        guard isTracking,
              let deadline = try? Deadline(["deadline_unix_ms": UInt64((Date().timeIntervalSince1970 + 0.04) * 1_000)]),
              let pid = try? axPID(element, deadline) else { return }
        // Callback elements retain the changed window, so B's queued event can
        // invalidate A -> B -> A even if the current focus has already returned.
        if let id = try? axWindowID(element, deadline) {
            lock.lock(); defer { lock.unlock() }
            let before = epoch
            if epoch?.targetPID == pid { epoch?.focusedWindow(id) }
            if before?.invalidated == false && epoch?.invalidated == true {
                var fields: [String: Any] = ["operation":"focus_epoch_invalidated", "source":"window_notification",
                                            "observed_pid":pid, "observed_window":id]
                if let expected = before?.targetWindow { fields["expected_window"] = expected }
                traceSemanticFailure(fields)
            }
        } else { sampleFocus() }
    }
    private func observeWindows(_ pid: Int32, on loop: CFRunLoop, deadline: Deadline) throws {
        let completed = DispatchSemaphore(value: 0)
        CFRunLoopPerformBlock(loop, CFRunLoopMode.defaultMode.rawValue) { [self] in
            defer { completed.signal() }
            if observedPID == pid { return }
            if let axObserver { CFRunLoopRemoveSource(loop, AXObserverGetRunLoopSource(axObserver), .defaultMode) }
            axObserver = nil; observedPID = nil
            var observer: AXObserver?
            guard AXObserverCreate(pid, { _, element, _, context in
                guard let context else { return }
                Unmanaged<FocusActivationObserver>.fromOpaque(context).takeUnretainedValue().windowNotification(element)
            }, &observer) == .success, let observer else { return }
            let application = AXUIElementCreateApplication(pid)
            guard (try? deadline.prepare(application)) != nil else { return }
            var supported = false
            for notification in [kAXFocusedWindowChangedNotification, kAXMainWindowChangedNotification] {
                guard (try? deadline.prepare(application)) != nil else { break }
                if AXObserverAddNotification(observer, application, notification as CFString,
                    Unmanaged.passUnretained(self).toOpaque()) == .success { supported = true }
            }
            if supported {
                axObserver = observer; observedPID = pid
                CFRunLoopAddSource(loop, AXObserverGetRunLoopSource(observer), .defaultMode)
            }
        }
        CFRunLoopWakeUp(loop)
        let left = deadline.uptime - ProcessInfo.processInfo.systemUptime
        guard left > 0, completed.wait(timeout: .now() + left) == .success else {
            throw failure("deadline_exceeded", "Focus observation setup exceeded its call budget")
        }
        try deadline.check()
    }
    func begin(initialPID: Int32, targetPID: Int32, window: CGWindowID, deadline: Deadline) throws -> UInt64 {
        while true {
            lock.lock()
            if ready, let loop = observerRunLoop, let timer = pollTimer {
                serial &+= 1; epoch = FocusBorrowEpoch(initialPID: initialPID, targetPID: targetPID, targetWindow: window)
                let token = serial; lock.unlock()
                try observeWindows(targetPID, on: loop, deadline: deadline)
                CFRunLoopPerformBlock(loop, CFRunLoopMode.defaultMode.rawValue) { timer.fireDate = Date() }
                CFRunLoopWakeUp(loop)
                return token
            }
            lock.unlock(); try deadline.check(); Thread.sleep(forTimeInterval: 0.002)
        }
    }
    func established(_ token: UInt64, pid: Int32, window: CGWindowID) {
        lock.lock(); defer { lock.unlock() }
        guard token == serial else { return }
        epoch?.established(pid, window: window)
        traceSemanticFailure(["operation":"focus_epoch_established", "pid":pid, "window":window,
                              "sample_revision":epoch?.sampleRevision as Any,
                              "uptime":ProcessInfo.processInfo.systemUptime])
    }
    func invalidated(_ token: UInt64) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return token != serial || epoch?.invalidated != false
    }
    func retarget(_ token: UInt64, to pid: Int32, window: CGWindowID?, deadline: Deadline) throws -> Bool {
        lock.lock(); let loop = observerRunLoop; let valid = token == serial && epoch?.invalidated == false; lock.unlock()
        guard valid, let loop else { return false }
        try observeWindows(pid, on: loop, deadline: deadline)
        lock.lock(); defer { lock.unlock() }
        guard token == serial, var current = epoch, current.retarget(pid, window: window) else { return false }
        epoch = current
        return true
    }
    func end(_ token: UInt64) {
        lock.lock(); defer { lock.unlock() }
        if token == serial { epoch = nil }
    }
}

// One in-flight CUA action owns this cache. Its original monotonic deadline is
// reused for every callback; a step can never establish or extend an admission.
private final class BackgroundInputValidation {
    let target: NativeTarget
    let deadlineMilliseconds: UInt64
    let keyboard: Bool
    let context: RequestContext
    let window: AXUIElement?
    private var children: [(element: AXUIElement, isWindow: Bool)] = []

    init(target: NativeTarget, deadlineMilliseconds: UInt64, keyboard: Bool,
         context: RequestContext, window: AXUIElement?) {
        self.target = target; self.deadlineMilliseconds = deadlineMilliseconds
        self.keyboard = keyboard; self.context = context; self.window = window
    }
    func matches(target: NativeTarget, deadlineMilliseconds: UInt64, keyboard: Bool) -> Bool {
        self.target == target && self.deadlineMilliseconds == deadlineMilliseconds && self.keyboard == keyboard
    }
    // A busy AX server is not proof that the window changed. Wait only for a
    // fresh read of the same target, never re-prime or replay an input event.
    // True identity/modal failures stop immediately and the original action
    // deadline remains authoritative throughout the bounded read recovery.
    static func waitForReadableTarget(remaining: () -> TimeInterval,
                                      check: () throws -> Void,
                                      read: () throws -> Void,
                                      wait: (TimeInterval) -> Void) throws {
        var firstTransient: NativeFailure?
        while remaining() > 0 {
            try check()
            if remaining() <= 0 { break }
            do {
                try read()
                try check()
                if remaining() > 0 { return }
                break
            } catch let error as NativeFailure {
                // Only the local read budget may expire here. The check below
                // still gives the original caller deadline/cancellation priority.
                if error.code == "deadline_exceeded", remaining() <= 0, firstTransient != nil { break }
                guard error.code == "ax_unavailable", error.attributeFailure?.error == .cannotComplete else {
                    throw error
                }
                if firstTransient == nil { firstTransient = error }
            }
            // A blocking read cannot hide closure, replacement, cancellation,
            // or the caller's deadline even when the AX provider stays busy.
            try check()
            let available = remaining()
            if available > 0 { wait(min(0.02, available)) }
        }
        try check()
        if let firstTransient { throw firstTransient }
        throw failure("ax_unavailable", "Target AX validation did not complete within its read budget")
    }

    func checkModalState(revalidate: () throws -> Void) throws {
        guard window != nil else { return }
        let started = ProcessInfo.processInfo.systemUptime
        let deadline = context.deadline.limited(to: started + 0.75)
        var samples = 0
        try Self.waitForReadableTarget(remaining: {
            deadline.uptime - ProcessInfo.processInfo.systemUptime
        }, check: {
            guard !dtwInputStopping() else { throw failure("native_stopping", "Input admission is closed") }
            try self.context.deadline.check()
            try revalidate()
        }, read: {
            samples += 1
            try self.readModalState(deadline)
        }, wait: { Thread.sleep(forTimeInterval: $0) })
        if samples > 1 {
            traceSemanticFailure(["operation":"background_validation_recovered", "samples":samples,
                                  "elapsed_ms":Int((ProcessInfo.processInfo.systemUptime - started) * 1_000)])
        }
    }

    private func readModalState(_ deadline: Deadline) throws {
        guard let window else { return }
        try requireNonModalWindow(window, deadline)
        if let sheets = try axAttribute(window, "AXSheets", deadline) {
            guard CFGetTypeID(sheets) == CFArrayGetTypeID() else {
                throw failure("modal_state_unproven", "AXSheets is not an array")
            }
            guard CFArrayGetCount((sheets as! CFArray)) == 0 else {
                throw failure("unsupported_modal", "The target window has an attached sheet")
            }
        }
        // Refresh only the target's shallow child membership. Known control
        // objects need no repeated role lookup; newly attached sheets still stop
        // before another event. Nested AX content is never traversed here.
        let current = try axList(window, kAXChildrenAttribute, limit: 4_097, deadline, required: true)
        guard current.count <= 4_096 else {
            throw failure("modal_state_unproven", "Modal child inventory exceeds the supported limit")
        }
        var next: [(element: AXUIElement, isWindow: Bool)] = []
        for child in current {
            let isWindow: Bool
            if let retained = children.first(where: { CFEqual($0.element, child) }) {
                isWindow = retained.isWindow
            } else {
                guard let role = try axAttribute(child, kAXRoleAttribute, deadline) as? String else {
                    throw failure("modal_state_unproven", "An AX child has no readable role")
                }
                guard role != kAXSheetRole else {
                    throw failure("unsupported_modal", "A sheet is present; sheet/modal routing is not supported")
                }
                isWindow = role == kAXWindowRole
            }
            if isWindow { try requireNonModalWindow(child, deadline) }
            next.append((child, isWindow))
        }
        children = next
        try deadline.check()
    }
}

private final class NativeStore: @unchecked Sendable {
    let lock = NSLock()
    private struct BorrowedFocus {
        let previousApp: NSRunningApplication
        let previousGeneration: UInt64
        let previousWindow: AXUIElement?
        let previousWindowID: CGWindowID?
        let activationEpoch: UInt64
        var target: NativeTarget
        var window: AXUIElement?
        let pointer: CGPoint?
        var inputPoint: CGPoint?
    }
    private var borrowedFocus: BorrowedFocus?
    private var backgroundValidation: BackgroundInputValidation?
    private lazy var focusObserver = FocusActivationObserver()
    fileprivate private(set) var focusCleanupFailure: NativeFailure?
    fileprivate var focusOwner: NativeApp? {
        guard let borrowed = borrowedFocus else { return nil }
        // External focus choice ends our borrow. No physical-input lease is
        // needed to discard that ownership; no restoration will be attempted.
        if focusObserver.invalidated(borrowed.activationEpoch) {
            borrowedFocus = nil; focusObserver.end(borrowed.activationEpoch)
            return nil
        }
        return borrowed.target.app
    }
    private var windows: [WindowSlot: NativeTarget] = [:]
    private var semanticWindows: [WindowSlot: RegisteredWindow] = [:]
    private var enabledApps: [NativeApp: Bool] = [:]
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

    // CGWindow.h defines CGWindowID as unique within the user session. The
    // registry adds this worker's generation and the kernel app birth stamp;
    // neither title, geometry nor an AX object's availability establishes identity.
    private func listWindows(_ app: NativeApp, _ context: RequestContext) throws -> [String: Any] {
        try context.verify(app)
        let registeredApps = Set(windows.keys.map(\.app)).union(enabledApps.keys)
        var departed: Set<NativeApp> = []
        for owner in registeredApps where try context.hasExited(owner) { departed.insert(owner) }
        windows = windows.filter { !departed.contains($0.key.app) }
        enabledApps = enabledApps.filter { !departed.contains($0.key) }
        if let validation = backgroundValidation, departed.contains(validation.target.app) { backgroundValidation = nil }
        let source = try cgWindowRows(context.deadline)
        let (candidates, coverage) = try surfaceInventory(source, pid: app.pid, resolve: { record in
            try exactSurfaceRecord(cgWindowRows(context.deadline, only: record.id), expected: record)
        })
        guard candidates.count <= 128 else { throw failure("window_limit", "More than 128 system window surfaces") }
        try context.verify(app)
        let active = Set(candidates.map { WindowSlot(app: app, id: $0.0.id) })
        windows = windows.filter { $0.key.app.pid != app.pid || ($0.key.app == app && active.contains($0.key)) }
        semanticWindows = semanticWindows.filter { windows[$0.key] == $0.value.target }
        observations = observations.filter { windows[WindowSlot(app: $0.value.target.app, id: $0.value.target.nativeID)] == $0.value.target }
        var rows: [[String: Any]] = []
        for (record, title) in candidates {
            let slot = WindowSlot(app: app, id: record.id)
            let target = try windows[slot] ?? NativeTarget(app: app, nativeID: record.id, generation: nextGeneration())
            windows[slot] = target
            rows.append(["target": target.json, "title": title, "bounds": record.coordinates, "onscreen": record.onscreen])
        }
        return ["windows": rows, "coverage": coverage]
    }

    fileprivate func validateSurface(_ target: NativeTarget, _ context: RequestContext) throws -> CGRecord {
        let slot = WindowSlot(app: target.app, id: target.nativeID)
        guard windows[slot] == target else { throw failure("stale_window", "Surface was not registered in this worker generation") }
        try context.verify(target.app)
        let records = try cgWindowRows(context.deadline, only: target.nativeID).compactMap(cgRecord)
            .filter { $0.id == target.nativeID }
        guard records.count == 1, let record = records.first, record.pid == target.app.pid else {
            windows.removeValue(forKey: slot)
            semanticWindows.removeValue(forKey: slot)
            observations = observations.filter { $0.value.target != target }
            throw failure("target_changed", "WindowServer no longer proves the requested surface owner")
        }
        try context.verify(target.app)
        return record
    }

    fileprivate func validate(_ target: NativeTarget, _ context: RequestContext,
                              input: Bool = true, phase: FocusValidationPhase = .validation,
                              originalTarget: AXUIElement? = nil) throws -> CandidateWindow {
        let slot = WindowSlot(app: target.app, id: target.nativeID)
        let record = try validateSurface(target, context)
        guard AXIsProcessTrusted() else { throw failure("permission_denied", "Accessibility permission is required") }
        let application = AXUIElementCreateApplication(target.app.pid)
        let current = try axList(application, kAXWindowsAttribute, limit: 129, context.deadline, required: true)
        guard current.count <= 128 else { throw failure("window_limit", "AX window inventory exceeds the supported limit") }
        func checkOriginalTarget() throws {
            guard phase == .postRaise else { return }
            // Prove the retained target root first. A replacement target AX
            // object must never be mistaken for an expired auxiliary popup.
            guard let originalTarget, current.contains(where: { CFEqual($0, originalTarget) }) else {
                throw failure("stale_observation", "Target AX root changed during focus establishment")
            }
            let targetStillValid = try readModalSnapshot(phase: phase, scope: .targetRoot, isTarget: true, read: {
                try axWindowID(originalTarget, context.deadline) == target.nativeID
                    && ordinaryWindow(originalTarget, pid: target.app.pid, deadline: context.deadline)
            })
            guard targetStillValid else {
                throw failure("stale_observation", "Target AX root identity changed during focus establishment")
            }
        }
        try checkOriginalTarget()
        let modal = FocusModalContext(phase: phase, originalTarget: originalTarget, checkTarget: {
            guard !dtwInputStopping() else { throw failure("native_stopping", "Native input admission is closing") }
            _ = try self.validateSurface(target, context)
            try checkOriginalTarget()
        })
        if input { try requireAppWithoutModal(application, windows: current, context: context, modal: modal) }
        var matched: [AXUIElement] = []
        for element in current {
            do {
                if try axWindowID(element, context.deadline) == target.nativeID { matched.append(element) }
            } catch let error as NativeFailure where error.code == "window_unproven" { continue }
        }
        guard matched.count == 1, let element = matched.first else {
            throw failure("semantic_window_unavailable", "No unique supported AX window matches this system surface")
        }
        let ordinary = try readModalSnapshot(phase: phase, scope: .targetRoot, isTarget: true, read: {
            try ordinaryWindow(element, pid: target.app.pid, deadline: context.deadline)
        })
        guard ordinary else {
            throw failure("semantic_window_unavailable", "No unique supported AX window matches this system surface")
        }
        if let retained = semanticWindows[slot], !CFEqual(retained.element, element) {
            throw failure("stale_observation", "AX window identity changed; observe this surface again")
        }
        semanticWindows[slot] = RegisteredWindow(target: target, element: element)
        if input { try requireWindowWithoutSheet(element, context, modal: modal) }
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


    private func pressable(_ element: AXUIElement, role: String, enabled: Bool, window: AXUIElement,
                           target: NativeTarget, deadline: Deadline) throws -> Bool {
        guard ![kAXWindowRole, kAXApplicationRole].contains(role), enabled,
              try actions(element, deadline).contains(kAXPressAction) else { return false }
        return try owned(element, by: window, target: target, deadline: deadline)
    }

    private func settable(_ element: AXUIElement, role: String, enabled: Bool, window: AXUIElement,
                          target: NativeTarget, deadline: Deadline) throws -> Bool {
        guard enabled, supportsStringValueEditing(role),
              try owned(element, by: window, target: target, deadline: deadline),
              try axAttribute(element, kAXValueAttribute, deadline) is String else { return false }
        try deadline.prepare(element)
        var settable = DarwinBoolean(false)
        let result = AXUIElementIsAttributeSettable(element, kAXValueAttribute as CFString, &settable)
        try deadline.check()
        if result == .attributeUnsupported || result == .noValue { return false }
        guard result == .success else {
            traceSemanticFailure(["operation":"attribute_settable", "attribute":kAXValueAttribute, "ax_error":result.rawValue])
            throw failure("ax_unavailable", "AXValue writability unavailable")
        }
        return settable.boolValue
    }

    private func discoveryPage(_ observation: inout NativeObservation,
                               binding: String, context: RequestContext) throws -> [String: Any] {
        let window = try validate(observation.target, context, input: false)
        guard CFEqual(window.element, observation.root) else {
            throw failure("target_changed", "Discovery root was replaced")
        }
        // Cooperative page budget is separate from the enclosing RPC deadline.
        // A stuck AX provider is still terminated by the parent worker owner.
        let pageDeadline = context.deadline.limited(to: ProcessInfo.processInfo.systemUptime + 0.75)
        var rows: [NativeElement] = []
        var bytes = 2
        var work = 0
        func rowBytes(_ row: NativeElement) throws -> Int {
            try JSONSerialization.data(withJSONObject: row.json).count + 1
        }
        if let buffered = observation.buffered {
            rows.append(buffered)
            observation.elements.append(buffered)
            bytes += try rowBytes(buffered)
            observation.buffered = nil
        }
        pageLoop: while !observation.pending.isEmpty && rows.count < observation.query.limit && work < 256 {
            if ProcessInfo.processInfo.systemUptime >= pageDeadline.uptime { break }
            if observation.visits >= 8_192 || observation.elements.count >= 4_096 || observation.edges >= 32_768 {
                observation.note("scan_limit")
                observation.pending.removeAll()
                break
            }
            work += 1
            let frame = observation.pending.removeLast()
            switch frame {
            case let .node(element, depth, attempt):
                guard observation.enter(element) else { continue }
                var scheduledChildren = false
                var nodeRole = "unknown"
                do {
                    guard try axPID(element, pageDeadline) == observation.target.app.pid else {
                        observation.note("ownership_unproven")
                        continue
                    }
                    let role = try axAttribute(element, kAXRoleAttribute, pageDeadline) as? String ?? ""
                    nodeRole = semanticRole(role)
                    if [kAXSheetRole, kAXMenuRole, kAXMenuBarRole].contains(role) { continue }
                    if role == kAXGroupRole, depth == 1,
                       try axChildCount(element, pageDeadline, required: false) == 0 {
                        observation.note("content_unproven")
                    }
                    if (role.isEmpty || role == kAXUnknownRole), depth <= 4,
                       try axChildCount(element, pageDeadline, required: false) == 0 {
                        observation.note("unsupported_semantic_tree")
                    }
                    observation.pending.append(.children(element, depth, 0, nil, nil))
                    scheduledChildren = true
                    let subrole = [kAXButtonRole, kAXRadioButtonRole].contains(role)
                        ? try axAttribute(element, kAXSubroleAttribute, pageDeadline) as? String : nil
                    let normalizedRole = semanticRole(role, subrole: subrole)
                    // A nonmatching parent is still traversed. Expensive name,
                    // value and pressability reads are needed only for matches.
                    if let required = observation.query.role, required != normalizedRole { continue }
                    var matched = observation.query.text == nil
                    func text(_ attribute: String) throws -> String? {
                        let value = try axAttribute(element, attribute, pageDeadline)
                        guard let text = (value as? String) ?? (value as? NSNumber)?.stringValue else { return nil }
                        if let needle = observation.query.text {
                            let source = text as NSString
                            let length = min(source.length, 4_096)
                            if source.length > length { observation.note("text_limit") }
                            if source.range(of: needle, options: [.caseInsensitive, .diacriticInsensitive],
                                            range: NSRange(location: 0, length: length)).location != NSNotFound {
                                matched = true
                            }
                        }
                        return String(String.UnicodeScalarView(text.unicodeScalars.prefix(256)))
                    }
                    let title = try text(kAXTitleAttribute)
                    let value = try text(kAXValueAttribute)
                    let descriptionRead = try optionalDescription(alreadyMatched: matched) {
                        try text(kAXDescriptionAttribute)
                    }
                    if descriptionRead.incompleteMatch { observation.note("node_unavailable") }
                    let description = descriptionRead.text
                    guard matched else { continue }
                    let labels = [title, value, description].compactMap { $0 }.filter { !$0.isEmpty }
                    let rawEnabled = try axAttribute(element, kAXEnabledAttribute, pageDeadline)
                    let enabled: Bool? = rawEnabled.flatMap {
                        CFGetTypeID($0) == CFBooleanGetTypeID() ? ($0 as? Bool) : nil
                    }
                    // Advisory support only. Press performs full modal, current
                    // ownership and enabled-state checks immediately before input.
                    let canPress = try pressable(element, role: role, enabled: enabled == true,
                                                 window: window.element, target: observation.target,
                                                 deadline: pageDeadline)
                    let row = NativeElement(id: UInt64(observation.elements.count + 1), element: element,
                                            label: labels.isEmpty ? String(String.UnicodeScalarView(role.unicodeScalars.prefix(256))) : labels.joined(separator: " | "),
                                            role: normalizedRole, name: title?.isEmpty == false ? title : description,
                                            value: value, enabled: enabled, canPress: canPress,
                                            canSetValue: try settable(element, role: role, enabled: enabled == true, window: window.element, target: observation.target, deadline: pageDeadline))
                    let size = try rowBytes(row)
                    if bytes + size > 12 * 1_024 {
                        observation.buffered = row
                        break
                    }
                    rows.append(row)
                    observation.elements.append(row)
                    bytes += size
                } catch {
                    traceSemanticFailure(["operation":"node", "role":nodeRole, "depth":depth,
                                          "code":(error as? NativeFailure)?.code ?? "internal"])
                    if let native = error as? NativeFailure, native.code == "deadline_exceeded",
                       ProcessInfo.processInfo.systemUptime < context.deadline.uptime, attempt == 0 {
                        // Yield rather than lose the node at a page boundary.
                        // One fresh-page retry is enough to distinguish boundary
                        // pressure from an individually unresponsive node.
                        if scheduledChildren { observation.pending.removeLast() }
                        observation.deferVisit(element)
                        observation.pending.append(.node(element, depth, 1))
                        break pageLoop
                    }
                    // One unreadable/changing node must not hide other branches.
                    // The saved child frame, if any, is visited on a later page.
                    observation.note("node_unavailable")
                }
            case let .children(element, depth, index, expectedCount, previous):
                do {
                    let count = try axChildCount(element, pageDeadline)
                    if let expectedCount, expectedCount != count { observation.note("tree_changed") }
                    guard index < count else { continue } // Includes count == 0.
                    guard depth < 64 else { observation.note("depth_limit"); continue }
                    // Overlap one element to detect common insertion/reorder
                    // races. AX has no cross-page snapshot guarantee; even a
                    // successful walk is explicitly a live traversal.
                    let start = previous == nil ? index : index - 1
                    var children = try axChildRange(element, index: start,
                                                     count: min(16 + (index - start), count - start), pageDeadline)
                    if let previous {
                        if !CFEqual(children[0], previous) { observation.note("tree_changed") }
                        children.removeFirst()
                    }
                    guard !children.isEmpty else { observation.note("tree_changed"); continue }
                    observation.edges += children.count
                    guard observation.pending.count + children.count + 1 <= 256 else {
                        observation.note("frontier_limit")
                        observation.pending.removeAll()
                        break
                    }
                    observation.pending.append(.children(element, depth, index + children.count, count, children.last))
                    observation.pending.append(contentsOf: children.reversed().map { .node($0, depth + 1, 0) })
                } catch {
                    if let native = error as? NativeFailure, native.code == "deadline_exceeded",
                       ProcessInfo.processInfo.systemUptime < context.deadline.uptime {
                        observation.pending.append(frame)
                        break pageLoop
                    }
                    observation.note("children_unavailable")
                }
            }
            if observation.buffered != nil { break }
        }
        let final = try validate(observation.target, context, input: false)
        guard CFEqual(final.element, observation.root) else {
            throw failure("target_changed", "Discovery root changed during page construction")
        }
        try context.deadline.check()
        let hasMore = !observation.pending.isEmpty || observation.buffered != nil
        return ["target": observation.target.json, "binding": binding, "elements": rows.map(\.json),
                "discovery": ["has_more": hasMore, "complete": !hasMore && observation.reason == nil,
                              "reason": observation.reason as Any? ?? NSNull(), "visited": observation.visits]]
    }

    // Source: CUA 0.38.0 AX enablement. Opt-in is process-scoped, but readiness
    // is sampled for every new query. No bundle-name classifier or whole-page
    // loading assertion is used. Unknown custom leaves remain explicitly partial.
    private func prepareSemantics(_ target: NativeTarget, _ context: RequestContext) throws -> String? {
        _ = try validateSurface(target, context)
        guard AXIsProcessTrusted() else { throw failure("permission_denied", "Accessibility permission is required") }
        let application = AXUIElementCreateApplication(target.app.pid)
        let manual: Bool
        if let cached = enabledApps[target.app] { manual = cached }
        else {
            try context.deadline.prepare(application)
            var result = AXUIElementSetAttributeValue(application, "AXManualAccessibility" as CFString, kCFBooleanTrue)
            manual = result == .success
            if result == .attributeUnsupported {
                result = AXUIElementSetAttributeValue(application, "AXEnhancedUserInterface" as CFString, kCFBooleanTrue)
            }
            try context.deadline.check()
            guard [.success, .attributeUnsupported, .notImplemented].contains(result) else {
                throw failure("semantic_not_ready", "Native accessibility opt-in was not accepted (\(result.rawValue))")
            }
            enabledApps[target.app] = manual
        }
        let started = ProcessInfo.processInfo.systemUptime
        let until = min(context.deadline.uptime, started + 3)
        let readiness = context.deadline.limited(to: until)
        var samples = 0
        var probedEmptyRoots: [AXUIElement] = []
        var last: [[String: Any]] = []
        var lastPending = false
        let tracing = ProcessInfo.processInfo.environment["DTW_AX_READINESS_TRACE"] == "1"
        func trace(_ decision: String) {
            guard tracing else { return }
            let value: [String: Any] = ["decision":decision, "samples":samples,
                "elapsed_ms":Int((ProcessInfo.processInfo.systemUptime-started)*1000),
                "manual_opt_in":manual, "nodes":last]
            if let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) {
                FileHandle.standardError.write(Data("dtw_semantic_readiness ".utf8) + data + Data("\n".utf8))
            }
        }
        do {
            while ProcessInfo.processInfo.systemUptime < until {
                let window = try validate(target, context, input: false)
                // Breadth-first and bounded: inspect container roots before a
                // wide toolbar's descendants. Never materialize an entire array.
                var queue: [(AXUIElement, Int, String)] = [(window.element, 0, "")]
                var index = 0
                var sample = SemanticReadinessSample()
                last = []
                while index < queue.count && index < 400 {
                    let (element, depth, parentRole) = queue[index]; index += 1
                    let role = try axAttribute(element, kAXRoleAttribute, readiness) as? String ?? ""
                    let count = try axChildCount(element, readiness, required: false)
                    sample.add(role: role, children: count, depth: depth, parentRole: parentRole)
                    var node: [String: Any] = ["role":role,"children":count,"depth":depth,"parent":parentRole]
                    // Count alone may expose a remote WebKit placeholder. Read
                    // one actual child slot to ask the provider to materialize
                    // this exact content root; never copy the entire child array.
                    if role == kAXGroupRole, depth == 1, parentRole == kAXWindowRole, count == 0,
                       !probedEmptyRoots.contains(where: { CFEqual($0, element) }) {
                        probedEmptyRoots.append(element)
                        try readiness.prepare(element)
                        var child: CFArray?
                        let result = AXUIElementCopyAttributeValues(element, kAXChildrenAttribute as CFString, 0, 1, &child)
                        try readiness.check()
                        node["child_probe_error"] = result.rawValue
                        node["child_probe_count"] = child.map(CFArrayGetCount) ?? 0
                    }
                    if last.count < 32 { last.append(node) }
                    if role == "AXWebArea" && count > 0 { break }
                    if depth < 10 && role != kAXMenuBarRole && count > 0 {
                        var offset = 0
                        let admitted = min(count, max(0, 400-queue.count))
                        while offset < admitted {
                            let children = try axChildRange(element, index: offset, count: min(16, admitted-offset), readiness)
                            queue.append(contentsOf: children.map { ($0, depth+1, role) }); offset += children.count
                        }
                    }
                }
                samples += 1
                let decision = sample.decision(manualOptIn: manual,
                    elapsed: ProcessInfo.processInfo.systemUptime-started)
                lastPending = decision != .ready
                if samples == 1 { trace(decision.rawValue) }
                switch decision {
                case .ready: if samples > 1 { trace("ready") }; return nil
                case .opaque: trace("unsupported_semantic_tree"); return "unsupported_semantic_tree"
                case .emptyContent: trace("content_unproven"); return "content_unproven"
                case .pending: break
                }
                CFRunLoopRunInMode(.defaultMode, min(0.05, max(0, until-ProcessInfo.processInfo.systemUptime)), false)
            }
        } catch let error as NativeFailure where error.code == "deadline_exceeded" {
            try context.deadline.check()
        }
        try context.deadline.check()
        trace(lastPending ? "semantic_not_ready" : "probe_budget_exhausted")
        throw failure("semantic_not_ready", "Native accessibility roots remained unproven within the readiness budget; capture remains available")
    }

    private func observe(_ request: [String: Any], _ context: RequestContext) throws -> [String: Any] {
        let target = try NativeTarget(request["target"])
        observations = observations.filter { $0.value.target != target }
        guard observations.count < 8 else {
            throw failure("registry_exhausted", "At most eight live semantic queries may be retained")
        }
        let query = try NativeQuery(request["query"])
        semanticWindows.removeValue(forKey: WindowSlot(app: target.app, id: target.nativeID))
        let readinessReason = try prepareSemantics(target, context)
        let window = try validate(target, context, input: false)
        let binding = UUID().uuidString
        var observation = NativeObservation(target: target, root: window.element, query: query,
                                            pending: [.node(window.element, 0, 0)])
        observation.reason = readinessReason
        let page = try discoveryPage(&observation, binding: binding, context: context)
        observations[binding] = observation
        return page
    }

    private func observeNext(_ request: [String: Any], _ context: RequestContext) throws -> [String: Any] {
        guard let binding = request["binding"] as? String,
              var observation = observations.removeValue(forKey: binding) else {
            throw failure("stale_observation", "Discovery query was retired or consumed")
        }
        guard observation.target == (try NativeTarget(request["target"])) else {
            throw failure("target_changed", "Discovery query belongs to another target")
        }
        guard !observation.pending.isEmpty || observation.buffered != nil else {
            throw failure("stale_observation", "Discovery query has no remaining pages")
        }
        let page = try discoveryPage(&observation, binding: binding, context: context)
        observations[binding] = observation
        return page
    }

    private func focused(_ target: NativeTarget, _ root: AXUIElement, _ deadline: Deadline) throws -> Bool {
        try focused(target.app.pid, root, windowID: target.nativeID, deadline)
    }

    private func focused(_ pid: Int32, _ root: AXUIElement?, windowID: CGWindowID?,
                         _ deadline: Deadline) throws -> Bool {
        try exactInputFocus(expectedPID: pid, focusedPID: {
            let system = AXUIElementCreateSystemWide()
            guard let app = axElement(try axAttribute(system, kAXFocusedApplicationAttribute, deadline)) else { return nil }
            return try axPID(app, deadline)
        }, windowMatches: {
            // A positively absent original focused window only promises the
            // application return; the caller reports that distinct outcome.
            if root == nil && windowID == nil { return true }
            guard let root, let windowID else {
                throw failure("focus_restore_unproven", "Incomplete original window identity")
            }
            let app = AXUIElementCreateApplication(pid)
            guard let window = axElement(try axAttribute(app, kAXFocusedWindowAttribute, deadline)) else {
                throw missingFocus(.window)
            }
            return try CFEqual(window, root) && axWindowID(window, deadline) == windowID
        })
    }

    private func borrowFocus(_ target: NativeTarget, _ window: AXUIElement?, _ context: RequestContext,
                             inputPoint: CGPoint? = nil) throws {
        guard !dtwInputStopping() else { throw failure("native_stopping", "Input admission is closing") }
        if let focusCleanupFailure { throw focusCleanupFailure }
        if let prior = borrowedFocus {
            let stillOwned: Bool
            if let priorWindow = prior.window {
                stillOwned = (try? focused(prior.target, priorWindow, context.deadline)) == true
            } else {
                stillOwned = try windowServerFrontmost(prior.target.app.pid, context.deadline)
            }
            if stillOwned, try focusObserver.retarget(prior.activationEpoch, to: target.app.pid,
                                                      window: target.nativeID, deadline: context.deadline) {
                borrowedFocus?.target = target; borrowedFocus?.window = window
                borrowedFocus?.inputPoint = inputPoint; return
            }
            // An external choice ends the old borrow permanently. A later
            // explicit focus starts from the user's new foreground, never the
            // stale application remembered by an earlier focus in this script.
            focusObserver.end(prior.activationEpoch); borrowedFocus = nil
        }
        // NSWorkspace.frontmostApplication is a main-runloop snapshot and can
        // remain stale after this resident worker waited for a new stdin frame.
        // Select from fresh AX, then independently prove the WindowServer front
        // process before retaining any return identity or dispatching input.
        let system = AXUIElementCreateSystemWide()
        guard let previousAX = axElement(try axAttribute(system, kAXFocusedApplicationAttribute, context.deadline)) else {
            throw missingFocus(.application)
        }
        let previousPID = try axPID(previousAX, context.deadline)
        guard try windowServerFrontmost(previousPID, context.deadline) else {
            traceSemanticFailure(["operation":"focus_snapshot", "phase":"front_process_mismatch", "previous_pid":previousPID])
            throw failure("focus_unproven", "System AX and WindowServer disagree about the current foreground application")
        }
        guard let previous = NSRunningApplication(processIdentifier: previousPID) else {
            throw failure("focus_unavailable", "Cannot identify the current foreground application")
        }
        guard let previousGeneration = try context.startStamp(previous.processIdentifier) else {
            throw failure("focus_unproven", "Previous foreground process identity is unavailable")
        }
        let previousValue = try axAttribute(previousAX, kAXFocusedWindowAttribute, context.deadline,
                                            unsupportedIsAbsent: false)
        let previousWindow = axElement(previousValue)
        guard previousValue == nil || previousWindow != nil else {
            throw failure("focus_unproven", "Previous focused window was not an AX window object")
        }
        let previousWindowID = try previousWindow.map { window in
            guard try axPID(window, context.deadline) == previous.processIdentifier else {
                throw failure("focus_unproven", "Previous focused window belongs to another process")
            }
            return try axWindowID(window, context.deadline)
        }
        var snapshotFields: [String: Any] = ["operation":"focus_snapshot", "phase":"captured",
                                              "previous_pid":previousPID, "target_pid":target.app.pid]
        if let previousWindowID { snapshotFields["previous_window"] = previousWindowID }
        traceSemanticFailure(snapshotFields)
        let epoch = try focusObserver.begin(initialPID: previous.processIdentifier, targetPID: target.app.pid,
                                            window: target.nativeID, deadline: context.deadline)
        do {
            try validateFocusSnapshot(check: {
                guard try context.startStamp(previous.processIdentifier) == previousGeneration,
                      try windowServerFrontmost(previous.processIdentifier, context.deadline) else {
                    throw failure("focus_unproven", "Previous foreground changed while its return target was captured")
                }
            }, read: {
                if let previousWindow, let previousWindowID {
                    return try focused(previous.processIdentifier, previousWindow,
                                       windowID: previousWindowID, context.deadline)
                }
                // Absence is a captured state too: a newly focused original-app
                // window must not silently become an application-only return.
                return try axAttribute(previousAX, kAXFocusedWindowAttribute, context.deadline,
                                       unsupportedIsAbsent: false) == nil
            })
        } catch {
            snapshotFields["phase"] = "failed"
            snapshotFields["error"] = (error as? NativeFailure)?.code ?? "unknown"
            traceSemanticFailure(snapshotFields)
            focusObserver.end(epoch)
            throw error
        }
        borrowedFocus = BorrowedFocus(previousApp: previous, previousGeneration: previousGeneration,
                                      previousWindow: previousWindow, previousWindowID: previousWindowID, activationEpoch: epoch,
                                      target: target, window: window,
                                      pointer: CGEvent(source: nil)?.location, inputPoint: inputPoint)
    }

    private func establishFocus(_ target: NativeTarget, _ context: RequestContext, changed: inout Bool) throws -> CandidateWindow {
        let window = try validate(target, context, phase: .preRaise)
        // CUA can establish app-local key/AX focus without raising a window.
        // A selected physical route must perform its own foreground transition;
        // AX focus alone cannot short-circuit the raise.
        func trace(_ phase: String) {
            guard ProcessInfo.processInfo.environment["DTW_AX_READINESS_TRACE"] == "1" else { return }
            let line = "dtw_focus_transition pid=\(target.app.pid) window=\(target.nativeID) phase=\(phase)\n"
            FileHandle.standardError.write(Data(line.utf8))
        }
        // prepare_foreground already owns the transition to this target. Do
        // not replace its return target merely because the raise has not run yet.
        if borrowedFocus?.target != target || borrowedFocus.map({ focusObserver.invalidated($0.activationEpoch) }) != false {
            try borrowFocus(target, window.element, context, inputPoint: borrowedFocus?.inputPoint)
        }
        try context.deadline.prepare(window.element)
        // Read-only AX preflight is outside admission. The worker's cross-process
        // input owner still covers all focus checks and the ensuing input.
        guard let _: Bool = try dtwWithInputAdmission({
            try context.deadline.check()
            changed = true // Raising itself may have effects even if focus is never proven.
            // Ported CUA exact-window foreground primitive. App-local AX focus
            // after background delivery is not WindowServer activation; AppKit
            // activate() may accept that already-active state without fronting it.
            trace("front_process_requested")
            guard dtwSetFrontProcess(target.app.pid, target.nativeID) else {
                throw failure("focus_failed", "WindowServer did not accept exact foreground activation")
            }
            trace("raise_requested")
            let raised = AXUIElementPerformAction(window.element, kAXRaiseAction as CFString)
            guard raised == .success else { throw failure("focus_failed", "AXRaise did not confirm the requested window") }
            try context.deadline.check()
            guard !dtwInputStopping() else { throw failure("native_stopping", "Input closed after raising the window") }
            guard let app = NSRunningApplication(processIdentifier: target.app.pid),
                  app.activate(options: []) else { throw failure("focus_failed", "Application activation was not accepted") }
            return true
        }) else { throw failure("native_stopping", "Native input admission is closed") }
        // Do not start another poll after 750ms. A poll already in flight keeps
        // the request's absolute deadline and each AX read's 200ms timeout.
        let until = min(context.deadline.uptime, ProcessInfo.processInfo.systemUptime + 0.75)
        let established = try waitForEstablishedFocus(remaining: {
            until - ProcessInfo.processInfo.systemUptime
        }, check: {
            try context.deadline.check()
            guard !dtwInputStopping() else { throw failure("native_stopping", "Native input admission is closing") }
            try context.verify(target.app)
        }, read: {
            let live = try validate(target, context, phase: .postRaise, originalTarget: window.element)
            return try focused(target, live.element, context.deadline) ? live : nil
        }, wait: { interval in
            CFRunLoopRunInMode(.defaultMode, interval, false)
        })
        trace("focus_established")
        if let borrowed = borrowedFocus { focusObserver.established(borrowed.activationEpoch, pid: target.app.pid, window: target.nativeID) }
        return established
    }

    // Only return foreground we still own. An external activation invalidates
    // the epoch even if the user later returns to this same app.
    fileprivate func releaseFocus(_ deadline: Deadline) throws -> [String: Any] {
        if let focusCleanupFailure { throw focusCleanupFailure }
        guard let borrowed = borrowedFocus else { return ["released": true, "focus": "unchanged"] }
        func trace(_ phase: String, _ fields: [String: Any] = [:]) {
            var value = fields
            value["operation"] = "focus_return"; value["phase"] = phase
            value["target_pid"] = borrowed.target.app.pid; value["target_window"] = borrowed.target.nativeID
            value["previous_pid"] = borrowed.previousApp.processIdentifier
            if let id = borrowed.previousWindowID { value["previous_window"] = id }
            traceSemanticFailure(value)
        }
        defer { borrowedFocus = nil; focusObserver.end(borrowed.activationEpoch) }
        trace("begin")
        do {
            guard !focusObserver.invalidated(borrowed.activationEpoch) else {
                trace("changed_externally_before_return")
                return ["released": true, "focus": "changed_externally"]
            }
            let stillOwned: Bool
            if let window = borrowed.window {
                stillOwned = try focusedBeforeEstablishment({ try focused(borrowed.target, window, deadline) })
            } else {
                let frontWindow = try cgWindows(deadline).first { $0.pid == borrowed.target.app.pid && $0.onscreen }
                stillOwned = try windowServerFrontmost(borrowed.target.app.pid, deadline)
                    && frontWindow?.id == borrowed.target.nativeID
            }
            let invalidatedAfterOwnershipRead = focusObserver.invalidated(borrowed.activationEpoch)
            guard stillOwned && !invalidatedAfterOwnershipRead else {
                trace("changed_externally_after_ownership_read", ["still_owned": stillOwned,
                                                               "epoch_invalidated": invalidatedAfterOwnershipRead])
                return ["released": true, "focus": "changed_externally"]
            }
            var outcome = "previous_app_closed"
            if !borrowed.previousApp.isTerminated {
                let context = RequestContext(deadline: deadline)
                func verifyPrevious() throws {
                    guard try context.startStamp(borrowed.previousApp.processIdentifier) == borrowed.previousGeneration else {
                        throw failure("focus_restore_failed", "Previous foreground process identity changed")
                    }
                    if let window = borrowed.previousWindow, let id = borrowed.previousWindowID {
                        guard try axPID(window, deadline) == borrowed.previousApp.processIdentifier,
                              try axWindowID(window, deadline) == id else {
                            throw failure("focus_restore_failed", "Previous foreground window identity changed")
                        }
                    }
                }
                try verifyPrevious()
                guard try focusObserver.retarget(borrowed.activationEpoch, to: borrowed.previousApp.processIdentifier,
                                                window: borrowed.previousWindowID, deadline: deadline) else {
                    trace("changed_externally_during_retarget")
                    return ["released": true, "focus": "changed_externally"]
                }
                if let window = borrowed.previousWindow {
                    guard try axPID(window, deadline) == borrowed.previousApp.processIdentifier else {
                        throw failure("focus_restore_failed", "Previous foreground window no longer belongs to its application")
                    }
                    try deadline.prepare(window)
                    guard !focusObserver.invalidated(borrowed.activationEpoch) else {
                        trace("changed_externally_before_raise")
                        return ["released": true, "focus": "changed_externally"]
                    }
                    trace("raise_requested")
                    guard AXUIElementPerformAction(window, kAXRaiseAction as CFString) == .success else {
                        throw failure("focus_restore_failed", "Previous foreground window could not be restored")
                    }
                }
                guard !focusObserver.invalidated(borrowed.activationEpoch) else {
                    trace("changed_externally_before_activate")
                    return ["released": true, "focus": "changed_externally"]
                }
                trace("activate_requested")
                guard borrowed.previousApp.activate(options: []) else {
                    throw failure("focus_restore_failed", "Previous foreground application could not be restored")
                }
                let restored = try waitForRestoredFocus(remaining: {
                    deadline.uptime - ProcessInfo.processInfo.systemUptime
                }, check: {
                    try deadline.check()
                    if !focusObserver.invalidated(borrowed.activationEpoch) { try verifyPrevious() }
                }, invalidated: {
                    focusObserver.invalidated(borrowed.activationEpoch)
                }, read: {
                    // App-local/system AX focus alone need not prove an actual
                    // WindowServer frontmost transition after directed input.
                    guard try windowServerFrontmost(borrowed.previousApp.processIdentifier, deadline) else {
                        return false
                    }
                    let exact = try focused(borrowed.previousApp.processIdentifier, borrowed.previousWindow,
                                            windowID: borrowed.previousWindowID, deadline)
                    return try exact && windowServerFrontmost(borrowed.previousApp.processIdentifier, deadline)
                }, wait: { interval in
                    CFRunLoopRunInMode(.defaultMode, interval, false)
                })
                guard restored, !focusObserver.invalidated(borrowed.activationEpoch) else {
                    trace("changed_externally_during_readback")
                    return ["released": true, "focus": "changed_externally"]
                }
                outcome = borrowed.previousWindow == nil ? "application_restored" : "restored"
            }
            if let previous = borrowed.pointer, let point = borrowed.inputPoint,
               let current = CGEvent(source: nil)?.location,
               !focusObserver.invalidated(borrowed.activationEpoch),
               abs(current.x - point.x) < 0.5, abs(current.y - point.y) < 0.5 {
                guard CGWarpMouseCursorPosition(previous) == .success,
                      CGAssociateMouseAndMouseCursorPosition(1) == .success else {
                    throw failure("pointer_restore_failed", "Borrowed pointer position could not be restored")
                }
            }
            trace("finished", ["outcome": outcome])
            return ["released": true, "focus": outcome]
        } catch {
            let original = (error as? NativeFailure) ?? failure("focus_restore_failed", String(describing: error))
            trace("failed", ["error": original.code])
            focusCleanupFailure = focusCleanupFailure ?? original
            throw original
        }
    }

    private func windowInput(_ request: [String: Any]) -> [String: Any] {
        var changed = false
        var pendingInput: NativePendingInput?
        func finished(_ result: [String: Any]) -> [String: Any] {
            guard let pendingInput, !dtwFinishInput(pendingInput) else { return result }
            // Keep a primary error if one already exists. The gate retains its
            // cleanup failure and will report an unclean worker shutdown too.
            if result["error"] != nil { return result }
            return ["dispatch": changed ? "unknown" : "not_dispatched", "evidence": "unverified",
                    "error": "input_release_unproven", "message": "Posted input releases were not confirmed within the cleanup budget"]
        }
        do {
            let context = try RequestContext(request)
            let target = try NativeTarget(request["target"])
            let operation = request["operation"] as? String ?? ""
            let text = request["text"] as? String
            var chord: [NativeKeyPair]?
            var keyboardSource: CGEventSource?
            var scroll: (Int32, Int32)?
            if operation == "type_text" {
                guard let text, !text.isEmpty, text.utf8.count <= 16_384 else { throw failure("invalid_text", "text must contain 1..16384 UTF-8 bytes") }
                guard let source = CGEventSource(stateID: .privateState) else { throw failure("input_unavailable", "Cannot create Unicode input source") }
                keyboardSource = source
            } else if operation == "keypress" {
                guard let keys = request["keys"] as? [String], dtwChordPlan(keys) != nil else { throw failure("invalid_keys", "Unsupported chord; use modifiers followed by one named key") }
                guard let source = CGEventSource(stateID: .privateState), let events = dtwChord(keys, source: source) else { throw failure("input_unavailable", "Cannot allocate keyboard events and their releases") }
                keyboardSource = source
                chord = events
            } else if operation == "scroll" {
                func delta(_ name: String) throws -> Int32 {
                    guard let value = dtwWheelSteps(request[name]) else { throw failure("invalid_scroll", "Scroll requires integer wheel steps between -100 and 100") }
                    return value
                }
                scroll = (try delta("delta_x"), try delta("delta_y"))
                guard scroll!.0 != 0 || scroll!.1 != 0 else { throw failure("invalid_scroll", "Scroll requires a nonzero delta") }
            } else if operation != "focus" { throw failure("invalid_request", "Unknown window input operation") }
            let physicalModifiers: CGEventFlags = [.maskCommand, .maskControl, .maskAlternate, .maskShift]
            guard CGEventSource.flagsState(.combinedSessionState).intersection(physicalModifiers).isEmpty else {
                throw failure("input_busy", "Session modifiers are already held; no synthetic input was admitted")
            }
            let window = try establishFocus(target, context, changed: &changed)
            observations = observations.filter { $0.value.target != target }
            func check(focus: Bool = true) throws {
                try context.deadline.check()
                if dtwInputStopping() { throw failure("native_stopping", "Input stopped after releasing admitted keys") }
                if focus {
                    try context.verify(target.app)
                    guard try focused(target, window.element, context.deadline) else {
                        throw failure("focus_changed", "Exact focused window changed before the next input event. Re-observe the target window before continuing; earlier input may have partial effects, so do not replay the entire text or chord")
                    }
                }
            }
            try check()
            if let keyboardSource {
                guard let input = dtwTrackInput(source: keyboardSource, eventTypes: [.keyUp, .flagsChanged]) else {
                    throw failure("native_stopping", "Native input admission is closed")
                }
                pendingInput = input
            }
            if operation == "type_text", let text, let source = keyboardSource, let input = pendingInput {
                for scalar in dtwTextScalars(text) {
                    // No AX read is counted as active input; closing the gate
                    // while preflight blocks refuses the later down entirely.
                    try check()
                    guard let pair = dtwUnicodePair(scalar, source: source) else { throw failure("input_unavailable", "Cannot allocate Unicode key release") }
                    try check(focus: false)
                    guard dtwWithPairedInput(press: {
                        changed = true; dtwPostKey(pair.down, input: input)
                    }, release: { dtwPostKey(pair.up, input: input) }, between: {
                        Thread.sleep(forTimeInterval: 0.003)
                    }) else { throw failure("native_stopping", "Native input admission is closed") }
                }
            } else if operation == "keypress", let chord, let input = pendingInput {
                // The chord is one admitted held-key interval. Its cheap budget
                // checks do not perform AX work; release only the pressed prefix.
                guard let _: Bool = try dtwWithInputAdmission({
                    var pressed: [NativeKeyPair] = []
                    defer {
                        for pair in pressed.reversed() { dtwPostKey(pair.up, input: input) }
                    }
                    for pair in chord {
                        try check(focus: false); changed = true
                        dtwPostKey(pair.down, input: input); pressed.append(pair)
                        Thread.sleep(forTimeInterval: 0.005)
                    }
                    return true
                }) else { throw failure("native_stopping", "Native input admission is closed") }
            } else if let (dx, dy) = scroll {
                let point = CGPoint(x: window.cg.bounds.midX, y: window.cg.bounds.midY)
                guard let source = CGEventSource(stateID: .privateState) else {
                    throw failure("input_unavailable", "Cannot allocate scroll event source")
                }
                guard let proof = dtwProveSurfacePoint(point, id: target.nativeID, pid: target.app.pid,
                                                       bounds: window.cg.bounds, deadline: context.deadline.uptime,
                                                       overlayCandidates: dtwSceneOverlayCandidates(request)) else {
                    try check(focus: false)
                    throw failure("target_occluded", "Scroll center is not a proven visible point of the target window")
                }
                try check()
                guard proof.matchesCurrent() else { throw failure("stale_observation", "Scroll hit stack changed during focus validation") }
                guard let positioned = dtwWithInputAdmission({
                    changed = true
                    guard CGWarpMouseCursorPosition(point) == .success else { return false }
                    dtwPresentPostedPointer(point, kind: 0)
                    return CGAssociateMouseAndMouseCursorPosition(1) == .success
                }) else { throw failure("native_stopping", "Native input admission is closed") }
                guard positioned else { throw failure("input_unavailable", "Cannot position scroll pointer") }
                guard dtwSettleInput(seconds: 0.04, deadline: context.deadline.uptime) else {
                    try check(focus: false)
                    throw failure("deadline_exceeded", "Insufficient budget for pointer routing before scroll")
                }
                try check()
                guard proof.matchesCurrent(requirePointer: true) else { throw failure("stale_observation", "Scroll target, hit stack or pointer changed before dispatch") }
                // Construct the wheel only for the settled pointer. The owned
                // cross-window control loses an early-constructed event even
                // after setting its location; do not reuse or replay that event.
                guard let event = CGEvent(scrollWheelEvent2Source: source, units: .line, wheelCount: 2,
                                          wheel1: -dy, wheel2: -dx, wheel3: 0) else {
                    throw failure("input_unavailable", "Cannot allocate scroll event")
                }
                guard let input = dtwTrackInput(source: source, eventTypes: [.scrollWheel]) else {
                    throw failure("native_stopping", "Native input admission is closed")
                }
                pendingInput = input
                guard let _: Bool = try dtwWithInputAdmission({
                    try check(focus: false)
                    event.location = point; event.flags = []
                    input.posted(eventType: CGEventType.scrollWheel.rawValue)
                    event.post(tap: .cghidEventTap)
                    dtwPresentPostedPointer(point, kind: 3)
                    return true
                }) else { throw failure("native_stopping", "Native input admission is closed") }
            }
            try check(focus: false)
            return finished(["dispatch": "dispatched", "evidence": "unverified"])
        } catch let error as NativeFailure {
            return finished(error.json.merging(["dispatch": changed ? "unknown" : "not_dispatched", "evidence": "unverified"]) { _, new in new })
        } catch {
            return finished(["dispatch": changed ? "unknown" : "not_dispatched", "evidence": "unverified", "error": "native_error", "message": String(describing: error)])
        }
    }

    private func setValue(_ request: [String: Any]) -> [String: Any] {
        var dispatched = false
        do {
            guard let binding = request["binding"] as? String,
                  let observation = observations.removeValue(forKey: binding) else { throw failure("stale_observation", "Native observation was retired or consumed") }
            let context = try RequestContext(request)
            let target = try NativeTarget(request["target"])
            observations = observations.filter { $0.value.target != target }
            guard observation.target == target else { throw failure("target_changed", "Binding belongs to another target") }
            let id = try integer(request["element"], "element")
            guard let value = request["value"] as? String, value.utf8.count <= 16_384 else { throw failure("invalid_value", "value exceeds the supported string limit") }
            guard let element = observation.elements.first(where: { $0.id == id }), element.canSetValue else { throw failure("element_not_settable", "The observed element did not advertise setValue") }
            let window = try validate(target, context)
            let enabled = try axAttribute(element.element, kAXEnabledAttribute, context.deadline) as? Bool == true
            let role = try axAttribute(element.element, kAXRoleAttribute, context.deadline) as? String ?? ""
            guard try settable(element.element, role: role, enabled: enabled, window: window.element, target: target, deadline: context.deadline) else { throw failure("element_not_settable", "Current AXValue writability or ownership changed") }
            try context.deadline.prepare(element.element)
            guard let result = dtwWithInputAdmission({
                dispatched = true
                return AXUIElementSetAttributeValue(element.element, kAXValueAttribute as CFString, value as CFString)
            }) else { throw failure("native_stopping", "Native input admission is closed") }
            try context.deadline.check()
            guard result == .success else { throw failure("ax_action_failed", "AXValue update returned \(result.rawValue); inspect before another action") }
            return ["dispatch": "dispatched", "evidence": "unverified"]
        } catch let error as NativeFailure {
            return error.json.merging(["dispatch": dispatched ? "unknown" : "not_dispatched", "evidence": "unverified"]) { _, new in new }
        } catch { return ["dispatch": dispatched ? "unknown" : "not_dispatched", "evidence": "unverified", "error": "native_error", "message": String(describing: error)] }
    }

    private func press(_ request: [String: Any]) -> [String: Any] {
        do {
            guard let binding = request["binding"] as? String,
                  let observation = observations.removeValue(forKey: binding) else {
                throw failure("stale_observation", "Native observation was retired or consumed")
            }
            let context = try RequestContext(request)
            let target = try NativeTarget(request["target"])
            observations = observations.filter { $0.value.target != target }
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
            if operation == "set_value" { return setValue(request) }
            if ["focus", "type_text", "keypress", "scroll"].contains(operation) { return windowInput(request) }
            let context = try RequestContext(request)
            switch operation {
            case "prepare_foreground":
                let target = try NativeTarget(request["target"])
                _ = try validateSurface(target, context)
                let window = try? validate(target, context)
                let point: CGPoint?
                if let x = request["screen_x"] as? Double, let y = request["screen_y"] as? Double {
                    point = CGPoint(x: x, y: y)
                } else { point = nil }
                try borrowFocus(target, window?.element, context, inputPoint: point)
                return ["prepared": true, "borrowed": borrowedFocus != nil]
            case "release_focus": return try releaseFocus(context.deadline)
            case "validate_background":
                let target = try NativeTarget(request["target"])
                let milliseconds = try integer(request["deadline_unix_ms"], "deadline_unix_ms")
                let keyboard = request["keyboard"] as? Bool == true
                if request["step"] as? Bool == true {
                    guard let validation = backgroundValidation,
                          validation.matches(target: target, deadlineMilliseconds: milliseconds, keyboard: keyboard) else {
                        throw failure("invalid_request", "Background input step has no matching admitted action")
                    }
                    let current = try validateSurface(target, validation.context)
                    try validation.checkModalState { _ = try self.validateSurface(target, validation.context) }
                    try validation.context.verify(target.app)
                    return ["target": target.json, "bounds": current.coordinates]
                }
                backgroundValidation = nil
                let current = try validateSurface(target, context)
                let window = keyboard ? try validate(target, context).element : nil
                var result: [String: Any] = ["target": target.json, "bounds": current.coordinates]
                if request["consume"] as? Bool == true {
                    let validation = BackgroundInputValidation(target: target, deadlineMilliseconds: milliseconds,
                        keyboard: keyboard, context: context, window: window)
                    try validation.checkModalState { _ = try self.validateSurface(target, validation.context) }
                    observations = observations.filter { $0.value.target != target }
                    guard let owner = try context.app(target.app.pid), owner.app == target.app else {
                        throw failure("app_changed", "Native app identity changed during input route selection")
                    }
                    let frameworks = URL(fileURLWithPath: owner.metadata.path).appendingPathComponent("Contents/Frameworks")
                    let known = ["Electron Framework.framework", "Chromium Framework.framework",
                                 "Google Chrome Framework.framework", "Google Chrome Beta Framework.framework",
                                 "Google Chrome Dev Framework.framework", "Google Chrome Canary Framework.framework",
                                 "Microsoft Edge Framework.framework", "Brave Browser Framework.framework"]
                    result["chromium"] = known.contains { name in
                        var directory: ObjCBool = false
                        return FileManager.default.fileExists(atPath: frameworks.appendingPathComponent(name).path,
                                                              isDirectory: &directory) && directory.boolValue
                    }
                    try context.verify(target.app)
                    backgroundValidation = validation
                }
                return result
            case "validate_input_owner":
                let app = try NativeApp(request["app"])
                try context.verify(app)
                return ["app": app.json]
            case "catalog": return try context.catalog()
            case "windows": return try listWindows(NativeApp(request["app"]), context)
            case "observe": return try observe(request, context)
            case "observe_next": return try observeNext(request, context)
            case "validate":
                let target = try NativeTarget(request["target"])
                let current = try validateSurface(target, context)
                return ["target": target.json, "bounds": current.coordinates]
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

// Read-only ownership lookup. Callers take instance -> physical leases before
// restoring foreground; no-borrow callers can still query sticky cleanup failure.
@_cdecl("dtw_focus_owner")
public func dtwFocusOwner(_ timeoutMilliseconds: UInt32, _ pid: UnsafeMutablePointer<UInt32>?,
                          _ generation: UnsafeMutablePointer<UInt64>?) -> Int32 {
    let until = ProcessInfo.processInfo.systemUptime + Double(timeoutMilliseconds) / 1_000
    while !nativeStore.lock.try() {
        if ProcessInfo.processInfo.systemUptime >= until { return -1 }
        Thread.sleep(forTimeInterval: 0.002)
    }
    defer { nativeStore.lock.unlock() }
    guard let owner = nativeStore.focusOwner else { return 0 }
    guard let pid, let generation else { return -1 }
    pid.pointee = UInt32(owner.pid); generation.pointee = owner.generation
    return 1
}

@_cdecl("dtw_focus_quiesce")
public func dtwFocusQuiesce(_ timeoutMilliseconds: UInt32) -> Bool {
    let until = ProcessInfo.processInfo.systemUptime + Double(timeoutMilliseconds) / 1_000
    while !nativeStore.lock.try() {
        if ProcessInfo.processInfo.systemUptime >= until { return false }
        Thread.sleep(forTimeInterval: 0.005)
    }
    defer { nativeStore.lock.unlock() }
    do {
        if nativeStore.focusOwner == nil { return nativeStore.focusCleanupFailure == nil }
        let left = until - ProcessInfo.processInfo.systemUptime
        guard left > 0.001 else { return false }
        let deadline = try Deadline(["deadline_unix_ms": UInt64((Date().timeIntervalSince1970 + left) * 1_000)]).limited(to: until)
        _ = try nativeStore.releaseFocus(deadline)
        return true
    } catch { return false }
}

// Scene.swift calls this immediately before its own admitted pixel dispatch.
// Every pixel operation requires the same typed target as the semantic provider.
func dtwValidatePixelTarget(_ request: [String: Any]) -> [String: Any]? {
    nativeStore.lock.lock()
    defer { nativeStore.lock.unlock() }
    do {
        let context = try RequestContext(request)
        _ = try nativeStore.validateSurface(NativeTarget(request["target"]), context)
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
            var reply = nativeStore.call(request)
            if let cleanup = nativeStore.focusCleanupFailure { reply["cleanup_error"] = cleanup.json }
            result = reply
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
