import Foundation
import AppKit
import ScreenCaptureKit
import ImageIO
import UniformTypeIdentifiers
import Darwin

// Process-local prompt deduplication. macOS decides whether a previously
// denied request can show UI; absence of a prompt is never treated as a grant.
private final class PermissionRequests: @unchecked Sendable {
    let lock = NSLock()
    var requested = Set<String>()
    func once(_ capability: String, request: () -> Void) {
        lock.lock()
        let first = requested.insert(capability).inserted
        lock.unlock()
        if first { request() }
    }
}
private let permissionRequests = PermissionRequests()
private func permissionGranted(_ capability: String, request: Bool,
                               gate: PermissionRequests = permissionRequests,
                               preflight: () -> Bool, prompt: () -> Void) -> Bool {
    if preflight() { return true }
    if request { gate.once(capability, request: prompt) }
    return preflight()
}
func dtwAccessibilityGranted(request: Bool = false) -> Bool {
    permissionGranted("accessibility", request: request, preflight: AXIsProcessTrusted) {
        _ = AXIsProcessTrustedWithOptions([kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary)
    }
}
func dtwScreenRecordingGranted(request: Bool = false) -> Bool {
    permissionGranted("screen_recording", request: request, preflight: CGPreflightScreenCaptureAccess) {
        _ = CGRequestScreenCaptureAccess()
    }
}
// An ancestry hint, not an assertion about the private TCC attribution rules.
private func permissionHost() -> [String: Any]? {
    var pid = getppid()
    var seen = Set<pid_t>()
    while pid > 1 && seen.insert(pid).inserted {
        if let app = NSRunningApplication(processIdentifier: pid), app.activationPolicy == .regular,
           let name = app.localizedName, let bundle = app.bundleIdentifier {
            return ["name": name, "bundle_id": bundle, "source": "process_ancestry_hint"]
        }
        var info = proc_bsdinfo()
        guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, Int32(MemoryLayout<proc_bsdinfo>.size)) == MemoryLayout<proc_bsdinfo>.size else { break }
        pid = pid_t(info.pbi_ppid)
    }
    return nil
}
func dtwPermissionStatus() -> [String: Any] {
    var status: [String: Any] = ["accessibility": dtwAccessibilityGranted() ? "granted" : "required",
        "screen_recording": dtwScreenRecordingGranted() ? "granted" : "required",
        "guidance": "On first use DTW requests missing macOS access. Grant the app named by macOS (often the launching Harness), not necessarily dtw. If no prompt appears, use System Settings > Privacy & Security. Screen Recording changes may require restarting that host app."]
    if let host = permissionHost() { status["host"] = host }
    return status
}
func dtwPermissionError(_ capability: String) -> [String: Any] {
    let screen = capability == "screen_recording"
    let host = permissionHost()
    let name = host?["name"] as? String
    let hint = name.map { " The launching app appears to be \($0); follow the app name shown by macOS." } ?? " Grant the app named by macOS, usually the launching Harness or terminal."
    var details: [String: Any] = ["capability": capability, "retryable": true,
        "restart_may_be_required": screen, "prompt_requested": true]
    if let host { details["host"] = host }
    return ["error": "permission_required", "details": details,
        "message": "macOS \(screen ? "Screen Recording" : "Accessibility") access is required. A system permission request was made; if no prompt appears, open System Settings > Privacy & Security." + hint + (screen ? " After granting, restart the host if macOS requests it, reconnect DTW, then retry." : " After granting, refresh the window state before retrying.")]
}

// DTW screenshot/input bridge; linked only with the native provider's guards.
private struct Window {
    let pid: pid_t; let id: CGWindowID; let layer: Int; let rank: Int; let rect: CGRect
}
// The WindowServer cursor is not an input occluder and is not SCK-shareable.
// Layer alone is insufficient: an application may create a high-level window.
private let windowServerExecutable = URL(fileURLWithPath:
    "/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer")
    .resolvingSymlinksInPath().standardizedFileURL.path
private func isSystemCursor(layer: Int, executable: () -> String?) -> Bool {
    layer == Int(CGWindowLevelForKey(.cursorWindow)) &&
        executable() == windowServerExecutable
}
// Candidates are supplied only by the worker's private peer-helper discovery.
// They are hints, never a PID-wide, level-wide, or title-based exemption.
struct SceneOverlayCandidate {
    let pid: pid_t
    let birth: UInt64
    let windows: Set<CGWindowID>
}

