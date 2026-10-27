import Foundation
import AppKit
import ScreenCaptureKit
import ImageIO
import UniformTypeIdentifiers

// Small public-API bridge. No CUA types, private AX symbols or app-specific rules.
private func attr(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    return AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success ? value : nil
}
private func frame(_ element: AXUIElement) -> CGRect? {
    guard let p = attr(element, kAXPositionAttribute), let s = attr(element, kAXSizeAttribute),
          CFGetTypeID(p) == AXValueGetTypeID(), CFGetTypeID(s) == AXValueGetTypeID() else { return nil }
    var point = CGPoint.zero; var size = CGSize.zero
    guard AXValueGetValue(p as! AXValue, .cgPoint, &point), AXValueGetValue(s as! AXValue, .cgSize, &size) else { return nil }
    return CGRect(origin: point, size: size)
}
private func hit(_ point: CGPoint, client: pid_t) -> (pid_t, CGRect?, String)? {
    let system = AXUIElementCreateSystemWide()
    AXUIElementSetMessagingTimeout(system, 0.15)
    var result: AXUIElement?
    guard AXUIElementCopyElementAtPosition(system, Float(point.x), Float(point.y), &result) == .success,
          let element = result else { return nil }
    var pid: pid_t = 0
    guard AXUIElementGetPid(element, &pid) == .success, pid == client else { return nil }
    AXUIElementSetMessagingTimeout(element, 0.15)
    // A remote menu's AXWindow is its chooser, not the menu surface. Walk
    // public ancestors to the nearest menu/window before following AXWindow.
    var current=element
    for _ in 0..<12 {
        AXUIElementSetMessagingTimeout(current, 0.15)
        let role=attr(current,kAXRoleAttribute) as? String ?? ""
        if role == kAXMenuRole || role == kAXWindowRole || role == kAXMenuBarRole {
            return (pid,frame(current),role)
        }
        guard let next=attr(current,kAXParentAttribute),CFGetTypeID(next)==AXUIElementGetTypeID() else {break}
        current=next as! AXUIElement
    }
    let window = attr(element, kAXWindowAttribute) ?? attr(element, kAXTopLevelUIElementAttribute)
    if let window, CFGetTypeID(window) == AXUIElementGetTypeID() {
        return (pid, frame(window as! AXUIElement), attr(window as! AXUIElement, kAXRoleAttribute) as? String ?? "")
    }
    return (pid, frame(element), attr(element,kAXRoleAttribute) as? String ?? "")
}
private struct Window {
    let pid: pid_t; let id: CGWindowID; let layer: Int; let rank: Int; let rect: CGRect
    func json(_ association: String) -> [String: Any] {
        ["pid": Int(pid), "id": Int(id), "layer": layer, "rank": rank,
         "bounds": [rect.minX,rect.minY,rect.width,rect.height], "association": association]
    }
}
private func windows() -> [Window] {
    let rows = CGWindowListCopyWindowInfo([.optionOnScreenOnly,.excludeDesktopElements],0) as? [[String:Any]] ?? []
    return rows.enumerated().compactMap { rank, row in
        guard let pid = row[kCGWindowOwnerPID as String] as? Int32,
              let id = row[kCGWindowNumber as String] as? UInt32,
              let bounds = row[kCGWindowBounds as String] as? NSDictionary,
              let rect = CGRect(dictionaryRepresentation: bounds), rect.width > 1, rect.height > 1,
              (row[kCGWindowAlpha as String] as? Double ?? 1) > 0.01 else { return nil }
        return Window(pid:pid,id:id,layer:row[kCGWindowLayer as String] as? Int ?? 0,rank:rank,rect:rect)
    }
}
private func near(_ a: CGRect, _ b: CGRect) -> Bool {
    abs(a.minX-b.minX) <= 2 && abs(a.minY-b.minY) <= 2 && abs(a.width-b.width) <= 2 && abs(a.height-b.height) <= 2
}
private func fail(_ code: String, _ message: String) -> [String:Any] { ["error":code,"message":message] }
private func discover(_ pid: pid_t) -> [String:Any] {
    guard AXIsProcessTrusted(), CGPreflightScreenCaptureAccess() else { return fail("permission_denied","Accessibility and Screen Recording are required") }
    guard let app = NSRunningApplication(processIdentifier:pid), !app.isTerminated else { return fail("app_closed","Target application exited") }
    let all = windows()
    let owned = all.filter { $0.pid == pid }
    var selected: [[String:Any]] = []
    let deadline=Date().addingTimeInterval(10)
    for w in all {
        if Date()>deadline {return fail("discovery_timeout","Window attribution timed out; no partial inventory returned")}
        if w.pid == pid { selected.append(w.json("same_pid")); continue }
        // A public AX hit test identifies the client; exact AX window geometry
        // binds that client to this visible WindowServer surface, not a neighbor.
        let points = [CGPoint(x:w.rect.midX,y:w.rect.midY),CGPoint(x:w.rect.minX+12,y:w.rect.minY+12),CGPoint(x:w.rect.maxX-12,y:w.rect.maxY-12)]
        if points.contains(where: { p in
            guard let (client, rect, role) = hit(p,client:pid), client == pid, role != kAXMenuBarRole, let rect else { return false }
            return near(rect,w.rect) && visibleAt(p, target:w, all:all)
        }) { selected.append(w.json("public_ax_client_geometry")) }
    }
    let ids = Set(selected.compactMap { $0["id"] as? Int })
    let frontRank = selected.compactMap { $0["rank"] as? Int }.min() ?? Int.max
    let possible = all.filter { w in
        !ids.contains(Int(w.id)) && w.rank < frontRank && owned.contains(where: { $0.rect.intersects(w.rect) })
    }.map { $0.json("unattributed_occluder") }
    return ["windows":selected,"occluders":possible,"association_complete":false,
            "process_identity":"\(pid):\(app.launchDate?.timeIntervalSince1970 ?? 0)"]
}

