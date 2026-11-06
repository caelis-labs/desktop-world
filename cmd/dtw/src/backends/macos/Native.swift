// Narrow source port from Cua Driver 0.38.0, commit
// 85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19. Copyright (c) 2025 Cua AI, Inc.
// MIT; full source mapping and retained license: docs/native-source-provenance.md.
// The worker owns this registry. No CUA ToolRegistry, focus or input fallback.
import Foundation
import ApplicationServices
import AppKit
import Darwin

// NativeBridge.h imports the Rust exports with their C ABI in both the product
// build and pure Swift controls. Keep the existing Swift-facing names local.
private func dtwSetFrontProcess(_ pid: Int32, _ window: UInt32) -> Bool { dtw_set_front_process(pid, window) }
private func dtwSetVisualFrontProcess(_ pid: Int32, _ window: UInt32) -> Bool { dtw_set_visual_front_process(pid, window) }
private func dtwFrontProcessMatches(_ pid: Int32) -> Int32 { dtw_front_process_matches(pid) }
private func dtwFrontProcessPID() -> Int32 { dtw_front_process_pid() }

private struct NativeFailure: Error {
    let code: String
    let message: String
    let details: [String: Any]?
    let attributeFailure: AXAttributeFailure?
    let focusAbsence: FocusAbsence?
    // Private evidence only: a fresh complete parent inventory no longer
    // contains a non-target AX object read from the previous snapshot.
    let modalSnapshotExpired: Bool
    // A successful System AX application snapshot contradicts the separately
    // queried WindowServer front process. This is not a PID/birth substitution.
    let foregroundSnapshotMismatch: Bool

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
                     modalSnapshotExpired: Bool = false,
                     foregroundSnapshotMismatch: Bool = false) -> NativeFailure {
    NativeFailure(code: code, message: message, details: details,
                  attributeFailure: attributeFailure, focusAbsence: focusAbsence,
                  modalSnapshotExpired: modalSnapshotExpired,
                  foregroundSnapshotMismatch: foregroundSnapshotMismatch)
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
            throw failure("ax_unavailable", "Cannot set AX messaging deadline: \(result.rawValue)",
                          attributeFailure: AXAttributeFailure(name: "AXMessagingTimeout", error: result))
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

private final class ApplicationOpenCompletion: @unchecked Sendable {
    private let lock = NSLock()
    private var value: (NSRunningApplication?, Error?)?
    func set(_ application: NSRunningApplication?, _ error: Error?) {
        lock.lock(); defer { lock.unlock() }; value = (application, error)
    }
    var finished: Bool { lock.lock(); defer { lock.unlock() }; return value != nil }
    var application: NSRunningApplication? { lock.lock(); defer { lock.unlock() }; return value?.0 }
    var error: Error? { lock.lock(); defer { lock.unlock() }; return value?.1 }
}

private struct BundleMetadata {
    let path: String
    let identifier: String
    let name: String
    let uiElement: Bool
    let backgroundOnly: Bool
}

// Discovery policy is separate from process identity. In particular, a user
// agent with a window must pass the same identity checks as an ordinary app.
private func catalogIncludes(_ bundle: BundleMetadata, knownRoot: Bool, hasSurface: Bool) -> Bool {
    guard !bundle.backgroundOnly, !bundle.path.contains(".app/Contents/") else { return false }
    if bundle.uiElement { return hasSurface }
    return knownRoot || bundle.identifier == "com.apple.finder" || hasSurface
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

private func isExitedKernelProcess(_ info: kinfo_proc, pid: Int32) -> Bool {
    // A zombie cannot be any live incarnation of this PID. A missing or
    // unreadable executable, AX root or process record is never this proof.
    pid > 0 && info.kp_proc.p_pid == pid && info.kp_proc.p_stat == Int8(SZOMB)
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
                                    uiElement: flag("LSUIElement"), backgroundOnly: flag("LSBackgroundOnly"))
        metadata[path] = result
        return result
    }

    func app(_ pid: Int32) throws -> KernelApp? {
        guard let before = try startStamp(pid, stage: "stamp_before"), let path = try executable(pid),
              let bundle = try bundle(path, pid: pid) else { return nil }
        traceCatalog(pid, "bundle_classification", ["ui_element": bundle.uiElement,
                                                    "background_only": bundle.backgroundOnly])
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
        // libproc omits unreaped exited children, although kill(pid, 0) still
        // succeeds. KERN_PROC_PID retains their explicit SZOMB status. Read
        // failure or a short/empty record remains unproven, not "exited".
        try deadline.check()
        var info = kinfo_proc()
        let size = MemoryLayout<kinfo_proc>.size
        var length = size
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, expected.pid]
        let result = sysctl(&mib, UInt32(mib.count), &info, &length, nil, 0)
        try deadline.check()
        if result == 0, length == size, isExitedKernelProcess(info, pid: expected.pid) { return true }
        // An unreadable/permission-denied process is not proof of death.
        return kill(expected.pid, 0) == -1 && errno == ESRCH
    }

    func runningApplications(_ identifier: String, path: String? = nil) throws -> [KernelApp] {
        try deadline.check()
        let count = proc_listallpids(nil, 0)
        guard count > 0, count < 1_000_000 else { throw failure("catalog_unavailable", "Cannot enumerate application instances") }
        var pids = [Int32](repeating: 0, count: Int(count) + 64)
        let written = pids.withUnsafeMutableBytes { proc_listallpids($0.baseAddress, Int32($0.count)) }
        guard written > 0, written < pids.count else { throw failure("catalog_unavailable", "Application enumeration changed beyond capacity") }
        var matches: [KernelApp] = []
        for pid in Set(pids.prefix(Int(written))).sorted() where pid > 0 {
            try deadline.check()
            guard let candidate = try app(pid), candidate.metadata.identifier == identifier,
                  !candidate.metadata.backgroundOnly, !candidate.metadata.path.contains(".app/Contents/"),
                  path == nil || candidate.metadata.path == path else { continue }
            matches.append(candidate)
        }
        return matches
    }

    func resolveApplication(_ identifier: String) throws -> (BundleMetadata, [KernelApp]) {
        let explicitPath = identifier.hasPrefix("/")
        let url: URL
        if explicitPath {
            url = URL(fileURLWithPath: identifier).resolvingSymlinksInPath().standardizedFileURL
        } else {
            let running = try runningApplications(identifier)
            if let first = running.first { return (first.metadata, running) }
            let candidates = Set(NSWorkspace.shared.urlsForApplications(withBundleIdentifier: identifier)
                .map { $0.resolvingSymlinksInPath().standardizedFileURL })
                .filter { !$0.path.contains(".app/Contents/") }
            guard candidates.count == 1, let candidate = candidates.first else {
                throw failure(candidates.isEmpty ? "app_unavailable" : "ambiguous_app",
                              "Resolve one installed application by bundle ID or an explicit .app path")
            }
            url = candidate
        }
        guard url.pathExtension == "app", let installed = Bundle(url: url),
              let executable = installed.executableURL,
              let metadata = try bundle(executable.resolvingSymlinksInPath().path, pid: 0),
              metadata.path == url.path, !metadata.backgroundOnly,
              !metadata.path.contains(".app/Contents/"),
              explicitPath || metadata.identifier == identifier else {
            throw failure("app_unavailable", "Target is not an installed user application bundle")
        }
        return (metadata, try runningApplications(metadata.identifier, path: explicitPath ? metadata.path : nil))
    }

    func applicationTarget(_ identifier: String) throws -> [String: Any] {
        let (metadata, running) = try resolveApplication(identifier)
        return ["app_id": identifier, "name": metadata.name, "bundle_path": metadata.path,
                "apps": running.map(\.json)]
    }

    func discoverApplications() throws -> [String: Any] {
        let inventory = try catalog()
        let roots = ["/Applications", "/System/Applications", "/System/Library/CoreServices/Applications",
                     "/Users/Shared/Applications", NSHomeDirectory() + "/Applications"]
        var found: [String: BundleMetadata] = [:]
        var unreadable = 0
        for path in roots {
            try deadline.check()
            var directory: ObjCBool = false
            guard FileManager.default.fileExists(atPath: path, isDirectory: &directory), directory.boolValue else { continue }
            guard let enumerator = FileManager.default.enumerator(at: URL(fileURLWithPath: path),
                includingPropertiesForKeys: nil, options: [.skipsHiddenFiles, .skipsPackageDescendants],
                errorHandler: { _, _ in unreadable += 1; return true }) else { unreadable += 1; continue }
            var visits = 0
            for case let url as URL in enumerator {
                try deadline.check()
                visits += 1
                guard visits <= 20_000 else { unreadable += 1; break }
                if enumerator.level > 5 { enumerator.skipDescendants(); continue }
                guard url.pathExtension.lowercased() == "app" else { continue }
                enumerator.skipDescendants()
                guard let executable = Bundle(url: url)?.executableURL,
                      let entry = try bundle(executable.resolvingSymlinksInPath().path, pid: 0),
                      !entry.backgroundOnly, !entry.path.contains(".app/Contents/") else { continue }
                found[entry.path] = entry
            }
        }
        let rows = Self.discoveredApplicationRows(installed: found, inventory: inventory, metadata: metadata)
        return ["apps": rows, "coverage": ["scope":"installed_application_folders_and_running_user_bundles",
                "complete":false, "installed":true, "unreadable":unreadable,
                "excludes":["unregistered_locations_not_running", "nested_helpers", "background_only_bundles", "unreadable_bundles"]]]
    }

    static func discoveredApplicationRows(installed: [String: BundleMetadata], inventory: [String: Any],
                                          metadata: [String: BundleMetadata]) -> [[String: Any]] {
        let running = inventory["apps"] as? [[String: Any]] ?? []
        let live = inventory["running_bundles"] as? [[String: Any]] ?? running
        var found = installed
        // Default catalog targets omit windowless agents; discovery still lists
        // their proven running bundles, including those outside installed roots.
        for row in live {
            if let path = row["bundle_path"] as? String, let entry = metadata[path] { found[path] = entry }
        }
        let counts = Dictionary(grouping: Array(found.values), by: \.identifier).mapValues(\.count)
        return found.values.sorted { ($0.name, $0.path) < ($1.name, $1.path) }.map { entry in
            let instances = live.filter { $0["bundle_path"] as? String == entry.path }.count
            return ["app": counts[entry.identifier] == 1 ? entry.identifier : entry.path,
                    "name": entry.name, "running": instances > 0, "instances": instances]
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
        var liveBundles: [KernelApp] = []
        let enumeratedPIDs = Set(pids.prefix(Int(written)))
        for pid in tracedPIDs.sorted() {
            traceCatalog(pid, "process_table", ["enumerated": enumeratedPIDs.contains(pid),
                                                 "return_value": written, "size": pids.count])
        }
        for pid in enumeratedPIDs.sorted() where pid > 0 {
            try deadline.check()
            guard let candidate = try app(pid) else { continue }
            if !candidate.metadata.backgroundOnly && !candidate.metadata.path.contains(".app/Contents/") { liveBundles.append(candidate) }
            let knownRoot = roots.contains { candidate.metadata.path.hasPrefix($0) }
            let included = catalogIncludes(candidate.metadata, knownRoot: knownRoot,
                                           hasSurface: windowed.contains(pid))
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
        return ["apps": apps.map(\.json), "running_bundles": liveBundles.map(\.json), "permissions": dtwPermissionStatus(), "coverage": [
            "scope": "kernel_running_user_app_bundles", "complete": false,
            "excludes": ["unreadable_processes", "background_only_bundles", "nested_helpers",
                         "windowless_ui_agents", "non_bundle_executables"],
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

// These rows must come from optionOnScreenOnly, whose order is documented.
// Existing foreground overlays are preserved; only a target window newly
// crossing the exact original window can require a return.
private func targetWindowsAbove(_ rows: [CGRecord], original: CGWindowID,
                                owner: Int32, target: Int32) -> Set<CGWindowID>? {
    guard let index = rows.firstIndex(where: { $0.id == original && $0.pid == owner }) else { return nil }
    return Set(rows.prefix(index).filter { $0.pid == target }.map(\.id))
}

private func originalAboveOtherApps(_ rows: [CGRecord], original: CGWindowID, owner: Int32) -> Bool {
    guard let index = rows.firstIndex(where: { $0.id == original && $0.pid == owner }) else { return false }
    return rows.prefix(index).allSatisfy { $0.pid == owner }
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

// A scene worker binds one already-selected CG surface. It must not infer an
// empty app inventory or consult unrelated AX/SCK surfaces to initialize it.
private func selectedSurfaceRecord(_ rows: [[String: Any]], id: CGWindowID) throws -> CGRecord {
    guard !rows.isEmpty else { throw failure("target_changed", "Selected window surface is no longer available") }
    guard rows.count == 1, let record = cgRecord(rows[0]) else {
        throw failure("window_unproven", "Selected surface metadata is incomplete or duplicated")
    }
    guard record.id == id else { throw failure("target_changed", "Exact lookup returned another window surface") }
    return record
}

private func selectedSurfaceCandidate<Root>(_ rows: [[String: Any]], id: CGWindowID, pid: Int32,
    birth: (Int32) throws -> UInt64?, associate: (CGRecord) throws -> Root?,
    refresh: (CGWindowID) throws -> [[String: Any]]) throws
    -> (record: CGRecord, title: String, association: (birth: UInt64, root: Root)?) {
    let record = try selectedSurfaceRecord(rows, id: id)
    let title = String((rows[0][kCGWindowName as String] as? String ?? "").prefix(512))
    // The caller verifies the logical app's birth before and after this read.
    // Same-PID backing surfaces require no AX availability/foreign attribution.
    if record.pid == pid { return (record, title, nil) }
    guard let before = try birth(record.pid) else {
        throw failure("stale_window", "Selected surface owner identity is unavailable")
    }
    guard let root = try associate(record) else {
        throw failure("stale_window", "Selected surface has no current proven association to this app")
    }
    let live = try selectedSurfaceRecord(refresh(id), id: id)
    guard live.pid == record.pid, live.bounds == record.bounds, live.onscreen == record.onscreen else {
        throw failure("target_changed", "Selected associated surface changed during attribution")
    }
    guard try birth(record.pid) == before else {
        throw failure("stale_window", "Selected surface owner process changed during attribution")
    }
    return (live, title, (before, root))
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
    var record = fields
    record["worker_pid"] = getpid()
    record["main_thread"] = Thread.isMainThread
    record["uptime"] = ProcessInfo.processInfo.systemUptime
    if let data = try? JSONSerialization.data(withJSONObject: record, options: [.sortedKeys]) {
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
    guard result == .success else {
        throw failure("ax_unavailable", "AX process identity unavailable: \(result.rawValue)",
                      attributeFailure: AXAttributeFailure(name: "AXPID", error: result))
    }
    guard pid > 0 else { throw failure("ax_unavailable", "AX process identity unavailable") }
    return pid
}

// System AX can lack its global focused-application attribute while a live
// app still publishes an exact focused window. Only these typed global failures
// permit an independently proven WindowServer candidate, never a window guess.
private func systemFocusValue<Element>(_ result: AXError, element: Element?) throws -> Element {
    guard result == .success else {
        throw failure(result == .noValue ? "focus_unproven" : "ax_unavailable",
                      "System AX focused application failed: \(result.rawValue)",
                      details: ["attribute": kAXFocusedApplicationAttribute, "ax_error": result.rawValue],
                      attributeFailure: AXAttributeFailure(name: kAXFocusedApplicationAttribute, error: result),
                      focusAbsence: result == .noValue ? .application : nil)
    }
    guard let element else {
        throw failure("focus_unproven", "System AX focused application is not an AX application object")
    }
    return element
}

private struct FocusApplication<Element> {
    let element: Element
    let pid: Int32
    let generation: UInt64
    let fromCandidate: Bool
}

private func resolveFocusedApplication<Element>(system: () throws -> Element,
    candidate: () throws -> Element?, pid: (Element) throws -> Int32,
    birth: (Int32) throws -> UInt64?, frontmost: (Int32) throws -> Bool) throws -> FocusApplication<Element>? {
    let element: Element
    let fromCandidate: Bool
    do { element = try system(); fromCandidate = false }
    catch let error as NativeFailure where error.attributeFailure?.name == kAXFocusedApplicationAttribute
        && (error.attributeFailure?.error == .cannotComplete || error.attributeFailure?.error == .noValue) {
        guard let proposed = try candidate() else { throw error }
        element = proposed; fromCandidate = true
    }
    let process = try pid(element)
    guard process > 0, let generation = try birth(process), generation > 0 else {
        throw failure("focus_unproven", "Focused process identity is unavailable")
    }
    guard try frontmost(process) else {
        // A known target's negative PSN proof means it is not frontmost. An AX
        // success contradicting WindowServer must never select another source.
        if fromCandidate { return nil }
        throw failure("focus_unproven", "System AX and WindowServer disagree about the current foreground application",
                      details:["ax_pid":process], foregroundSnapshotMismatch:true)
    }
    guard try birth(process) == generation else {
        throw failure("focus_unproven", "Focused process identity changed during validation")
    }
    return FocusApplication(element: element, pid: process, generation: generation, fromCandidate: fromCandidate)
}

// Recheck app identity around the potentially blocking exact-window query.
// A candidate proven not frontmost is false; unreadable/malformed evidence keeps
// its error. Neither a sibling window nor a reused PID can authorize input.
private func exactInputFocus<Element>(expectedPID: Int32,
    focusedApplication: () throws -> FocusApplication<Element>?,
    windowMatches: (Element) throws -> Bool) throws -> Bool {
    guard let before = try focusedApplication(), before.pid == expectedPID else { return false }
    guard try windowMatches(before.element) else { return false }
    guard let after = try focusedApplication(), after.pid == expectedPID else { return false }
    guard after.generation == before.generation else {
        throw failure("focus_unproven", "Focused process identity changed during the window query")
    }
    return true
}

// The operation scope owns a logical window, not the lifetime of its old AX root.
// Replacing that root after an input (for example closing a document tab) does
// not make an unreadable old object evidence about today's foreground. This
// helper intentionally receives no retained AX object to query or rebind.
private func borrowedFocusStillOwned<Element>(app: NativeApp, windowID: CGWindowID,
    invalidated: () -> Bool, focusedApplication: () throws -> FocusApplication<Element>?,
    currentWindowID: (Element) throws -> CGWindowID?) throws -> Bool {
    guard !invalidated() else { return false }
    let owned: Bool
    do {
        owned = try exactInputFocus(expectedPID: app.pid, focusedApplication: {
            guard let current = try focusedApplication(), current.generation == app.generation else { return nil }
            return current
        }, windowMatches: { try currentWindowID($0) == windowID })
    } catch {
        // A proven external choice ends this operation scope;
        // otherwise a failed current read remains unresolved, never "closed".
        if invalidated() { return false }
        throw error
    }
    return owned && !invalidated()
}

// Publish a continued borrow only after both current ownership and observer
// retarget succeed. Errors leave the prior operation scope intact;
// only a definite false result lets the caller end that old borrow.
private func retargetFocusBorrow<Borrow>(_ prior: Borrow, owned: () throws -> Bool,
    retarget: () throws -> Bool, update: (inout Borrow) -> Void) throws -> Borrow? {
    guard try owned(), try retarget() else { return nil }
    var next = prior
    update(&next)
    return next
}

// A sheet may be the exact AXFocusedWindow even though WindowServer activation
// must use its parent AXWindow. Never normalize the focus proof to that parent.
private func exactFocusedSurface<Element>(requested: Element, requestedWindow: CGWindowID,
                                          focused: Element, equal: (Element, Element) -> Bool,
                                          windowID: (Element) throws -> CGWindowID) throws -> Bool {
    try equal(requested, focused) && windowID(focused) == requestedWindow
}

// Background menus may own accessibility focus while keyboard foreground stays
// with the user. Passive AXRaise restores only order, so use the real front
// process and that app's exact local focused window, not global AX focus. This
// proof grants no keyboard/pointer input or activation authority.
private func passiveOrderOwner<Element>(requested: Element, requestedWindow: CGWindowID,
    frontmost: () throws -> Bool, focusedWindow: () throws -> Element?,
    equal: (Element, Element) -> Bool, windowID: (Element) throws -> CGWindowID) throws -> Bool {
    guard try frontmost(), let current = try focusedWindow(),
          try exactFocusedSurface(requested: requested, requestedWindow: requestedWindow,
              focused: current, equal: equal, windowID: windowID) else { return false }
    return try frontmost()
}

private func resolveActivationWindow<Element>(_ surface: Element, pid: Int32,
                                              owner: (Element) throws -> Int32,
                                              role: (Element) throws -> String?,
                                              parent: (Element) throws -> Element?) throws -> Element? {
    var current = surface
    for _ in 0..<40 {
        guard try owner(current) == pid else { return nil }
        let currentRole = try role(current)
        if currentRole == kAXWindowRole { return current }
        if currentRole == kAXApplicationRole { return nil }
        guard let next = try parent(current) else { return nil }
        current = next
    }
    return nil
}

// Main-window changes may refer to the sheet's parent while the keyboard still
// belongs to the sheet. Only focused-window notifications name a focus change;
// other notifications cause a fresh sample instead of granting/invalidating it.
private func notificationNamesFocusedWindow(_ notification: String) -> Bool {
    notification == kAXFocusedWindowChangedNotification
}

// A script can follow a sheet or popup only through retained object ancestry.
// A sibling application window is an external choice, even with the same PID.
private func executionFocusFamilyMatches<Element>(_ surface: Element, anchor: Element, pid: Int32,
    equal: (Element, Element) -> Bool, owner: (Element) throws -> Int32,
    parent: (Element) throws -> Element?) throws -> Bool {
    guard try owner(surface) == pid, try owner(anchor) == pid else { return false }
    return try boundedAncestor(surface, is: anchor, equal: equal) { element in
        guard try owner(element) == pid else { return nil }
        return try parent(element)
    }
}

private func executionRestoreIdentityCurrent(pid: Int32, generation: UInt64,
    birth: (Int32) throws -> UInt64?, windowMatches: () throws -> Bool) throws -> Bool {
    guard try birth(pid) == generation, try windowMatches(), try birth(pid) == generation else { return false }
    return true
}

// Only a positive lifecycle postcondition can retire an execution anchor.
// Closing a descendant sheet does not terminate its logical root scope.
private func confirmedExecutionTerminal(operation: String, evidence: String?, postcondition: String?,
    sameApp: Bool, closedWindow: CGWindowID?, anchorWindow: CGWindowID) -> String? {
    guard sameApp, evidence == "confirmed" else { return nil }
    if operation == "quit_app", postcondition == "application_exited" { return postcondition }
    if operation == "close_window", postcondition == "window_closed", closedWindow == anchorWindow { return postcondition }
    return nil
}

private func settledExecutionTerminal(_ terminal: String, originalCurrent: () throws -> Bool) -> [String: Any] {
    do {
        let restored = try originalCurrent()
        return ["released": true, "focus": restored ? "restored" : "changed_externally", "terminal": terminal,
                "terminal_focus": restored ? "original_restored" : "not_original"]
    } catch {
        // Confirmed closure is already terminal. Unknown current focus grants
        // neither activation nor a claim that cleanup failed on the dead root.
        return ["released": true, "focus": "changed_externally", "terminal": terminal,
                "terminal_focus": "unproven", "terminal_focus_error": (error as? NativeFailure)?.code ?? "native_error"]
    }
}

private func coordinateMenuSurface(role: String?, associated: Bool) -> Bool {
    associated && role == kAXMenuRole
}

// Modal keyboard delivery is not proven by a matching key window. Keep this
// restriction distinct from permission to activate; the worker rejects the
// unproven route before preparation. Pointer input does not use this gate.
private func modalInputRequiresForeground(role: String?, modal: () throws -> Bool?) throws -> Bool {
    if role == kAXSheetRole { return true }
    guard role == kAXWindowRole else { return false }
    guard let value = try modal() else { throw failure("modal_state_unproven", "Input routing requires a typed AXModal state") }
    return value
}

// NSOpenPanel embeds an NSRemoteView. Addressed events reach its host window
// but do not invoke the remote controls, even when the host is already active.
// This exact native identifier/role is established by the real document probe;
// ordinary modal windows, unknown AX and other frameworks keep directed input.
private func nativeOpenPanelClick(role: String?, identifier: String?) -> Bool {
    [kAXWindowRole, kAXSheetRole].contains(role ?? "") && identifier == "open-panel"
}

private func backgroundRequiresForeground(_ window: AXUIElement?, deadline: Deadline) throws -> Bool {
    guard let window else { return false }
    let role = try axAttribute(window, kAXRoleAttribute, deadline) as? String
    return try modalInputRequiresForeground(role: role) {
        guard let value = try axAttribute(window, kAXModalAttribute, deadline),
              CFGetTypeID(value) == CFBooleanGetTypeID() else { return nil }
        return value as? Bool
    }
}

// Native dialogs remain keyboard receivers even while a background app's
// AXFocusedWindow still names its document. This is routing proof, not a grant
// to activate the application or a claim that background delivery cannot work.
private func nativeModalKeyboardReceiver(_ window: AXUIElement?, deadline: Deadline) throws -> Bool {
    guard let window else { return false }
    if try backgroundRequiresForeground(window, deadline: deadline) { return true }
    return try nativeOpenPanelClick(
        role: axAttribute(window, kAXRoleAttribute, deadline) as? String,
        identifier: axAttribute(window, kAXIdentifierAttribute, deadline) as? String)
}

// A physical pointer requirement does not itself grant activation. Recheck at
// actual focus establishment as well as preparation; unreadable proof propagates
// without entering the activation branch.
private func activationRequired(allowed: Bool, isForeground: () throws -> Bool) throws -> Bool {
    if try isForeground() { return false }
    guard allowed else {
        throw failure("foreground_required", "This input has no proven capability to activate an inactive application")
    }
    return true
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

private func frontProcessPID(_ result: Int32) throws -> Int32 {
    guard result > 0 else {
        throw failure("focus_unproven", "WindowServer foreground PID is unavailable or changed during lookup",
                      details:["native_result":result])
    }
    return result
}

private func foregroundCandidatePID(expectedPID: Int32?, fresh: () throws -> Int32) throws -> Int32 {
    if let expectedPID { return expectedPID }
    return try fresh()
}

private func windowServerForegroundPID(_ deadline: Deadline) throws -> Int32 {
    try deadline.check()
    let result = dtwFrontProcessPID()
    try deadline.check()
    if ProcessInfo.processInfo.environment["DTW_AX_READINESS_TRACE"] == "1" {
        // Diagnostic comparison only. A normal lookup never reads this cache,
        // and even a matching cached candidate cannot influence admission.
        let cached = NSWorkspace.shared.frontmostApplication?.processIdentifier
        traceSemanticFailure(["operation":"foreground_pid_lookup", "native_result":result,
                              "windowserver_pid":result > 0 ? result as Any : NSNull(),
                              "workspace_candidate_pid":cached as Any? ?? NSNull(),
                              "stable_psn_roundtrip":result > 0])
    }
    return try frontProcessPID(result)
}

private func nativeFocusedApplication(expectedPID: Int32? = nil,
                                      deadline: Deadline, traceStage: String? = nil) throws -> FocusApplication<AXUIElement>? {
    let context = RequestContext(deadline: deadline)
    let result: FocusApplication<AXUIElement>?
    do { result = try resolveFocusedApplication(system: {
        let system = AXUIElementCreateSystemWide()
        try deadline.prepare(system)
        var value: CFTypeRef?
        let started = ProcessInfo.processInfo.systemUptime
        let result = AXUIElementCopyAttributeValue(system, kAXFocusedApplicationAttribute as CFString, &value)
        if let traceStage {
            traceSemanticFailure(["operation":"focus_read_returned", "stage":traceStage,
                                  "attribute":kAXFocusedApplicationAttribute, "ax_error":result.rawValue,
                                  "elapsed_ms":(ProcessInfo.processInfo.systemUptime - started) * 1_000,
                                  "remaining_ms":(deadline.uptime - ProcessInfo.processInfo.systemUptime) * 1_000])
        }
        try deadline.check()
        if result != .success {
            traceSemanticFailure(["operation":"system_focus_read", "ax_error":result.rawValue])
        }
        return try systemFocusValue(result, element: axElement(value))
    }, candidate: {
        // Unknown prior focus uses fresh WindowServer identity, never the
        // resident AppKit cache. Known targets still use the exact requested
        // PID so a negative PSN proof means a real loss of target ownership.
        let pid = try foregroundCandidatePID(expectedPID: expectedPID,
            fresh: { try windowServerForegroundPID(deadline) })
        guard pid > 0 else { return nil }
        return AXUIElementCreateApplication(pid)
    }, pid: { try axPID($0, deadline) }, birth: { try context.startStamp($0) },
       frontmost: { try windowServerFrontmost($0, deadline) })
    } catch let error as NativeFailure where error.foregroundSnapshotMismatch {
        if ProcessInfo.processInfo.environment["DTW_AX_READINESS_TRACE"] == "1",
           ProcessInfo.processInfo.systemUptime < deadline.uptime {
            // Additional read-only diagnosis cannot choose a replacement source
            // or change the already-established mismatch outcome.
            let actual = dtwFrontProcessPID()
            traceSemanticFailure(["operation":"foreground_snapshot_mismatch",
                "ax_pid":error.details?["ax_pid"] ?? NSNull(), "windowserver_result":actual])
        }
        throw error
    }
    try deadline.check()
    return result
}

private func focusedApplicationStillCurrent<Element>(_ app: FocusApplication<Element>,
    birth: (Int32) throws -> UInt64?, frontmost: (Int32) throws -> Bool) throws -> Bool {
    try birth(app.pid) == app.generation && frontmost(app.pid) && birth(app.pid) == app.generation
}

private func focusedWindowValue<Element>(_ result: AXError, element: Element?) throws -> Element? {
    if result == .noValue { return nil }
    guard result == .success else {
        throw failure("ax_unavailable", "AX attribute \(kAXFocusedWindowAttribute) failed: \(result.rawValue)",
                      details: ["attribute":kAXFocusedWindowAttribute, "ax_error":result.rawValue],
                      attributeFailure: AXAttributeFailure(name: kAXFocusedWindowAttribute, error: result))
    }
    guard let element else { throw failure("focus_unproven", "Focused window is not an AX window object") }
    return element
}

private func nativeFocusedWindow(_ app: AXUIElement, pid: Int32,
                                  deadline: Deadline, traceStage: String? = nil) throws -> AXUIElement? {
    try deadline.prepare(app)
    let started = ProcessInfo.processInfo.systemUptime
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(app, kAXFocusedWindowAttribute as CFString, &value)
    if let traceStage {
        traceSemanticFailure(["operation":"focus_read_returned", "stage":traceStage,
                              "attribute":kAXFocusedWindowAttribute, "ax_error":result.rawValue,
                              "elapsed_ms":(ProcessInfo.processInfo.systemUptime - started) * 1_000,
                              "remaining_ms":(deadline.uptime - ProcessInfo.processInfo.systemUptime) * 1_000])
    }
    try deadline.check()
    if result != .success && result != .noValue {
        traceSemanticFailure(["operation":"attribute", "attribute":kAXFocusedWindowAttribute,
                              "ax_error":result.rawValue,
                              "elapsed_ms":Int((ProcessInfo.processInfo.systemUptime - started) * 1_000)])
    }
    guard let window = try focusedWindowValue(result, element: axElement(value)) else { return nil }
    guard try axPID(window, deadline) == pid else {
        throw failure("focus_unproven", "Focused window belongs to another process")
    }
    return window
}

// A typed cannotComplete can follow the already-issued WindowServer raise.
// Do not issue another activation in that branch: only the existing exact,
// bounded read-only focus proof may permit the later requested input.
private func activationAfterRaise(_ result: AXError, activate: () throws -> Void) throws -> AXError? {
    switch result {
    case .success:
        try activate()
        return nil
    case .cannotComplete:
        return result
    default:
        throw failure("focus_failed", "AXRaise did not confirm the requested window")
    }
}

private func verifyAfterRaise<T>(_ unconfirmed: AXError?, proof: () throws -> T) throws -> T {
    do { return try proof() }
    catch let original as NativeFailure {
        guard let unconfirmed else { throw original }
        var details = original.details ?? [:]
        details["unconfirmed_raise"] = ["action":kAXRaiseAction, "ax_error":unconfirmed.rawValue]
        // The native bridge retains code/message; preserve the original raise
        // cause there too without classifying or recovering from message text.
        throw failure(original.code,
                      original.message + " [AXRaise unconfirmed: AXError=\(unconfirmed.rawValue)]",
                      details: details, attributeFailure: original.attributeFailure,
                      focusAbsence: original.focusAbsence, modalSnapshotExpired: original.modalSnapshotExpired,
                      foregroundSnapshotMismatch: original.foregroundSnapshotMismatch)
    }
}

// Only used after the one issued raise/activation. These polls are read-only:
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

// Activation returning to the original app can precede its AX provider becoming
// ready. Confirm the whole exact-window proof under the existing cleanup budget;
// these callbacks only read state and never repeat activation or raise a window.
private func confirmRestoredForeground(remaining: () -> TimeInterval,
    check: () throws -> Void, frontmost: () throws -> Bool,
    windowMatches: () throws -> Bool, wait: (TimeInterval) -> Void) throws {
    let _: Bool = try waitForEstablishedFocus(remaining: remaining, check: check, read: {
        guard try frontmost() else { return nil }
        guard try windowMatches() else {
            throw failure("focus_restore_failed", "Original exact focused window was not restored")
        }
        return try frontmost() ? true : nil
    }, wait: wait)
}

private func axWindowIdentity(_ element: AXUIElement, _ deadline: Deadline,
                              traceStage: String? = nil) throws -> (AXError, CGWindowID) {
    guard let function = exactWindowSymbol.function else {
        throw failure("unsupported_platform", "Exact AX window identity SPI is unavailable")
    }
    try deadline.prepare(element)
    var id: CGWindowID = 0
    let started = ProcessInfo.processInfo.systemUptime
    let result = function(element, &id)
    if let traceStage {
        traceSemanticFailure(["operation":"focus_read_returned", "stage":traceStage,
                              "attribute":"AXWindowID", "ax_error":result.rawValue,
                              "elapsed_ms":(ProcessInfo.processInfo.systemUptime - started) * 1_000,
                              "remaining_ms":(deadline.uptime - ProcessInfo.processInfo.systemUptime) * 1_000])
    }
    try deadline.check()
    return (result, id)
}

// Keep the external identity error unchanged. The private SPI status is
// evidence about this read, never an absence or permission to retry input
// validation or replace an old AX root.
private func requireAXWindowID(_ result: AXError, _ id: CGWindowID) throws -> CGWindowID {
    guard result == .success, id > 0 else {
        throw failure("window_unproven", "AX exact window identity unresolved: AXError=\(result.rawValue), id=\(id)",
                      attributeFailure: result == .cannotComplete
                        ? AXAttributeFailure(name: "AXWindowID", error: result) : nil)
    }
    return id
}

private func axWindowID(_ element: AXUIElement, _ deadline: Deadline,
                        traceStage: String? = nil) throws -> CGWindowID {
    let result: AXError
    let id: CGWindowID
    do { (result, id) = try axWindowIdentity(element, deadline, traceStage: traceStage) }
    catch {
        if let traceStage {
            traceSemanticFailure(["operation":"focus_window_identity", "stage":traceStage,
                                  "error":(error as? NativeFailure)?.code ?? "unknown"])
        }
        throw error
    }
    if result != .success || id == 0 {
        if let traceStage {
            traceSemanticFailure(["operation":"focus_window_identity", "stage":traceStage,
                                  "error":"window_unproven", "ax_error":result.rawValue, "window":id])
        }
    }
    return try requireAXWindowID(result, id)
}

private func axRect(_ element: AXUIElement, _ deadline: Deadline) throws -> CGRect? {
    guard let position = try axAttribute(element, kAXPositionAttribute, deadline),
          let size = try axAttribute(element, kAXSizeAttribute, deadline),
          CFGetTypeID(position) == AXValueGetTypeID(), CFGetTypeID(size) == AXValueGetTypeID() else { return nil }
    var point = CGPoint.zero; var extent = CGSize.zero
    guard AXValueGetValue(position as! AXValue, .cgPoint, &point),
          AXValueGetValue(size as! AXValue, .cgSize, &extent),
          [point.x, point.y, extent.width, extent.height].allSatisfy(\.isFinite),
          extent.width > 0, extent.height > 0 else { return nil }
    return CGRect(origin: point, size: extent)
}

// Full native text is distinct from the bounded name/value summaries used by
// discovery. AXStringForRange is the platform fallback when AXValue is absent.
private func nativeText(_ element: AXUIElement, _ deadline: Deadline) throws -> String {
    let role = try axAttribute(element, kAXRoleAttribute, deadline) as? String ?? ""
    guard role == kAXStaticTextRole || supportsStringValueEditing(role) else {
        throw failure("unsupported_capability", "readText requires an AX text element, not a name or label")
    }
    let capacity = 1_048_576 // UTF-16 units retained for stable pagination.
    if let text = try axAttribute(element, kAXValueAttribute, deadline) as? String {
        guard (text as NSString).length <= capacity else {
            throw failure("resource_limit", "Native text exceeds the 1048576 UTF-16 unit snapshot budget; choose a smaller text element")
        }
        return text
    }
    guard let count = try axAttribute(element, kAXNumberOfCharactersAttribute, deadline) as? NSNumber,
          CFGetTypeID(count) != CFBooleanGetTypeID(), count.intValue >= 0 else {
        throw failure("unsupported_capability", "This text element exposes neither AXValue nor a native text range")
    }
    guard count.intValue <= capacity else {
        throw failure("resource_limit", "Native text exceeds the 1048576 UTF-16 unit snapshot budget; choose a smaller text element")
    }
    var range = CFRange(location: 0, length: count.intValue)
    guard let parameter = AXValueCreate(.cfRange, &range) else { throw failure("text_unavailable", "Cannot construct native text range") }
    try deadline.prepare(element)
    var value: CFTypeRef?
    let result = AXUIElementCopyParameterizedAttributeValue(element, kAXStringForRangeParameterizedAttribute as CFString,
                                                            parameter, &value)
    try deadline.check()
    if result == .parameterizedAttributeUnsupported || result == .attributeUnsupported || result == .notImplemented {
        throw failure("unsupported_capability", "The AX provider does not expose native text ranges")
    }
    guard result == .success, let text = value as? String else {
        throw failure("text_unavailable", "Native text range failed: \(result.rawValue)")
    }
    guard (text as NSString).length == count.intValue else {
        throw failure("stale_observation", "Native text changed while its range was read")
    }
    return text
}

// Only a read-only semantic query may translate missing AXWindows into a
// semantic gap. Input/modal validation keeps the original hard refusal.
private func readOnlySemantics<T>(_ read: () throws -> T) throws -> T {
    do { return try read() }
    catch let error as NativeFailure where error.code == "modal_state_unproven"
        && error.attributeFailure?.name == kAXWindowsAttribute
        && (error.attributeFailure?.error == .attributeUnsupported || error.attributeFailure?.error == .noValue) {
        throw failure("semantic_window_unavailable", error.message,
                      details: error.details, attributeFailure: error.attributeFailure)
    }
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
        if required { throw failure("modal_state_unproven", "Required AX inventory \(attribute) is unavailable",
                                    attributeFailure: AXAttributeFailure(name: attribute, error: result)) }
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

// AXPress callbacks may synchronously enter modal processing. Read timeouts and
// a separate short action cap can expire after delivery but before the reply
// (see AXUIElementPerformAction's SDK contract). Use the caller's remaining
// action budget for one reply; the parent also bounds and cancels the worker.
// Neither a timeout nor another AX error permits replay or proves acceptance.
private func performSemanticPress(deadline: Deadline, actionName: String = "AXPress",
                                  now: () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
                                  setTimeout: (Float) -> AXError,
                                  dispatch: () -> AXError?) throws -> [String: Any] {
    let remaining = deadline.uptime - now()
    guard remaining > 0.001 else { throw failure("deadline_exceeded", "No AX action budget remains") }
    let configured = setTimeout(Float(remaining))
    guard configured == .success else {
        throw failure("ax_unavailable", "Cannot set AX action messaging deadline: \(configured.rawValue)",
                      attributeFailure: AXAttributeFailure(name: "AXMessagingTimeout", error: configured))
    }
    guard now() < deadline.uptime else { throw failure("deadline_exceeded", "Request deadline elapsed before AXPress") }
    guard let result = dispatch() else { throw failure("closed", "Native input admission is closed") }
    // Preserve the provider's positive acknowledgement even if it arrived late.
    // Core separately records deadline/cancellation uncertainty and retains this
    // original provider receipt; acceptance does not prove the UI effect.
    if result == .success { return ["dispatch": "dispatched", "evidence": "unverified"] }
    if now() >= deadline.uptime {
        return ["dispatch": "unknown", "evidence": "unverified",
            "error": "deadline_exceeded", "message": "\(actionName) returned after the request deadline"]
    }
    return ["dispatch": "unknown", "evidence": "unverified", "error": "ax_action_failed",
            "message": "\(actionName) returned \(result.rawValue); inspect before another action"]
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
    if role == kAXSheetRole { return .supported }
    guard role == kAXWindowRole else { excluded("excluded_non_window_role"); return .excluded }
    guard let subrole = try subrole(), !subrole.isEmpty else { return .subroleUnproven }
    guard [kAXStandardWindowSubrole, kAXDialogSubrole, kAXSystemDialogSubrole, kAXFloatingWindowSubrole,
           kAXUnknownSubrole].contains(subrole) else { excluded("excluded_non_standard_subrole"); return .excluded }
    _ = identifier // Identifiers name a surface; they do not make a chooser unsafe.
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
    if modal { throw failure("unsupported_modal", "A modal blocks the selected window. Inspect the current app.look() view before choosing input for the related modal.") }
}

private func rejectModalSurface(_ element: AXUIElement, _ context: RequestContext) throws {
    try context.admitModalVisit()
    let deadline = context.deadline
    guard let role = try axAttribute(element, kAXRoleAttribute, deadline) as? String else {
        throw failure("modal_state_unproven", "An AX inventory element has no readable role")
    }
    if role == kAXSheetRole {
        throw failure("unsupported_modal", "An attached sheet blocks the selected window. Inspect the current app.look() view before choosing input for the related modal.")
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

private func requireWindowWithoutSheet(_ element: AXUIElement, _ context: RequestContext,
                                      modal: FocusModalContext) throws {
    try readModalSnapshot(phase: modal.phase, scope: .targetRoot, isTarget: true, read: {
        try context.admitModalVisit()
        let deadline = context.deadline
        // A modal surface may itself be operated. Only its attached child modal
        // blocks this target; an unrelated sibling's sheet does not.
        // Some providers expose AXSheets, but it is not a public SDK constant.
        // Its absence is inconclusive: the standard AXChildren check is mandatory.
        if let sheets = try axAttribute(element, "AXSheets", deadline) {
            guard CFGetTypeID(sheets) == CFArrayGetTypeID() else {
                throw failure("modal_state_unproven", "AXSheets is not an array")
            }
            if CFArrayGetCount((sheets as! CFArray)) > 0 {
                throw failure("unsupported_modal", "An attached sheet blocks the selected window. Inspect the current app.look() view before choosing input for the related modal.")
            }
        }
        try inspectModalChildren(element, context: context) { child in
            try modal.inspect(child, scope: .targetChild, context: context) {
                try completeModalChildren(element, context).contains { CFEqual($0, child) }
            }
        }
    })
}

// Ported from CUA 0.38.0 exact_target's bounded sheet/parent traversal. Keep
// native AX objects: a title, matching rectangle or PID is not ancestry.
private func boundedAncestor<Element>(_ element: Element, is ancestor: Element, limit: Int = 40,
                                      equal: (Element, Element) -> Bool,
                                      parent: (Element) throws -> Element?) rethrows -> Bool {
    var current = element
    var visited: [Element] = []
    for _ in 0..<limit {
        if equal(current, ancestor) { return true }
        if visited.contains(where: { equal($0, current) }) { return false }
        visited.append(current)
        guard let next = try parent(current) else { return false }
        current = next
    }
    return false
}

private func axAncestor(_ element: AXUIElement, is ancestor: AXUIElement,
                        _ deadline: Deadline, limit: Int = 40) throws -> Bool {
    try boundedAncestor(element, is: ancestor, limit: limit, equal: CFEqual) {
        axElement(try axAttribute($0, kAXParentAttribute, deadline))
    }
}

private func retryableAXBusyFailure(_ error: NativeFailure) -> Bool {
    error.code == "ax_unavailable" && error.attributeFailure?.error == .cannotComplete
}

// Typed AX busy replies can restart a complete read-only inventory or ownership proof.
// A partial array is never resumed or merged. Cancellation, identity and the absolute caller deadline remain final.
private func retryCompleteAXRead<Value>(original: NativeFailure,
    remaining: () -> TimeInterval, check: () throws -> Void, read: () throws -> Value,
    wait: (TimeInterval) -> Void) throws -> Value {
    var latest = original
    while remaining() > 0 {
        try check()
        wait(min(0.02, max(0, remaining())))
        try check()
        guard remaining() > 0 else { break }
        do {
            let snapshot = try read()
            try check()
            if remaining() > 0 { return snapshot }
            break
        } catch let error as NativeFailure {
            if error.code == "deadline_exceeded", remaining() <= 0 { break }
            guard retryableAXBusyFailure(error) else { throw error }
            latest = error
        }
    }
    try check()
    throw latest
}

// Focus can become AX-busy again after a successful activation poll. Restart
// the complete read-only proof, never activation, payload, or a partial result.
private func readFocusSnapshot<Value>(remaining: () -> TimeInterval,
    check: () throws -> Void, read: () throws -> Value,
    wait: (TimeInterval) -> Void) throws -> Value {
    try check()
    do {
        let value = try read()
        try check()
        return value
    } catch let original as NativeFailure where retryableAXBusyFailure(original) {
        return try retryCompleteAXRead(original: original, remaining: remaining,
            check: check, read: read, wait: wait)
    }
}

private func readInventorySnapshot<Value>(_ app: NativeApp, _ context: RequestContext,
                                          read: (RequestContext) throws -> Value) throws -> Value {
    func check() throws {
        guard !dtwInputStopping() else { throw failure("native_stopping", "Window discovery was cancelled") }
        try context.deadline.check()
        try context.verify(app)
    }
    try check()
    do {
        let snapshot = try read(context)
        try check()
        return snapshot
    } catch let original as NativeFailure where retryableAXBusyFailure(original) {
        let deadline = context.deadline.limited(to: ProcessInfo.processInfo.systemUptime + 0.75)
        return try retryCompleteAXRead(original: original, remaining: {
            deadline.uptime - ProcessInfo.processInfo.systemUptime
        }, check: check, read: {
            try read(RequestContext(deadline: deadline))
        }, wait: { CFRunLoopRunInMode(.defaultMode, $0, false) })
    }
}

private func semanticWindowRoots(_ application: AXUIElement, _ deadline: Deadline,
                                 requestedID: CGWindowID? = nil) throws -> [AXUIElement] {
    var roots = try axList(application, kAXWindowsAttribute, limit: 129, deadline, required: true)
    guard roots.count <= 128 else { throw failure("window_limit", "AX window inventory exceeds the supported limit") }
    if let requestedID {
        for root in roots {
            do { if try axWindowID(root, deadline) == requestedID { return roots } }
            catch let error as NativeFailure where error.code == "window_unproven" { continue }
        }
    }
    let appChildren = try axList(application, kAXChildrenAttribute, limit: 129, deadline)
    guard appChildren.count <= 128 else { throw failure("window_limit", "Application surface inventory exceeds the supported limit") }
    for child in appChildren where !roots.contains(where: { CFEqual($0, child) }) {
        if try axAttribute(child, kAXRoleAttribute, deadline) as? String == kAXSheetRole {
            roots.append(child)
        }
    }
    if let requestedID {
        for root in roots {
            do { if try axWindowID(root, deadline) == requestedID { return roots } }
            catch let error as NativeFailure where error.code == "window_unproven" { continue }
        }
    }
    var index = 0
    while index < roots.count {
        let root = roots[index]; index += 1
        let sheets = try axList(root, "AXSheets", limit: 129, deadline)
        let children = try axList(root, kAXChildrenAttribute, limit: 4_097, deadline)
        guard children.count <= 4_096 else { throw failure("modal_state_unproven", "Modal child inventory exceeds the supported limit") }
        for child in sheets + children {
            guard !roots.contains(where: { CFEqual($0, child) }),
                  try axAttribute(child, kAXRoleAttribute, deadline) as? String == kAXSheetRole,
                  try axAncestor(child, is: root, deadline) else { continue }
            roots.append(child)
            guard roots.count <= 128 else { throw failure("window_limit", "AX window and sheet inventory exceeds the supported limit") }
        }
    }
    return roots
}

// A focused modal can operate itself despite another direct application-modal
// sibling. Focused UI is corroboration, not a second stable identity: nil means
// absent, false means a readable contradiction, and native errors stay errors.
private func permitsActiveModalSibling(targetIsModal: () throws -> Bool,
    targetIsDirectAppChild: () throws -> Bool, exactFocusedRoot: () throws -> Bool,
    focusedUIWithinTarget: () throws -> Bool?) throws -> Bool {
    guard try targetIsModal(), try targetIsDirectAppChild(), try exactFocusedRoot() else { return false }
    return try focusedUIWithinTarget() != false
}

private func permitsOtherModal(descendant: Bool, applicationModal: () throws -> Bool,
                               activeModalTarget: () throws -> Bool) throws -> Bool {
    if descendant {
        throw failure("target_blocked_by_modal", "An attached modal blocks the selected window. Inspect the current app.look() view before choosing input for the related modal.")
    }
    guard try applicationModal() else { return false }
    guard try activeModalTarget() else {
        throw failure("target_blocked_by_modal", "An application modal blocks the selected window. Inspect the current app.look() view before choosing input for the related modal.")
    }
    return true
}

private func recheckActiveModalSibling(_ used: Bool, proof: () throws -> Bool) throws {
    if used, try !proof() {
        throw failure("focus_changed", "Focused modal changed during validation; observe the current dialog before continuing")
    }
}

private func activeModalTarget(_ target: AXUIElement, application: AXUIElement,
                               pid: Int32, windowID: CGWindowID, deadline: Deadline) throws -> Bool {
    try permitsActiveModalSibling(targetIsModal: {
        guard try axPID(target, deadline) == pid else { return false }
        let role = try axAttribute(target, kAXRoleAttribute, deadline) as? String
        if role == kAXSheetRole { return true }
        guard role == kAXWindowRole else { return false }
        guard let raw = try axAttribute(target, kAXModalAttribute, deadline),
              CFGetTypeID(raw) == CFBooleanGetTypeID(), let value = raw as? Bool else {
            throw failure("modal_state_unproven", "Target has no readable Boolean AXModal state")
        }
        return value
    }, targetIsDirectAppChild: {
        guard let raw = try axAttribute(target, kAXParentAttribute, deadline) else { return false }
        guard let parent = axElement(raw) else { throw failure("modal_state_unproven", "Target AXParent is not an element") }
        return try CFEqual(parent, application) && axPID(parent, deadline) == pid
            && axAttribute(parent, kAXRoleAttribute, deadline) as? String == kAXApplicationRole
    }, exactFocusedRoot: {
        guard let raw = try axAttribute(application, kAXFocusedWindowAttribute, deadline) else { return false }
        guard let focused = axElement(raw) else { throw failure("modal_state_unproven", "AXFocusedWindow is not an element") }
        return try CFEqual(focused, target) && axPID(focused, deadline) == pid
            && axWindowID(target, deadline) == windowID && axWindowID(focused, deadline) == windowID
    }, focusedUIWithinTarget: {
        guard let raw = try axAttribute(application, kAXFocusedUIElementAttribute, deadline) else { return nil }
        guard let focused = axElement(raw) else { throw failure("modal_state_unproven", "AXFocusedUIElement is not an element") }
        // Some providers assign a control its own CG ID. Only its real parent
        // chain proves it belongs to this modal; never compare control CG IDs.
        return try boundedAncestor(focused, is: target, equal: CFEqual) { current in
            guard try axPID(current, deadline) == pid else { return nil }
            return axElement(try axAttribute(current, kAXParentAttribute, deadline))
        }
    })
}

private func requireNoBlockingModal(_ target: AXUIElement, roots: [AXUIElement], windowID: CGWindowID,
                                    context: RequestContext, modal: FocusModalContext? = nil) throws {
    let pid = try axPID(target, context.deadline)
    let application = AXUIElementCreateApplication(pid)
    var ignoredSibling = false
    func activeTargetProof() throws -> Bool {
        try readModalSnapshot(phase: modal?.phase ?? .validation, scope: .targetRoot, isTarget: true, read: {
            try activeModalTarget(target, application: application, pid: pid, windowID: windowID, deadline: context.deadline)
        })
    }
    var candidates = roots
    for child in try completeModalChildren(application, context)
        where !candidates.contains(where: { CFEqual($0, child) }) { candidates.append(child) }
    for other in candidates where !CFEqual(other, target) {
        try readModalSnapshot(phase: modal?.phase ?? .validation, scope: .appWindow,
            isTarget: false, read: {
            // An ancestor modal owns this target; it does not block its own sheet.
            if try axAncestor(target, is: other, context.deadline) { return }
            let role = try axAttribute(other, kAXRoleAttribute, context.deadline) as? String
            let isSheet = role == kAXSheetRole
            let isModal: Bool
            if isSheet { isModal = true }
            else if role == kAXWindowRole {
                guard let value = try axAttribute(other, kAXModalAttribute, context.deadline),
                      CFGetTypeID(value) == CFBooleanGetTypeID(), let value = value as? Bool else {
                    throw failure("modal_state_unproven", "Another window has no readable AXModal state")
                }
                isModal = value
            } else { isModal = false }
            guard isModal else { return }
            // Only a proven active modal may disregard a direct same-app
            // sibling. Descendants are rejected before any focus exemption.
            var parent: AXUIElement?
            let ignored = try permitsOtherModal(descendant: axAncestor(other, is: target, context.deadline), applicationModal: {
                parent = axElement(try axAttribute(other, kAXParentAttribute, context.deadline))
                guard let parent else { return false }
                return try axAttribute(parent, kAXRoleAttribute, context.deadline) as? String == kAXApplicationRole
            }, activeModalTarget: {
                guard let parent else { return false }
                return try CFEqual(parent, application) && axPID(parent, context.deadline) == pid
                    && axPID(other, context.deadline) == pid && activeTargetProof()
            })
            ignoredSibling = ignoredSibling || ignored
        }, checkTarget: { try modal?.checkTarget() }, parentContains: {
            let fresh = try axList(application, kAXWindowsAttribute, limit: 129, context.deadline, required: true)
            guard fresh.count <= 128 else { throw failure("window_limit", "AX window inventory exceeds the supported limit") }
            return try fresh.contains(where: { CFEqual($0, other) })
                || completeModalChildren(application, context).contains(where: { CFEqual($0, other) })
        })
    }
    try recheckActiveModalSibling(ignoredSibling, proof: activeTargetProof)
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

private func currentWindowBindings<Key: Hashable, Binding>(
    _ bindings: [Key: Binding], windows: [WindowSlot: NativeTarget],
    target: (Binding) -> NativeTarget
) -> [Key: Binding] {
    bindings.filter {
        let retained = target($0.value)
        return windows[WindowSlot(app: retained.app, id: retained.nativeID)] == retained
    }
}

private enum ScopedWindowCondition: String { case dialog, unblocked, closed }

// Closed waits may outlive their original process. Only positive kernel death
// can replace a live identity check, never an unreadable or reused PID.
private func verifyWindowWaitProcess(expected: UInt64, stamp: UInt64?,
    gone: () -> Bool, verifyLive: () throws -> Void) throws {
    if let stamp {
        guard stamp == expected else { throw failure("app_changed", "Window wait process identity was reused") }
        do { try verifyLive() }
        catch let error as NativeFailure where error.code == "app_changed" {
            guard gone() else { throw error }
        }
    } else {
        guard gone() else { throw failure("app_changed", "Window wait process identity is unavailable") }
    }
}

// An exact absence check never depends on AX readiness, a title, app inventory
// coverage or onscreen state. A retained zero-sized/hidden surface is still open.
private func exactWindowPresent(_ rows: [[String: Any]], id: CGWindowID, owner: Int32) throws -> Bool {
    guard !rows.isEmpty else { return false }
    guard rows.count == 1, let foundID = rows[0][kCGWindowNumber as String] as? UInt32,
          foundID != 0, let foundOwner = rows[0][kCGWindowOwnerPID as String] as? Int32, foundOwner > 0 else {
        throw failure("window_inventory_unproven", "Exact window presence metadata is incomplete or duplicated")
    }
    guard foundID == id, foundOwner == owner else {
        throw failure("target_changed", "Exact window presence returned a different owner or surface")
    }
    return true
}

// WindowServer can publish a growing sheet at zero area before it is ready.
// That is a present-but-unready surface, not malformed metadata or closure.
private func readyModalSurface(_ rows: [[String: Any]], id: CGWindowID) throws -> CGRecord? {
    guard !rows.isEmpty else { return nil }
    guard rows.count == 1, let owner = rows[0][kCGWindowOwnerPID as String] as? Int32, owner > 0,
          rows[0][kCGWindowLayer as String] is Int,
          let dictionary = rows[0][kCGWindowBounds as String] as? NSDictionary,
          let bounds = CGRect(dictionaryRepresentation: dictionary),
          bounds.minX.isFinite, bounds.minY.isFinite, bounds.width.isFinite, bounds.height.isFinite,
          bounds.width >= 0, bounds.height >= 0 else {
        throw failure("window_unproven", "Dialog surface metadata is incomplete or malformed")
    }
    _ = try exactWindowPresent(rows, id: id, owner: owner)
    if bounds.width == 0 || bounds.height == 0 { return nil }
    let record = try selectedSurfaceRecord(rows, id: id)
    return record.onscreen ? record : nil
}

// The caller has already proved every candidate's relation to the anchor. A
// parent modal is not actionable while another candidate descends from it.
private func terminalModalRoots<Root>(_ roots: [Root], equal: (Root, Root) -> Bool,
                                     descendant: (Root, Root) throws -> Bool) rethrows -> [Root] {
    try roots.filter { root in
        try !roots.contains { other in try !equal(root, other) && descendant(other, root) }
    }
}

private func windowProbeFailureReason(_ error: NativeFailure,
                                      checkCaller: () throws -> Void) throws -> String? {
    // Typed AX busy may settle on a fresh read, but a true caller deadline,
    // cancellation or process-identity failure always remains authoritative.
    try checkCaller()
    return windowProbePending(error)
}

private func windowProbePending(_ error: NativeFailure) -> String? {
    if error.attributeFailure?.error == .cannotComplete,
       error.code == "ax_unavailable" || (error.code == "window_unproven" && error.attributeFailure?.name == "AXWindowID") {
        return "ax_busy"
    }
    switch error.code {
    case "target_blocked_by_modal", "unsupported_modal": return "blocked_by_modal"
    case "semantic_window_unavailable": return "semantic_window_unavailable"
    case "focus_changed": return "focus_changed"
    default: return nil
    }
}

private struct SurfaceAssociation {
    let ownerPID: Int32
    let ownerBirth: UInt64
    // Public AX may proxy a service-owned surface into its client application.
    // Retain both identities; never relabel the WindowServer owner as the app.
    let root: AXUIElement
}

private enum AssociationProofScope {
    case full, sceneScrollGuard

    func provePixels(_ prove: () throws -> Bool) throws -> Bool {
        // The leased Scene brackets its resident semantic guard with its own
        // visibility/stack/full association proof. This branch proves only a
        // fresh AX relation; it cannot register or replace an association.
        if self == .sceneScrollGuard { return true }
        return try prove()
    }
}

private func retainedAssociationMatches<Root>(_ retained: Root, ownerBirth: UInt64,
    birth: () throws -> UInt64?, root: () throws -> Root?, equal: (Root, Root) -> Bool) throws -> Bool {
    guard try birth() == ownerBirth, let current = try root(), equal(retained, current),
          try birth() == ownerBirth else { return false }
    return true
}

// Resident workers may retain AX state, but must never initialize SCK. The
// Rust owner delegates this synchronous proof to a short-lived scene owner and
// retains its global scene lease through exit. No hook means no pixel proof.
private final class AssociatedSurfaceProver: @unchecked Sendable {
    typealias Prove = @convention(c) (UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?
    typealias Release = @convention(c) (UnsafeMutablePointer<CChar>) -> Void
    private let lock = NSLock()
    private var callbacks: (Prove, Release)?

    func set(_ prove: Prove?, _ release: Release?) {
        lock.lock(); defer { lock.unlock() }
        callbacks = prove.flatMap { prove in release.map { (prove, $0) } }
    }

    func prove(_ request: [String: Any]) throws -> Bool {
        lock.lock(); let current = callbacks; lock.unlock()
        guard let (prove, release) = current else {
            throw failure("association_unavailable", "Short-lived scene proof owner is unavailable")
        }
        let data = try JSONSerialization.data(withJSONObject: request, options: [.sortedKeys])
        guard let result = String(decoding: data, as: UTF8.self).withCString({ prove($0) }) else {
            throw failure("association_unavailable", "Scene proof returned no result")
        }
        defer { release(result) }
        let resultData = Data(String(cString: result).utf8)
        guard resultData.count <= 1_048_576,
              let reply = (try? JSONSerialization.jsonObject(with: resultData)) as? [String: Any] else {
            throw failure("invalid_response", "Scene proof returned malformed evidence")
        }
        if let code = reply["error"] as? String {
            throw failure(code, reply["message"] as? String ?? "Scene proof failed",
                          details: reply["details"] as? [String: Any])
        }
        guard reply.count == 1, let proven = reply["proven"] as? NSNumber,
              CFGetTypeID(proven) == CFBooleanGetTypeID() else {
            throw failure("invalid_response", "Scene proof did not return an exact boolean result")
        }
        return proven.boolValue
    }
}
private let associatedSurfaceProver = AssociatedSurfaceProver()

@_cdecl("dtw_set_associated_surface_prover")
public func dtwSetAssociatedSurfaceProver(
    _ prove: (@convention(c) (UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?)?,
    _ release: (@convention(c) (UnsafeMutablePointer<CChar>) -> Void)?) {
    associatedSurfaceProver.set(prove, release)
}

private struct AssociatedPointSnapshot {
    let app: NativeApp
    let ownerPID: Int32
    let ownerBirth: UInt64
    let id: CGWindowID
    let bounds: CGRect
    let point: CGPoint

    init(_ request: [String: Any]) throws {
        app = try NativeApp(request["app"])
        guard let owner = request["surface_owner"] as? [String: Any] else {
            throw failure("invalid_request", "Associated surface owner is missing")
        }
        let pid = try integer(owner["pid"], "surface_owner.pid")
        ownerBirth = try integer(owner["generation"], "surface_owner.generation")
        let window = try integer(request["window_id"], "window_id")
        guard pid > 0, pid <= UInt64(Int32.max), ownerBirth > 0,
              window > 0, window <= UInt64(CGWindowID.max),
              let rect = request["bounds"] as? [Double], rect.count == 4, rect.allSatisfy({ $0.isFinite }),
              rect[2] > 0, rect[3] > 0,
              let x = request["screen_x"] as? Double, let y = request["screen_y"] as? Double,
              x.isFinite, y.isFinite else {
            throw failure("invalid_request", "Associated point identity or geometry is invalid")
        }
        ownerPID = Int32(pid); id = CGWindowID(window)
        bounds = CGRect(x: rect[0], y: rect[1], width: rect[2], height: rect[3])
        point = CGPoint(x: x, y: y)
        guard bounds.contains(point) else { throw failure("invalid_point", "Associated point is outside the exact surface") }
    }

    func validate(checkApp: (NativeApp) throws -> Void, birth: (Int32) throws -> UInt64?,
                  read: (CGWindowID) throws -> [[String: Any]]) throws {
        try checkApp(app)
        guard try birth(ownerPID) == ownerBirth else { throw failure("target_changed", "Associated surface owner changed") }
        let current = try selectedSurfaceRecord(read(id), id: id)
        guard current.pid == ownerPID, current.bounds == bounds, current.onscreen,
              try birth(ownerPID) == ownerBirth else {
            throw failure("target_changed", "Associated surface identity or geometry changed")
        }
        try checkApp(app)
    }

    func validate(_ context: RequestContext) throws {
        try validate(checkApp: { app in
            guard !dtwInputStopping() else { throw failure("native_stopping", "Associated surface proof was cancelled") }
            try context.deadline.check()
            try context.verify(app)
        }, birth: { try context.startStamp($0) }, read: { try cgWindowRows(context.deadline, only: $0) })
    }
}

private func checkedAssociatedPoint(check: () throws -> Void, prove: () throws -> Bool) throws -> Bool {
    try check()
    let result = try prove()
    try check()
    return result
}

private func delegatedAssociatedPoint(_ point: CGPoint, surface: CGRecord, app: NativeApp,
                                      _ context: RequestContext) throws -> Bool {
    guard !dtwInputStopping() else { throw failure("native_stopping", "Associated surface proof was cancelled") }
    try context.deadline.check()
    guard let birth = try context.startStamp(surface.pid) else {
        throw failure("target_changed", "Associated surface owner identity is unavailable")
    }
    let remaining = context.deadline.uptime - ProcessInfo.processInfo.systemUptime
    guard remaining > 0 else { throw failure("deadline_exceeded", "Associated surface proof deadline elapsed") }
    let request: [String: Any] = ["operation": "prove_associated_surface", "app": app.json,
        "surface_owner": ["pid": surface.pid, "generation": birth], "window_id": surface.id,
        "bounds": surface.coordinates, "screen_x": Double(point.x), "screen_y": Double(point.y),
        "deadline_uptime": context.deadline.uptime,
        "deadline_unix_ms": UInt64((Date().timeIntervalSince1970 + remaining) * 1_000)]
    let snapshot = try AssociatedPointSnapshot(request)
    return try checkedAssociatedPoint(check: { try snapshot.validate(context) },
                                     prove: { try associatedSurfaceProver.prove(request) })
}

// Only Scene.swift's private operation invokes this local SCK path. Its Rust
// caller must already own the global scene lease until the helper exits.
func dtwReadAssociatedSurfaceProof(_ request: [String: Any]) -> [String: Any] {
    do {
        guard let until = request["deadline_uptime"] as? Double, until.isFinite, until > 0 else {
            throw failure("invalid_request", "Associated surface proof has no monotonic deadline")
        }
        let context = RequestContext(deadline: try Deadline(request).limited(to: until))
        let snapshot = try AssociatedPointSnapshot(request)
        let proven = try checkedAssociatedPoint(check: { try snapshot.validate(context) }, prove: {
            dtwProveAssociatedSurfacePoint(snapshot.point, id: snapshot.id, pid: snapshot.ownerPID,
                                          bounds: snapshot.bounds, deadline: context.deadline.uptime)
        })
        return ["proven": proven]
    } catch let error as NativeFailure { return error.json }
    catch { return failure("native_error", String(describing: error)).json }
}

func associatedGeometry(_ ax: CGRect, _ surface: CGRect) -> Bool {
    abs(ax.minX - surface.minX) <= 2 && abs(ax.minY - surface.minY) <= 2 &&
        abs(ax.width - surface.width) <= 2 && abs(ax.height - surface.height) <= 2
}

// Optional discovery has not established a logical-app relation at this point.
// A hit naming our app is not yet a complete surface ownership proof. Typed
// AX availability failures omit that unproven surface with partial coverage.
// Known-surface validation calls associatedClientRoot directly and stays strict.
private func unprovenAssociationProbe<Value>(check: () throws -> Void,
                                            read: () throws -> Value?) throws -> (Value?, Int32?) {
    do { return (try read(), nil) }
    catch let error as NativeFailure where ["association_unavailable", "ax_unavailable"].contains(error.code) {
        guard let axError = error.attributeFailure?.error,
              [AXError.failure, .cannotComplete, .invalidUIElement].contains(axError) else { throw error }
        // This optional proof has not authorized a surface. Losing its hit or
        // ancestors cannot invalidate independently proven WindowServer rows.
        // Retained-surface/input validation never uses this discovery wrapper.
        try check()
        return (nil, axError.rawValue)
    }
}

// Port of legacy DTW 79a097b app_view::discover/hit, tightened with exact
// WindowServer owner, kernel birth and retained AX object revalidation. Geometry
// is only one conjunction: a visible public AX hit must independently name the
// logical client. An unrelated same-title/service-name window grants nothing.
private func associatedClientRoot(_ surface: CGRecord, app: NativeApp,
                                   _ context: RequestContext,
                                   proofScope: AssociationProofScope = .full) throws -> AXUIElement? {
    guard dtwAccessibilityGranted(request: true), surface.onscreen else { return nil }
    let points = [CGPoint(x: surface.bounds.midX, y: surface.bounds.midY),
                  CGPoint(x: surface.bounds.minX + min(12, surface.bounds.width / 3),
                          y: surface.bounds.minY + min(12, surface.bounds.height / 3)),
                  CGPoint(x: surface.bounds.maxX - min(12, surface.bounds.width / 3),
                          y: surface.bounds.maxY - min(12, surface.bounds.height / 3))]
    for point in points {
        let system = AXUIElementCreateSystemWide()
        try context.deadline.prepare(system)
        var hit: AXUIElement?
        let result = AXUIElementCopyElementAtPosition(system, Float(point.x), Float(point.y), &hit)
        try context.deadline.check()
        if [AXError.failure, .cannotComplete, .invalidUIElement].contains(result) {
            throw failure("association_unavailable", "Public AX surface attribution failed: \(result.rawValue)",
                          attributeFailure: AXAttributeFailure(name: "AXElementAtPosition", error: result))
        }
        guard result == .success, let hit, try axPID(hit, context.deadline) == app.pid else { continue }
        var cursor = hit
        for _ in 0..<40 {
            guard try axPID(cursor, context.deadline) == app.pid else { break }
            let role = try axAttribute(cursor, kAXRoleAttribute, context.deadline) as? String
            if [kAXWindowRole, kAXSheetRole, kAXMenuRole].contains(role ?? "") {
                if let rect = try axRect(cursor, context.deadline), associatedGeometry(rect, surface.bounds),
                   try proofScope.provePixels({ try delegatedAssociatedPoint(point, surface: surface, app: app, context) }) {
                    try context.verify(app)
                    return cursor
                }
                break
            }
            if role == kAXApplicationRole || role == kAXMenuBarRole { break }
            guard let parent = axElement(try axAttribute(cursor, kAXParentAttribute, context.deadline)) else { break }
            cursor = parent
        }
    }
    return nil
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
    case kAXSliderRole: return "slider"
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
    case kAXMenuItemRole, kAXMenuBarItemRole: return "menu_item"
    case kAXMenuBarRole: return "menu_bar"
    case kAXMenuRole: return "menu"
    case kAXHeadingRole: return "heading"
    default: return "unknown"
    }
}

// The public preview and selector use the same fields. Native attribute reads
// can additionally match within their existing bounded search prefix.
private struct SemanticTextFields {
    let label: String
    let name: String?
    let value: String?

    init(role: String, title: String?, value: String?, description: String?, subrole: String? = nil) {
        // Standard window controls often publish no title or description. Use
        // their native identity, never button position/order, to name them.
        let chromeName: String?
        switch (role, subrole) {
        case (kAXButtonRole, "AXCloseButton"?): chromeName = "Close window"
        case (kAXButtonRole, "AXMinimizeButton"?): chromeName = "Minimize window"
        case (kAXButtonRole, "AXZoomButton"?): chromeName = "Zoom window"
        case (kAXButtonRole, "AXFullScreenButton"?): chromeName = "Full screen window"
        default: chromeName = nil
        }
        let labels = [title, value, description].compactMap { $0 }.filter { !$0.isEmpty }
        label = labels.isEmpty ? chromeName ?? String(String.UnicodeScalarView(role.unicodeScalars.prefix(256))) : labels.joined(separator: " | ")
        name = title?.isEmpty == false ? title : description?.isEmpty == false ? description : chromeName ?? description
        self.value = value
    }

    func matches(_ needle: String?) -> Bool {
        guard let needle else { return true }
        return [label, name, value].compactMap { $0 }.contains { semanticTextContains($0, needle: needle) }
    }
}

private func semanticTextContains(_ text: String, needle: String) -> Bool {
    let source = text as NSString
    return source.range(of: needle, options: [.caseInsensitive, .diacriticInsensitive],
                        range: NSRange(location: 0, length: min(source.length, 4_096))).location != NSNotFound
}

// Matching has its own observation-wide UTF-16 budget; published fields remain
// 256-scalar previews. A proven node match needs no further field searches.
private struct NativeTextSearchBudget {
    var remaining = 1_048_576

    mutating func search(_ text: String, needle: String) -> (matched: Bool, unsearched: Bool) {
        let source = text as NSString
        let length = min(source.length, remaining)
        remaining -= length
        let matched = source.range(of: needle, options: [.caseInsensitive, .diacriticInsensitive],
                                   range: NSRange(location: 0, length: length)).location != NSNotFound
        return (matched, length < source.length)
    }
}

private struct NativeNodeTextMatch {
    var matched: Bool
    private var unsearchedText = false
    var incomplete: Bool { !matched && unsearchedText }

    init(matched: Bool) {
        self.matched = matched
    }

    mutating func includePreview(_ fields: SemanticTextFields, needle: String?) {
        matched = matched || fields.matches(needle)
    }

    mutating func inspect(_ text: String, needle: String, budget: inout NativeTextSearchBudget) {
        guard !matched else { return }
        let result = budget.search(text, needle: needle)
        matched = result.matched
        unsearchedText = unsearchedText || result.unsearched
    }
}

private func finitePositiveRect(_ rect: CGRect) -> Bool {
    [rect.minX, rect.minY, rect.width, rect.height].allSatisfy(\.isFinite) && rect.width > 0 && rect.height > 0
}

// A region is a visible native-geometry scope, clipped by proven AXScrollArea
// viewports. Other containers do not promise clipping; never prune their trees.
private func regionExcludesScrollContents(role: String, bounds: CGRect?, region: CGRect) -> Bool {
    guard role == kAXScrollAreaRole, let bounds,
          finitePositiveRect(bounds), finitePositiveRect(region) else { return false }
    return !bounds.intersects(region)
}

private func visibleRegionIntersection(element: CGRect, region: CGRect, viewports: [CGRect]) -> CGRect? {
    guard finitePositiveRect(element), finitePositiveRect(region), viewports.allSatisfy(finitePositiveRect) else { return nil }
    var visible = element.intersection(region)
    for viewport in viewports { visible = visible.intersection(viewport) }
    return visible.isNull || visible.isEmpty ? nil : visible
}

private struct RegionElementProof<Element> {
    let region: CGRect
    let window: CGRect
    let element: CGRect
    let viewports: [(Element, CGRect)]
    func matches(_ live: Self, equal: (Element, Element) -> Bool) -> Bool {
        region == live.region && window == live.window && element == live.element &&
        viewports.count == live.viewports.count && zip(viewports, live.viewports).allSatisfy {
            equal($0.0.0, $0.1.0) && $0.0.1 == $0.1.1
        }
    }
}

// A derived query keeps the original window object even when readiness
// refreshes the window cache. Reparenting a retained child cannot rebind its ref.
private struct RetainedSubtree<Element, Root> {
    let element: Element
    let root: Root
    func checkedElement(in current: Root, equal: (Root, Root) -> Bool) throws -> Element {
        guard equal(root, current) else {
            throw failure("stale_observation", "Original AX root changed before subtree discovery; observe again")
        }
        return element
    }
}

private enum NativePressCapability {
    case action, focus
    case selection(AXUIElement)
}

private struct NativeElement {
    let id: UInt64
    let identity: UInt64
    let element: AXUIElement
    let label: String
    let role: String
    let name: String?
    let value: String?
    let enabled: Bool?
    let pressCapability: NativePressCapability?
    var canPress: Bool { pressCapability != nil }
    let canSetValue: Bool
    let regionProof: RegionElementProof<AXUIElement>?

    var json: [String: Any] {
        var row: [String: Any] = ["id": id, "identity": "ax:\(identity)", "rebind_identity": "ax:\(identity)", "label": label, "role": role,
         "name": name as Any? ?? NSNull(), "value": value as Any? ?? NSNull(),
         "enabled": enabled as Any? ?? NSNull(), "actions": (canPress ? ["press"] : []) + (canSetValue ? ["setValue"] : []), "can_press": canPress]
        // Missing AXEnabled does not disable text focus. pressable() admitted
        // this case only with owned text role and writable AXFocused proof.
        if canPress && enabled == nil { row["native_text_focus"] = true }
        return row
    }
}

// Geometry is sampled before the action and checked once before its first
// wheel. It proves the retained object's location, not unchanged content pixels.
struct ScrollAnchorGeometry {
    let element: CGRect
    let window: CGRect
    let point: CGPoint
    init(element: CGRect, window: CGRect, clipping: [CGRect] = []) throws {
        let values = [element.minX, element.minY, element.width, element.height,
                      window.minX, window.minY, window.width, window.height]
        guard values.allSatisfy(\.isFinite), element.width > 0, element.height > 0,
              window.width > 0, window.height > 0 else { throw failure("element_not_visible", "Scroll anchor has no finite visible geometry") }
        guard let visible = visibleRegionIntersection(element: element, region: window, viewports: clipping) else {
            throw failure("element_not_visible", "Scroll anchor is outside its window or proven region viewport")
        }
        self.element = element; self.window = window
        point = CGPoint(x: visible.midX, y: visible.midY)
    }
    func matches(element: CGRect, window: CGRect, point: CGPoint) -> Bool {
        self.element == element && self.window == window && self.point == point
    }
}
private func scrollRect(_ value: Any?, _ name: String) throws -> CGRect {
    guard let values = value as? [Double], values.count == 4,
          values.allSatisfy(\.isFinite), values[2] > 0, values[3] > 0 else {
        throw failure("invalid_request", "Invalid \(name) scroll geometry")
    }
    return CGRect(x: values[0], y: values[1], width: values[2], height: values[3])
}
func fixedScrollPoint(_ request: [String: Any], current: CGRect) throws -> CGPoint {
    guard request["bounds"] != nil else { return CGPoint(x: current.midX, y: current.midY) }
    guard try scrollRect(request["bounds"], "window") == current else {
        throw failure("stale_observation", "Scroll anchor window geometry changed; observe again")
    }
    guard let x = request["screen_x"] as? Double, let y = request["screen_y"] as? Double,
          x.isFinite, y.isFinite, x >= current.minX, y >= current.minY,
          x < current.maxX, y < current.maxY else { throw failure("invalid_point", "Bound scroll point is outside its window") }
    return CGPoint(x: x, y: y)
}
private func executionScopeCheck() -> UnsafeMutablePointer<CChar>? { dtw_execution_scope_check() }
private func executionScopeFree(_ value: UnsafeMutablePointer<CChar>) { dtw_execution_scope_free(value) }
private func executionScopeActive() -> Bool { dtw_execution_scope_active() }
private func admitExecutionScope() throws {
    guard let response = executionScopeCheck() else { return }
    defer { executionScopeFree(response) }
    guard let data = String(cString: response).data(using: .utf8),
          let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let code = value["error"] as? String, let message = value["message"] as? String else {
        throw failure("execution_scope_unavailable", "Execution foreground scope proof was unavailable")
    }
    throw failure(code, message)
}

private func scrollAnchorCheck() -> UnsafeMutablePointer<CChar>? { dtw_scroll_anchor_check() }
private func scrollAnchorFree(_ value: UnsafeMutablePointer<CChar>) { dtw_scroll_anchor_free(value) }
private func scrollAnchorPosted() { dtw_scroll_anchor_posted() }
private func admitScrollAnchor() throws {
    guard let response = scrollAnchorCheck() else { return }
    defer { scrollAnchorFree(response) }
    guard let data = String(cString: response).data(using: .utf8),
          let value = try JSONSerialization.jsonObject(with: data) as? [String: Any],
          let code = value["error"] as? String, let message = value["message"] as? String else {
        throw failure("scroll_anchor_unavailable", "Scroll anchor proof was unavailable")
    }
    throw failure(code, message)
}

private struct NativeObservation {
    let target: NativeTarget
    let root: AXUIElement
    let query: NativeQuery
    var discoveryRoot: AXUIElement? = nil
    var menuScope = false
    var textSnapshots: [UInt64: String] = [:]
    var textSearchBudget = NativeTextSearchBudget()
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
    let scope: [String: Any]

    // The Rust bridge sends an explicit default scope. Keep it equivalent to
    // the original omitted scope for bounded cold AX materialization reads.
    var isWindowScope: Bool { scope.isEmpty || scope["kind"] as? String == "window" }

    init(_ value: Any?) throws {
        if let value, !(value is [String: Any]) {
            throw failure("invalid_request", "Discovery query must be an object")
        }
        let object = value as? [String: Any] ?? [:]
        scope = object["scope"] as? [String: Any] ?? [:]
        if let kind = scope["kind"] as? String,
           !["window", "menu", "subtree", "region"].contains(kind) {
            throw failure("invalid_request", "Unknown discovery scope")
        }
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

// UTF-16 offsets match AX/NSString and are returned verbatim to callers. Never
// manufacture a replacement character by splitting a surrogate pair.
func textPage(_ text: String, offset: Int, limit: Int) throws -> (String, Int?) {
    let source = text as NSString
    guard offset >= 0, offset <= source.length, limit >= 2, limit <= 16_384 else {
        throw failure("invalid_text_range", "Text page offset or limit is outside its bounds")
    }
    func low(_ unit: unichar) -> Bool { (0xDC00...0xDFFF).contains(unit) }
    if offset < source.length, low(source.character(at: offset)) {
        throw failure("invalid_text_range", "Text offset splits a UTF-16 scalar")
    }
    var end = min(source.length, offset + limit)
    if end < source.length, low(source.character(at: end)) { end -= 1 }
    return (source.substring(with: NSRange(location: offset, length: end - offset)),
            end < source.length ? end : nil)
}

// Healthy providers stay on the bounded count/range path. Some AX servers
// implement only CopyAttributeValue(AXChildren); count/range support is not a
// prerequisite for a readable tree. The compatibility copy is request-local.
private enum DiscoveryFrame {
    case node(AXUIElement, Int, Int)
    case children(AXUIElement, Int, Int, Int?, AXUIElement?)
}

private func permitsAXArrayCopy(_ error: AXError) -> Bool {
    [.failure, .attributeUnsupported, .noValue, .notImplemented, .illegalArgument].contains(error)
}

private func requiresVisualInput(_ error: NativeFailure) -> Bool {
    ["semantic_window_unavailable", "semantic_not_ready", "ax_unavailable",
     "children_unavailable", "modal_state_unproven", "unsupported_modal"].contains(error.code)
}

private func copiedAXChildren(_ element: AXUIElement, _ deadline: Deadline,
                              required: Bool = false) throws -> CFArray {
    try deadline.prepare(element)
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &value)
    try deadline.check()
    return try checkedAXChildren(value, result: result, required: required)
}

private func checkedAXChildren(_ value: CFTypeRef?, result: AXError, required: Bool) throws -> CFArray {
    if result == .attributeUnsupported || result == .noValue {
        if required { throw failure("modal_state_unproven", "Required AXChildren inventory is unavailable") }
        return [] as CFArray
    }
    guard result == .success, let value, CFGetTypeID(value) == CFArrayGetTypeID() else {
        throw failure("children_unavailable", "AX children value failed: \(result.rawValue)",
                      attributeFailure: AXAttributeFailure(name: kAXChildrenAttribute, error: result))
    }
    let array = value as! CFArray
    guard CFArrayGetCount(array) <= 32_768 else {
        throw failure("children_unavailable", "Copied AXChildren exceeds the 32768-child compatibility budget")
    }
    return array
}

private func resolvedAXChildCount(result: AXError, count: Int, copy: () throws -> CFArray) throws -> Int {
    if result == .success, count >= 0 { return count }
    guard permitsAXArrayCopy(result) || result == .success else {
        throw failure("children_unavailable", "AX child count failed: \(result.rawValue)",
                      attributeFailure: AXAttributeFailure(name: kAXChildrenAttribute, error: result))
    }
    return CFArrayGetCount(try copy())
}

private func axChildCount(_ element: AXUIElement, _ deadline: Deadline, required: Bool = false) throws -> Int {
    try deadline.prepare(element)
    var count: CFIndex = 0
    let result = AXUIElementGetAttributeValueCount(element, kAXChildrenAttribute as CFString, &count)
    try deadline.check()
    return try resolvedAXChildCount(result: result, count: count) {
        try copiedAXChildren(element, deadline, required: required)
    }
}

private func axChildRange(_ element: AXUIElement, index: Int, count: Int,
                          _ deadline: Deadline) throws -> [AXUIElement] {
    precondition(index >= 0 && count > 0 && count <= 17)
    try deadline.prepare(element)
    var value: CFArray?
    let result = AXUIElementCopyAttributeValues(element, kAXChildrenAttribute as CFString,
                                               index, count, &value)
    try deadline.check()
    return try resolvedAXChildRange(result: result, value: value, index: index, count: count) {
        try copiedAXChildren(element, deadline, required: true)
    }
}

private func resolvedAXChildRange(result: AXError, value: CFArray?, index: Int, count: Int,
                                  copy: () throws -> CFArray) throws -> [AXUIElement] {
    let array: CFArray
    let start: Int
    if result != .success, permitsAXArrayCopy(result) {
        array = try copy()
        start = index
        guard start < CFArrayGetCount(array) else {
            throw failure("children_unavailable", "AX child range changed during compatibility read")
        }
    } else if result == .success, let value, CFGetTypeID(value) == CFArrayGetTypeID(),
              CFArrayGetCount(value) > 0, CFArrayGetCount(value) <= count {
        array = value
        start = 0
    } else {
        // An empty successful range is a coverage gap, never an empty subtree.
        throw failure("children_unavailable", "AX child range failed: \(result.rawValue)")
    }
    var children: [AXUIElement] = []
    for offset in start..<min(start + count, CFArrayGetCount(array)) {
        guard let pointer = CFArrayGetValueAtIndex(array, offset) else {
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
private let semanticReadinessBudget: TimeInterval = 3
private struct SemanticEnablement {
    let manual: Bool
    let manualResult: Int32
    let manualAttemptedAt: TimeInterval?
    let enhancedResult: Int32?
    let enhancedAttemptedAt: TimeInterval?
}

// One read-only opportunity within an already-proven explicit-focus scope.
// Its clock is never a new AX enablement attempt or a focus-return obligation.
private struct FocusSemanticReadiness {
    private(set) var until: TimeInterval?
    private(set) var consumed = false
    mutating func arm(now: TimeInterval) {
        guard until == nil && !consumed else { return }
        until = now + semanticReadinessBudget
    }
    mutating func discard() { consumed = true }
    mutating func consume(shellOnly: Bool, complete: Bool, now: TimeInterval,
                          deadline: TimeInterval, audit: (TimeInterval) throws -> Bool) throws -> TimeInterval? {
        guard shellOnly && complete, !consumed, let until else { return nil }
        // An expired/failed attempt also spends the opportunity. No later
        // observe or repeated focus in this epoch can restart its deadline.
        consumed = true
        let bounded = min(until, deadline)
        guard now < bounded, try audit(bounded) else { return nil }
        return bounded
    }
}

// Either accepted opt-in may materialize asynchronously. Retain the first
// attempt's clock; reasserting the setter can restart the provider's debounce.
private func semanticMaterializationUntil(attemptedAt: TimeInterval?, now: TimeInterval) -> TimeInterval? {
    guard let attemptedAt, now < attemptedAt + semanticReadinessBudget else { return nil }
    return attemptedAt + semanticReadinessBudget
}

// A legacy setter can schedule materialization even when it returns
// notImplemented. Only its filtered, complete empty first page takes a second
// look; successful Manual uses the narrower structural-shell proof below.
private func emptySemanticRetryUntil(filtered: Bool, complete: Bool, matches: Int,
                                     enhancedAttemptedAt: TimeInterval?, now: TimeInterval) -> TimeInterval? {
    guard filtered, complete, matches == 0 else { return nil }
    return semanticMaterializationUntil(attemptedAt: enhancedAttemptedAt, now: now)
}

private func semanticMaterializationPending(until: TimeInterval?, now: TimeInterval,
                                            populated: Bool, check: () throws -> Void) throws -> Bool {
    guard let until else { return false }
    try check()
    return !populated && now < until
}

private func mergeWindowInventoryRows(all: [[String: Any]], visible: [[String: Any]]) -> [[String: Any]] {
    let ids = Set(visible.compactMap { $0[kCGWindowNumber as String] as? UInt32 })
    return visible + all.filter { row in
        guard let id = row[kCGWindowNumber as String] as? UInt32 else { return true }
        return !ids.contains(id)
    }
}

private func inputCreatedWindowAllowed(admitted: Bool, candidate: CGWindowID, known: Set<CGWindowID>) -> Bool {
    admitted && candidate > 0 && !known.contains(candidate)
}

private func semanticWindowDecoration(role: String, subrole: String?) -> Bool {
    guard role == kAXButtonRole, let subrole else { return false }
    return [kAXCloseButtonSubrole, kAXMinimizeButtonSubrole, kAXZoomButtonSubrole,
            kAXFullScreenButtonSubrole].contains(subrole)
}

// ScreenCaptureKit's indicator is composited, but is not the document/receiver.
// Require its native control identity in a single-child structural chain.
// A document title, dimensions or z-order never establish eligibility. AppKit
// exposes the exact internal control name as AXTitle on current macOS.
private func sharingIndicatorSurface<Element>(_ root: Element,
    role: (Element) throws -> String?, identifier: (Element) throws -> String?,
    description: (Element) throws -> String?, title: (Element) throws -> String?,
    children: (Element) throws -> [Element]) throws -> Bool {
    var element = root
    for _ in 0..<4 {
        let kind = try role(element)
        if kind == kAXButtonRole {
            return try identifier(element) == "WindowSharingSessionButton"
                || description(element) == "WindowSharingSessionButton"
                || title(element) == "WindowSharingSessionButton"
        }
        guard kind == kAXWindowRole || kind == kAXGroupRole else { return false }
        let next = try children(element)
        guard next.count == 1 else { return false }
        element = next[0]
    }
    return false
}

private struct SemanticReadinessSample {
    private var onlyStructure = true
    private var hasContainer = false
    var shellOnly: Bool { onlyStructure && hasContainer }
    var populatedWebArea = false
    var emptyWebArea = false
    var shallowUnknown = false
    var emptyContentGroup = false
    mutating func add(role: String, children: Int, depth: Int, parentRole: String,
                      decoration: Bool = false) {
        if !decoration {
            if [kAXWindowRole, kAXGroupRole, kAXScrollAreaRole, "AXSplitGroup"].contains(role) {
                hasContainer = hasContainer || role != kAXWindowRole
            } else { onlyStructure = false }
        }
        if role == "AXWebArea" {
            populatedWebArea = populatedWebArea || children > 0
            emptyWebArea = emptyWebArea || children == 0
        }
        if role == kAXGroupRole, children == 0, depth == 1, parentRole == kAXWindowRole { emptyContentGroup = true }
        let containers = ["", kAXWindowRole, kAXGroupRole, kAXScrollAreaRole, "AXSplitGroup", kAXUnknownRole]
        if (role.isEmpty || role == kAXUnknownRole), children == 0,
           depth <= 4, containers.contains(parentRole) { shallowUnknown = true }
    }
    func decision(manualOptIn: Bool, elapsed: TimeInterval,
                  materializationRemaining: TimeInterval? = nil, scanComplete: Bool = true) -> SemanticReadinessDecision {
        if populatedWebArea { return .ready }
        // Electron defers either opt-in. A completely scanned shell is not
        // ready merely because its empty containers are nested more deeply.
        // Only typed window decorations (and their descendants) are ignored;
        // any ordinary native control keeps the existing immediate path.
        if manualOptIn, let remaining = materializationRemaining, scanComplete && shellOnly {
            return remaining > 0 ? .pending : .emptyContent
        }
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
    private(set) var applicationInvalidated = false
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
        } else if reachedTarget || pid != initialPID { invalidated = true; applicationInvalidated = true }
    }
    mutating func focusedWindow(_ id: CGWindowID) {
        guard let targetWindow else { return }
        if id == targetWindow {
            if !reachedWindow { sampleRevision &+= 1 }
            reachedWindow = true
        } else if reachedWindow { invalidated = true }
    }
    mutating func sampled(_ pid: Int32, window: CGWindowID?, revision: UInt64) -> Bool {
        guard revision == sampleRevision else { return false }
        let wasInvalidated = invalidated
        activated(pid)
        // Continue recording app leave/return after window invalidation. A
        // fresh sample that first invalidates this epoch is still consumed,
        // so its caller can record the invalidation rather than hide it.
        guard !wasInvalidated else { return false }
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
    mutating func invalidate() { invalidated = true }
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
    private var windowFamily: ((AXUIElement, Deadline) throws -> Bool)?
    private var familyArmed = false
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
        return epoch?.applicationInvalidated == false
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
        lock.lock(); let token = serial; let revision = epoch?.sampleRevision
        let family = familyArmed ? windowFamily : nil
        let needsWindow = epoch?.targetWindow != nil || family != nil; lock.unlock()
        guard let revision else { return }
        if !needsWindow {
            let pid = dtwFrontProcessPID()
            if pid > 0 { sampled(pid, window: nil, token: token, revision: revision, source: "sample_visual_application") }
            return
        }
        guard let deadline = try? Deadline(["deadline_unix_ms": UInt64((Date().timeIntervalSince1970 + 0.04) * 1_000)]) else { return }
        do {
            guard let app = try nativeFocusedApplication(deadline: deadline) else { return }
            var windowID: CGWindowID?
            do {
                let window = try nativeFocusedWindow(app.element, pid: app.pid, deadline: deadline)
                if let family, let window {
                    try checkFamily(window, deadline: deadline, token: token, revision: revision, matches: family)
                }
                windowID = try window.map { try axWindowID($0, deadline) }
            } catch {
                // Failed app-window evidence can never become a CG list-order
                // guess. A separately re-proven app may supply only PID evidence.
                windowID = nil
            }
            let context = RequestContext(deadline: deadline)
            guard try focusedApplicationStillCurrent(app, birth: { try context.startStamp($0) },
                frontmost: { try windowServerFrontmost($0, deadline) }) else { return }
            try deadline.check()
            sampled(app.pid, window: windowID, token: token, revision: revision,
                    source: windowID == nil ? "sample_application_only"
                        : (app.fromCandidate ? "sample_windowserver_application" : "sample"),
                    sampleStarted: started)
        } catch {
            // The total 40ms sample budget, permission errors and contradictory
            // identities are not another chance to select a convenient source.
            return
        }
    }
    private func checkFamily(_ window: AXUIElement, deadline: Deadline, token: UInt64, revision: UInt64,
                             matches: (AXUIElement, Deadline) throws -> Bool) throws {
        let pid = try axPID(window, deadline)
        lock.lock(); let relevant = token == serial && epoch?.sampleRevision == revision
            && epoch?.targetPID == pid && familyArmed; lock.unlock()
        guard relevant else { return }
        let matches = try matches(window, deadline)
        lock.lock(); defer { lock.unlock() }
        if token == serial, epoch?.sampleRevision == revision, !matches { epoch?.invalidate() }
    }
    private func windowNotification(_ element: AXUIElement, notification: String) {
        lock.lock(); let needsWindow = epoch?.targetWindow != nil
        let family = familyArmed ? windowFamily : nil
        let token = serial; let revision = epoch?.sampleRevision; lock.unlock()
        if let family, let revision, notificationNamesFocusedWindow(notification),
           let deadline = try? Deadline(["deadline_unix_ms": UInt64((Date().timeIntervalSince1970 + 0.04) * 1_000)]) {
            // Some providers name the registered application rather than the
            // changed window. Only an exact window callback carries history.
            guard let id = try? axWindowID(element, deadline), id > 0 else { sampleFocus(); return }
            // Retain callback identity, so sibling A -> B -> A cannot rearm a scope.
            do { try checkFamily(element, deadline: deadline, token: token, revision: revision, matches: family) }
            catch { sampleFocus() }
            return
        }
        if !needsWindow { sampleFocus(); return }
        guard notificationNamesFocusedWindow(notification) else { sampleFocus(); return }
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
            guard AXObserverCreate(pid, { _, element, notification, context in
                guard let context else { return }
                Unmanaged<FocusActivationObserver>.fromOpaque(context).takeUnretainedValue().windowNotification(element, notification: notification as String)
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
    func begin(initialPID: Int32, targetPID: Int32, window: CGWindowID?, deadline: Deadline,
               family: ((AXUIElement, Deadline) throws -> Bool)? = nil) throws -> UInt64 {
        while true {
            lock.lock()
            if ready, let loop = observerRunLoop, let timer = pollTimer {
                serial &+= 1; epoch = FocusBorrowEpoch(initialPID: initialPID, targetPID: targetPID, targetWindow: window)
                windowFamily = family; familyArmed = false
                let token = serial; lock.unlock()
                do {
                    if window != nil || family != nil { try observeWindows(targetPID, on: loop, deadline: deadline) }
                } catch { end(token); throw error }
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
        familyArmed = true
        traceSemanticFailure(["operation":"focus_epoch_established", "pid":pid, "window":window,
                              "sample_revision":epoch?.sampleRevision as Any,
                              "uptime":ProcessInfo.processInfo.systemUptime])
    }
    func invalidated(_ token: UInt64) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return token != serial || epoch?.invalidated != false
    }
    func applicationInvalidated(_ token: UInt64) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return token != serial || epoch?.applicationInvalidated != false
    }
    func retarget(_ token: UInt64, to pid: Int32, window: CGWindowID?, deadline: Deadline) throws -> Bool {
        lock.lock(); let loop = observerRunLoop; let valid = token == serial && epoch?.invalidated == false; lock.unlock()
        guard valid, let loop else { return false }
        if window != nil { try observeWindows(pid, on: loop, deadline: deadline) }
        lock.lock(); defer { lock.unlock() }
        guard token == serial, var current = epoch, current.retarget(pid, window: window) else { return false }
        epoch = current
        return true
    }
    func end(_ token: UInt64) {
        lock.lock(); defer { lock.unlock() }
        if token == serial { epoch = nil; windowFamily = nil; familyArmed = false }
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
        try requireNoBlockingModal(window,
            roots: semanticWindowRoots(AXUIElementCreateApplication(target.app.pid), deadline, requestedID: target.nativeID),
            windowID: target.nativeID, context: RequestContext(deadline: deadline))
        if let sheets = try axAttribute(window, "AXSheets", deadline) {
            guard CFGetTypeID(sheets) == CFArrayGetTypeID() else {
                throw failure("modal_state_unproven", "AXSheets is not an array")
            }
            guard CFArrayGetCount((sheets as! CFArray)) == 0 else {
                throw failure("unsupported_modal", "An attached sheet blocks the selected window. Inspect the current app.look() view before choosing input for the related modal.")
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
                    throw failure("unsupported_modal", "An attached sheet blocks the selected window. Inspect the current app.look() view before choosing input for the related modal.")
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

// Launch policy is independent of whether a later input may need foreground.
private func applicationOpenConfiguration() -> NSWorkspace.OpenConfiguration {
    let configuration = NSWorkspace.OpenConfiguration()
    configuration.activates = false
    configuration.hides = false
    configuration.createsNewApplicationInstance = false
    configuration.allowsRunningApplicationSubstitution = false
    configuration.addsToRecentItems = false
    return configuration
}

private final class NativeStore: @unchecked Sendable {
    let lock = NSLock()
    private struct BorrowedFocus {
        let activationEpoch: UInt64
        var changedForeground: Bool
        var target: NativeTarget
        var window: AXUIElement?
        // The exact logical focus surface can differ from the associated CG
        // owner surface. Save its proven ID before the AX root can disappear.
        var focusWindowID: CGWindowID
        var semanticReadiness = FocusSemanticReadiness()
        let pointer: CGPoint?
        var inputPoint: CGPoint?
        var executionScoped = false
        var pointerHandoff = false
    }
    private var borrowedFocus: BorrowedFocus?
    private var releasedExecutionPointer: CGPoint?
    private struct PriorForeground {
        let application: NSRunningApplication
        let pid: Int32
        let generation: UInt64
        let window: AXUIElement?
        let windowID: CGWindowID?
    }
    private struct BackgroundWindowOrder {
        let epoch: UInt64
        let original: PriorForeground
        var previouslyAbove: [NativeApp: Set<CGWindowID>]
        var admitted: Set<NativeApp> = []
    }
    // Resident semantic AXPress can create/raise windows without activating its
    // app. This obligation lasts until script settlement, not just AX's reply.
    private var backgroundWindowOrder: BackgroundWindowOrder?
    // Resident payloads and the observer share one monotonic window anchor.
    // A scope begun without windows binds its first concrete focused root once.
    private final class ExecutionFocusAnchor: @unchecked Sendable {
        private let lock = NSLock()
        private var root: AXUIElement?
        private var rootID: CGWindowID
        private var knownWindowIDs: Set<CGWindowID> = []
        private var inputAdmitted = false
        func configure(_ ids: Set<CGWindowID>) { lock.lock(); knownWindowIDs = ids; lock.unlock() }
        func admitInput() { lock.lock(); inputAdmitted = true; lock.unlock() }
        var hasInput: Bool { lock.lock(); defer { lock.unlock() }; return inputAdmitted }
        init(_ root: AXUIElement? = nil, id: CGWindowID = 0) { self.root = root; rootID = id }
        var id: CGWindowID { lock.lock(); defer { lock.unlock() }; return rootID }
        var isEmpty: Bool { lock.lock(); defer { lock.unlock() }; return root == nil }
        func matches(_ window: AXUIElement, pid: Int32, deadline: Deadline) throws -> Bool {
            lock.lock()
            var retained = root; var retainedID = rootID
            lock.unlock()
            if retained == nil {
                guard let initial = try resolveActivationWindow(window, pid: pid,
                    owner: { try axPID($0, deadline) },
                    role: { try axAttribute($0, kAXRoleAttribute, deadline) as? String },
                    parent: { axElement(try axAttribute($0, kAXParentAttribute, deadline)) }) else { return false }
                let id = try axWindowID(initial, deadline)
                guard id > 0 else { return false }
                // AX IPC is outside the lock. The observer and resident may race
                // to discover the first window, but only one root can ever win.
                lock.lock()
                if root == nil { root = initial; rootID = id; knownWindowIDs.insert(id) }
                retained = root; retainedID = rootID
                lock.unlock()
            }
            guard let retained, try axWindowID(retained, deadline) == retainedID else { return false }
            if try executionFocusFamilyMatches(window, anchor: retained, pid: pid,
                equal: CFEqual, owner: { try axPID($0, deadline) },
                parent: { axElement(try axAttribute($0, kAXParentAttribute, deadline)) }) { return true }
            // A submitted action can create a new top-level document. This is
            // distinct from selecting an already existing sibling window.
            guard let next = try resolveActivationWindow(window, pid: pid,
                owner: { try axPID($0, deadline) },
                role: { try axAttribute($0, kAXRoleAttribute, deadline) as? String },
                parent: { axElement(try axAttribute($0, kAXParentAttribute, deadline)) }) else { return false }
            let nextID = try axWindowID(next, deadline)
            lock.lock(); let allowed = inputCreatedWindowAllowed(admitted: inputAdmitted,
                candidate: nextID, known: knownWindowIDs); lock.unlock()
            guard allowed, try ordinaryWindow(next, pid: pid, deadline: deadline),
                  try cgWindows(deadline).contains(where: { $0.id == nextID && $0.pid == pid && $0.onscreen }) else { return false }
            lock.lock()
            if inputCreatedWindowAllowed(admitted: inputAdmitted, candidate: nextID, known: knownWindowIDs) {
                root = next; rootID = nextID; knownWindowIDs.insert(nextID)
            }
            let selected = root; lock.unlock()
            guard let selected else { return false }
            return try executionFocusFamilyMatches(window, anchor: selected, pid: pid,
                equal: CFEqual, owner: { try axPID($0, deadline) },
                parent: { axElement(try axAttribute($0, kAXParentAttribute, deadline)) })
        }
    }
    private struct ExecutionFocus {
        let activationEpoch: UInt64
        let app: NativeApp
        let original: PriorForeground
        let anchor: ExecutionFocusAnchor
        let initialWindowIDs: Set<CGWindowID>
        let nativeOpenPanel: Bool
        var closedPanelRestorationAllowed: Bool
        var target: NativeTarget?
        var changedForeground: Bool
        let pointerOrigin: CGPoint?
        var pointerEndpoint: CGPoint?
        var invalidated = false
        var terminal: String?
    }
    // Only the permanent resident worker owns this restoration obligation.
    // Short-lived scene workers hold BorrowedFocus proof and pointer state only.
    private var executionFocus: ExecutionFocus?
    private lazy var executionFocusObserver = FocusActivationObserver()
    private var backgroundValidation: BackgroundInputValidation?
    private lazy var focusObserver = FocusActivationObserver()
    fileprivate private(set) var focusCleanupFailure: NativeFailure?
    fileprivate var focusOwner: NativeApp? {
        // Keep an invalidated durable scope until release: repeated begin must
        // never turn a user leave/return into permission to reacquire foreground.
        if let execution = executionFocus { return execution.app }
        guard let borrowed = borrowedFocus else { return backgroundWindowOrder?.admitted.first }
        // External focus choice ends our borrow. No physical-input lease is
        // needed to discard that ownership; no restoration will be attempted.
        if focusObserver.invalidated(borrowed.activationEpoch) {
            borrowedFocus = nil; focusObserver.end(borrowed.activationEpoch)
            return backgroundWindowOrder?.admitted.first
        }
        return borrowed.target.app
    }
    private var windows: [WindowSlot: NativeTarget] = [:]
    private var associatedSurfaces: [WindowSlot: SurfaceAssociation] = [:]
    private var semanticWindows: [WindowSlot: RegisteredWindow] = [:]
    // Keep the original opt-in decision and raw diagnostics together so a
    // later query can distinguish cached enablement from a new setter attempt.
    private var enabledApps: [NativeApp: SemanticEnablement] = [:]
    private var observations: [String: NativeObservation] = [:]
    private var semanticIdentities: [CFHashCode: [(AXUIElement, UInt64)]] = [:]
    private var semanticIdentityCount = 0
    private func semanticIdentity(_ element: AXUIElement) throws -> UInt64 {
        let hash = CFHash(element)
        if let old = semanticIdentities[hash]?.first(where: { CFEqual($0.0,element) }) { return old.1 }
        if semanticIdentityCount >= 4096 { semanticIdentities.removeAll(); semanticIdentityCount = 0 }
        let id = try nextGeneration()
        semanticIdentities[hash,default:[]].append((element,id))
        semanticIdentityCount += 1
        return id
    }
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
        for attempt in 0..<3 {
            do { return try readInventorySnapshot(app, context) { try listWindowsSnapshot(app, $0) } }
            catch let error as NativeFailure where error.code == "window_inventory_changed" && attempt < 2 {
                // Only positive cross-source evidence permits repeating this
                // read. Never replay input or guess which window is current.
                try context.deadline.check(); try context.verify(app)
            }
        }
        throw failure("window_inventory_changed", "Window inventory did not stabilize")
    }

    private func registerSurface(_ record: CGRecord, title: String, app: NativeApp,
                                 association: SurfaceAssociation?) throws -> [String: Any] {
        let slot = WindowSlot(app: app, id: record.id)
        let previous = associatedSurfaces[slot]
        let sameAssociation: Bool
        if let previous, let association {
            sameAssociation = previous.ownerPID == association.ownerPID && previous.ownerBirth == association.ownerBirth
                && CFEqual(previous.root, association.root)
        } else { sameAssociation = previous == nil && association == nil }
        let target = try (sameAssociation ? windows[slot] : nil)
            ?? NativeTarget(app: app, nativeID: record.id, generation: nextGeneration())
        windows[slot] = target
        associatedSurfaces[slot] = association
        return ["target": target.json, "title": title, "bounds": record.coordinates, "onscreen": record.onscreen]
    }

    private func reconcileWindowBindings() {
        // Compare only final target generations. A scoped bind leaves other
        // slots present, so their valid observations/AX roots remain usable.
        semanticWindows = currentWindowBindings(semanticWindows, windows: windows) { $0.target }
        observations = currentWindowBindings(observations, windows: windows) { $0.target }
    }

    private func bindWindow(_ request: [String: Any], _ context: RequestContext) throws -> [String: Any] {
        let app = try NativeApp(request["app"])
        let rawID = try integer(request["native_id"], "native_id")
        guard rawID > 0, rawID <= UInt64(CGWindowID.max) else {
            throw failure("invalid_request", "Invalid selected window identity")
        }
        let id = CGWindowID(rawID)
        return try readInventorySnapshot(app, context) { current in
            let source = try cgWindowRows(current.deadline, only: id)
            let selected = try selectedSurfaceCandidate(source, id: id, pid: app.pid,
                birth: { try current.startStamp($0) },
                associate: { try associatedClientRoot($0, app: app, current) },
                refresh: { try cgWindowRows(current.deadline, only: $0) })
            let (_, baseCoverage) = try surfaceInventory(source, pid: app.pid)
            var coverage = baseCoverage
            coverage["scope"] = "selected_surface"
            coverage["associated_count"] = selected.association == nil ? 0 : 1
            coverage["association_complete"] = false
            coverage["association_probe_failures"] = 0
            coverage["association_error_codes"] = [Int32]()
            try current.verify(app)
            try current.deadline.check()
            var association = selected.association.map {
                SurfaceAssociation(ownerPID: selected.record.pid, ownerBirth: $0.birth, root: $0.root)
            }
            let slot = WindowSlot(app: app, id: id)
            if selected.record.pid == app.pid, let retained = windows[slot],
               let clientMenu = associatedSurfaces[slot] {
                // A scoped bind must not erase a client menu's proven AX root
                // merely because its backing surface has the same PID.
                _ = try validateSurface(retained, current)
                association = clientMenu
            }
            let row = try self.registerSurface(selected.record, title: selected.title, app: app, association: association)
            self.reconcileWindowBindings()
            return ["windows": [row], "coverage": coverage]
        }
    }

    private func listWindowsSnapshot(_ app: NativeApp, _ context: RequestContext) throws -> [String: Any] {
        try context.verify(app)
        let registeredApps = Set(windows.keys.map(\.app)).union(enabledApps.keys)
        var departed: Set<NativeApp> = []
        for owner in registeredApps where try context.hasExited(owner) { departed.insert(owner) }
        windows = windows.filter { !departed.contains($0.key.app) }
        enabledApps = enabledApps.filter { !departed.contains($0.key) }
        if let validation = backgroundValidation, departed.contains(validation.target.app) { backgroundValidation = nil }
        let allSource = try cgWindowRows(context.deadline)
        // optionAll includes hidden windows but does not promise z-order.
        // Only optionOnScreenOnly documents front-to-back ordering.
        guard let visibleSource = CGWindowListCopyWindowInfo([.optionOnScreenOnly,.excludeDesktopElements],0) as? [[String:Any]] else {
            throw failure("windows_unavailable", "Cannot establish visible app layering")
        }
        // A just-created visible window may not be in the earlier optionAll
        // snapshot. Prefer the newer visible records without losing hidden ones.
        let source = mergeWindowInventoryRows(all: allSource, visible: visibleSource)
        try context.deadline.check()
        var (candidates, coverage) = try surfaceInventory(source, pid: app.pid, resolve: { record in
            try exactSurfaceRecord(cgWindowRows(context.deadline, only: record.id), expected: record)
        })
        var associated: [CGWindowID: SurfaceAssociation] = [:]
        var associationErrors: [Int32] = []
        let onScreen = visibleSource.compactMap(cgRecord).filter(\.onscreen)
        if dtwAccessibilityGranted(request: true), let firstOwned = onScreen.firstIndex(where: { $0.pid == app.pid }) {
            // Associated sheets/menus are in front of their client window. Do
            // not probe unrelated hidden windows or applications behind it.
            // AppKit popup menus can own their CG surface in the client process
            // without appearing in AXWindows. Reuse the live AX/pixel proof of
            // service-owned menus for the frontmost same-process menu as well.
            let attributionCandidates = [onScreen[firstOwned]] + onScreen.prefix(firstOwned).filter { $0.pid != app.pid }
            for record in attributionCandidates {
                guard let birth = try context.startStamp(record.pid) else { continue }
                let (candidate, probeError) = try unprovenAssociationProbe(check: {
                    guard !dtwInputStopping() else { throw failure("native_stopping", "Window discovery was cancelled") }
                    try context.deadline.check()
                    try context.verify(app)
                }, read: {
                    try associatedClientRoot(record, app: app, context)
                })
                if let probeError {
                    associationErrors.append(probeError)
                    // Stop this optional phase after its first unavailable proof.
                    // More unrelated AX reads must not consume the entire call
                    // budget and discard already proven same-process surfaces.
                    break
                }
                guard let root = candidate, try context.startStamp(record.pid) == birth,
                      let live = try exactSurfaceRecord(cgWindowRows(context.deadline, only: record.id), expected: record),
                      live.bounds == record.bounds else { continue }
                let sameProcess = record.pid == app.pid
                if sameProcess, try axAttribute(root, kAXRoleAttribute, context.deadline) as? String != kAXMenuRole { continue }
                let alreadyListed = candidates.contains(where: { $0.0.id == record.id })
                if sameProcess && !alreadyListed { continue }
                guard associated[record.id] == nil,
                      !alreadyListed || sameProcess else {
                    throw failure("target_changed", "Associated surface identity duplicated during enumeration; list windows again")
                }
                associated[record.id] = SurfaceAssociation(ownerPID: record.pid, ownerBirth: birth, root: root)
                if !alreadyListed {
                    candidates.append((live, String((source.first { ($0[kCGWindowNumber as String] as? UInt32) == record.id }?[kCGWindowName as String] as? String ?? "").prefix(512))))
                }
            }
        }
        coverage["scope"] = "application_and_proven_associated_surfaces"
        // The coverage contract adds this count to the same-process inventory.
        // A client-owned menu gains an AX root, not an additional CG row.
        coverage["associated_count"] = associated.values.filter { $0.ownerPID != app.pid }.count
        coverage["association_complete"] = false
        coverage["association_probe_failures"] = associationErrors.count
        coverage["association_error_codes"] = associationErrors
        coverage["accessibility_available"] = dtwAccessibilityGranted()
        guard candidates.count <= 128 else { throw failure("window_limit", "More than 128 system window surfaces") }
        try context.verify(app)
        let active = Set(candidates.map { WindowSlot(app: app, id: $0.0.id) })
        windows = windows.filter { $0.key.app.pid != app.pid || ($0.key.app == app && active.contains($0.key)) }
        associatedSurfaces = associatedSurfaces.filter { windows[$0.key] != nil }
        // AX focus is an optional routing hint, never a surface ownership proof.
        // Omnibox/tooltip surfaces can be in front without receiving keys.
        var keyboardWindow: CGWindowID?
        var decorations: Set<CGWindowID> = []
        var modalReceivers: Set<CGWindowID> = []
        if !candidates.isEmpty && dtwAccessibilityGranted() {
            let hintDeadline = context.deadline.limited(to: ProcessInfo.processInfo.systemUptime + 0.1)
            do {
                // AXWindows can retain the parent while a descendant sheet has
                // its own CG surface. Its existing ancestry/owner proof also
                // supplies the routing hint; do not hide it behind its parent.
                for (id, association) in associated {
                    if try nativeModalKeyboardReceiver(association.root, deadline: hintDeadline) {
                        modalReceivers.insert(id)
                    }
                }
                let application = AXUIElementCreateApplication(app.pid)
                if let focused = try nativeFocusedWindow(application, pid: app.pid, deadline: hintDeadline) {
                    let id = try axWindowID(focused, hintDeadline)
                    if candidates.contains(where: { $0.0.id == id }) { keyboardWindow = id }
                    else if !source.contains(where: { ($0[kCGWindowNumber as String] as? UInt32) == id }),
                            try cgWindowRows(context.deadline, only: id).compactMap(cgRecord)
                                .contains(where: { $0.id == id && $0.pid == app.pid }) {
                        throw failure("window_inventory_changed", "A newly focused owned window appeared after the WindowServer snapshot")
                    }
                }
                let roots = try semanticWindowRoots(application, hintDeadline)
                for root in roots.prefix(128) {
                    guard let id = try? axWindowID(root, hintDeadline),
                          candidates.contains(where: { $0.0.id == id && $0.0.pid == app.pid }) else { continue }
                    if try nativeModalKeyboardReceiver(root, deadline: hintDeadline) { modalReceivers.insert(id) }
                    if try sharingIndicatorSurface(root,
                        role: { try axAttribute($0, kAXRoleAttribute, hintDeadline) as? String },
                        identifier: { try axAttribute($0, kAXIdentifierAttribute, hintDeadline) as? String },
                        description: { try axAttribute($0, kAXDescriptionAttribute, hintDeadline) as? String },
                        title: { try axAttribute($0, kAXTitleAttribute, hintDeadline) as? String },
                        children: { try axList($0, kAXChildrenAttribute, limit: 2, hintDeadline) }) {
                        decorations.insert(id)
                    }
                }
            } catch let error as NativeFailure where ["ax_unavailable", "window_unproven", "focus_unproven", "deadline_exceeded"].contains(error.code) {
                // A local hint timeout or unreadable AX must not poison CG reads.
                try context.deadline.check()
                try context.verify(app)
            }
        }
        var rows: [[String: Any]] = []
        for (record, title) in candidates {
            var row = try registerSurface(record, title: title, app: app, association: associated[record.id])
            row["scene"] = ["bounds": record.coordinates,
                            "rank": visibleSource.firstIndex { ($0[kCGWindowNumber as String] as? UInt32) == record.id } ?? (visibleSource.count + (source.firstIndex { ($0[kCGWindowNumber as String] as? UInt32) == record.id } ?? source.count))]
            if let keyboardWindow {
                var scene = row["scene"] as! [String: Any]
                scene["keyboard_focused"] = record.id == keyboardWindow
                row["scene"] = scene
            }
            if modalReceivers.contains(record.id) {
                var scene = row["scene"] as! [String: Any]
                scene["keyboard_modal"] = true
                row["scene"] = scene
            }
            if decorations.contains(record.id) {
                var scene = row["scene"] as! [String: Any]
                scene["semantic_candidate"] = false
                scene["keyboard_focused"] = false
                row["scene"] = scene
            }
            rows.append(row)
        }
        // Associated AX root replacement can keep the CG ID/owner while making
        // a new generation above. Retire caches against those final targets,
        // never against the old slot contents before generation assignment.
        reconcileWindowBindings()
        return ["windows": rows, "coverage": coverage]
    }

    private func probeWindowState(_ request: [String: Any], _ context: RequestContext) throws -> [String: Any] {
        let target = try NativeTarget(request["target"])
        guard let value = request["state"] as? String, let desired = ScopedWindowCondition(rawValue: value) else {
            throw failure("invalid_request", "Unsupported window condition")
        }
        let slot = WindowSlot(app: target.app, id: target.nativeID)
        guard windows[slot] == target else { throw failure("stale_window", "Window condition requires its registered target") }
        func check() throws {
            guard !dtwInputStopping() else { throw failure("native_stopping", "Window condition read was cancelled") }
            try context.deadline.check()
            try verifyWindowWaitApp(target.app, desired: desired, context)
        }
        try check()
        // The Runtime condition deadline only controls another probe. This
        // complete read retains its original call budget, including scoped SCK
        // helpers, which are reaped before the reply and any inter-probe wait.
        do {
            let result = try windowStateSnapshot(target, desired: desired, context)
            try check()
            return result
        } catch let error as NativeFailure {
            if let reason = try windowProbeFailureReason(error, checkCaller: check) {
                return ["status":"pending", "reason":reason]
            }
            throw error
        }
    }

    private func verifyWindowWaitApp(_ app: NativeApp, desired: ScopedWindowCondition,
                                     _ context: RequestContext) throws {
        guard desired == .closed else { try context.verify(app); return }
        try verifyWindowWaitProcess(expected: app.generation, stamp: context.startStamp(app.pid),
            gone: { kill(app.pid, 0) == -1 && errno == ESRCH }, verifyLive: { try context.verify(app) })
        try context.deadline.check()
    }

    private func windowStateSnapshot(_ target: NativeTarget, desired: ScopedWindowCondition,
                                     _ context: RequestContext) throws -> [String: Any] {
        let slot = WindowSlot(app: target.app, id: target.nativeID)
        let association = associatedSurfaces[slot]
        let ownerPID = association?.ownerPID ?? target.app.pid
        let ownerBirth = association?.ownerBirth ?? target.app.generation
        func checkOwner() throws {
            if desired == .closed {
                try verifyWindowWaitProcess(expected: ownerBirth, stamp: context.startStamp(ownerPID),
                    gone: { kill(ownerPID, 0) == -1 && errno == ESRCH }, verifyLive: {})
            } else if try context.startStamp(ownerPID) != ownerBirth {
                throw failure("stale_window", "Present window owner birth identity changed")
            }
        }
        try checkOwner()
        let present = try exactWindowPresent(cgWindowRows(context.deadline, only: target.nativeID),
                                             id: target.nativeID, owner: ownerPID)
        try verifyWindowWaitApp(target.app, desired: desired, context)
        try checkOwner()
        if !present {
            if desired == .closed {
                windows.removeValue(forKey: slot)
                associatedSurfaces.removeValue(forKey: slot)
                semanticWindows.removeValue(forKey: slot)
                observations = observations.filter { $0.value.target != target }
                return ["status":"closed"]
            }
            throw failure("target_changed", "Anchor window closed before the requested condition was proven")
        }
        if desired == .closed { return ["status":"pending", "reason":"window_present"] }
        guard dtwAccessibilityGranted(request: true) else { let error = dtwPermissionError("accessibility")
            throw failure("permission_required", error["message"] as! String, details: error["details"] as? [String: Any]) }
        let anchor = try validate(target, context, input: desired == .unblocked)
        guard anchor.cg.onscreen else { return ["status":"pending", "reason":"window_not_visible"] }
        if desired == .unblocked {
            let kind = try windowConditionKind(anchor.element, context.deadline)
            return ["status":"matched", "kind":kind, "relation":"same_window",
                    "windows":[["target":target.json, "title":anchor.title, "onscreen":true]]]
        }
        let application = AXUIElementCreateApplication(target.app.pid)
        var roots = try semanticWindowRoots(application, context.deadline)
        if !roots.contains(where: { CFEqual($0, anchor.element) }) { roots.append(anchor.element) }
        var related: [(root: AXUIElement, kind: String, relation: String)] = []
        for root in roots where !CFEqual(root, anchor.element) {
            guard try axPID(root, context.deadline) == target.app.pid else {
                throw failure("modal_state_unproven", "Modal root belongs to another AX application")
            }
            let kind = try windowConditionKind(root, context.deadline)
            guard kind != "window" else { continue }
            if try axAncestor(root, is: anchor.element, context.deadline) {
                related.append((root, kind, "child_modal"))
            } else if try !axAncestor(anchor.element, is: root, context.deadline),
                      let parent = axElement(try axAttribute(root, kAXParentAttribute, context.deadline)),
                      CFEqual(parent, application), try axPID(parent, context.deadline) == target.app.pid {
                // This is an AX-proven application-modal relation, never a
                // guessed relationship based on an empty title or CG ordering.
                related.append((root, kind, "application_modal"))
            }
        }
        let terminal = try terminalModalRoots(related, equal: { CFEqual($0.root, $1.root) },
            descendant: { try axAncestor($0.root, is: $1.root, context.deadline) })
        guard !terminal.isEmpty else { return ["status":"pending", "reason":"dialog_not_present"] }
        var ready: [(root: AXUIElement, kind: String, relation: String, record: CGRecord,
                     title: String, association: SurfaceAssociation?)] = []
        var unresolvedSurface = false
        for modal in terminal {
            guard let surface = try windowModalSurface(modal.root, anchor: target, context) else {
                unresolvedSurface = true; continue
            }
            do {
                try requireNoBlockingModal(modal.root, roots: roots, windowID: surface.record.id, context: context)
                let proof = FocusModalContext(phase: .validation, originalTarget: modal.root, checkTarget: {
                    try context.verify(target.app)
                    guard let live = try exactSurfaceRecord(cgWindowRows(context.deadline, only: surface.record.id), expected: surface.record),
                          live.bounds == surface.record.bounds, live.onscreen else {
                        throw failure("target_changed", "Modal surface changed during condition validation")
                    }
                })
                try requireWindowWithoutSheet(modal.root, context, modal: proof)
            } catch let error as NativeFailure where ["target_blocked_by_modal", "unsupported_modal"].contains(error.code) {
                continue
            }
            ready.append((modal.root, modal.kind, modal.relation, surface.record, surface.title, surface.association))
        }
        guard !unresolvedSurface, ready.count == 1, let chosen = ready.first else {
            return ["status":"pending", "reason":unresolvedSurface || ready.isEmpty ? "dialog_not_ready" : "ambiguous_dialog"]
        }
        // Recheck the original anchor and the selected live root/owner. A new
        // ordinary window or another process cannot be adopted as its dialog.
        _ = try validateSurface(target, context)
        guard try axPID(chosen.root, context.deadline) == target.app.pid,
              let live = try exactSurfaceRecord(cgWindowRows(context.deadline, only: chosen.record.id), expected: chosen.record),
              live.bounds == chosen.record.bounds, live.onscreen else {
            throw failure("target_changed", "Dialog changed before publication")
        }
        if let associated = chosen.association,
           try context.startStamp(associated.ownerPID) != associated.ownerBirth {
            throw failure("stale_window", "Associated dialog owner changed before publication")
        }
        let relationStillValid: Bool
        if chosen.relation == "child_modal" {
            relationStillValid = try axAncestor(chosen.root, is: anchor.element, context.deadline)
        } else {
            relationStillValid = try axElement(axAttribute(chosen.root, kAXParentAttribute, context.deadline))
                .map { CFEqual($0, application) } ?? false
        }
        guard relationStillValid else { throw failure("stale_window", "Dialog relation changed before publication") }
        try context.verify(target.app)
        try context.deadline.check()
        let selectedSlot = WindowSlot(app: target.app, id: chosen.record.id)
        if let old = semanticWindows[selectedSlot], !CFEqual(old.element, chosen.root) {
            windows.removeValue(forKey: selectedSlot)
        }
        let row = try registerSurface(live, title: chosen.title, app: target.app, association: chosen.association)
        reconcileWindowBindings()
        if let selected = windows[selectedSlot] {
            semanticWindows[selectedSlot] = RegisteredWindow(target: selected, element: chosen.root)
        }
        return ["status":"matched", "kind":chosen.kind, "relation":chosen.relation, "windows":[row]]
    }

    private func windowConditionKind(_ root: AXUIElement, _ deadline: Deadline) throws -> String {
        let role = try axAttribute(root, kAXRoleAttribute, deadline) as? String
        if role == kAXSheetRole { return "sheet" }
        guard role == kAXWindowRole else { return "window" }
        guard let raw = try axAttribute(root, kAXModalAttribute, deadline),
              CFGetTypeID(raw) == CFBooleanGetTypeID(), let modal = raw as? Bool else {
            throw failure("modal_state_unproven", "Window has no readable Boolean modal state")
        }
        return modal ? "dialog" : "window"
    }

    private func windowModalSurface(_ root: AXUIElement, anchor: NativeTarget, _ context: RequestContext) throws
        -> (record: CGRecord, title: String, association: SurfaceAssociation?)? {
        let id = try axWindowID(root, context.deadline)
        if id != anchor.nativeID {
            let rows = try cgWindowRows(context.deadline, only: id)
            if !rows.isEmpty {
                guard let selected = try readyModalSurface(rows, id: id) else { return nil }
                if selected.pid == anchor.app.pid {
                    return (selected, String((rows[0][kCGWindowName as String] as? String ?? "").prefix(512)), nil)
                }
            }
        }
        // Some client AX roots describe a service-owned backing surface, while
        // AXWindowID names the logical parent. Geometry only narrows candidates;
        // fresh public AX ancestry + visible pixel proof must return THIS root.
        guard let rect = try axRect(root, context.deadline) else { return nil }
        let rows = try cgWindowRows(context.deadline)
        let candidates = rows.compactMap(cgRecord).filter {
            $0.id != anchor.nativeID && $0.onscreen && associatedGeometry(rect, $0.bounds)
        }
        guard candidates.count <= 128 else { throw failure("window_limit", "Modal backing surface candidates exceed the supported limit") }
        var proven: [(record: CGRecord, title: String, association: SurfaceAssociation?)] = []
        for candidate in candidates {
            guard let birth = try context.startStamp(candidate.pid),
                  let associatedRoot = try associatedClientRoot(candidate, app: anchor.app, context),
                  CFEqual(associatedRoot, root),
                  try context.startStamp(candidate.pid) == birth,
                  let live = try exactSurfaceRecord(cgWindowRows(context.deadline, only: candidate.id), expected: candidate),
                  live.bounds == candidate.bounds, live.onscreen else { continue }
            let title = String((rows.first { ($0[kCGWindowNumber as String] as? UInt32) == live.id }?[kCGWindowName as String] as? String ?? "").prefix(512))
            proven.append((live, title, SurfaceAssociation(ownerPID: live.pid, ownerBirth: birth, root: root)))
        }
        return proven.count == 1 ? proven[0] : nil
    }

    fileprivate func validateSurface(_ target: NativeTarget, _ context: RequestContext,
                                     associationScope: AssociationProofScope = .full) throws -> CGRecord {
        let slot = WindowSlot(app: target.app, id: target.nativeID)
        guard windows[slot] == target else { throw failure("stale_window", "Surface was not registered in this worker generation") }
        try context.verify(target.app)
        let records = try cgWindowRows(context.deadline, only: target.nativeID).compactMap(cgRecord)
            .filter { $0.id == target.nativeID }
        let association = associatedSurfaces[slot]
        guard records.count == 1, let record = records.first,
              record.pid == (association?.ownerPID ?? target.app.pid) else {
            windows.removeValue(forKey: slot)
            semanticWindows.removeValue(forKey: slot)
            observations = observations.filter { $0.value.target != target }
            throw failure("target_changed", "WindowServer no longer proves the requested surface owner")
        }
        if let association {
            guard try retainedAssociationMatches(association.root, ownerBirth: association.ownerBirth,
                birth: { try context.startStamp(association.ownerPID) },
                root: { try associatedClientRoot(record, app: target.app, context, proofScope: associationScope) },
                equal: CFEqual) else {
                throw failure("stale_window", "Associated surface owner or public AX client relation changed; list windows again")
            }
        }
        try context.verify(target.app)
        return record
    }

    private func surfaceOwner(_ target: NativeTarget) -> [String: Any] {
        let associated = associatedSurfaces[WindowSlot(app: target.app, id: target.nativeID)]
        return ["pid": associated?.ownerPID ?? target.app.pid,
                "generation": associated?.ownerBirth ?? target.app.generation]
    }

    private func inputFramework(_ target: NativeTarget, _ context: RequestContext) throws -> String {
        guard let owner = try context.app(target.app.pid), owner.app == target.app else {
            throw failure("app_changed", "Input application incarnation changed")
        }
        let bundle = URL(fileURLWithPath: owner.metadata.path)
        let frameworks = bundle.appendingPathComponent("Contents/Frameworks")
        let known = ["Electron Framework.framework", "Chromium Framework.framework",
                     "Google Chrome Framework.framework", "Google Chrome Beta Framework.framework",
                     "Google Chrome Dev Framework.framework", "Google Chrome Canary Framework.framework",
                     "Microsoft Edge Framework.framework", "Brave Browser Framework.framework"]
        if known.contains(where: { FileManager.default.fileExists(atPath: frameworks.appendingPathComponent($0).path) }) {
            return "chromium"
        }
        if ["QtCore.framework", "Qt6Core.framework", "QtGui.framework"].contains(where: {
            FileManager.default.fileExists(atPath: frameworks.appendingPathComponent($0).path)
        }) { return "qt" }
        let info = Bundle(url: bundle)?.infoDictionary ?? [:]
        if info["NSPrincipalClass"] as? String == "NSApplication"
            || info["NSMainNibFile"] is String || info["NSMainStoryboardFile"] is String { return "appkit" }
        return "unknown"
    }

    fileprivate func validate(_ target: NativeTarget, _ context: RequestContext,
                              input: Bool = true, phase: FocusValidationPhase = .validation,
                              originalTarget: AXUIElement? = nil,
                              associationScope: AssociationProofScope = .full) throws -> CandidateWindow {
        try readInventorySnapshot(target.app, context) { snapshot in
            try validateSnapshot(target, snapshot, input: input, phase: phase,
                                 originalTarget: originalTarget, associationScope: associationScope)
        }
    }

    private func validateSnapshot(_ target: NativeTarget, _ context: RequestContext,
                                  input: Bool, phase: FocusValidationPhase,
                                  originalTarget: AXUIElement?, associationScope: AssociationProofScope) throws -> CandidateWindow {
        let slot = WindowSlot(app: target.app, id: target.nativeID)
        let record = try validateSurface(target, context, associationScope: associationScope)
        guard dtwAccessibilityGranted(request: true) else { let error = dtwPermissionError("accessibility")
            throw failure("permission_required", error["message"] as! String, details: error["details"] as? [String: Any]) }
        let associated = associatedSurfaces[slot]
        var current = try semanticWindowRoots(AXUIElementCreateApplication(target.app.pid), context.deadline, requestedID: target.nativeID)
        if let associated, !current.contains(where: { CFEqual($0, associated.root) }) {
            current.append(associated.root)
        }
        func matchesSurface(_ element: AXUIElement) throws -> Bool {
            if let associated { return CFEqual(element, associated.root) }
            return try axWindowID(element, context.deadline) == target.nativeID
        }
        func supportedRoot(_ element: AXUIElement) throws -> Bool {
            if let associated {
                return try CFEqual(element, associated.root) && axPID(element, context.deadline) == target.app.pid
            }
            return try ordinaryWindow(element, pid: target.app.pid, deadline: context.deadline)
        }
        func checkOriginalTarget() throws {
            guard phase == .postRaise else { return }
            // Prove the retained target root first. A replacement target AX
            // object must never be mistaken for an expired auxiliary popup.
            guard let originalTarget, current.contains(where: { CFEqual($0, originalTarget) }) else {
                throw failure("stale_observation", "Target AX root changed during focus establishment")
            }
            let targetStillValid = try readModalSnapshot(phase: phase, scope: .targetRoot, isTarget: true, read: {
                try matchesSurface(originalTarget) && supportedRoot(originalTarget)
            })
            guard targetStillValid else {
                throw failure("stale_observation", "Target AX root identity changed during focus establishment")
            }
        }
        try checkOriginalTarget()
        let modal = FocusModalContext(phase: phase, originalTarget: originalTarget, checkTarget: {
            guard !dtwInputStopping() else { throw failure("native_stopping", "Native input admission is closing") }
            _ = try self.validateSurface(target, context, associationScope: associationScope)
            try checkOriginalTarget()
        })
        var matched: [AXUIElement] = []
        for element in current {
            do {
                if try matchesSurface(element) { matched.append(element) }
            } catch let error as NativeFailure where error.code == "window_unproven" { continue }
        }
        guard matched.count == 1, let element = matched.first else {
            throw failure("semantic_window_unavailable", "No unique supported AX window matches this system surface")
        }
        let ordinary = try readModalSnapshot(phase: phase, scope: .targetRoot, isTarget: true, read: {
            try supportedRoot(element)
        })
        guard ordinary else {
            throw failure("semantic_window_unavailable", "No unique supported AX window matches this system surface")
        }
        if let retained = semanticWindows[slot], !CFEqual(retained.element, element) {
            throw failure("stale_observation", "AX window identity changed; observe this surface again")
        }
        if input {
            try requireNoBlockingModal(element, roots: current, windowID: target.nativeID, context: context, modal: modal)
            try requireWindowWithoutSheet(element, context, modal: modal)
        }
        try context.verify(target.app)
        try context.deadline.check()
        semanticWindows[slot] = RegisteredWindow(target: target, element: element)
        return CandidateWindow(cg: record, element: element, title: "")
    }

    private func owned(_ element: AXUIElement, by window: AXUIElement,
                       target: NativeTarget, deadline: Deadline) throws -> Bool {
        guard try axPID(element, deadline) == target.app.pid else { return false }
        if let attributed = axElement(try axAttribute(element, kAXWindowAttribute, deadline)) {
            guard try axPID(attributed, deadline) == target.app.pid else { return false }
            if !CFEqual(attributed, window) {
                guard try [kAXSheetRole, kAXMenuRole].contains(axAttribute(window, kAXRoleAttribute, deadline) as? String ?? ""),
                      try axAncestor(window, is: attributed, deadline) else { return false }
            }
        } else if try menuOwned(element, target: target, deadline: deadline) { return true }
        // A sheet is a distinct operation root. Stop at it rather than silently
        // treating a dialog control as a control of its blocked parent window.
        var cursor = element
        for _ in 0..<40 {
            guard try axPID(cursor, deadline) == target.app.pid else { return false }
            if CFEqual(cursor, window) {
                if let associated = associatedSurfaces[WindowSlot(app: target.app, id: target.nativeID)] {
                    return CFEqual(cursor, associated.root)
                }
                return try axWindowID(cursor, deadline) == target.nativeID
            }
            let role = try axAttribute(cursor, kAXRoleAttribute, deadline) as? String
            if [kAXWindowRole, kAXApplicationRole, kAXSheetRole, kAXMenuRole, kAXMenuBarRole].contains(role ?? "") { return false }
            guard let parent = axElement(try axAttribute(cursor, kAXParentAttribute, deadline)) else { return false }
            cursor = parent
        }
        return false
    }

    private func menuOwned(_ element: AXUIElement, target: NativeTarget, deadline: Deadline) throws -> Bool {
        let application = AXUIElementCreateApplication(target.app.pid)
        guard let bar = axElement(try axAttribute(application, kAXMenuBarAttribute, deadline)) else { return false }
        var current = element
        for _ in 0..<40 {
            guard try axPID(current, deadline) == target.app.pid else { return false }
            if CFEqual(current, bar) { return true }
            let role = try axAttribute(current, kAXRoleAttribute, deadline) as? String
            if [kAXWindowRole, kAXSheetRole, kAXApplicationRole].contains(role ?? "") { return false }
            guard let parent = axElement(try axAttribute(current, kAXParentAttribute, deadline)) else { return false }
            current = parent
        }
        return false
    }


    // CUA input/ax_actions.rs (96b18ccd): select the nearest collection
    // container, not its label. Copyright (c) 2026 Cua AI, Inc., MIT.
    // DTW additionally requires writable selection and retains the exact
    // container with the observed label; no pointer fallback follows a write.
    private func selectionContainer(_ element: AXUIElement, role: String, window: AXUIElement,
                                    target: NativeTarget, deadline: Deadline) throws -> AXUIElement? {
        let containers = ["AXRow", "AXCell", "AXListItem", "AXImage"]
        guard (containers + [kAXTextFieldRole, kAXStaticTextRole]).contains(role) else { return nil }
        var cursor = element
        var seen: [AXUIElement] = []
        for _ in 0..<8 {
            guard !seen.contains(where: { CFEqual($0, cursor) }) else { return nil }
            seen.append(cursor)
            let currentRole = try axAttribute(cursor, kAXRoleAttribute, deadline) as? String ?? ""
            if containers.contains(currentRole),
               try axAttribute(cursor, kAXSelectedAttribute, deadline) is Bool {
                try deadline.prepare(cursor)
                var writable = DarwinBoolean(false)
                let status = AXUIElementIsAttributeSettable(cursor, kAXSelectedAttribute as CFString, &writable)
                try deadline.check()
                if status == .success && writable.boolValue,
                   try owned(cursor, by: window, target: target, deadline: deadline) { return cursor }
                if ![.success, .attributeUnsupported, .noValue].contains(status) {
                    throw failure("ax_unavailable", "Collection selection writability unavailable",
                                  attributeFailure: AXAttributeFailure(name: kAXSelectedAttribute, error: status))
                }
            }
            if CFEqual(cursor, window) { return nil }
            guard let parent = axElement(try axAttribute(cursor, kAXParentAttribute, deadline)) else { return nil }
            cursor = parent
        }
        return nil
    }

    private func pressCapability(_ element: AXUIElement, role: String, enabled: Bool?, window: AXUIElement,
                                 target: NativeTarget, deadline: Deadline) throws -> NativePressCapability? {
        guard ![kAXWindowRole, kAXApplicationRole].contains(role), enabled != false,
              try owned(element, by: window, target: target, deadline: deadline) else { return nil }
        if enabled == true,
           let container = try selectionContainer(element, role: role, window: window, target: target, deadline: deadline) {
            return .selection(container)
        }
        if try actions(element, deadline).contains(kAXPressAction) { return enabled == true ? .action : nil }
        guard supportsStringValueEditing(role) else { return nil }
        try deadline.prepare(element)
        var writable = DarwinBoolean(false)
        let result = AXUIElementIsAttributeSettable(element, kAXFocusedAttribute as CFString, &writable)
        try deadline.check()
        if result == .attributeUnsupported || result == .noValue { return nil }
        guard result == .success else {
            throw failure("ax_unavailable", "Control focus writability unavailable",
                          attributeFailure: AXAttributeFailure(name: kAXFocusedAttribute, error: result))
        }
        return writable.boolValue ? .focus : nil
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

    private func regionElementProof(_ element: AXUIElement, root: AXUIElement, target: NativeTarget,
                                    region: CGRect, bounds: CGRect, deadline: Deadline) throws -> RegionElementProof<AXUIElement>? {
        guard let rect = try axRect(element, deadline) else {
            throw failure("geometry_unavailable", "Region candidate has no finite native geometry")
        }
        var viewports: [(AXUIElement, CGRect)] = []
        var cursor = element
        var visited: [AXUIElement] = []
        for _ in 0..<40 {
            guard try axPID(cursor, deadline) == target.app.pid,
                  !visited.contains(where: { CFEqual($0, cursor) }) else {
                throw failure("stale_observation", "Region candidate ancestry changed")
            }
            if CFEqual(cursor, root) {
                guard visibleRegionIntersection(element: rect, region: region, viewports: viewports.map { $0.1 }) != nil else { return nil }
                return RegionElementProof(region: region, window: bounds, element: rect, viewports: viewports)
            }
            visited.append(cursor)
            guard let role = try axAttribute(cursor, kAXRoleAttribute, deadline) as? String else {
                throw failure("geometry_unavailable", "Region ancestor role is unavailable; clipping cannot be proven")
            }
            if role == kAXScrollAreaRole {
                guard let viewport = try axRect(cursor, deadline) else {
                    throw failure("geometry_unavailable", "Region scroll viewport has no finite native geometry")
                }
                viewports.append((cursor, viewport))
            }
            guard let parent = axElement(try axAttribute(cursor, kAXParentAttribute, deadline)) else {
                throw failure("stale_observation", "Region candidate left its original root")
            }
            cursor = parent
        }
        throw failure("stale_observation", "Region ancestor proof exceeds its bounded scope")
    }

    private func validateRegionElement(_ element: NativeElement, root: AXUIElement, target: NativeTarget,
                                       bounds: CGRect, deadline: Deadline) throws {
        guard let expected = element.regionProof else { return }
        do {
            guard expected.window == bounds,
                  let current = try regionElementProof(element.element, root: root, target: target,
                    region: expected.region, bounds: bounds, deadline: deadline),
                  expected.matches(current, equal: CFEqual) else {
                throw failure("stale_observation", "Region element or its scroll viewport moved or changed; observe the region again")
            }
        } catch let error as NativeFailure where error.code == "geometry_unavailable"
            || error.attributeFailure?.error == .invalidUIElement {
            throw failure("stale_observation", "Region element or viewport geometry is no longer available; observe again")
        }
    }

    private func discoveryPage(_ observation: inout NativeObservation,
                               binding: String, context: RequestContext) throws -> [String: Any] {
        let window = try validate(observation.target, context, input: false)
        guard CFEqual(window.element, observation.root) else {
            throw failure("target_changed", "Discovery root was replaced")
        }
        if let scoped = observation.discoveryRoot,
           try !owned(scoped, by: window.element, target: observation.target, deadline: context.deadline) {
            throw failure("stale_observation", "Discovery subtree or menu left its original scope")
        }
        let region: CGRect?
        if observation.query.scope["kind"] as? String == "region" {
            guard try scrollRect(observation.query.scope["bounds"], "source window") == window.cg.bounds else {
                throw failure("stale_observation", "Region source window moved; capture again")
            }
            region = try scrollRect(observation.query.scope["rect"], "region")
        } else { region = nil }
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
                    if !CFEqual(element, observation.root), [kAXSheetRole, kAXWindowRole].contains(role) { continue }
                    if !observation.menuScope,
                       [kAXMenuRole, kAXMenuBarRole].contains(role),
                       !CFEqual(element, observation.root) { continue }
                    if let region, role == kAXScrollAreaRole {
                        var viewport: CGRect?
                        do { viewport = try axRect(element, pageDeadline) }
                        catch let error as NativeFailure where error.code == "ax_unavailable" {
                            // Pruning is optional. Unavailable geometry takes the
                            // full traversal; matching descendants still need a
                            // complete ancestor clipping proof before publication.
                        }
                        if regionExcludesScrollContents(role: role, bounds: viewport, region: region) { continue }
                    }
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
                    var textMatch = NativeNodeTextMatch(matched: observation.query.text == nil)
                    func text(_ attribute: String) throws -> String? {
                        let value = try axAttribute(element, attribute, pageDeadline)
                        guard let text = (value as? String) ?? (value as? NSNumber)?.stringValue else { return nil }
                        if let needle = observation.query.text {
                            textMatch.inspect(text, needle: needle, budget: &observation.textSearchBudget)
                            try pageDeadline.check()
                        }
                        return String(String.UnicodeScalarView(text.unicodeScalars.prefix(256)))
                    }
                    let title = try text(kAXTitleAttribute)
                    let value = try text(kAXValueAttribute)
                    let descriptionRead = try optionalDescription(alreadyMatched: textMatch.matched) {
                        try text(kAXDescriptionAttribute)
                    }
                    if descriptionRead.incompleteMatch { observation.note("node_unavailable") }
                    let fields = SemanticTextFields(role: role, title: title, value: value,
                        description: descriptionRead.text, subrole: subrole)
                    textMatch.includePreview(fields, needle: observation.query.text)
                    guard textMatch.matched else {
                        if textMatch.incomplete { observation.note("text_limit") }
                        continue
                    }
                    // Only possible text/role matches need a visible-region
                    // proof. A native scroll viewport clips its descendants;
                    // this is not WindowServer occlusion or pixel evidence.
                    let regionProof: RegionElementProof<AXUIElement>?
                    if let region {
                        regionProof = try regionElementProof(element, root: observation.root,
                            target: observation.target, region: region, bounds: window.cg.bounds, deadline: pageDeadline)
                        guard regionProof != nil else { continue }
                    } else { regionProof = nil }
                    let rawEnabled = try axAttribute(element, kAXEnabledAttribute, pageDeadline)
                    let enabled: Bool? = rawEnabled.flatMap {
                        CFGetTypeID($0) == CFBooleanGetTypeID() ? ($0 as? Bool) : nil
                    }
                    // Advisory support only. Press performs full modal, current
                    // ownership and enabled-state checks immediately before input.
                    let press = try pressCapability(element, role: role, enabled: enabled,
                                                 window: window.element, target: observation.target,
                                                 deadline: pageDeadline)
                    let row = NativeElement(id: UInt64(observation.elements.count + 1), identity: try semanticIdentity(element), element: element,
                                            label: fields.label, role: normalizedRole, name: fields.name,
                                            value: fields.value, enabled: enabled, pressCapability: press,
                                            canSetValue: try settable(element, role: role, enabled: enabled == true, window: window.element, target: observation.target, deadline: pageDeadline),
                                            regionProof: regionProof)
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
                    observation.note((error as? NativeFailure)?.code == "geometry_unavailable" ? "geometry_unavailable" : "node_unavailable")
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
        if let scoped = observation.discoveryRoot,
           try !owned(scoped, by: final.element, target: observation.target, deadline: context.deadline) {
            throw failure("stale_observation", "Discovery subtree or menu changed during page construction")
        }
        for row in rows {
            try validateRegionElement(row, root: final.element, target: observation.target,
                                      bounds: final.cg.bounds, deadline: context.deadline)
        }
        try context.deadline.check()
        let hasMore = !observation.pending.isEmpty || observation.buffered != nil
        return ["target": observation.target.json, "binding": binding, "elements": rows.map(\.json),
                "discovery": ["has_more": hasMore, "complete": !hasMore && observation.reason == nil,
                              "reason": observation.reason as Any? ?? NSNull(), "visited": observation.visits]]
    }

    private func focusReadinessCurrent(_ scope: BorrowedFocus, _ deadline: Deadline) throws -> Bool {
        guard !dtwInputStopping() else { throw failure("native_stopping", "Semantic materialization was cancelled") }
        try deadline.check()
        guard let current = borrowedFocus, current.activationEpoch == scope.activationEpoch,
              current.target == scope.target else { return false }
        return try ownsBorrowedFocus(scope, deadline, traceStage: "semantic.foreground_readiness")
    }

    private func consumeFocusReadiness(_ target: NativeTarget, shellOnly: Bool, complete: Bool,
                                       _ deadline: Deadline) throws -> (BorrowedFocus, TimeInterval)? {
        guard let scope = borrowedFocus, scope.target == target else { return nil }
        var opportunity = scope.semanticReadiness
        // NativeStore's lock covers this call. Publish consumption even when a
        // current-focus read throws; never recreate a missing borrow on return.
        defer { borrowedFocus?.semanticReadiness = opportunity }
        guard let until = try opportunity.consume(shellOnly: shellOnly, complete: complete,
            now: ProcessInfo.processInfo.systemUptime, deadline: deadline.uptime, audit: { until in
                try self.focusReadinessCurrent(scope, deadline.limited(to: until))
            }) else { return nil }
        return (scope, until)
    }

    // Source: CUA 0.38.0 AX enablement. Opt-in is process-scoped, but readiness
    // is sampled for every new query. No bundle-name classifier or whole-page
    // loading assertion is used. Unknown custom leaves remain explicitly partial.
    private func prepareSemantics(_ target: NativeTarget, _ context: RequestContext,
                                  materializationUntil: TimeInterval? = nil) throws -> String? {
        _ = try validateSurface(target, context)
        guard dtwAccessibilityGranted(request: true) else { let error = dtwPermissionError("accessibility")
            throw failure("permission_required", error["message"] as! String, details: error["details"] as? [String: Any]) }
        let application = AXUIElementCreateApplication(target.app.pid)
        let enablement: SemanticEnablement
        let enablementCached = enabledApps[target.app] != nil
        if let cached = enabledApps[target.app] { enablement = cached }
        else {
            try context.deadline.prepare(application)
            let manualAttemptedAt = ProcessInfo.processInfo.systemUptime
            var result = AXUIElementSetAttributeValue(application, "AXManualAccessibility" as CFString, kCFBooleanTrue)
            let manualResult = result.rawValue
            let manual = result == .success
            var enhancedResult: Int32?
            var enhancedAttemptedAt: TimeInterval?
            if result != .success {
                let attemptedAt = ProcessInfo.processInfo.systemUptime
                try context.deadline.prepare(application)
                result = AXUIElementSetAttributeValue(application, "AXEnhancedUserInterface" as CFString, kCFBooleanTrue)
                enhancedResult = result.rawValue
                if result == .success { enhancedAttemptedAt = attemptedAt }
            }
            try context.deadline.check()
            // These application opt-ins are optional capabilities. Their AX
            // errors say nothing about ordinary window/tree readability. The
            // actual reads below still prove permission, identity and coverage.
            try context.verify(target.app)
            enablement = SemanticEnablement(manual: manual, manualResult: manualResult,
                                            manualAttemptedAt: manual ? manualAttemptedAt : nil,
                                            enhancedResult: enhancedResult, enhancedAttemptedAt: enhancedAttemptedAt)
            enabledApps[target.app] = enablement
        }
        let started = ProcessInfo.processInfo.systemUptime
        // Keep the first successful Manual poke's clock in the birth-keyed
        // cache. Reading another window/query never restarts its debounce.
        let manualUntil = enablement.manualAttemptedAt.map { $0 + semanticReadinessBudget }
        let manualWaitUntil = semanticMaterializationUntil(attemptedAt: enablement.manualAttemptedAt, now: started)
        var until = min(context.deadline.uptime, started + semanticReadinessBudget,
                        materializationUntil ?? .greatestFiniteMagnitude, manualWaitUntil ?? .greatestFiniteMagnitude)
        // An expired attempt still classifies a fresh, complete shell as
        // unproven. Only its remaining time can extend a readonly wait; later
        // queries scan once without another poke or a restarted budget.
        var readiness = context.deadline.limited(to: until)
        var foregroundReadiness: BorrowedFocus?
        var samples = 0
        var probedEmptyRoots: [AXUIElement] = []
        var last: [[String: Any]] = []
        var lastPending = false
        var lastManualShellPending = false
        var waitedOnReady = false
        var lastFlags: [String: Bool] = [:]
        var lastScanTruncated = false
        let tracing = ProcessInfo.processInfo.environment["DTW_AX_READINESS_TRACE"] == "1"
        func trace(_ decision: String) {
            guard tracing else { return }
            let value: [String: Any] = ["decision":decision, "samples":samples,
                "elapsed_ms":Int((ProcessInfo.processInfo.systemUptime-started)*1000),
                "manual_opt_in":enablement.manual,
                "enablement_cached":enablementCached,
                "manual_setter_result":enablement.manualResult,
                "enhanced_setter_result":enablement.enhancedResult as Any? ?? NSNull(),
                "materialization_recheck":materializationUntil != nil,
                "foreground_readiness":foregroundReadiness != nil,
                "readiness_flags":lastFlags, "scan_truncated":lastScanTruncated, "nodes":last]
            if let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) {
                FileHandle.standardError.write(Data("dtw_semantic_readiness ".utf8) + data + Data("\n".utf8))
            }
        }
        func materializationPending(until: TimeInterval?, populated: Bool = false) throws -> Bool {
            try semanticMaterializationPending(until: until,
                now: ProcessInfo.processInfo.systemUptime, populated: populated, check: {
                    guard !dtwInputStopping() else { throw failure("native_stopping", "Semantic materialization was cancelled") }
                    try context.deadline.check()
                })
        }
        do {
            while ProcessInfo.processInfo.systemUptime < until {
                _ = try materializationPending(until: manualUntil ?? materializationUntil)
                let window = try validate(target, context, input: false)
                // Breadth-first and bounded: inspect container roots before a
                // wide toolbar's descendants. Healthy providers stay paged;
                // count/range failures use the capped compatibility copy.
                var queue: [(AXUIElement, Int, String, Bool)] = [(window.element, 0, "", false)]
                var index = 0
                var sample = SemanticReadinessSample()
                last = []
                lastScanTruncated = false
                while index < queue.count && index < 400 {
                    let (element, depth, parentRole, parentDecoration) = queue[index]; index += 1
                    let role = try axAttribute(element, kAXRoleAttribute, readiness) as? String ?? ""
                    var decoration = parentDecoration
                    if manualUntil != nil && role == kAXButtonRole && !decoration {
                        let value = try axAttribute(element, kAXSubroleAttribute, readiness)
                        guard value == nil || value is String else {
                            throw failure("ax_unavailable", "Native window decoration subrole was not a string")
                        }
                        decoration = semanticWindowDecoration(role: role, subrole: value as? String)
                    }
                    let count = try axChildCount(element, readiness, required: false)
                    sample.add(role: role, children: count, depth: depth, parentRole: parentRole, decoration: decoration)
                    var node: [String: Any] = ["role":role,"children":count,"depth":depth,"parent":parentRole,
                                             "window_decoration":decoration]
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
                        if admitted < count { lastScanTruncated = true }
                        while offset < admitted {
                            let children = try axChildRange(element, index: offset, count: min(16, admitted-offset), readiness)
                            queue.append(contentsOf: children.map { ($0, depth+1, role, decoration) }); offset += children.count
                        }
                    } else if depth >= 10 && role != kAXMenuBarRole && count > 0 {
                        lastScanTruncated = true
                    }
                }
                if index >= 400 && index < queue.count { lastScanTruncated = true }
                lastFlags = ["populated_web_area":sample.populatedWebArea,
                    "empty_web_area":sample.emptyWebArea, "shallow_unknown":sample.shallowUnknown,
                    "empty_content_group":sample.emptyContentGroup, "shell_only":sample.shellOnly]
                samples += 1
                _ = try materializationPending(until: manualUntil, populated: sample.populatedWebArea)
                let sampledAt = ProcessInfo.processInfo.systemUptime
                var decision = sample.decision(manualOptIn: enablement.manual,
                    elapsed: sampledAt-started,
                    materializationRemaining: manualUntil.map { $0-sampledAt }, scanComplete: !lastScanTruncated)
                lastPending = decision != .ready
                // Expiry during this final sample must not turn an observed
                // shell into ready. Preserve the attempt even at zero budget.
                lastManualShellPending = manualUntil != nil && !lastScanTruncated && sample.shellOnly
                if decision == .emptyContent, lastManualShellPending, foregroundReadiness == nil,
                   let (scope, opportunityUntil) = try consumeFocusReadiness(target,
                       shellOnly: sample.shellOnly, complete: !lastScanTruncated, context.deadline) {
                    foregroundReadiness = scope
                    until = min(until, opportunityUntil)
                    readiness = context.deadline.limited(to: until)
                }
                if foregroundReadiness != nil, lastManualShellPending {
                    decision = sample.decision(manualOptIn: enablement.manual, elapsed: sampledAt-started,
                        materializationRemaining: until-ProcessInfo.processInfo.systemUptime, scanComplete: true)
                }
                // A fresh ready tree does not depend on foreground. Only
                // additional waiting needs the still-owned exact scope.
                if let scope = foregroundReadiness, decision == .pending, samples > 1,
                   try !focusReadinessCurrent(scope, readiness) {
                    trace("foreground_readiness_lost")
                    return "content_unproven"
                }
                lastPending = decision != .ready
                waitedOnReady = try decision == .ready && materializationPending(until: materializationUntil,
                    populated: sample.populatedWebArea)
                if samples == 1 { trace(waitedOnReady ? "materialization_pending" : decision.rawValue) }
                switch decision {
                case .ready:
                    if waitedOnReady { break }
                    if samples > 1 { trace("ready") }
                    return nil
                case .opaque: trace("unsupported_semantic_tree"); return "unsupported_semantic_tree"
                case .emptyContent: trace("content_unproven"); return "content_unproven"
                case .pending: break
                }
                CFRunLoopRunInMode(.defaultMode, min(0.05, max(0, until-ProcessInfo.processInfo.systemUptime)), false)
            }
        } catch let error as NativeFailure where error.code == "deadline_exceeded" {
            try context.deadline.check()
        }
        _ = try materializationPending(until: manualUntil ?? materializationUntil)
        try context.deadline.check()
        if lastManualShellPending {
            trace("content_unproven")
            return "content_unproven"
        }
        // This bounded second look does not require web content in a native
        // window. A still-readable, genuinely empty native tree remains valid.
        if materializationUntil != nil && (waitedOnReady || samples == 0) {
            trace("materialization_window_elapsed")
            return nil
        }
        trace(lastPending ? "semantic_not_ready" : "probe_budget_exhausted")
        throw failure("semantic_not_ready", "Native accessibility roots remained unproven within the readiness budget; capture remains available")
    }

    private func observe(_ request: [String: Any], _ context: RequestContext) throws -> [String: Any] {
        let target = try NativeTarget(request["target"])
        traceSemanticFailure(["operation":"observe_start", "window":target.nativeID])
        let query = try NativeQuery(request["query"])
        let source: RetainedSubtree<NativeElement, AXUIElement>?
        let sourceInMenu: Bool
        if query.scope["kind"] as? String == "subtree" {
            guard let binding = query.scope["binding"] as? String,
                  let prior = observations[binding], prior.target == target,
                  let retained = prior.elements.first(where: { $0.id == (try? integer(query.scope["element"], "scope.element")) }) else {
                throw failure("stale_observation", "Subtree source reference is no longer retained")
            }
            source = RetainedSubtree(element: retained, root: prior.root)
            sourceInMenu = prior.menuScope
        } else { source = nil; sourceInMenu = false }
        guard observations.count < 8 else {
            throw failure("registry_exhausted", "At most eight live semantic queries may be retained")
        }
        semanticWindows.removeValue(forKey: WindowSlot(app: target.app, id: target.nativeID))
        let readinessReason = query.scope["kind"] as? String == "menu" ? nil : try prepareSemantics(target, context)
        let window = try validate(target, context, input: false)
        let root: AXUIElement
        if query.scope["kind"] as? String == "menu" {
            guard let bar = axElement(try axAttribute(AXUIElementCreateApplication(target.app.pid), kAXMenuBarAttribute, context.deadline)),
                  try axPID(bar, context.deadline) == target.app.pid else {
                throw failure("semantic_menu_unavailable", "The application has no proven AX menu bar")
            }
            root = bar
        } else if let source {
            let source = try source.checkedElement(in: window.element, equal: CFEqual)
            try validateRegionElement(source, root: window.element, target: target,
                                      bounds: window.cg.bounds, deadline: context.deadline)
            guard try owned(source.element, by: window.element, target: target, deadline: context.deadline) else {
                throw failure("stale_observation", "Subtree source left the original window")
            }
            root = source.element
        } else { root = window.element }
        if try sharingIndicatorSurface(window.element,
            role: { try axAttribute($0, kAXRoleAttribute, context.deadline) as? String },
            identifier: { try axAttribute($0, kAXIdentifierAttribute, context.deadline) as? String },
            description: { try axAttribute($0, kAXDescriptionAttribute, context.deadline) as? String },
            title: { try axAttribute($0, kAXTitleAttribute, context.deadline) as? String },
            children: { try axList($0, kAXChildrenAttribute, limit: 2, context.deadline) }) {
            throw failure("non_content_surface", "This native sharing indicator is not application content")
        }
        let binding = UUID().uuidString
        var observation = NativeObservation(target: target, root: window.element, query: query,
                                            discoveryRoot: CFEqual(root, window.element) ? nil : root,
                                            menuScope: query.scope["kind"] as? String == "menu" || sourceInMenu,
                                            pending: [.node(root, 0, 0)])
        observation.reason = readinessReason
        var page = try discoveryPage(&observation, binding: binding, context: context)
        let complete = observation.pending.isEmpty && observation.buffered == nil && observation.reason == nil
        if query.isWindowScope, let until = emptySemanticRetryUntil(filtered: query.role != nil || query.text != nil,
            complete: complete, matches: observation.elements.count,
            enhancedAttemptedAt: enabledApps[target.app]?.enhancedAttemptedAt,
            now: ProcessInfo.processInfo.systemUptime) {
            let reason = try prepareSemantics(target, context, materializationUntil: until)
            let refreshed = try validate(target, context, input: false)
            observation = NativeObservation(target: target, root: refreshed.element, query: query,
                                            pending: [.node(refreshed.element, 0, 0)])
            observation.reason = reason
            page = try discoveryPage(&observation, binding: binding, context: context)
        }
        observations[binding] = observation
        return page
    }

    private func readText(_ request: [String: Any], _ context: RequestContext) throws -> [String: Any] {
        guard let binding = request["binding"] as? String, var observation = observations[binding] else {
            throw failure("stale_observation", "Text source observation is no longer retained")
        }
        let target = try NativeTarget(request["target"])
        let id = try integer(request["element"], "element")
        guard target == observation.target, let retained = observation.elements.first(where: { $0.id == id }) else {
            throw failure("stale_observation", "Text source is not part of this observation")
        }
        let window = try validate(target, context, input: false)
        guard CFEqual(window.element, observation.root),
              try owned(retained.element, by: window.element, target: target, deadline: context.deadline) else {
            throw failure("stale_observation", "Text source object or ancestry changed")
        }
        try validateRegionElement(retained, root: window.element, target: target,
                                  bounds: window.cg.bounds, deadline: context.deadline)
        let snapshot: String
        if let previous = observation.textSnapshots[id] {
            guard try nativeText(retained.element, context.deadline) == previous else {
                observations.removeValue(forKey: binding)
                throw failure("stale_observation", "Text changed between pages; observe and read again")
            }
            snapshot = previous
        } else {
            snapshot = try nativeText(retained.element, context.deadline)
            let retainedUnits = observation.textSnapshots.values.reduce(0) { $0 + ($1 as NSString).length }
            guard retainedUnits + (snapshot as NSString).length <= 2_097_152 else {
                throw failure("resource_limit", "This observation's native text snapshots exceed the 2097152 UTF-16 unit budget; observe a smaller subtree")
            }
        }
        let rawOffset = try integer(request["offset"] ?? 0, "offset")
        let rawLimit = try integer(request["limit"] ?? 4_096, "limit")
        guard let offset = Int(exactly: rawOffset), let limit = Int(exactly: rawLimit) else { throw failure("invalid_request", "Text range exceeds integer bounds") }
        let (text, next) = try textPage(snapshot, offset: offset, limit: limit)
        let final = try validate(target, context, input: false)
        guard CFEqual(window.element, final.element),
              try owned(retained.element, by: final.element, target: target, deadline: context.deadline),
              try nativeText(retained.element, context.deadline) == snapshot else {
            throw failure("stale_observation", "Text or source changed during page construction")
        }
        try validateRegionElement(retained, root: final.element, target: target,
                                  bounds: final.cg.bounds, deadline: context.deadline)
        observation.textSnapshots[id] = snapshot
        observations[binding] = observation
        return ["target": target.json, "text": text, "offset": offset,
                "next_offset": next as Any? ?? NSNull(), "total_length": (snapshot as NSString).length, "unit": "utf16"]
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

    private func focused(_ target: NativeTarget, _ root: AXUIElement, _ deadline: Deadline,
                         traceStage: String? = nil) throws -> Bool {
        try focused(target.app.pid, root,
                    windowID: axWindowID(root, deadline, traceStage: traceStage.map { "\($0).requested" }),
                    deadline, traceStage: traceStage)
    }

    // Parent resolution is only for activation/raise. The exact requested
    // surface remains the focus proof.
    private func activationRoot(_ surface: AXUIElement, pid: Int32, deadline: Deadline,
                                failureCode: String = "focus_unproven") throws -> AXUIElement {
        guard let root = try resolveActivationWindow(surface, pid: pid,
            owner: { try axPID($0, deadline) },
            role: { try axAttribute($0, kAXRoleAttribute, deadline) as? String },
            parent: { axElement(try axAttribute($0, kAXParentAttribute, deadline)) }) else {
            throw failure(failureCode, "Surface has no proven logical application window for activation")
        }
        return root
    }

    private func focused(_ pid: Int32, _ root: AXUIElement?, windowID: CGWindowID,
                         _ deadline: Deadline, traceStage: String? = nil) throws -> Bool {
        try exactInputFocus(expectedPID: pid, focusedApplication: {
            try nativeFocusedApplication(expectedPID: pid, deadline: deadline, traceStage: traceStage)
        }, windowMatches: { app in
            guard let root else {
                throw failure("focus_unproven", "Requested window has no exact AX focus identity")
            }
            guard let window = try nativeFocusedWindow(app, pid: pid, deadline: deadline,
                                                       traceStage: traceStage) else {
                throw missingFocus(.window)
            }
            return try exactFocusedSurface(requested: root, requestedWindow: windowID,
                                           focused: window, equal: CFEqual,
                                           windowID: { try axWindowID($0, deadline,
                                               traceStage: traceStage.map { "\($0).current" }) })
        })
    }

    private func ownsBorrowedFocus(_ borrowed: BorrowedFocus, _ deadline: Deadline,
                                   traceStage: String) throws -> Bool {
        if borrowed.window == nil {
            _ = try validateSurface(borrowed.target, RequestContext(deadline: deadline))
            return try !focusObserver.invalidated(borrowed.activationEpoch)
                && windowServerFrontmost(borrowed.target.app.pid, deadline)
        }
        let started = ProcessInfo.processInfo.systemUptime
        var completed = false
        traceSemanticFailure(["operation":"focus_ownership", "stage":traceStage, "phase":"begin",
                              "remaining_ms":(deadline.uptime - started) * 1_000])
        defer {
            traceSemanticFailure(["operation":"focus_ownership", "stage":traceStage, "phase":"end",
                                  "completed":completed,
                                  "elapsed_ms":(ProcessInfo.processInfo.systemUptime - started) * 1_000,
                                  "remaining_ms":(deadline.uptime - ProcessInfo.processInfo.systemUptime) * 1_000])
        }
        let owned = try borrowedFocusStillOwned(app: borrowed.target.app, windowID: borrowed.focusWindowID,
            invalidated: { focusObserver.invalidated(borrowed.activationEpoch) }, focusedApplication: {
                try nativeFocusedApplication(expectedPID: borrowed.target.app.pid, deadline: deadline,
                                             traceStage: traceStage)
            }, currentWindowID: { app in
                guard let current = try nativeFocusedWindow(app, pid: borrowed.target.app.pid, deadline: deadline,
                                                            traceStage: traceStage) else {
                    return nil
                }
                return try axWindowID(current, deadline, traceStage: traceStage)
            })
        completed = true
        return owned
    }

    fileprivate func validateVisualInput(_ target: NativeTarget, _ context: RequestContext) throws {
        guard dtwAccessibilityGranted() else {
            let error = dtwPermissionError("accessibility")
            throw failure("permission_required", error["message"] as! String, details: error["details"] as? [String: Any])
        }
        guard let scope = borrowedFocus, scope.target == target else {
            throw failure("focus_changed", "Visual foreground admission changed; inspect before another input")
        }
        try requireExecutionForeground(["_execution_foreground": scope.executionScoped], target, context)
        guard try ownsBorrowedFocus(scope, context.deadline, traceStage: "visual.pixel") else {
            throw failure("focus_changed", "Visual foreground admission changed; inspect before another input")
        }
    }

    private func borrowFocus(_ target: NativeTarget, _ window: AXUIElement?, _ context: RequestContext,
                             inputPoint: CGPoint? = nil, allowActivation: Bool = true,
                              executionScoped: Bool = false) throws {
        guard !dtwInputStopping() else { throw failure("native_stopping", "Input admission is closing") }
        if !allowActivation {
            guard try windowServerFrontmost(target.app.pid, context.deadline),
                  try window.map({ try focused(target, $0, context.deadline) }) ?? true else {
                throw failure("focus_changed", "Execution foreground changed; activation is forbidden")
            }
        }
        let focusID = try window.map { try axWindowID($0, context.deadline) } ?? target.nativeID
        if let focusCleanupFailure { throw focusCleanupFailure }
        if let prior = borrowedFocus {
            if let continued = try retargetFocusBorrow(prior, owned: {
                try ownsBorrowedFocus(prior, context.deadline, traceStage: "retarget.borrowed_current")
            }, retarget: {
                try focusObserver.retarget(prior.activationEpoch, to: target.app.pid,
                                           window: window == nil ? nil : focusID, deadline: context.deadline)
            }, update: { next in
                next.changedForeground = next.changedForeground || prior.target != target || prior.focusWindowID != focusID
                if next.target != target { next.semanticReadiness.discard() }
                next.target = target; next.window = window; next.focusWindowID = focusID
                next.inputPoint = inputPoint
                next.executionScoped = prior.executionScoped || executionScoped
            }) {
                borrowedFocus = continued
                return
            }
            // An external choice ends the old borrow permanently. A later
            // explicit focus starts from the user's new foreground, never the
            // prior target remembered by an earlier focus in this script.
            focusObserver.end(prior.activationEpoch); borrowedFocus = nil
        }
        // This action-local scope owns pointer cleanup only. The permanent
        // resident execution scope separately retains the original foreground.
        try context.deadline.check()
        let initialPID = dtwFrontProcessPID()
        guard initialPID > 0 else { throw failure("focus_unproven", "Current foreground application is unavailable") }
        let alreadyFocused: Bool
        if let window {
            alreadyFocused = try initialPID == target.app.pid && focused(target, window, context.deadline)
        } else {
            alreadyFocused = initialPID == target.app.pid
        }
        let epoch = try focusObserver.begin(initialPID: initialPID, targetPID: target.app.pid,
                                            window: window == nil ? nil : focusID, deadline: context.deadline)
        _ = dtwTakePostedPointerPosition() // A prior action never owns this borrow's pointer.
        borrowedFocus = BorrowedFocus(activationEpoch: epoch, changedForeground: !alreadyFocused,
                                      target: target, window: window, focusWindowID: focusID,
                                      pointer: CGEvent(source: nil)?.location, inputPoint: inputPoint,
                                       executionScoped: executionScoped)
    }

    private func establishVisualFocus(_ target: NativeTarget, _ context: RequestContext,
                                      allowActivation: Bool, changed: inout Bool) throws -> CGRecord {
        let surface = try validateSurface(target, context)
        guard dtwAccessibilityGranted(request: true) else {
            let error = dtwPermissionError("accessibility")
            throw failure("permission_required", error["message"] as! String, details: error["details"] as? [String: Any])
        }
        guard let scope = borrowedFocus, scope.target == target,
              !focusObserver.invalidated(scope.activationEpoch) else {
            throw failure("focus_changed", "Visual input lost its admitted foreground scope")
        }
        if try activationRequired(allowed: allowActivation, isForeground: {
            try windowServerFrontmost(target.app.pid, context.deadline)
        }) {
            guard let _: Bool = try dtwWithInputAdmission({
                try context.deadline.check()
                changed = true
                // A foreign sheet/menu ID cannot be made key in its logical
                // client process. AppKit activation handles that relationship;
                // the following surface/hit proof must still survive it.
                if surface.pid == target.app.pid && !dtwSetVisualFrontProcess(target.app.pid, target.nativeID) {
                    throw failure("focus_failed", "WindowServer did not accept visual foreground activation")
                }
                guard let app = NSRunningApplication(processIdentifier: target.app.pid),
                      app.activate(options: []) else {
                    throw failure("focus_failed", "AppKit did not accept visual application activation")
                }
                return true
            }) else { throw failure("native_stopping", "Native input admission is closed") }
        }
        let until = min(context.deadline.uptime, ProcessInfo.processInfo.systemUptime + 0.75)
        while true {
            try context.deadline.check()
            guard !dtwInputStopping(), !focusObserver.invalidated(scope.activationEpoch) else {
                throw failure("focus_changed", "Foreground changed while admitting visual input")
            }
            let current = try validateSurface(target, context)
            if try windowServerFrontmost(target.app.pid, context.deadline) {
                focusObserver.established(scope.activationEpoch, pid: target.app.pid, window: target.nativeID)
                return current
            }
            guard ProcessInfo.processInfo.systemUptime < until else {
                throw failure("focus_unproven", "Visual application foreground could not be confirmed")
            }
            CFRunLoopRunInMode(.defaultMode, 0.01, false)
        }
    }

    private func establishFocus(_ target: NativeTarget, _ context: RequestContext, changed: inout Bool,
                                allowActivation: Bool = true) throws -> CandidateWindow {
        let window = try validate(target, context, phase: .preRaise)
        if try coordinateMenuSurface(role: axAttribute(window.element, kAXRoleAttribute, context.deadline) as? String,
            associated: associatedSurfaces[WindowSlot(app: target.app, id: target.nativeID)] != nil) {
            // The association and real surface owner have just been verified.
            // A menu has no AXWindow to raise; only the existing exact visible-
            // pixel path may proceed, with its own live owner and hit proof.
            throw failure("semantic_window_unavailable", "This associated menu has no AXWindow focus target; use its verified pixel route")
        }
        if !allowActivation {
            guard try windowServerFrontmost(target.app.pid, context.deadline),
                  try focused(target, window.element, context.deadline) else {
                throw failure("focus_changed", "Execution foreground changed; activation is forbidden")
            }
            try borrowFocus(target, window.element, context, inputPoint: borrowedFocus?.inputPoint, allowActivation: false)
            guard let scope = borrowedFocus, !focusObserver.invalidated(scope.activationEpoch) else {
                throw failure("focus_changed", "Execution foreground proof was invalidated")
            }
            focusObserver.established(scope.activationEpoch, pid: target.app.pid,
                                      window: try axWindowID(window.element, context.deadline))
            return window
        }
        let focusWindow = try activationRoot(window.element, pid: target.app.pid, deadline: context.deadline)
        let focusID = try axWindowID(focusWindow, context.deadline)
        let focusedSurfaceID = try axWindowID(window.element, context.deadline)
        guard let focusRecord = try cgWindows(context.deadline).first(where: { $0.id == focusID }),
              focusRecord.pid == target.app.pid else {
            throw failure("focus_unproven", "Logical focus window has no matching WindowServer owner")
        }
        // CUA can establish app-local key/AX focus without raising a window.
        // AX focus alone cannot short-circuit a physical foreground transition.
        // Only a fresh WindowServer + exact AX proof can establish no borrow.
        func trace(_ phase: String) {
            guard ProcessInfo.processInfo.environment["DTW_AX_READINESS_TRACE"] == "1" else { return }
            let line = "dtw_focus_transition pid=\(target.app.pid) window=\(target.nativeID) activation_window=\(focusID) phase=\(phase)\n"
            FileHandle.standardError.write(Data(line.utf8))
        }
        // prepare_foreground already owns the transition to this target. Do
        // not replace its activation epoch merely because the raise has not run yet.
        if borrowedFocus?.target != target || borrowedFocus.map({ focusObserver.invalidated($0.activationEpoch) }) != false {
            try borrowFocus(target, window.element, context, inputPoint: borrowedFocus?.inputPoint)
        }
        if let borrowed = borrowedFocus, !borrowed.changedForeground {
            guard try windowServerFrontmost(target.app.pid, context.deadline),
                  try focused(target, window.element, context.deadline),
                  try windowServerFrontmost(target.app.pid, context.deadline),
                  !focusObserver.invalidated(borrowed.activationEpoch) else {
                throw failure("focus_changed", "Foreground changed after the exact in-place target was captured. Refresh the target before continuing")
            }
            try context.verify(target.app)
            guard !dtwInputStopping() else { throw failure("native_stopping", "Native input admission is closing") }
            focusObserver.established(borrowed.activationEpoch, pid: target.app.pid, window: focusedSurfaceID)
            trace("already_at_exact_target")
            return window
        }
        try context.deadline.prepare(focusWindow)
        // Read-only AX preflight is outside admission. The worker's cross-process
        // input owner still covers all focus checks and the ensuing input.
        var unconfirmedRaise: AXError?
        guard let _: Bool = try dtwWithInputAdmission({
            try context.deadline.check()
            changed = true // Raising itself may have effects even if focus is never proven.
            // Ported CUA exact-window foreground primitive. App-local AX focus
            // after background delivery is not WindowServer activation; AppKit
            // activate() may accept that already-active state without fronting it.
            trace("front_process_requested")
            // This path requires exact AX window focus, including for a physical
            // pointer payload. App-only activation can leave a never-key window
            // without AXFocusedWindow. Reuse CUA's exact-window preparation;
            // pixel-only surfaces retain establishVisualFocus without key records.
            guard dtwSetFrontProcess(target.app.pid, focusID) else {
                throw failure("focus_failed", "WindowServer did not accept exact foreground activation")
            }
            trace("front_process_returned")
            trace("raise_requested")
            let raiseStarted = ProcessInfo.processInfo.systemUptime
            let raised = AXUIElementPerformAction(focusWindow, kAXRaiseAction as CFString)
            traceSemanticFailure(["operation":"focus_raise", "phase":"returned", "pid":target.app.pid,
                "window":target.nativeID, "activation_window":focusID, "ax_error":raised.rawValue,
                "elapsed_ms":(ProcessInfo.processInfo.systemUptime - raiseStarted) * 1_000])
            if raised != .success {
                traceSemanticFailure(["operation":"focus_raise", "phase":"establish", "pid":target.app.pid,
                                      "window":target.nativeID, "activation_window":focusID, "ax_error":raised.rawValue])
            }
            unconfirmedRaise = try activationAfterRaise(raised) {
                try context.deadline.check()
                guard !dtwInputStopping() else { throw failure("native_stopping", "Input closed after raising the window") }
                trace("activate_requested")
                guard let app = NSRunningApplication(processIdentifier: target.app.pid),
                      app.activate(options: []) else { throw failure("focus_failed", "Application activation was not accepted") }
                trace("activate_returned")
            }
            return true
        }) else { throw failure("native_stopping", "Native input admission is closed") }
        // Do not start another poll after 750ms. A poll already in flight keeps
        // the request's absolute deadline and each AX read's 200ms timeout.
        let until = min(context.deadline.uptime, ProcessInfo.processInfo.systemUptime + 0.75)
        let established = try verifyAfterRaise(unconfirmedRaise) {
            try waitForEstablishedFocus(remaining: {
                until - ProcessInfo.processInfo.systemUptime
            }, check: {
                try context.deadline.check()
                guard !dtwInputStopping() else { throw failure("native_stopping", "Native input admission is closing") }
                try context.verify(target.app)
            }, read: {
                trace("post_raise_validate_begin")
                let live: CandidateWindow
                do { live = try validate(target, context, phase: .postRaise, originalTarget: window.element) }
                catch {
                    traceSemanticFailure(["operation":"focus_validation", "phase":"post_raise_failed",
                        "pid":target.app.pid, "window":target.nativeID, "activation_window":focusID,
                        "error":(error as? NativeFailure)?.code ?? "unknown",
                        "attribute":(error as? NativeFailure)?.attributeFailure?.name ?? "",
                        "ax_error":(error as? NativeFailure)?.attributeFailure?.error.rawValue ?? 0])
                    throw error
                }
                trace("post_raise_validate_returned")
                return try focused(target, live.element, context.deadline) ? live : nil
            }, wait: { interval in
                CFRunLoopRunInMode(.defaultMode, interval, false)
            })
        }
        trace("focus_established")
        if let borrowed = borrowedFocus { focusObserver.established(borrowed.activationEpoch, pid: target.app.pid, window: focusedSurfaceID) }
        return established
    }

    private func capturePriorForeground(_ context: RequestContext) throws -> PriorForeground {
        let pid = dtwFrontProcessPID()
        guard pid > 0, let birth = try context.startStamp(pid), birth > 0,
              let application = NSRunningApplication(processIdentifier: pid), !application.isTerminated else {
            throw failure("focus_unproven", "Original foreground application identity is unavailable")
        }
        var window: AXUIElement?
        var id: CGWindowID?
        do {
            if let candidate = try nativeFocusedWindow(AXUIElementCreateApplication(pid), pid: pid,
                                                       deadline: context.deadline) {
                let candidateID = try axWindowID(candidate, context.deadline)
                if candidateID > 0, try cgWindows(context.deadline).contains(where: { $0.id == candidateID && $0.pid == pid }) {
                    window = candidate; id = candidateID
                }
            }
        } catch {
            // An unavailable prior AX window never becomes a guessed window ID.
            // App identity and the foreground snapshot must still be proven.
            try context.deadline.check()
        }
        guard try context.startStamp(pid) == birth, try windowServerFrontmost(pid, context.deadline) else {
            throw failure("focus_changed", "Original foreground changed during capture")
        }
        return PriorForeground(application: application, pid: pid, generation: birth, window: window, windowID: id)
    }

    private func executionFamily(_ window: AXUIElement, _ scope: ExecutionFocus, _ deadline: Deadline) throws -> Bool {
        try scope.anchor.matches(window, pid: scope.app.pid, deadline: deadline)
    }

    // A destroyed anchor never authorizes another input. After an admitted
    // payload, restoration may follow a proved close that reveals another
    // window in this same app. A live-anchor sibling selection and app
    // leave/return still invalidate ownership.
    private func closedAnchorRestorationOwnership(_ scope: ExecutionFocus, _ context: RequestContext) throws -> Bool? {
        guard scope.anchor.id > 0 else { return nil }
        let rows = try cgWindows(context.deadline)
        let oldOnscreen = rows.contains(where: { $0.id == scope.anchor.id && $0.pid == scope.app.pid && $0.onscreen })
        traceSemanticFailure(["operation":"closed_anchor_restoration", "stage":"cg_liveness",
            "anchor":scope.anchor.id, "onscreen":oldOnscreen,
            "panel_transition":scope.closedPanelRestorationAllowed,
            "scope_invalidated":scope.invalidated,
            "application_invalidated":executionFocusObserver.applicationInvalidated(scope.activationEpoch)])
        guard !oldOnscreen else { return nil }
        // AppKit can retain a closed NSWindow and its offscreen CG record.
        // Offscreen alone is not closure: require the complete live AX inventory
        // to exclude the old root as well, preserving hidden/minimized windows.
        guard let inventory = try listWindows(scope.app, context)["windows"] as? [[String: Any]] else {
            throw failure("focus_unproven", "Closed-anchor inventory is unavailable")
        }
        let liveIDs = try inventory.map { try NativeTarget($0["target"]).nativeID }
        traceSemanticFailure(["operation":"closed_anchor_restoration", "stage":"live_inventory",
            "anchor":scope.anchor.id, "live_ids":liveIDs, "initial_ids":Array(scope.initialWindowIDs)])
        guard !liveIDs.contains(scope.anchor.id) else { return nil }
        guard !scope.invalidated, !executionFocusObserver.applicationInvalidated(scope.activationEpoch) else { return false }
        try context.verify(scope.app)
        guard let app = try nativeFocusedApplication(expectedPID: scope.app.pid, deadline: context.deadline),
              app.pid == scope.app.pid, app.generation == scope.app.generation else { return false }
        if let window = try nativeFocusedWindow(app.element, pid: scope.app.pid, deadline: context.deadline) {
            let root = try activationRoot(window, pid: scope.app.pid, deadline: context.deadline)
            let id = try axWindowID(root, context.deadline)
            guard (scope.closedPanelRestorationAllowed && !scope.initialWindowIDs.contains(id)) || scope.anchor.hasInput,
                  rows.contains(where: { $0.id == id && $0.pid == scope.app.pid }) else { return false }
        } else {
            guard inventory.isEmpty else { return false }
        }
        try context.verify(scope.app)
        return try windowServerFrontmost(scope.app.pid, context.deadline)
            && !executionFocusObserver.applicationInvalidated(scope.activationEpoch)
    }

    private func ownsExecutionFocus(_ scope: ExecutionFocus, _ context: RequestContext,
                                    allowClosedAnchor: Bool = false) throws -> Bool {
        let until = min(context.deadline.uptime, ProcessInfo.processInfo.systemUptime + 0.75)
        return try readFocusSnapshot(remaining: { until - ProcessInfo.processInfo.systemUptime },
            check: { try context.deadline.check(); try context.verify(scope.app) },
            read: { try self.executionFocusOwnershipSnapshot(scope, context, allowClosedAnchor: allowClosedAnchor) },
            wait: { CFRunLoopRunInMode(.defaultMode, $0, false) })
    }

    private func executionFocusOwnershipSnapshot(_ scope: ExecutionFocus, _ context: RequestContext,
                                                allowClosedAnchor: Bool) throws -> Bool {
        if allowClosedAnchor, let owned = try closedAnchorRestorationOwnership(scope, context) { return owned }
        guard !scope.invalidated, !executionFocusObserver.invalidated(scope.activationEpoch) else { return false }
        try context.verify(scope.app)
        let owned = try exactInputFocus(expectedPID: scope.app.pid, focusedApplication: { () throws -> FocusApplication<AXUIElement>? in
            guard let app = try nativeFocusedApplication(expectedPID: scope.app.pid, deadline: context.deadline),
                  app.generation == scope.app.generation else { return nil }
            return app
        }, windowMatches: { application in
            guard let current = try nativeFocusedWindow(application, pid: scope.app.pid, deadline: context.deadline) else {
                if scope.anchor.isEmpty { return true }
                throw missingFocus(.window)
            }
            return try self.executionFamily(current, scope, context.deadline)
        })
        return owned && !executionFocusObserver.invalidated(scope.activationEpoch)
    }

    private func requireExecutionForeground(_ request: [String: Any], _ target: NativeTarget,
                                            _ context: RequestContext) throws {
        try requireExecutionForeground(request, target.app, context)
    }

    private func requireExecutionForeground(_ request: [String: Any], _ app: NativeApp,
                                            _ context: RequestContext) throws {
        guard executionFocus != nil || request["_execution_foreground"] as? Bool == true || executionScopeActive() else { return }
        if let scope = executionFocus {
            // Resident payloads validate locally. Calling their own guard IPC
            // while holding NativeStore.lock would recurse into this store.
            guard scope.app == app, scope.terminal == nil, try ownsExecutionFocus(scope, context) else {
                executionFocus?.invalidated = true
                throw failure("focus_changed", "Execution foreground scope changed; further payloads are forbidden")
            }
        } else {
            // Every new child payload queries the resident's monotonic epoch.
            // A foreground app returning after a user choice cannot rearm it.
            try admitExecutionScope()
        }
        try context.verify(app)
        guard try windowServerFrontmost(app.pid, context.deadline) else {
            throw failure("focus_changed", "Execution foreground changed before scene input; activation is forbidden")
        }
        try context.verify(app)
    }

    private func validateExecutionFocus(_ request: [String: Any], _ context: RequestContext) throws -> [String: Any] {
        guard let raw = request["target"] as? [String: Any] else { throw failure("invalid_request", "Missing execution focus target") }
        let app = try NativeApp(raw["app"])
        let until = min(context.deadline.uptime, ProcessInfo.processInfo.systemUptime + 0.75)
        return try readFocusSnapshot(remaining: { until - ProcessInfo.processInfo.systemUptime },
            check: { try context.deadline.check(); try context.verify(app) },
            read: { try self.executionFocusAdmissionSnapshot(request, context) },
            wait: { CFRunLoopRunInMode(.defaultMode, $0, false) })
    }

    private func executionFocusAdmissionSnapshot(_ request: [String: Any], _ context: RequestContext) throws -> [String: Any] {
        guard let rawTarget = request["target"] as? [String: Any] else {
            throw failure("invalid_request", "Missing execution focus target")
        }
        let app = try NativeApp(rawTarget["app"])
        guard var scope = executionFocus else { throw failure("focus_changed", "No execution foreground scope is active") }
        guard scope.app == app, scope.terminal == nil, try ownsExecutionFocus(scope, context) else {
            executionFocus?.invalidated = true
            throw failure("focus_changed", "Execution foreground scope changed externally; it cannot be reacquired")
        }
        if rawTarget["native_id"] == nil {
            return ["scope_active": true, "borrowed": scope.changedForeground, "target": ["app": app.json]]
        }
        let target = try NativeTarget(rawTarget)
        // A scene's private payload callback already holds Scene ownership.
        // Revalidate retained owner/root identity without launching a second
        // pixel-proof child that would wait on the caller's own Scene lease.
        let associationScope: AssociationProofScope = request["_execution_scope_callback"] as? Bool == true
            ? .sceneScrollGuard : .full
        let window = try validate(target, context, input: false, associationScope: associationScope)
        // A visible first window may not yet be key. Prove key focus before
        // binding an empty anchor, so a clean refusal can still restore the app.
        guard try focused(target, window.element, context.deadline),
              try executionFamily(window.element, scope, context.deadline),
              try ownsExecutionFocus(scope, context) else {
            executionFocus?.invalidated = true
            throw failure("focus_changed", "Requested surface is not the exact owned execution foreground")
        }
        scope.target = target
        if request["requested_input"] as? String == "click", scope.nativeOpenPanel {
            scope.closedPanelRestorationAllowed = true
            scope.anchor.admitInput()
        }
        executionFocus = scope
        return ["scope_active": true, "borrowed": scope.changedForeground, "target": target.json,
                "bounds": window.cg.coordinates, "surface_owner": surfaceOwner(target)]
    }

    private func beginExecutionFocus(_ request: [String: Any]) -> [String: Any] {
        var changed = false
        var stage = "preflight"
        do {
            let context = try RequestContext(request)
            // A real borrow supersedes the passive order guard. Settle any
            // prior background effect before capturing this borrow's owner.
            _ = releaseBackgroundWindowOrder(context.deadline)
            guard request["input_policy"] as? String == "auto" else {
                throw failure("foreground_required", "Execution foreground borrow requires automatic input policy")
            }
            if executionFocus != nil {
                return try validateExecutionFocus(request, context).merging(
                    ["dispatch": "dispatched", "evidence": "unverified"]) { _, new in new }
            }
            if let focusCleanupFailure { throw focusCleanupFailure }
            guard let raw = request["target"] as? [String: Any] else { throw failure("invalid_request", "Missing focus target") }
            let app = try NativeApp(raw["app"])
            try context.verify(app)
            let target: NativeTarget?
            let window: CandidateWindow?
            let anchor: ExecutionFocusAnchor
            if raw["native_id"] != nil {
                let selected = try NativeTarget(raw)
                let candidate = try validate(selected, context, input: false)
                let root = try activationRoot(candidate.element, pid: app.pid, deadline: context.deadline)
                target = selected; window = candidate
                anchor = ExecutionFocusAnchor(root, id: try axWindowID(root, context.deadline))
            } else {
                // A missing AX attribute is not proof of a windowless app.
                // Reuse the full owning inventory before any activation.
                let inventory = try listWindows(app, context)
                guard let rows = inventory["windows"] as? [[String: Any]], rows.isEmpty else {
                    throw failure("target_changed", "Application gained a window before windowless focus; observe again")
                }
                target = nil; window = nil; anchor = ExecutionFocusAnchor()
            }
            let initialWindowIDs = Set(try cgWindows(context.deadline).filter { $0.pid == app.pid }.map(\.id))
            anchor.configure(initialWindowIDs)
            // The typed plan already proved this exact surface in its clean
            // background preflight. Optional metadata must not block focus.
            let nativeOpenPanel = request["reason"] as? String == "native_open_panel_click"
                || (window.map { nativeOpenPanelClick(
                    role: (try? axAttribute($0.element, kAXRoleAttribute, context.deadline)) as? String,
                    identifier: (try? axAttribute($0.element, kAXIdentifierAttribute, context.deadline)) as? String) } ?? false)
            let allowPanelRestoration = nativeOpenPanel && request["reason"] as? String == "native_open_panel_click"
            stage = "capture_original"
            let original = try capturePriorForeground(context)
            let epoch = try executionFocusObserver.begin(initialPID: original.pid, targetPID: app.pid,
                window: nil, deadline: context.deadline, family: { surface, deadline in
                    try anchor.matches(surface, pid: app.pid, deadline: deadline)
                })
            executionFocus = ExecutionFocus(activationEpoch: epoch, app: app, original: original,
                anchor: anchor, initialWindowIDs: initialWindowIDs, nativeOpenPanel: nativeOpenPanel,
                closedPanelRestorationAllowed: allowPanelRestoration,
                target: target, changedForeground: false, pointerOrigin: CGEvent(source: nil)?.location)
            do {
                if let target, let window {
                    stage = "capture_target"
                    try borrowFocus(target, window.element, context)
                    stage = "establish_target"
                    _ = try establishFocus(target, context, changed: &changed)
                } else {
                    guard !executionFocusObserver.invalidated(epoch) else {
                        throw failure("focus_changed", "Foreground changed before windowless activation")
                    }
                    if try !windowServerFrontmost(app.pid, context.deadline) {
                        try context.verify(app)
                        guard let application = NSRunningApplication(processIdentifier: app.pid), !application.isTerminated else {
                            throw failure("app_changed", "Windowless application exited before focus")
                        }
                        changed = true
                        guard application.activate(options: []) else {
                            throw failure("focus_unproven", "Windowless application activation was not accepted")
                        }
                    }
                    let until = min(context.deadline.uptime, ProcessInfo.processInfo.systemUptime + 0.5)
                    while try !windowServerFrontmost(app.pid, context.deadline) {
                        try context.verify(app)
                        guard !executionFocusObserver.invalidated(epoch) else { throw failure("focus_changed", "Windowless activation lost ownership") }
                        guard ProcessInfo.processInfo.systemUptime < until else { throw failure("focus_unproven", "Windowless application focus did not settle") }
                        CFRunLoopRunInMode(.defaultMode, 0.005, false)
                    }
                }
            } catch {
                executionFocus?.changedForeground = changed
                throw error
            }
            executionFocus?.changedForeground = changed
            executionFocusObserver.established(epoch, pid: app.pid, window: anchor.id)
            stage = "validate_established_target"
            let result = try validateExecutionFocus(request, context)
            return result.merging(["dispatch": "dispatched", "evidence": "unverified"]) { _, new in new }
        } catch let error as NativeFailure {
            return error.json.merging(["message": "Execution foreground preparation at \(stage): \(error.message)",
                                       "dispatch": changed ? "unknown" : "not_dispatched", "evidence": "unverified",
                                       "borrowed": executionFocus?.changedForeground ?? false,
                                       "scope_active": executionFocus != nil]) { _, new in new }
        } catch {
            return ["error": "native_error", "message": String(describing: error),
                    "dispatch": changed ? "unknown" : "not_dispatched", "evidence": "unverified",
                    "borrowed": executionFocus?.changedForeground ?? false, "scope_active": executionFocus != nil]
        }
    }

    private func releaseLocalFocus(_ deadline: Deadline, handoff: Bool = false) throws -> Bool {
        guard var borrowed = borrowedFocus else { return false }
        let invalidated = focusObserver.invalidated(borrowed.activationEpoch)
        let posted = dtwTakePostedPointerPosition()
        releasedExecutionPointer = nil
        if handoff, borrowed.executionScoped, let posted {
            // Keep fallback ownership until the permanent owner acknowledges.
            // EOF/cancellation before acknowledgement returns locally instead.
            borrowed.inputPoint = posted; borrowed.pointerHandoff = true
            borrowedFocus = borrowed; releasedExecutionPointer = posted
            return invalidated
        }
        defer { borrowedFocus = nil; focusObserver.end(borrowed.activationEpoch) }
        let endpoint = posted ?? (borrowed.pointerHandoff ? borrowed.inputPoint : nil)
        if let previous = borrowed.pointer, let point = endpoint,
           let current = CGEvent(source: nil)?.location, !invalidated,
           abs(current.x - point.x) < 0.5, abs(current.y - point.y) < 0.5 {
            guard let placement = dtwPointerRestorationPoint(previous),
                  CGWarpMouseCursorPosition(placement) == .success,
                  CGAssociateMouseAndMouseCursorPosition(1) == .success,
                  dtwPointerReturnConfirmed(previous, deadline: deadline.uptime,
                                            read: { CGEvent(source: nil)?.location }) else {
                let error = failure("pointer_restore_failed", "Borrowed pointer return could not be confirmed")
                focusCleanupFailure = error
                throw error
            }
        }
        return invalidated
    }

    private func priorForegroundIsCurrent(_ original: PriorForeground, _ context: RequestContext) throws -> Bool {
        try executionRestoreIdentityCurrent(pid: original.pid, generation: original.generation,
            birth: { try context.startStamp($0) }, windowMatches: {
                guard !original.application.isTerminated,
                      try windowServerFrontmost(original.pid, context.deadline) else { return false }
                guard let window = original.window, let id = original.windowID else { return true }
                return try self.focused(original.pid, window, windowID: id, context.deadline)
            })
    }

    private func visibleWindowOrder(_ deadline: Deadline) throws -> [CGRecord] {
        try deadline.check()
        guard let rows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], 0) as? [[String: Any]] else {
            throw failure("windows_unavailable", "Cannot observe visible window order")
        }
        try deadline.check()
        return rows.filter { $0[kCGWindowLayer as String] as? Int == 0 }.compactMap(cgRecord)
    }

    private func prepareBackgroundWindowOrder(_ app: NativeApp, _ context: RequestContext) {
        guard executionFocus == nil, borrowedFocus == nil else { return }
        // Optional user-friendliness proof must not consume the payload's full
        // deadline or become another functional background-input gate.
        let deadline = context.deadline.limited(to: ProcessInfo.processInfo.systemUptime + 0.15)
        do {
            if backgroundWindowOrder == nil {
                let original = try capturePriorForeground(RequestContext(deadline: deadline))
                guard original.pid != app.pid, let id = original.windowID, original.window != nil else { return }
                let rows = try visibleWindowOrder(deadline)
                // Raising cannot insert below an existing foreign overlay.
                // Preserve that user-chosen layering instead of disturbing it.
                guard originalAboveOtherApps(rows, original: id, owner: original.pid),
                      let above = targetWindowsAbove(rows, original: id,
                          owner: original.pid, target: app.pid) else { return }
                let epoch = try executionFocusObserver.begin(initialPID: original.pid, targetPID: original.pid,
                    window: id, deadline: deadline)
                executionFocusObserver.established(epoch, pid: original.pid, window: id)
                backgroundWindowOrder = BackgroundWindowOrder(epoch: epoch, original: original, previouslyAbove: [app: above])
            } else if backgroundWindowOrder?.previouslyAbove[app] == nil,
                      let scope = backgroundWindowOrder, let id = scope.original.windowID,
                      let above = targetWindowsAbove(try visibleWindowOrder(deadline), original: id,
                          owner: scope.original.pid, target: app.pid) {
                backgroundWindowOrder?.previouslyAbove[app] = above
            }
        } catch {
            traceSemanticFailure(["operation":"background_window_order_capture", "status":"unverified",
                "error":(error as? NativeFailure)?.code ?? "native_error"])
        }
    }

    private func releaseBackgroundWindowOrder(_ deadline: Deadline) -> String {
        guard let scope = backgroundWindowOrder else { return "unchanged" }
        backgroundWindowOrder = nil
        defer { executionFocusObserver.end(scope.epoch) }
        guard !scope.admitted.isEmpty, let id = scope.original.windowID,
              let window = scope.original.window else { return "unchanged" }
        do {
            let context = RequestContext(deadline: deadline)
            func ownerStillCurrent() throws -> Bool {
                guard !executionFocusObserver.invalidated(scope.epoch) else { return false }
                return try executionRestoreIdentityCurrent(pid: scope.original.pid, generation: scope.original.generation,
                    birth: { try context.startStamp($0) }, windowMatches: {
                        guard !scope.original.application.isTerminated else { return false }
                        return try passiveOrderOwner(requested: window, requestedWindow: id,
                            frontmost: { try windowServerFrontmost(scope.original.pid, deadline) },
                            focusedWindow: {
                                try nativeFocusedWindow(AXUIElementCreateApplication(scope.original.pid),
                                    pid: scope.original.pid, deadline: deadline)
                            }, equal: CFEqual, windowID: { try axWindowID($0, deadline) })
                    })
            }
            guard try ownerStillCurrent() else { return "changed_externally" }
            let rows = try visibleWindowOrder(deadline)
            var raised: Set<CGWindowID> = []
            for app in scope.admitted {
                // Exited/replaced PIDs never authorize restoring over a new app.
                guard try context.startStamp(app.pid) == app.generation else { continue }
                guard let above = targetWindowsAbove(rows, original: id, owner: scope.original.pid, target: app.pid) else {
                    return "unconfirmed"
                }
                raised.formUnion(above.subtracting(scope.previouslyAbove[app] ?? []))
            }
            guard !raised.isEmpty else { return "unchanged" }
            try deadline.prepare(window)
            guard try ownerStillCurrent() else { return "changed_externally" }
            // Restore layering only. Do not activate, make-key, move the pointer,
            // replay AXPress or touch windows that were already above the owner.
            guard AXUIElementPerformAction(window, kAXRaiseAction as CFString) == .success else { return "unconfirmed" }
            let result: Bool = try waitForEstablishedFocus(remaining: {
                deadline.uptime - ProcessInfo.processInfo.systemUptime
            }, check: { try deadline.check() }, read: {
                guard try ownerStillCurrent() else { return false }
                let current = try self.visibleWindowOrder(deadline)
                guard let index = current.firstIndex(where: { $0.id == id && $0.pid == scope.original.pid }) else { return false }
                return current.prefix(index).contains(where: { raised.contains($0.id) }) ? nil : true
            }, wait: { CFRunLoopRunInMode(.defaultMode, $0, false) })
            return result == true ? "restored" : "changed_externally"
        } catch {
            // A layering failure does not make the already delivered action or
            // the keyboard cleanup unsafe. Report settlement without retiring it.
            traceSemanticFailure(["operation":"background_window_order_return", "status":"unverified",
                "error":(error as? NativeFailure)?.code ?? "native_error"])
            return "unconfirmed"
        }
    }

    // A newly opened native panel can temporarily answer cannotComplete for
    // AXFocusedWindow. Re-read only the full ownership proof under the original
    // cleanup budget; no activation, payload or restoration is replayed.
    private func restorationOwnership(_ scope: ExecutionFocus, _ context: RequestContext) throws -> Bool {
        do { return try ownsExecutionFocus(scope, context, allowClosedAnchor: true) }
        catch let original as NativeFailure where retryableAXBusyFailure(original) {
            return try retryCompleteAXRead(original: original,
                remaining: { context.deadline.uptime - ProcessInfo.processInfo.systemUptime },
                check: { try context.deadline.check(); try context.verify(scope.app) },
                read: { try self.ownsExecutionFocus(scope, context, allowClosedAnchor: true) },
                wait: { Thread.sleep(forTimeInterval: $0) })
        }
    }

    private func restorePriorForeground(_ scope: ExecutionFocus, _ deadline: Deadline) throws -> String {
        let context = RequestContext(deadline: deadline)
        guard try restorationOwnership(scope, context) else {
            traceSemanticFailure(["operation":"foreground_return_declined", "stage":"current_owner"])
            return "changed_externally"
        }
        guard scope.changedForeground else {
            traceSemanticFailure(["operation":"foreground_return_declined", "stage":"unchanged_owner"])
            return "unchanged"
        }
        let original = scope.original
        guard try executionRestoreIdentityCurrent(pid: original.pid, generation: original.generation,
            birth: { try context.startStamp($0) }, windowMatches: {
                guard !original.application.isTerminated else { return false }
                guard let id = original.windowID, let window = original.window else { return true }
                guard try axPID(window, deadline) == original.pid, try axWindowID(window, deadline) == id,
                      try cgWindows(deadline).contains(where: { $0.id == id && $0.pid == original.pid }),
                      try semanticWindowRoots(AXUIElementCreateApplication(original.pid), deadline, requestedID: id)
                        .contains(where: { CFEqual($0, window) }) else { return false }
                // If borrowing another window in the same app, its app-local
                // key window is ours. Otherwise preserve the prior app's exact
                // background key window rather than raising over a changed one.
                if original.pid == scope.app.pid { return true }
                guard let current = try nativeFocusedWindow(AXUIElementCreateApplication(original.pid),
                                                            pid: original.pid, deadline: deadline) else { return false }
                return try exactFocusedSurface(requested: window, requestedWindow: id, focused: current,
                    equal: CFEqual, windowID: { try axWindowID($0, deadline) })
            }) else {
                traceSemanticFailure(["operation":"foreground_return_declined", "stage":"original_owner"])
                return "changed_externally"
            }
        guard try restorationOwnership(scope, context) else { return "changed_externally" }
        // Cleanup runs after admission closes on EOF/cancellation. The caller
        // already owns coordination, instance and physical leases; never reopen
        // input admission merely to fulfill this restoration obligation.
        do {
            try deadline.check()
            guard try context.startStamp(original.pid) == original.generation,
                  !self.executionFocusObserver.invalidated(scope.activationEpoch) else {
                throw failure("focus_restore_failed", "Original foreground identity changed before restoration")
            }
            if let id = original.windowID, !dtwSetFrontProcess(original.pid, id) {
                throw failure("focus_restore_failed", "Original exact window restoration was not accepted")
            }
            guard original.application.activate(options: []) else {
                throw failure("focus_restore_failed", "Original application restoration was not accepted")
            }
        }
        try confirmRestoredForeground(remaining: { deadline.uptime - ProcessInfo.processInfo.systemUptime },
            check: {
                try deadline.check()
                guard try context.startStamp(original.pid) == original.generation,
                      !original.application.isTerminated else {
                    throw failure("focus_restore_failed", "Original application identity changed during restoration")
                }
            }, frontmost: { try windowServerFrontmost(original.pid, deadline) },
            windowMatches: {
                guard let window = original.window, let id = original.windowID else { return true }
                return try self.focused(original.pid, window, windowID: id, deadline)
            }, wait: { CFRunLoopRunInMode(.defaultMode, $0, false) })
        if let window = original.window, let id = original.windowID {
            // Make-key/activation can restore keyboard ownership while
            // leaving our dialog above the original window. Raise only
            // that proven original window, once, after focus settles.
            // A layering issue must not retire otherwise usable input.
            do {
                try deadline.prepare(window)
                guard try windowServerFrontmost(original.pid, deadline),
                      try context.startStamp(original.pid) == original.generation else {
                    return "changed_externally"
                }
                let raised = AXUIElementPerformAction(window, kAXRaiseAction as CFString)
                traceSemanticFailure(["operation":"foreground_window_order_return",
                    "pid":original.pid, "window":id, "ax_error":raised.rawValue])
            } catch {
                traceSemanticFailure(["operation":"foreground_window_order_return",
                    "pid":original.pid, "window":id, "status":"unverified",
                    "error":(error as? NativeFailure)?.code ?? "native_error"])
            }
        }
        return "restored"
    }

    private func returnExecutionPointer(_ scope: ExecutionFocus, _ deadline: Deadline) throws -> String {
        guard let endpoint = scope.pointerEndpoint else { return "unchanged" }
        let context = RequestContext(deadline: deadline)
        let owned = try scope.terminal == nil
            ? restorationOwnership(scope, context)
            : priorForegroundIsCurrent(scope.original, context)
        guard owned else { return "changed_externally" }
        guard let current = CGEvent(source: nil)?.location else {
            throw failure("pointer_restore_failed", "Cannot read the borrowed cursor during return")
        }
        guard let destination = dtwPointerReturnDestination(original: scope.pointerOrigin, posted: endpoint, current: current) else {
            if scope.pointerOrigin == nil { throw failure("pointer_restore_failed", "Original cursor position was unavailable") }
            return "changed_externally"
        }
        guard let placement = dtwPointerRestorationPoint(destination),
              CGWarpMouseCursorPosition(placement) == .success,
              CGAssociateMouseAndMouseCursorPosition(1) == .success,
              dtwPointerReturnConfirmed(destination, deadline: deadline.uptime,
                read: { CGEvent(source: nil)?.location }) else {
            throw failure("pointer_restore_failed", "Script cursor return could not be confirmed")
        }
        return "restored"
    }

    fileprivate func releaseFocus(_ deadline: Deadline) throws -> [String: Any] {
        if let focusCleanupFailure { throw focusCleanupFailure }
        let localInvalidated = try releaseLocalFocus(deadline)
        guard let scope = executionFocus else {
            let order = releaseBackgroundWindowOrder(deadline)
            return ["released": true, "focus": localInvalidated ? "changed_externally" : order]
        }
        // Only the execution finalizer consumes the durable borrow. Per-input
        // cleanup has its own release_input_focus operation.
        defer { executionFocus = nil; executionFocusObserver.end(scope.activationEpoch) }
        var pointerStatus = "unchanged"
        var pointerError: NativeFailure?
        do { pointerStatus = try returnExecutionPointer(scope, deadline) }
        catch let error as NativeFailure { pointerStatus = "unconfirmed"; pointerError = error }
        // Pointer failure must not prevent the independently owed focus return.
        func settled(_ value: [String: Any]) -> [String: Any] {
            var value = value; value["pointer_return"] = pointerStatus
            if let pointerError { value["cleanup_error"] = pointerError.json }
            return value
        }
        if let terminal = scope.terminal {
            // A positively confirmed quit/root close may have destroyed anchor.
            // Observe only the original foreground; never activate after closure.
            return settled(settledExecutionTerminal(terminal) {
                try self.priorForegroundIsCurrent(scope.original, RequestContext(deadline: deadline))
            })
        }
        do {
            let status = try restorePriorForeground(scope, deadline)
            return settled(["released": true, "focus": status])
        } catch let error as NativeFailure {
            let sticky = failure("focus_restore_failed", "Execution foreground cleanup failed: \(error.message)")
            focusCleanupFailure = sticky
            throw sticky
        } catch {
            let sticky = failure("focus_restore_failed", "Execution foreground cleanup failed: \(error)")
            focusCleanupFailure = sticky
            throw sticky
        }
    }

    private func appKeypress(_ request: [String:Any]) -> [String:Any] {
        var changed=false
        var pending:NativePendingInput?
        func finished(_ value:[String:Any])->[String:Any] {
            guard let pending,!dtwFinishInput(pending) else{return value}
            return ["dispatch":changed ? "unknown":"not_dispatched","evidence":"unverified","error":"input_release_unproven","message":"Application shortcut releases were not confirmed"]
        }
        do {
            let context=try RequestContext(request)
            let app=try NativeApp((request["target"] as? [String:Any])?["app"])
            try context.verify(app)
            guard request["input_policy"] as? String == "auto" else {throw failure("foreground_required","Application shortcuts require foreground delivery")}
            guard dtwAccessibilityGranted(request:true) else {let e=dtwPermissionError("accessibility");throw failure("permission_required",e["message"] as! String)}
            let onlyFocus = request["operation"] as? String == "focus_app"
            let keys = request["keys"] as? [String] ?? []
            guard onlyFocus || dtwChordPlan(keys) != nil else {throw failure("invalid_keys","Unsupported application shortcut")}
            guard CGEventSource.flagsState(.combinedSessionState).intersection([.maskCommand,.maskControl,.maskAlternate,.maskShift]).isEmpty else {throw failure("input_busy","Session modifiers are held")}
            if request["_execution_foreground"] as? Bool != true {
                let inventory = try listWindows(app, context)
                guard let windows = inventory["windows"] as? [[String: Any]], windows.isEmpty else {
                    throw failure("target_changed", "Application gained a window; observe before application input")
                }
                guard request["_defer_foreground"] as? Bool == true else { throw failure("foreground_required", "Application input requires a script foreground scope") }
                return ["dispatch":"not_dispatched", "evidence":"unverified",
                        "foreground_plan":["reason":onlyFocus ? "explicit_focus" : "windowless_application", "visual":false]]
            }
            try requireExecutionForeground(request, app, context)
            guard try windowServerFrontmost(app.pid, context.deadline) else { throw failure("focus_changed", "Application foreground changed") }
            if onlyFocus { return ["dispatch":"dispatched", "evidence":"confirmed", "postcondition":"application_focused"] }
            guard let source=CGEventSource(stateID:.privateState),let chord=dtwChord(keys,source:source) else {throw failure("input_unavailable","Cannot prepare application shortcut")}
            func check() throws {
                try context.deadline.check();try context.verify(app)
                guard !dtwInputStopping() else {throw failure("native_stopping","Application shortcut stopped")}
                try requireExecutionForeground(request, app, context)
                guard dtwFrontProcessMatches(app.pid)==1 else {throw failure("focus_changed","Application foreground changed before shortcut delivery")}
            }
            try check()
            guard let input=dtwTrackInput(source:source,eventTypes:[.keyUp,.flagsChanged]) else {throw failure("native_stopping","Application shortcut admission closed")}
            pending=input
            guard let _:Bool=try dtwWithInputAdmission({
                self.executionFocus?.anchor.admitInput()
                var pressed:[NativeKeyPair]=[]
                defer {for pair in pressed.reversed(){dtwPostKey(pair.up,input:input)}}
                for pair in chord {try check();changed=true;dtwPostKey(pair.down,input:input);pressed.append(pair);Thread.sleep(forTimeInterval:0.005)}
                return true
            }) else {throw failure("native_stopping","Application shortcut admission closed")}
            try context.deadline.check()
            return finished(["dispatch":"dispatched","evidence":"unverified"])
        } catch let e as NativeFailure {return finished(e.json.merging(["dispatch":changed ? "unknown":"not_dispatched","evidence":"unverified"]){_,new in new})}
        catch {return finished(["dispatch":changed ? "unknown":"not_dispatched","evidence":"unverified","error":"native_error","message":String(describing:error)])}
    }

    private func windowInput(_ request: [String: Any]) -> [String: Any] {
        var changed = false
        var pendingInput: NativePendingInput?
        func finished(_ result: [String: Any]) -> [String: Any] {
            var result = result
            // The exact focus attempt can refresh an invalidated prepare scope.
            // Report the actual obligation, not just the earlier route plan.
            result["borrowed"] = borrowedFocus?.changedForeground ?? false
            guard let pendingInput, !dtwFinishInput(pendingInput) else { return result }
            // Keep a primary error if one already exists. The gate retains its
            // cleanup failure and will report an unclean worker shutdown too.
            if result["error"] != nil { return result }
            return ["dispatch": changed ? "unknown" : "not_dispatched", "evidence": "unverified",
                    "error": "input_release_unproven", "message": "Posted input releases were not confirmed within the cleanup budget",
                    "borrowed": borrowedFocus?.changedForeground ?? false]
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
            let visual = request["visual"] as? Bool == true
            let executionScoped = request["_execution_foreground"] as? Bool == true || executionScopeActive()
            let allowActivation = !executionScoped && request["_allow_activation"] as? Bool != false
            let window: CandidateWindow? = visual ? nil : try establishFocus(target, context, changed: &changed,
                                                                           allowActivation: allowActivation)
            let surface = try window?.cg ?? establishVisualFocus(target, context,
                allowActivation: allowActivation, changed: &changed)
            if executionScoped { borrowedFocus?.executionScoped = true }
            observations = observations.filter { $0.value.target != target }
            func check(focus: Bool = true) throws {
                try context.deadline.check()
                if dtwInputStopping() { throw failure("native_stopping", "Input stopped after releasing admitted keys") }
                try requireExecutionForeground(request, target, context)
                if focus {
                    try context.verify(target.app)
                    if visual {
                        try validateVisualInput(target, context)
                    } else if let window {
                        if associatedSurfaces[WindowSlot(app: target.app, id: target.nativeID)] != nil {
                            _ = try validateSurface(target, context)
                        }
                        guard try focused(target, window.element, context.deadline) else {
                            throw failure("focus_changed", "Exact focused window changed before the next input event. Re-observe the target window before continuing; earlier input may have partial effects, so do not replay the entire text or chord")
                        }
                        if keyboardSource != nil,
                           let destination = axElement(try axAttribute(AXUIElementCreateApplication(target.app.pid),
                                                                       kAXFocusedUIElementAttribute, context.deadline)),
                           try !owned(destination, by: window.element, target: target, deadline: context.deadline) {
                            throw failure("focus_changed", "Keyboard destination left the exact target surface, possibly for a dialog; observe again before continuing")
                        }
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
                    self.executionFocus?.anchor.admitInput()
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
                let point = try fixedScrollPoint(request, current: surface.bounds)
                guard let source = CGEventSource(stateID: .privateState) else {
                    throw failure("input_unavailable", "Cannot allocate scroll event source")
                }
                guard let proof = dtwProveSurfacePoint(point, id: target.nativeID, pid: surface.pid,
                                                       bounds: surface.bounds, deadline: context.deadline.uptime,
                                                       overlayCandidates: dtwSceneOverlayCandidates(request)) else {
                    try check(focus: false)
                    throw failure("target_occluded", "Scroll point is not a proven visible point of the target window")
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
                try admitScrollAnchor()
                try check()
                guard proof.matchesCurrent(requirePointer: true) else {
                    throw failure("stale_observation", "Scroll target or pointer changed during anchor validation")
                }
                guard let _: Bool = try dtwWithInputAdmission({
                    try check(focus: false)
                    event.location = point; event.flags = []
                    input.posted(eventType: CGEventType.scrollWheel.rawValue)
                    event.post(tap: .cghidEventTap)
                    scrollAnchorPosted()
                    dtwPresentPostedPointer(point, kind: 3)
                    return true
                }) else { throw failure("native_stopping", "Native input admission is closed") }
            }
            // A completed input may have closed its window or opened a modal.
            // Keep cancellation/deadline checks, but do not turn that effect
            // into an error by requiring the old AX focus again. A focus-only
            // operation still has to prove its requested focus postcondition.
            if operation == "focus" { try check(focus: false) }
            else {
                try context.deadline.check()
                if dtwInputStopping() { throw failure("native_stopping", "Input stopped after releasing admitted keys") }
            }
            if operation == "focus", var scope = borrowedFocus, scope.target == target,
               !focusObserver.invalidated(scope.activationEpoch) {
                scope.semanticReadiness.arm(now: ProcessInfo.processInfo.systemUptime)
                borrowedFocus = scope
            }
            return finished(operation == "focus"
                ? ["dispatch":"dispatched", "evidence":"confirmed", "postcondition":"window_focused"]
                : ["dispatch":"dispatched", "evidence":"unverified"])
        } catch let error as NativeFailure {
            return finished(error.json.merging(["dispatch": changed ? "unknown" : "not_dispatched", "evidence": "unverified"]) { _, new in new })
        } catch {
            return finished(["dispatch": changed ? "unknown" : "not_dispatched", "evidence": "unverified", "error": "native_error", "message": String(describing: error)])
        }
    }

    private func scrollAnchor(_ request: [String: Any], _ context: RequestContext, validating: Bool) throws -> [String: Any] {
        guard let token = request["binding"] as? String, let observation = observations[token] else {
            throw failure("stale_observation", "Scroll anchor binding was retired or consumed")
        }
        let target = try NativeTarget(request["target"])
        guard observation.target == target else { throw failure("target_changed", "Scroll binding belongs to another window") }
        let id = try integer(request["element"], "element")
        guard let retained = observation.elements.first(where: { $0.id == id }) else {
            throw failure("stale_observation", "Scroll anchor is absent from its retained observation")
        }
        // Only the live resident guard omits pixel acquisition: its Scene
        // caller already owns the scene lease and rechecks the same point after
        // this RPC. Initial resolve and every normal action retain full proof.
        let associationScope: AssociationProofScope = validating ? .sceneScrollGuard : .full
        do {
            let window = try validate(target, context, associationScope: associationScope)
            guard CFEqual(window.element, observation.root),
                  try owned(retained.element, by: window.element, target: target, deadline: context.deadline) else {
                throw failure("stale_observation", "Retained scroll object or its exact window ancestry changed")
            }
            try validateRegionElement(retained, root: window.element, target: target,
                                      bounds: window.cg.bounds, deadline: context.deadline)
            func elementRect() throws -> CGRect {
                guard let rawPosition = try axAttribute(retained.element, kAXPositionAttribute, context.deadline),
                  let rawSize = try axAttribute(retained.element, kAXSizeAttribute, context.deadline),
                  CFGetTypeID(rawPosition) == AXValueGetTypeID(), CFGetTypeID(rawSize) == AXValueGetTypeID() else {
                    throw failure("element_not_visible", "Retained scroll object has no readable position/size")
                }
                let position = unsafeBitCast(rawPosition, to: AXValue.self)
                let size = unsafeBitCast(rawSize, to: AXValue.self)
                var origin = CGPoint.zero; var extent = CGSize.zero
                guard AXValueGetType(position) == .cgPoint, AXValueGetValue(position, .cgPoint, &origin),
                  AXValueGetType(size) == .cgSize, AXValueGetValue(size, .cgSize, &extent) else {
                    throw failure("element_not_visible", "Retained scroll object geometry is unavailable")
                }
                return CGRect(origin: origin, size: extent)
            }
            let clipping = retained.regionProof.map { [$0.region] + $0.viewports.map { $0.1 } } ?? []
            let geometry = try ScrollAnchorGeometry(element: elementRect(), window: window.cg.bounds, clipping: clipping)
            if validating {
                let expectedElement = try scrollRect(request["element_bounds"], "element")
                let expectedWindow = try scrollRect(request["bounds"], "window")
                let point = try fixedScrollPoint(request, current: expectedWindow)
                guard geometry.matches(element: expectedElement, window: expectedWindow, point: point) else {
                    throw failure("stale_observation", "Retained scroll anchor geometry changed before the first wheel")
                }
            }
            let final = try validate(target, context, associationScope: associationScope)
            guard CFEqual(final.element, observation.root), final.cg.bounds == geometry.window,
                  try owned(retained.element, by: final.element, target: target, deadline: context.deadline),
                  try geometry.matches(element: elementRect(), window: final.cg.bounds, point: geometry.point) else {
                throw failure("stale_observation", "Scroll anchor ownership changed during its read")
            }
            try validateRegionElement(retained, root: final.element, target: target,
                                      bounds: final.cg.bounds, deadline: context.deadline)
            try context.deadline.check()
            return ["target": target.json, "bounds": final.cg.coordinates,
                    "element_bounds": [geometry.element.minX, geometry.element.minY, geometry.element.width, geometry.element.height],
                    "screen_x": geometry.point.x, "screen_y": geometry.point.y]
        } catch let error as NativeFailure where error.attributeFailure?.error == .invalidUIElement {
            throw failure("stale_observation", "The retained scroll AX object no longer exists; observe again")
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
            guard CFEqual(window.element, observation.root) else {
                throw failure("stale_observation", "Original AX root changed before setValue; observe again")
            }
            let enabled = try axAttribute(element.element, kAXEnabledAttribute, context.deadline) as? Bool == true
            let role = try axAttribute(element.element, kAXRoleAttribute, context.deadline) as? String ?? ""
            guard try settable(element.element, role: role, enabled: enabled, window: window.element, target: target, deadline: context.deadline) else { throw failure("element_not_settable", "Current AXValue writability or ownership changed") }
            try validateRegionElement(element, root: window.element, target: target,
                                      bounds: window.cg.bounds, deadline: context.deadline)
            try context.deadline.prepare(element.element)
            guard let result = try dtwWithInputAdmission({
                try self.requireExecutionForeground(request, target, context)
                dispatched = true
                return AXUIElementSetAttributeValue(element.element, kAXValueAttribute as CFString, value as CFString)
            }) else { throw failure("native_stopping", "Native input admission is closed") }
            try context.deadline.check()
            guard result == .success else { throw failure("ax_action_failed", "AXValue update returned \(result.rawValue); inspect before another action") }
            let current = try validate(target, context)
            guard try owned(element.element, by: current.element, target: target, deadline: context.deadline),
                  try axAttribute(element.element, kAXValueAttribute, context.deadline) as? String == value else {
                throw failure("value_unconfirmed", "Native value was written but exact readback did not confirm it; inspect before another action")
            }
            return ["dispatch": "dispatched", "evidence": "confirmed", "postcondition": "element_value_set"]
        } catch let error as NativeFailure {
            return error.json.merging(["dispatch": dispatched ? "unknown" : "not_dispatched", "evidence": "unverified"]) { _, new in new }
        } catch { return ["dispatch": dispatched ? "unknown" : "not_dispatched", "evidence": "unverified", "error": "native_error", "message": String(describing: error)] }
    }

    private func lifecycle(_ request: [String: Any]) -> [String: Any] {
        var attempted = false
        do {
            let context = try RequestContext(request)
            let operation = request["operation"] as? String ?? ""
            if operation == "launch_app" {
                guard let identifier = request["app_id"] as? String else { throw failure("invalid_request", "Missing application target") }
                let (metadata, running) = try context.resolveApplication(identifier)
                guard request["application_key"] as? String == metadata.path else {
                    throw failure("target_changed", "Installed application changed before launch admission")
                }
                if !running.isEmpty {
                    return ["dispatch": "not_dispatched", "evidence": "confirmed",
                            "postcondition": "application_running", "apps": running.map(\.json)]
                }
                let completion = ApplicationOpenCompletion()
                let configuration = applicationOpenConfiguration()
                try context.deadline.check()
                guard dtwWithInputAdmission({
                    attempted = true
                    NSWorkspace.shared.openApplication(at: URL(fileURLWithPath: metadata.path),
                        configuration: configuration) { application, error in completion.set(application, error) }
                    return true
                }) == true else { throw failure("native_stopping", "Application launch admission is closed") }
                while !completion.finished {
                    try context.deadline.check()
                    RunLoop.current.run(until: Date().addingTimeInterval(0.02))
                }
                guard let opened = completion.application, completion.error == nil,
                      let proven = try context.app(opened.processIdentifier),
                      proven.metadata.path == metadata.path,
                      proven.metadata.identifier == metadata.identifier else {
                    throw failure("launch_unconfirmed", "Launch returned without a proven matching process; inspect before new work")
                }
                try context.verify(proven.app)
                return ["dispatch": "dispatched", "evidence": "confirmed",
                        "postcondition": "application_running", "apps": [proven.json]]
            }
            let target = operation == "close_window" ? try NativeTarget(request["target"]) : nil
            let app = try target?.app ?? NativeApp((request["target"] as? [String: Any])?["app"])
            if try context.hasExited(app) {
                if let target, try !cgWindowRows(context.deadline, only: target.nativeID).isEmpty {
                    throw failure("target_changed", "An exited process does not prove the selected backing surface disappeared")
                }
                return ["dispatch": "not_dispatched", "evidence": "confirmed",
                        "postcondition": operation == "quit_app" ? "application_exited" : "window_closed"]
            }
            try context.verify(app)
            if operation == "quit_app" {
                guard let application = NSRunningApplication(processIdentifier: app.pid) else {
                    throw failure("app_unproven", "Cannot bind the verified application for normal termination")
                }
                guard let accepted = try dtwWithInputAdmission({
                    try self.requireExecutionForeground(request, app, context)
                    attempted = true
                    return application.terminate()
                }), accepted else {
                    throw failure("quit_unconfirmed", "Normal termination request was not confirmed; inspect application state")
                }
            } else if let target {
                let window = try validate(target, context)
                guard let close = axElement(try axAttribute(window.element, kAXCloseButtonAttribute, context.deadline)),
                      try owned(close, by: window.element, target: target, deadline: context.deadline),
                      try actions(close, context.deadline).contains(kAXPressAction) else {
                    throw failure("close_unsupported", "Exact window has no proven accessible close action")
                }
                _ = try validate(target, context)
                try context.deadline.prepare(close)
                guard let result = try dtwWithInputAdmission({
                    try self.requireExecutionForeground(request, target, context)
                    attempted = true
                    return AXUIElementPerformAction(close, kAXPressAction as CFString)
                }), result == .success else { throw failure("close_unconfirmed", "Window close may have been delivered; inspect its state") }
            } else { throw failure("invalid_request", "Unknown lifecycle operation") }
            // A bounded readback may end with a save dialog, an application
            // refusal or delayed termination. None permits another dispatch.
            let waitUntil = Date().addingTimeInterval(2)
            repeat {
                try context.deadline.check()
                let exited = try context.hasExited(app)
                let closed: Bool
                if let target {
                    if !exited { try context.verify(app) }
                    closed = try cgWindowRows(context.deadline, only: target.nativeID).isEmpty
                } else { closed = exited }
                if closed {
                    return ["dispatch": "dispatched", "evidence": "confirmed",
                            "postcondition": operation == "quit_app" ? "application_exited" : "window_closed"]
                }
                RunLoop.current.run(until: Date().addingTimeInterval(0.02))
            } while Date() < waitUntil
            return ["dispatch": "dispatched", "evidence": "unverified"]
        } catch let error as NativeFailure {
            return error.json.merging(["dispatch": attempted ? "unknown" : "not_dispatched",
                                       "evidence": "unverified"]) { _, new in new }
        } catch {
            return ["dispatch": attempted ? "unknown" : "not_dispatched", "evidence": "unverified",
                    "error": "lifecycle_failed", "message": String(describing: error)]
        }
    }

    // AppKit menu tracking can commit its highlighted item even when addressed
    // mouse events name another row (a mouseMoved primer does not fix this).
    // A visible, freshly attributed menu permits the existing AXPress primitive
    // at the proven pixel. Ordinary/opaque surfaces keep the CUA pointer route.
    private func tryMenuClick(_ request: [String: Any]) -> [String: Any] {
        var scopeFailure: NativeFailure?
        do {
            let context = try RequestContext(request)
            let target = try NativeTarget(request["target"])
            let surface = try validateSurface(target, context)
            let root: AXUIElement?
            do { root = try associatedClientRoot(surface, app: target.app, context) }
            catch let error as NativeFailure where ["ax_unavailable", "association_unavailable"].contains(error.code) {
                return ["native_menu": false]
            }
            guard let root, try axAttribute(root, kAXRoleAttribute, context.deadline) as? String == kAXMenuRole else {
                return ["native_menu": false]
            }
            guard let x = request["screen_x"] as? Double, let y = request["screen_y"] as? Double,
                  x.isFinite, y.isFinite, surface.bounds.contains(CGPoint(x: x, y: y)) else {
                throw failure("invalid_point", "Menu point is outside its current surface")
            }
            let point = CGPoint(x: x, y: y)
            let application = AXUIElementCreateApplication(target.app.pid)
            try context.deadline.prepare(application)
            var hit: AXUIElement?
            let hitResult = AXUIElementCopyElementAtPosition(application, Float(x), Float(y), &hit)
            try context.deadline.check()
            guard hitResult == .success, let hit,
                  try axPID(hit, context.deadline) == target.app.pid,
                  try axAttribute(hit, kAXRoleAttribute, context.deadline) as? String == kAXMenuItemRole,
                  try axAncestor(hit, is: root, context.deadline),
                  let itemBounds = try axRect(hit, context.deadline), itemBounds.contains(point),
                  try axAttribute(hit, kAXEnabledAttribute, context.deadline) as? Bool == true,
                  try actions(hit, context.deadline).contains(kAXPressAction) else {
                throw failure("element_not_pressable", "Menu pixel has no proven enabled item; no input was dispatched")
            }
            let current = try validateSurface(target, context)
            guard current.bounds == surface.bounds,
                  let currentRoot = try associatedClientRoot(current, app: target.app, context), CFEqual(root, currentRoot),
                  try axAncestor(hit, is: currentRoot, context.deadline),
                  try axRect(hit, context.deadline) == itemBounds else {
                throw failure("target_changed", "Menu or item changed before its pixel action")
            }
            var result = try performSemanticPress(deadline: context.deadline, setTimeout: {
                AXUIElementSetMessagingTimeout(hit, $0)
            }, dispatch: {
                do { try self.requireExecutionForeground(request, target, context) }
                catch let error as NativeFailure { scopeFailure = error; return nil }
                catch { scopeFailure = failure("execution_scope_unavailable", "Execution scope proof failed: \(error)"); return nil }
                return dtwWithInputAdmission {
                    self.executionFocus?.anchor.admitInput()
                    self.backgroundWindowOrder?.admitted.insert(target.app)
                    return AXUIElementPerformAction(hit, kAXPressAction as CFString)
                }
            })
            result["native_menu"] = true
            return result
        } catch let error as NativeFailure {
            return (scopeFailure ?? error).json.merging(["native_menu": true, "dispatch": "not_dispatched", "evidence": "unverified"]) { _, new in new }
        } catch {
            return ["native_menu": true, "dispatch": "not_dispatched", "evidence": "unverified",
                    "error": "native_error", "message": String(describing: error)]
        }
    }

    private func press(_ request: [String: Any]) -> [String: Any] {
        var scopeFailure: NativeFailure?
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
            guard CFEqual(window.element, observation.root) else {
                throw failure("stale_observation", "Original AX root changed before press; observe again")
            }
            let role = try axAttribute(element.element, kAXRoleAttribute, context.deadline) as? String ?? ""
            let enabled = try axAttribute(element.element, kAXEnabledAttribute, context.deadline) as? Bool
            if try menuOwned(element.element, target: target, deadline: context.deadline),
               try !focused(target, window.element, context.deadline) {
                throw failure("foreground_required", "App menu actions require the exact destination window already focused")
            }
            guard let capability = try pressCapability(element.element, role: role, enabled: enabled, window: window.element,
                                                       target: target, deadline: context.deadline) else {
                throw failure("element_not_pressable", "Current role, enabled state, ancestry or AXPress support changed")
            }
            let focusText: Bool
            let selection: AXUIElement?
            switch (element.pressCapability, capability) {
            case (.action?, .action): focusText = false; selection = nil
            case (.focus?, .focus): focusText = true; selection = nil
            case (.selection(let observed)?, .selection(let current)):
                guard CFEqual(observed, current) else { throw failure("target_changed", "Collection item changed before selection") }
                focusText = false; selection = current
            default: throw failure("target_changed", "Semantic press capability changed; observe again")
            }
            if focusText || selection != nil {
                // A reusable native text object must retain its observed meaning
                // across activation; never focus a replacement file-list cell.
                func preview(_ attribute: String) throws -> String? {
                    let raw = try axAttribute(element.element, attribute, context.deadline)
                    guard let value = (raw as? String) ?? (raw as? NSNumber)?.stringValue else { return nil }
                    return String(String.UnicodeScalarView(value.unicodeScalars.prefix(256)))
                }
                let fields = try SemanticTextFields(role: role, title: preview(kAXTitleAttribute),
                    value: preview(kAXValueAttribute), description: preview(kAXDescriptionAttribute))
                guard semanticRole(role) == element.role, fields.name == element.name, fields.value == element.value else {
                    throw failure("target_changed", "Editable control changed before semantic focus")
                }
            }
            let modalPress = try selection == nil && backgroundRequiresForeground(window.element, deadline: context.deadline)
            if focusText || modalPress {
                if try !focused(target, window.element, context.deadline) {
                    guard request["input_policy"] as? String == "auto" else {
                        throw failure("foreground_required", "Semantic text focus or modal press requires a foreground borrow")
                    }
                    // This entire preflight is read-only. Retain just this exact
                    // credential for the one post-activation action.
                    observations[binding] = observation
                    return ["dispatch":"not_dispatched", "evidence":"unverified",
                            "foreground_plan":["reason":focusText ? "semantic_text_focus" : "native_modal_press", "visual":false]]
                }
            }
            let finalWindow = try validate(target, context)
            guard try owned(element.element, by: finalWindow.element, target: target, deadline: context.deadline) else {
                throw failure("target_changed", "Element ancestry changed before dispatch")
            }
            try validateRegionElement(element, root: finalWindow.element, target: target,
                                      bounds: finalWindow.cg.bounds, deadline: context.deadline)
            let actionElement = selection ?? element.element
            if !focusText, selection == nil, request["input_policy"] as? String == "auto" {
                prepareBackgroundWindowOrder(target.app, context)
            }
            var result = try performSemanticPress(deadline: context.deadline,
                actionName: selection != nil ? "AXSelected" : focusText ? "AXFocus" : "AXPress", setTimeout: {
                AXUIElementSetMessagingTimeout(actionElement, $0)
            }, dispatch: {
                do { try self.requireExecutionForeground(request, target, context) }
                catch let error as NativeFailure { scopeFailure = error; return nil }
                catch { scopeFailure = failure("execution_scope_unavailable", "Execution scope proof failed: \(error)"); return nil }
                // The gate refuses before entry. Once entered, AX may already
                // have delivered effects, regardless of its eventual error.
                return dtwWithInputAdmission {
                    self.executionFocus?.anchor.admitInput()
                    self.backgroundWindowOrder?.admitted.insert(target.app)
                    if let selection {
                        return AXUIElementSetAttributeValue(selection, kAXSelectedAttribute as CFString, kCFBooleanTrue)
                    }
                    if focusText {
                        return AXUIElementSetAttributeValue(element.element, kAXFocusedAttribute as CFString, kCFBooleanTrue)
                    }
                    return AXUIElementPerformAction(element.element, kAXPressAction as CFString)
                }
            })
            if let selection, result["dispatch"] as? String == "dispatched" {
                do {
                    let currentWindow = try validate(target, context)
                    guard try owned(selection, by: currentWindow.element, target: target, deadline: context.deadline),
                          try axAttribute(selection, kAXSelectedAttribute, context.deadline) as? Bool == true else {
                        result["error"] = "selection_unconfirmed"
                        result["message"] = "Collection selection was dispatched but its selected state was not confirmed; inspect before another action"
                        result["dispatch"] = "unknown"
                        return result
                    }
                    result["evidence"] = "confirmed"
                    result["postcondition"] = "element_selected"
                } catch {
                    result["dispatch"] = "unknown"
                    result["error"] = "selection_unconfirmed"
                    result["message"] = "Collection selection readback failed after dispatch: \(error)"
                }
            }
            return result
        } catch let error as NativeFailure {
            return (scopeFailure ?? error).json.merging(["dispatch": "not_dispatched", "evidence": "unverified"]) { _, new in new }
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
            if operation == "begin_execution_focus" { return beginExecutionFocus(request) }
            if operation == "prepare_background_order" || operation == "admit_background_order" {
                let context = try RequestContext(request)
                let target = request["target"] as? [String: Any]
                let app = try NativeApp(target?["app"])
                try context.verify(app)
                if operation == "prepare_background_order" { prepareBackgroundWindowOrder(app, context) }
                // Record only a sent/uncertain payload. Refused preparation
                // creates no window-order obligation or input side effect.
                else if backgroundWindowOrder?.previouslyAbove[app] != nil {
                    backgroundWindowOrder?.admitted.insert(app)
                }
                return ["prepared": true]
            }
            if operation == "press" { return press(request) }
            if operation == "try_native_menu_click" { return tryMenuClick(request) }
            if operation == "set_value" { return setValue(request) }
            if ["focus", "type_text", "keypress", "scroll"].contains(operation) { return windowInput(request) }
            let context = try RequestContext(request)
            switch operation {
            case "prepare_foreground":
                let target = try NativeTarget(request["target"])
                let surface = try validateSurface(target, context)
                let window: CandidateWindow?
                if request["visual"] as? Bool == true { window = nil }
                else {
                    do { window = try validate(target, context) }
                    catch let error as NativeFailure where error.code == "semantic_window_unavailable" { window = nil }
                }
                let point: CGPoint?
                if let x = request["screen_x"] as? Double, let y = request["screen_y"] as? Double {
                    point = CGPoint(x: x, y: y)
                } else { point = nil }
                let isMenu = try window.map { try axAttribute($0.element, kAXRoleAttribute, context.deadline) as? String == kAXMenuRole } ?? false
                try requireExecutionForeground(request, target, context)
                let executionScoped = request["_execution_foreground"] as? Bool == true || executionScopeActive()
                try borrowFocus(target, isMenu ? nil : window?.element, context, inputPoint: point,
                                allowActivation: !executionScoped && request["_allow_activation"] as? Bool != false,
                                executionScoped: executionScoped)
                return ["prepared": true, "borrowed": borrowedFocus?.changedForeground ?? false,
                        "target": target.json, "bounds": surface.coordinates, "surface_owner": surfaceOwner(target)]
            case "validate_execution_focus": return try validateExecutionFocus(request, context)
            case "release_input_focus":
                let invalidated = try releaseLocalFocus(context.deadline, handoff: true)
                var result: [String: Any] = ["released": true, "focus": invalidated ? "changed_externally" : "unchanged"]
                if let point = releasedExecutionPointer { result["execution_pointer"] = [Double(point.x), Double(point.y)] }
                releasedExecutionPointer = nil
                return result
            case "ack_execution_pointer":
                let target = try NativeTarget(request["target"])
                guard let borrowed = borrowedFocus, borrowed.target == target, borrowed.pointerHandoff,
                      let point = request["point"] as? [Double], point.count == 2,
                      borrowed.inputPoint == CGPoint(x: point[0], y: point[1]) else {
                    throw failure("invalid_request", "Pointer acknowledgement does not match the pending exact handoff")
                }
                borrowedFocus = nil; releasedExecutionPointer = nil
                focusObserver.end(borrowed.activationEpoch)
                return ["acknowledged": true]
            case "note_execution_pointer":
                let target = try NativeTarget(request["target"])
                try context.verify(target.app)
                guard var scope = executionFocus, scope.app == target.app, scope.target == target, scope.pointerOrigin != nil,
                      let point = request["point"] as? [Double], point.count == 2, point.allSatisfy({ $0.isFinite }) else {
                    throw failure("invalid_request", "Pointer return requires the exact owned execution target and a finite endpoint")
                }
                scope.pointerEndpoint = CGPoint(x: point[0], y: point[1])
                executionFocus = scope
                return ["recorded": true]
            case "release_focus": return try releaseFocus(context.deadline)
            case "validate_background":
                let target = try NativeTarget(request["target"])
                try requireExecutionForeground(request, target, context)
                let milliseconds = try integer(request["deadline_unix_ms"], "deadline_unix_ms")
                let keyboard = request["keyboard"] as? Bool == true
                if request["step"] as? Bool == true {
                    guard let validation = backgroundValidation,
                          validation.matches(target: target, deadlineMilliseconds: milliseconds, keyboard: keyboard) else {
                        throw failure("invalid_request", "Background input step has no matching admitted action")
                    }
                    let current = try validateSurface(target, validation.context)
                    guard dtwAccessibilityGranted() else {throw failure("permission_required","Accessibility access is no longer granted")}
                    try validation.checkModalState { _ = try self.validateSurface(target, validation.context) }
                    try validation.context.verify(target.app)
                    try requireExecutionForeground(request, target, validation.context)
                    return ["target": target.json, "bounds": current.coordinates, "surface_owner": surfaceOwner(target),
                            "background_requires_foreground": try nativeModalKeyboardReceiver(validation.window,
                                deadline: validation.context.deadline)]
                }
                backgroundValidation = nil
                if !keyboard {
                    let current = try validateSurface(target, context)
                    guard dtwAccessibilityGranted(request: true) else {
                        let error = dtwPermissionError("accessibility")
                        throw failure("permission_required", error["message"] as! String, details:error["details"] as? [String:Any])
                    }
                    // AX is optional for ordinary visual input. Only a proven
                    // native Open panel selects its separately verified route.
                    let framework = try inputFramework(target, context)
                    var openPanel = false
                    if framework == "appkit", let window = try? validate(target, context, input: false) {
                        openPanel = nativeOpenPanelClick(
                            role: (try? axAttribute(window.element, kAXRoleAttribute, context.deadline)) as? String,
                            identifier: (try? axAttribute(window.element, kAXIdentifierAttribute, context.deadline)) as? String)
                    }
                    try requireExecutionForeground(request, target, context)
                    if request["consume"] as? Bool == true {
                        backgroundValidation = BackgroundInputValidation(target:target, deadlineMilliseconds:milliseconds,
                            keyboard:false, context:context, window:nil)
                    }
                    return ["target":target.json,"bounds":current.coordinates,"surface_owner":surfaceOwner(target),
                            "input_framework":framework,"native_open_panel":openPanel,
                            "chromium":framework == "chromium"]
                }
                // Missing or unreadable AX/modal proof stays an error. It cannot
                // grant activation or replace the exact keyboard destination.
                let current = try validateSurface(target, context)
                let window = try validate(target, context).element
                let framework = try inputFramework(target, context)
                let result: [String: Any] = ["target": target.json, "bounds": current.coordinates,
                    "surface_owner": surfaceOwner(target), "input_framework": framework,
                    "background_requires_foreground": try nativeModalKeyboardReceiver(window,
                        deadline: context.deadline)]
                if request["consume"] as? Bool == true {
                    let validation = BackgroundInputValidation(target: target, deadlineMilliseconds: milliseconds,
                        keyboard: keyboard, context: context, window: window)
                    try validation.checkModalState { _ = try self.validateSurface(target, validation.context) }
                    observations = observations.filter { $0.value.target != target }
                    try context.verify(target.app)
                    backgroundValidation = validation
                }
                try requireExecutionForeground(request, target, context)
                return result
            case "validate_input_owner":
                let app = try NativeApp(request["app"])
                try context.verify(app)
                if let owner = request["surface_owner"] as? [String:Any] {
                    let pid = try integer(owner["pid"], "surface owner PID")
                    let birth = try integer(owner["generation"], "surface owner generation")
                    guard pid > 0, pid <= UInt64(Int32.max), try context.startStamp(Int32(pid)) == birth else {
                        throw failure("app_changed", "Original input surface owner changed")
                    }
                }
                return ["app": app.json]
            case "resolve_app":
                guard let identifier = request["app_id"] as? String else { throw failure("invalid_request", "Missing application target") }
                return try context.applicationTarget(identifier)
            case "launch_app", "quit_app", "close_window":
                let result = lifecycle(request)
                if let scope = executionFocus, let rawTarget = request["target"] as? [String: Any],
                   let app = try? NativeApp(rawTarget["app"]) {
                    let closedWindow = (try? NativeTarget(rawTarget))?.nativeID
                    if let terminal = confirmedExecutionTerminal(operation: operation,
                        evidence: result["evidence"] as? String, postcondition: result["postcondition"] as? String,
                        sameApp: scope.app == app, closedWindow: closedWindow, anchorWindow: scope.anchor.id) {
                        executionFocus?.terminal = terminal
                    }
                }
                return result
            case "keypress_app", "focus_app": return appKeypress(request)
            case "catalog": return try context.catalog()
            case "discover_apps": return try context.discoverApplications()
            case "windows": return try listWindows(NativeApp(request["app"]), context)
            case "bind_window": return try bindWindow(request, context)
            case "probe_window_state": return try probeWindowState(request, context)
            case "observe": return try readOnlySemantics { try observe(request, context) }
            case "observe_next": return try readOnlySemantics { try observeNext(request, context) }
            case "read_text": return try readText(request, context)
            case "resolve_scroll_anchor": return try scrollAnchor(request, context, validating: false)
            case "validate_scroll_anchor": return try scrollAnchor(request, context, validating: true)
            case "validate":
                let target = try NativeTarget(request["target"])
                let current = try validateSurface(target, context)
                return ["target": target.json, "bounds": current.coordinates, "surface_owner": surfaceOwner(target)]
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
// cleaning input resources; callers without a focus scope can still query sticky cleanup failure.
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
        let target = try NativeTarget(request["target"])
        let surface = try nativeStore.validateSurface(target, context)
        guard let owner = request["surface_owner"] as? [String: Any],
              let pid = owner["pid"] as? Int32, pid == surface.pid,
              let birth = owner["generation"] as? UInt64,
              try context.startStamp(pid) == birth else {
            throw failure("target_changed", "Actual surface owner changed before pixel input")
        }
        try nativeStore.validateVisualInput(target, context)
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

// A native title bar is a window-manager operation, not a content drag. The
// close control and AX hit establish the title strip; labels/rectangles alone
// never identify another application's window. Position is set exactly once.
func dtwMoveNativeTitlebar(_ request: [String:Any], destinationVisible: (CGRect)->Bool) -> [String:Any]? {
    nativeStore.lock.lock()
    defer { nativeStore.lock.unlock() }
    var attempted = false
    do {
        let context = try RequestContext(request)
        let target = try NativeTarget(request["target"])
        let window = try nativeStore.validate(target,context,input:false)
        guard let path=request["path"] as? [[String:Double]],let first=path.first,let last=path.last,
              let x=first["x"],let y=first["y"],let endX=last["x"],let endY=last["y"],
              let close=axElement(try axAttribute(window.element,kAXCloseButtonAttribute,context.deadline)),
              let button=try axRect(close,context.deadline),
              let rect=try axRect(window.element,context.deadline),
              button.minY >= rect.minY,button.maxY <= rect.minY+80,
              y >= button.minY,y <= button.maxY,x > button.maxX+12,x < rect.maxX-12 else { return nil }
        var hit: AXUIElement?
        try context.deadline.prepare(AXUIElementCreateApplication(target.app.pid))
        guard AXUIElementCopyElementAtPosition(AXUIElementCreateApplication(target.app.pid),Float(x),Float(y),&hit) == .success,
              let hit else { return nil }
        let title=axElement(try axAttribute(window.element,kAXTitleUIElementAttribute,context.deadline))
        guard CFEqual(hit,window.element) || title.map({CFEqual(hit,$0)}) == true else { return nil }
        var settable: DarwinBoolean = false
        try context.deadline.prepare(window.element)
        guard AXUIElementIsAttributeSettable(window.element,kAXPositionAttribute as CFString,&settable) == .success,settable.boolValue else {
            return ["dispatch":"not_dispatched","evidence":"unverified","error":"window_move_unsupported","message":"This proven title bar has no writable native window position"]
        }
        var point=CGPoint(x:rect.minX+endX-x,y:rect.minY+endY-y)
        guard destinationVisible(CGRect(origin:point,size:rect.size)) else {
            return ["dispatch":"not_dispatched","evidence":"unverified","error":"display_proof_unavailable","message":"Window movement must remain fully on one display"]
        }
        guard let value=AXValueCreate(.cgPoint,&point) else { throw failure("input_unavailable","Cannot prepare native window position") }
        _ = try nativeStore.validate(target,context,input:false)
        try context.deadline.check()
        guard let result=dtwWithInputAdmission({attempted=true;return AXUIElementSetAttributeValue(window.element,kAXPositionAttribute as CFString,value)}),result == .success else {
            throw failure("window_move_unconfirmed","Native position may have changed; inspect the returned window bounds")
        }
        return ["dispatch":"dispatched","evidence":"unverified","route":"native_window_position"]
    } catch let error as NativeFailure {
        if !attempted && requiresVisualInput(error) { return nil }
        return error.json.merging(["dispatch":attempted ? "unknown":"not_dispatched","evidence":"unverified"]){_,new in new}
    } catch { return ["dispatch":attempted ? "unknown":"not_dispatched","evidence":"unverified","error":"native_error","message":String(describing:error)] }
}