private func overlayUnsigned(_ value: Any?) -> UInt64? {
    guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID() else { return nil }
    return UInt64(number.stringValue)
}

func dtwSceneOverlayCandidates(_ request: [String: Any]) -> [SceneOverlayCandidate] {
    guard let rows = request["_overlay_candidates"] as? [[String: Any]] else { return [] }
    return rows.compactMap { row in
        guard let rawPID = overlayUnsigned(row["pid"]), let pid = Int32(exactly: rawPID), pid > 0,
              let birth = overlayUnsigned(row["birth"]), birth > 0,
              let rawWindows = row["windows"] as? [Any], !rawWindows.isEmpty else { return nil }
        var ids: Set<CGWindowID> = []
        for raw in rawWindows {
            guard let value = overlayUnsigned(raw), let id = CGWindowID(exactly: value), id > 0 else { return nil }
            ids.insert(id)
        }
        return SceneOverlayCandidate(pid: pid, birth: birth, windows: ids)
    }
}

private func overlayProcessBirth(_ pid: pid_t) -> UInt64? {
    var info = proc_bsdinfo()
    let size = Int32(MemoryLayout<proc_bsdinfo>.size)
    guard pid > 0, proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size) == size,
          info.pbi_pid == UInt32(pid), info.pbi_start_tvusec < 1_000_000 else { return nil }
    let (seconds, overflow) = info.pbi_start_tvsec.multipliedReportingOverflow(by: 1_000_000)
    let (stamp, addedOverflow) = seconds.addingReportingOverflow(info.pbi_start_tvusec)
    return overflow || addedOverflow || stamp == 0 ? nil : stamp
}

private func surfaceOwnerCurrent(_ request: [String: Any]) -> Bool {
    // Internal alpha probes are not app-target operations. Every app-target
    // scene request carries the already validated actual WindowServer owner.
    guard request["target"] != nil else { return true }
    guard let owner = request["surface_owner"] as? [String: Any],
          let rawPID = overlayUnsigned(owner["pid"]), let pid = Int32(exactly: rawPID),
          let birth = overlayUnsigned(owner["generation"]), birth > 0,
          request["pid"] as? Int32 == pid else { return false }
    return overlayProcessBirth(pid) == birth
}

// Both birth reads bracket the WindowServer snapshot that proves the owner and
// exact window. Missing evidence leaves the surface in every visibility check.
private func withoutVerifiedOverlays(_ candidates: [SceneOverlayCandidate],
                                    birth: (pid_t) -> UInt64?, read: () -> [Window]) -> [Window] {
    let before = candidates.map { birth($0.pid) }
    let rows = read()
    let after = candidates.map { birth($0.pid) }
    return rows.filter { window in
        !candidates.enumerated().contains { index, candidate in
            window.pid == candidate.pid && candidate.windows.contains(window.id) &&
                before[index] == candidate.birth && after[index] == candidate.birth
        }
    }
}