@available(macOS 14.0, *)
private func capture(_ request: [String:Any]) -> [String:Any] {
    guard CGPreflightScreenCaptureAccess() else { return fail("permission_denied","Screen Recording is required") }
    let id = UInt32(request["window_id"] as? Int ?? 0)
    let pid = Int32(request["pid"] as? Int ?? 0)
    let cap = request["max_image_dimension"] as? Int ?? 1600
    let semaphore = DispatchSemaphore(value:0)
    let tracing = ProcessInfo.processInfo.environment["DTW_SCENE_TRACE"] == "1"
    let began = ProcessInfo.processInfo.systemUptime
    func trace(_ phase: String) {
        guard tracing else { return }
        let message = "dtw_scene capture pid=\(pid) window=\(id) elapsed=\(ProcessInfo.processInfo.systemUptime-began) phase=\(phase)\n"
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
        guard #available(macOS 14.2, *) else {
            box.result=fail("unsupported_platform","Exact single-window capture requires macOS 14.2 or later");semaphore.signal();return
        }
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
    }
    guard semaphore.wait(timeout:.now()+12) == .success else { trace("timeout"); return fail("capture_timeout","ScreenCaptureKit timed out; no input occurred") }
    return box.result
}
// WindowServer bounds include transparent full-screen utility surfaces. Test
// only the point's alpha locally; never return unrelated window pixels to Agent.
private func visibleAt(_ point: CGPoint, target: Window, all: [Window]) -> Bool {
    let covers=all.prefix(while: { $0.id != target.id }).filter { $0.rect.contains(point) }
    guard covers.count<=8 else {return false}
    for cover in covers {
        guard #available(macOS 14.0, *) else {return false}
        let sample=capture(["window_id":Int(cover.id),"pid":Int(cover.pid),"max_image_dimension":0,
                            "screen_x":Double(point.x),"screen_y":Double(point.y)])
        guard sample["alpha"] as? Int == 0 else {return false}
    }
    return true
}
private func click(_ request: [String:Any]) -> [String:Any] {
    guard AXIsProcessTrusted() else { return fail("permission_denied","Accessibility is required") }
    let id = UInt32(request["window_id"] as? Int ?? 0), pid = Int32(request["pid"] as? Int ?? 0)
    guard let b = request["bounds"] as? [Double], b.count == 4,
          let x = request["screen_x"] as? Double, let y = request["screen_y"] as? Double,
          x.isFinite, y.isFinite else { return fail("invalid_point","Missing screenshot mapping") }
    let expected = CGRect(x:b[0],y:b[1],width:b[2],height:b[3]), point = CGPoint(x:x,y:y)
    let all = windows()
    guard let target = all.first(where: { $0.id == id && $0.pid == pid }), target.rect == expected, target.rect.contains(point) else {
        return fail("stale_observation","Window closed or moved; observe again")
    }
    guard visibleAt(point,target:target,all:all) else {
        return fail("target_occluded","Another visible window covers the point or its transparency is unknown; no click posted")
    }
    // Sampling transparent surfaces takes time. Recheck the exact hit stack
    // immediately before dispatch, rather than trusting pre-sampling geometry.
    let after=windows()
    guard let fresh=after.first(where: {$0.id==id && $0.pid==pid}),fresh.rect==expected else {
        return fail("stale_observation","Window changed during visibility validation")
    }
    func signature(_ list:[Window])->[String] {
        list.prefix(while: {$0.id != id}).filter {$0.rect.contains(point)}.map {"\($0.pid):\($0.id):\($0.layer):\($0.rect)"}
    }
    guard signature(all)==signature(after) else {return fail("stale_observation","Hit stack changed during visibility validation")}
    if request["operation"] as? String == "validate_point" { return ["validated":true] }
    guard let source = CGEventSource(stateID:.hidSystemState),
          let down = CGEvent(mouseEventSource:source,mouseType:.leftMouseDown,mouseCursorPosition:point,mouseButton:.left),
          let up = CGEvent(mouseEventSource:source,mouseType:.leftMouseUp,mouseCursorPosition:point,mouseButton:.left) else {
        return fail("input_unavailable","Cannot create pointer events")
    }
    #if DTW_NATIVE_CHECKPOINTS
    if let error=dtwCheckpointValidate(request) { return error }
    if request["target"] != nil, (request["deadline_unix_ms"] as? Double ?? 0)/1000-Date().timeIntervalSince1970 < 0.3 {
        return fail("deadline_exceeded","Insufficient budget for pointer placement and click release")
    }
    #endif
    // This is visible-point input: AppKit can hit-test against the physical
    // cursor rather than only the event location. Never activate or retry.
    guard CGWarpMouseCursorPosition(point) == .success,
          CGAssociateMouseAndMouseCursorPosition(1) == .success else {
        return ["error":"input_unavailable","message":"Cannot position the pointer; no click posted","pointer_moved":true]
    }
    Thread.sleep(forTimeInterval:0.04)
    let settled=windows()
    guard let live=settled.first(where: {$0.id==id && $0.pid==pid}),live.rect==expected,
          signature(after)==signature(settled),
          let pointer=CGEvent(source:nil)?.location,
          abs(pointer.x-point.x)<=2,abs(pointer.y-point.y)<=2 else {
        return ["error":"stale_observation","message":"Window, hit stack or pointer changed after pointer placement; no click posted","pointer_moved":true]
    }
    #if DTW_NATIVE_CHECKPOINTS
    if var error=dtwCheckpointValidate(request) { error["pointer_moved"]=true; return error }
    if request["target"] != nil, (request["deadline_unix_ms"] as? Double ?? 0)/1000-Date().timeIntervalSince1970 < 0.1 {
        return ["error":"deadline_exceeded","message":"Insufficient budget to complete click release","pointer_moved":true]
    }
    #endif
    // A normal click has clickState=1. Zero can be delivered without activating
    // an NSButton. Separate press/release so tracking controls see both phases.
    down.flags=[];up.flags=[]
    down.setIntegerValueField(.mouseEventClickState,value:1)
    up.setIntegerValueField(.mouseEventClickState,value:1)
    down.post(tap:.cghidEventTap)
    Thread.sleep(forTimeInterval:0.028)
    up.post(tap:.cghidEventTap)
    Thread.sleep(forTimeInterval:0.04)
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
            case "discover": result = discover(Int32(request["pid"] as? Int ?? 0))
            case "capture":
                if #available(macOS 14.0, *) { result = capture(request) }
                else { result = fail("unsupported_platform","SCK screenshots require macOS 14+") }
            case "click", "validate_point": result = click(request)
            default: result = fail("invalid_operation","Unknown native extension operation")
            }
        } catch { result = fail("invalid_request",String(describing:error)) }
        #if DTW_NATIVE_CHECKPOINTS
        // The producer owns dispatch classification, including pointer-only effects.
        if let request = try? JSONSerialization.jsonObject(with:Data(String(cString:raw).utf8)) as? [String:Any], request["operation"] as? String == "click" {
            result["dispatch"] = result["delivery"] as? String == "posted" ? "dispatched" : (result["pointer_moved"] as? Bool == true ? "unknown" : "not_dispatched")
            result["evidence"] = "unverified"
        }
        #endif
        let data = (try? JSONSerialization.data(withJSONObject:result,options:[.sortedKeys])) ?? Data("{}".utf8)
        return strdup(String(decoding:data,as:UTF8.self))
    }
}
@_cdecl("dtw_scene_free")
public func dtwSceneFree(_ raw: UnsafeMutablePointer<CChar>?) { free(raw) }