private func windows(excluding candidates: [SceneOverlayCandidate] = []) -> [Window] {
    withoutVerifiedOverlays(candidates, birth: overlayProcessBirth) {
        let rows = CGWindowListCopyWindowInfo([.optionOnScreenOnly,.excludeDesktopElements],0) as? [[String:Any]] ?? []
        return rows.enumerated().compactMap { rank, row in
            guard let pid = row[kCGWindowOwnerPID as String] as? Int32,
                  let id = row[kCGWindowNumber as String] as? UInt32,
                  let bounds = row[kCGWindowBounds as String] as? NSDictionary,
                  let rect = CGRect(dictionaryRepresentation: bounds), rect.width > 1, rect.height > 1,
                  (row[kCGWindowAlpha as String] as? Double ?? 1) > 0.01 else { return nil }
            let layer = row[kCGWindowLayer as String] as? Int ?? 0
            if isSystemCursor(layer: layer, executable: {
                var bytes = [UInt8](repeating: 0, count: 4_096)
                let count = bytes.withUnsafeMutableBytes { proc_pidpath(pid, $0.baseAddress, UInt32($0.count)) }
                guard count > 0, count < bytes.count, let zero = bytes.firstIndex(of: 0) else { return nil }
                return String(bytes: bytes[..<zero], encoding: .utf8)
            }) {
                if ProcessInfo.processInfo.environment["DTW_SCENE_TRACE"] == "1" {
                    FileHandle.standardError.write(Data("dtw_scene excluded_system_cursor pid=\(pid) window=\(id)\n".utf8))
                }
                return nil
            }
            return Window(pid:pid,id:id,layer:layer,rank:rank,rect:rect)
        }
    }
}
private func near(_ a: CGRect, _ b: CGRect) -> Bool {
    abs(a.minX-b.minX) <= 2 && abs(a.minY-b.minY) <= 2 && abs(a.width-b.width) <= 2 && abs(a.height-b.height) <= 2
}
private func fail(_ code: String, _ message: String) -> [String:Any] { ["error":code,"message":message] }
@available(macOS 14.2, *)
private func capture(_ request: [String:Any], deadline: TimeInterval? = nil,
                     stopping: () -> Bool = { false }) -> [String:Any] {
    guard dtwScreenRecordingGranted(request: true) else { return dtwPermissionError("screen_recording") }
    guard surfaceOwnerCurrent(request) else { return fail("target_changed", "Surface owner changed before capture") }
    let id = UInt32(request["window_id"] as? Int ?? 0)
    let pid = Int32(request["pid"] as? Int ?? 0)
    let cap = request["max_image_dimension"] as? Int ?? 1600
    let semaphore = DispatchSemaphore(value:0)
    let tracing = ProcessInfo.processInfo.environment["DTW_SCENE_TRACE"] == "1"
    let began = ProcessInfo.processInfo.systemUptime
    func trace(_ phase: String) {
        guard tracing else { return }
        let message = "dtw_scene capture worker_pid=\(getpid()) main_thread=\(Thread.isMainThread) pid=\(pid) window=\(id) elapsed=\(ProcessInfo.processInfo.systemUptime-began) phase=\(phase)\n"
        FileHandle.standardError.write(Data(message.utf8))
    }
    trace("begin")
    // The callback retains its own box even after timeout; never accesses freed FFI memory.
    final class Box: @unchecked Sendable { var result: [String:Any] = [:] }
    let box = Box()
    SCShareableContent.getExcludingDesktopWindows(true, onScreenWindowsOnly:true) { content, error in
        trace("shareable_callback")
        guard let window = content?.windows.first(where: { $0.windowID == id && $0.owningApplication?.processID == pid }) else {
            box.result = fail("window_unavailable",error?.localizedDescription ?? "Window is no longer shareable"); semaphore.signal(); return
        }
        let filter = SCContentFilter(desktopIndependentWindow:window)
        // Child-window composition can return a parent+sheet canvas scaled
        // inside the requested sheet's dimensions. Bind one surface only.
        guard near(filter.contentRect,window.frame) else {
            box.result=fail("capture_geometry_mismatch","SCK content geometry differs from the window; no input binding created");semaphore.signal();return
        }
        let config = SCStreamConfiguration()
        let scale = cap > 0 ? min(2, Double(cap) / max(window.frame.width,window.frame.height)) : 2
        config.width = max(1, Int((window.frame.width*scale).rounded()))
        config.height = max(1, Int((window.frame.height*scale).rounded()))
        config.showsCursor = false; config.ignoreShadowsSingleWindow = true
        config.shouldBeOpaque = false
        config.includeChildWindows = false
        config.destinationRect=CGRect(x:0,y:0,width:config.width,height:config.height)
        trace("image_request")
        SCScreenshotManager.captureImage(contentFilter:filter,configuration:config) { image, error in
            trace("image_callback")
            guard let image else { box.result = fail("capture_failed",error?.localizedDescription ?? "Empty SCK frame"); semaphore.signal(); return }
            guard surfaceOwnerCurrent(request) else { box.result = fail("target_changed", "Surface owner changed during capture"); semaphore.signal(); return }
            guard image.width==config.width,image.height==config.height else {
                box.result=fail("capture_geometry_mismatch","SCK output dimensions differ from the configured frame");semaphore.signal();return
            }
            if let sx = request["screen_x"] as? Double, let sy = request["screen_y"] as? Double {
                let px = Int((sx-window.frame.minX)/window.frame.width*Double(image.width))
                let py = Int((sy-window.frame.minY)/window.frame.height*Double(image.height))
                guard px>=0,py>=0,px<image.width,py<image.height,
                      let crop=image.cropping(to:CGRect(x:px,y:py,width:1,height:1)) else {
                    box.result=fail("sample_failed","Point outside occluder");semaphore.signal();return
                }
                var pixel=[UInt8](repeating:0,count:4)
                let ok=pixel.withUnsafeMutableBytes { bytes -> Bool in
                    guard let context=CGContext(data:bytes.baseAddress,width:1,height:1,bitsPerComponent:8,bytesPerRow:4,space:CGColorSpaceCreateDeviceRGB(),bitmapInfo:CGImageAlphaInfo.premultipliedLast.rawValue) else {return false}
                    context.draw(crop,in:CGRect(x:0,y:0,width:1,height:1));return true
                }
                box.result=ok ? ["alpha":Int(pixel[3])] : fail("sample_failed","Alpha decoding failed")
                semaphore.signal();return
            }
            let data = NSMutableData()
            guard let dest = CGImageDestinationCreateWithData(data,UTType.png.identifier as CFString,1,nil) else {
                box.result = fail("capture_failed","PNG encoder unavailable"); semaphore.signal(); return
            }
            CGImageDestinationAddImage(dest,image,nil)
            guard CGImageDestinationFinalize(dest) else { box.result = fail("capture_failed","PNG encoding failed"); semaphore.signal(); return }
            box.result = ["png":data.base64EncodedString(),"width":image.width,"height":image.height,
                          "bounds":[window.frame.minX,window.frame.minY,window.frame.width,window.frame.height]]
            semaphore.signal()
        }
        trace("image_request_returned")
    }
    let until = min(deadline ?? .greatestFiniteMagnitude, ProcessInfo.processInfo.systemUptime + 12)
    trace("wait_semaphore")
    guard dtwWaitForScene(until: until, stopping: stopping, wait: {
        semaphore.wait(timeout: .now() + $0) == .success
    }) else {
        trace("timeout_or_cancel")
        return fail(stopping() ? "native_stopping" : "capture_timeout", "ScreenCaptureKit visibility wait ended; no input occurred")
    }
    return box.result
}

// Wait only while this operation is live. A late SCK callback retains its own
// box; cancellation never waits on it or lets it dispatch input.
func dtwWaitForScene(until: TimeInterval, stopping: () -> Bool,
                     now: () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
                     wait: (TimeInterval) -> Bool) -> Bool {
    while !stopping() {
        let remaining = until - now()
        guard remaining > 0 else { return false }
        if wait(min(0.01, remaining)) { return !stopping() && now() < until }
    }
    return false
}

private func transparentCovers(_ covers: [Window], permitted: () -> Bool,
                               sample: (Window) -> Int?) -> Bool {
    guard covers.count <= 8 else { return false }
    for cover in covers {
        guard permitted(), sample(cover) == 0, permitted() else { return false }
    }
    return permitted()
}
// WindowServer bounds include transparent full-screen utility surfaces. Test
// only the point's alpha locally; never return unrelated window pixels to Agent.
private func visibleAt(_ point: CGPoint, target: Window, all: [Window], deadline: TimeInterval) -> Bool {
    let covers=all.prefix(while: { $0.id != target.id }).filter { $0.rect.contains(point) }
    return transparentCovers(covers, permitted: {
        !dtwInputStopping() && ProcessInfo.processInfo.systemUptime < deadline
    }, sample: { cover in
        guard #available(macOS 14.2, *) else {return nil}
        let sample=capture(["window_id":Int(cover.id),"pid":Int(cover.pid),"max_image_dimension":0,
                            "screen_x":Double(point.x),"screen_y":Double(point.y)],
                           deadline: deadline, stopping: dtwInputStopping)
        if ProcessInfo.processInfo.environment["DTW_SCENE_TRACE"] == "1" {
            let message = "dtw_scene visibility target=\(target.id) target_pid=\(target.pid) target_rect=\(target.rect) cover=\(cover.id) pid=\(cover.pid) layer=\(cover.layer) rect=\(cover.rect) alpha=\(sample["alpha"] ?? "unknown") error=\(sample["error"] ?? "none")\n"
            FileHandle.standardError.write(Data(message.utf8))
        }
        return sample["alpha"] as? Int
    })
}

struct SurfacePointProof {
    fileprivate let target: Window
    fileprivate let point: CGPoint
    fileprivate let stack: [String]
    fileprivate let overlayCandidates: [SceneOverlayCandidate]

    func matchesCurrent(requirePointer: Bool = false) -> Bool {
        let current = windows(excluding: overlayCandidates)
        guard let live = current.first(where: { $0.id == target.id && $0.pid == target.pid }),
              live.rect == target.rect, hitStack(current, id: target.id, point: point) == stack else { return false }
        if requirePointer {
            guard let cursor = CGEvent(source: nil)?.location,
                  abs(cursor.x-point.x) <= 2, abs(cursor.y-point.y) <= 2 else { return false }
        }
        return true
    }
}

private func hitStack(_ list: [Window], id: CGWindowID, point: CGPoint) -> [String] {
    list.prefix(while: { $0.id != id }).filter { $0.rect.contains(point) }
        .map { "\($0.pid):\($0.id):\($0.layer):\($0.rect)" }
}

private struct PointReadinessFailure: Error {
    let result: [String: Any]
}

// A missing/replaced/moved target is terminal even while presentation is
// settling. Only visibility or hit-stack readiness may be sampled again.
private func proveSurfacePoint(_ point: CGPoint, id: CGWindowID, pid: pid_t,
                               bounds: CGRect, deadline: TimeInterval,
                               overlayCandidates: [SceneOverlayCandidate]) throws -> SurfacePointProof? {
    func trace(_ phase: String) {
        guard ProcessInfo.processInfo.environment["DTW_SCENE_TRACE"] == "1" else { return }
        let message = "dtw_scene point phase=\(phase) pid=\(pid) window=\(id) point=\(point) rect=\(bounds) uptime=\(ProcessInfo.processInfo.systemUptime)\n"
        FileHandle.standardError.write(Data(message.utf8))
    }
    func target(in list: [Window], phase: String) throws -> Window {
        guard let value = list.first(where: { $0.id == id && $0.pid == pid }),
              value.rect == bounds, bounds.contains(point) else {
            trace(phase)
            throw PointReadinessFailure(result: fail("stale_observation", "Window closed or moved; observe again"))
        }
        return value
    }
    let all = windows(excluding: overlayCandidates)
    let window = try target(in: all, phase: "target_invalid_before_sample")
    let visible = visibleAt(point, target: window, all: all, deadline: deadline)
    let current = windows(excluding: overlayCandidates)
    _ = try target(in: current, phase: "target_invalid_after_sample")
    guard visible else { trace("visibility_pending"); return nil }
    let stack = hitStack(all, id: id, point: point)
    guard hitStack(current, id: id, point: point) == stack else {
        trace("hit_stack_changed"); return nil
    }
    return SurfacePointProof(target: window, point: point, stack: stack, overlayCandidates: overlayCandidates)
}

// Used by scroll and click in a short-lived scene worker. Bounds alone never
// authorize a point underneath an unknown/opaque surface.
func dtwProveSurfacePoint(_ point: CGPoint, id: CGWindowID, pid: pid_t,
                          bounds: CGRect, deadline: TimeInterval,
                          overlayCandidates: [SceneOverlayCandidate] = []) -> SurfacePointProof? {
    try? proveSurfacePoint(point, id: id, pid: pid, bounds: bounds, deadline: deadline,
                           overlayCandidates: overlayCandidates)
}

// A transparent foreign surface above a client can pass a public AX hit test
// through to that client. Association therefore also requires an opaque pixel
// from this exact surface, never merely matching geometry and a hit below it.
func dtwProveAssociatedSurfacePoint(_ point: CGPoint, id: CGWindowID, pid: pid_t,
                                   bounds: CGRect, deadline: TimeInterval) -> Bool {
    guard let proof = dtwProveSurfacePoint(point, id: id, pid: pid, bounds: bounds, deadline: deadline),
          #available(macOS 14.2, *) else { return false }
    let sample = capture(["window_id":Int(id), "pid":Int(pid), "max_image_dimension":0,
                          "screen_x":Double(point.x), "screen_y":Double(point.y)],
                         deadline: deadline, stopping: dtwInputStopping)
    return (sample["alpha"] as? Int ?? 0) > 0 && proof.matchesCurrent()
}

// The foreground route has already requested its one raise. This only waits
// for that presentation to settle, before any pointer movement or down event.
private func waitForFirstPoint(deadline: TimeInterval, stopping: () -> Bool,
                               now: () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
                               validate: (TimeInterval) -> [String: Any]?,
                               prove: (TimeInterval) throws -> SurfacePointProof?,
                               wait: (TimeInterval) -> Void = { Thread.sleep(forTimeInterval: $0) }) throws -> SurfacePointProof {
    let until = min(deadline, now() + 0.75)
    func live(checkReadiness: Bool = true) throws {
        if stopping() {
            throw PointReadinessFailure(result: fail("native_stopping", "Input stopped before point readiness; no click posted"))
        }
        let current = now()
        if current >= deadline {
            throw PointReadinessFailure(result: fail("deadline_exceeded", "Action deadline elapsed before point readiness; no click posted"))
        }
        if checkReadiness && current >= until {
            throw PointReadinessFailure(result: fail("target_occluded", "Point visibility did not settle within the readiness budget; no click posted"))
        }
    }
    func validateTarget() throws {
        let error = validate(until)
        // Cancellation and the real action deadline remain authoritative. The
        // native validator receives a rounded millisecond local deadline, which
        // can expire just before our monotonic readiness clock reaches until.
        try live(checkReadiness: false)
        if let error {
            if error["error"] as? String == "deadline_exceeded", until < deadline {
                throw PointReadinessFailure(result: fail("target_not_ready",
                    "Local point readiness validation budget elapsed; no click posted. Refresh the target before another action."))
            }
            throw PointReadinessFailure(result: error)
        }
        try live()
    }
    while true {
        try live()
        try validateTarget()
        let proof = try prove(until)
        try live()
        try validateTarget()
        if let proof { return proof }
        wait(min(0.015, max(0, until - now())))
    }
}
// Each additional click is a new dispatch boundary: the earlier pair may
// already have changed the window stack. This shared sequence also drives the
// deterministic overlay/cancellation controls without generating OS input.
func dtwClickSequence(count: Int, validateNext: () -> [String: Any]?,
                      pair: (Int) -> Bool, between: () -> Void) -> [String: Any]? {
    for index in 1...count {
        if index > 1 {
            between()
            if let error = validateNext() { return error }
        }
        if !pair(index) {
            return ["error":"native_stopping", "message":"Input stopped after releasing admitted buttons", "pointer_moved":true]
        }
    }
    return nil
}

private func click(_ request: [String:Any]) -> [String:Any] {
    guard dtwAccessibilityGranted(request: true) else { return dtwPermissionError("accessibility") }
    let id = UInt32(request["window_id"] as? Int ?? 0), pid = Int32(request["pid"] as? Int ?? 0)
    guard let b = request["bounds"] as? [Double], b.count == 4,
          let x = request["screen_x"] as? Double, let y = request["screen_y"] as? Double,
          x.isFinite, y.isFinite else { return fail("invalid_point","Missing screenshot mapping") }
    let expected = CGRect(x:b[0],y:b[1],width:b[2],height:b[3]), point = CGPoint(x:x,y:y)
    let overlayCandidates = dtwSceneOverlayCandidates(request)
    let deadline = ProcessInfo.processInfo.systemUptime + max(0,
        (request["deadline_unix_ms"] as? Double ?? 0)/1000-Date().timeIntervalSince1970)
    let proof: SurfacePointProof
    do {
        proof = try waitForFirstPoint(deadline: deadline, stopping: dtwInputStopping, validate: { until in
            var validation = request
            validation["deadline_unix_ms"] = UInt64(max(0,
                (Date().timeIntervalSince1970 + until - ProcessInfo.processInfo.systemUptime) * 1_000))
            if let error = dtwValidatePixelTarget(validation) { return error }
            guard let current = windows(excluding: overlayCandidates).first(where: { $0.id == id && $0.pid == pid }),
                  current.rect == expected, current.rect.contains(point) else {
                return fail("stale_observation", "Window closed or moved; observe again")
            }
            return nil
        }, prove: { until in
            try proveSurfacePoint(point, id: id, pid: pid, bounds: expected, deadline: until,
                                  overlayCandidates: overlayCandidates)
        })
    } catch let error as PointReadinessFailure { return error.result }
    catch { return fail("native_error", String(describing: error)) }
    // Sampling transparent surfaces takes time. Recheck the exact hit stack
    // immediately before dispatch, rather than trusting pre-sampling geometry.
    let after=windows(excluding: overlayCandidates)
    guard proof.matchesCurrent(), let fresh=after.first(where: {$0.id==id && $0.pid==pid}),fresh.rect==expected else {
        return fail("stale_observation","Window changed during visibility validation")
    }
    func signature(_ list:[Window])->[String] {
        list.prefix(while: {$0.id != id}).filter {$0.rect.contains(point)}.map {"\($0.pid):\($0.id):\($0.layer):\($0.rect)"}
    }
    let buttonName = request["button"] as? String ?? "left"
    let count = request["count"] as? Int ?? 1
    guard ["left", "right"].contains(buttonName), (1...2).contains(count) else {
        return fail("invalid_click", "Click supports left/right and count 1/2")
    }
    let button: CGMouseButton = buttonName == "right" ? .right : .left
    let downType: CGEventType = buttonName == "right" ? .rightMouseDown : .leftMouseDown
    let upType: CGEventType = buttonName == "right" ? .rightMouseUp : .leftMouseUp
    guard let source = CGEventSource(stateID:.privateState),
          let down = CGEvent(mouseEventSource:source,mouseType:downType,mouseCursorPosition:point,mouseButton:button),
          let up = CGEvent(mouseEventSource:source,mouseType:upType,mouseCursorPosition:point,mouseButton:button) else {
        return fail("input_unavailable","Cannot create pointer events")
    }
    if let error=dtwValidatePixelTarget(request) { return error }
    if (request["deadline_unix_ms"] as? Double ?? 0)/1000-Date().timeIntervalSince1970 < 0.3 {
        return fail("deadline_exceeded","Insufficient budget for pointer placement and click release")
    }
    // This is visible-point input: AppKit can hit-test against the physical
    // cursor rather than only the event location. Never activate or retry.
    guard let positioned = dtwWithInputAdmission({
        guard CGWarpMouseCursorPosition(point) == .success else { return false }
        dtwPresentPostedPointer(point, kind: 0)
        return CGAssociateMouseAndMouseCursorPosition(1) == .success
    }) else { return fail("native_stopping", "Input admission is closed") }
    guard positioned else {
        return ["error":"input_unavailable","message":"Cannot position the pointer; no click posted","pointer_moved":true]
    }
    Thread.sleep(forTimeInterval:0.04)
    let settled=windows(excluding: overlayCandidates)
    guard let live=settled.first(where: {$0.id==id && $0.pid==pid}),live.rect==expected,
          signature(after)==signature(settled),
          let pointer=CGEvent(source:nil)?.location,
          abs(pointer.x-point.x)<=2,abs(pointer.y-point.y)<=2 else {
        return ["error":"stale_observation","message":"Window, hit stack or pointer changed after pointer placement; no click posted","pointer_moved":true]
    }
    if var error=dtwValidatePixelTarget(request) { error["pointer_moved"]=true; return error }
    if (request["deadline_unix_ms"] as? Double ?? 0)/1000-Date().timeIntervalSince1970 < 0.1 {
        return ["error":"deadline_exceeded","message":"Insufficient budget to complete click release","pointer_moved":true]
    }
    down.flags=[]; up.flags=[]
    guard let input = dtwTrackInput(source: source, eventTypes: [upType]) else {
        return ["error":"native_stopping","message":"Input admission is closed","pointer_moved":true]
    }
    let sequenceError = dtwClickSequence(count: count, validateNext: {
        func uncertain(_ code: String, _ message: String) -> [String: Any] {
            ["error":code,"message":message,"pointer_moved":true]
        }
        if dtwInputStopping() { return uncertain("native_stopping", "Input closed before the next click") }
        if (request["deadline_unix_ms"] as? Double ?? 0)/1000-Date().timeIntervalSince1970 < 0.1 {
            return uncertain("deadline_exceeded", "Insufficient budget for the next paired click")
        }
        let current = windows(excluding: overlayCandidates)
        guard let live = current.first(where: { $0.id == id && $0.pid == pid }),
              live.rect == expected, signature(current) == signature(settled),
              visibleAt(point, target: live, all: current, deadline: deadline) else {
            return uncertain("stale_observation", "Window geometry or hit stack changed after the earlier click; no further click posted")
        }
        // Alpha sampling is asynchronous. Recheck after it, including the
        // typed surface identity, before admitting another down event.
        let final = windows(excluding: overlayCandidates)
        guard let last = final.first(where: { $0.id == id && $0.pid == pid }),
              last.rect == expected, signature(final) == signature(current),
              let pointer = CGEvent(source:nil)?.location,
              abs(pointer.x-point.x)<=2, abs(pointer.y-point.y)<=2 else {
            return uncertain("stale_observation", "Target, stack or pointer changed before the next click")
        }
        if var error = dtwValidatePixelTarget(request) { error["pointer_moved"] = true; return error }
        if (request["deadline_unix_ms"] as? Double ?? 0)/1000-Date().timeIntervalSince1970 < 0.1 {
            return uncertain("deadline_exceeded", "Next click deadline elapsed during validation")
        }
        return nil
    }, pair: { index in
        down.setIntegerValueField(.mouseEventClickState,value:Int64(index))
        up.setIntegerValueField(.mouseEventClickState,value:Int64(index))
        return dtwWithPairedInput(press: {
                                     input.posted(eventType: nil, button: button.rawValue)
                                     down.post(tap:.cghidEventTap)
                                     dtwPresentPostedPointer(point, kind: 1)
                                 }, release: {
                                     input.posted(eventType: upType.rawValue, button: button.rawValue)
                                     up.post(tap:.cghidEventTap)
                                     dtwPresentPostedPointer(point, kind: 2)
                                 },
                                 between: { Thread.sleep(forTimeInterval:0.028) })
    }, between: { Thread.sleep(forTimeInterval:0.035) })
    let released = dtwFinishInput(input)
    if let error = sequenceError { return error }
    if !released {
        return ["error":"input_release_unproven","message":"Posted mouse releases were not confirmed within the cleanup budget","pointer_moved":true]
    }
    return ["delivery":"posted","effect":"unverified","route":"dtw_visible_point","window_id":Int(id),"pointer_moved":true]
}
@_cdecl("dtw_scene_call")
public func dtwSceneCall(_ raw: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>? {
    autoreleasepool {
        _ = CGMainDisplayID()
        var result: [String:Any]
        do {
            let request = try JSONSerialization.jsonObject(with:Data(String(cString:raw).utf8)) as? [String:Any] ?? [:]
            switch request["operation"] as? String {
            case "capture":
                if #available(macOS 14.2, *) { result = capture(request) }
                else { result = fail("unsupported_platform","SCK screenshots require macOS 14.2+") }
            case "click": result = click(request)
            case "prove_associated_surface": result = dtwReadAssociatedSurfaceProof(request)
            default: result = fail("invalid_operation","Unknown native extension operation")
            }
        } catch { result = fail("invalid_request",String(describing:error)) }
        // The producer owns dispatch classification, including pointer-only effects.
        if let request = try? JSONSerialization.jsonObject(with:Data(String(cString:raw).utf8)) as? [String:Any], request["operation"] as? String == "click" {
            result["dispatch"] = result["delivery"] as? String == "posted" ? "dispatched" : (result["pointer_moved"] as? Bool == true ? "unknown" : "not_dispatched")
            result["evidence"] = "unverified"
        }
        let data = (try? JSONSerialization.data(withJSONObject:result,options:[.sortedKeys])) ?? Data("{}".utf8)
        return strdup(String(decoding:data,as:UTF8.self))
    }
}
@_cdecl("dtw_scene_free")
public func dtwSceneFree(_ raw: UnsafeMutablePointer<CChar>?) { free(raw) }
