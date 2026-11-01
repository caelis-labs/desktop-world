// Manual acceptance setup for owned windows only. Not linked into DTW.
// layout: read display geometry. move PID exactTitle x y: set one AXPosition.
// pixel PNG_PATH x y: read one sRGB pixel from an existing screenshot.
// inspect PID exactTitle: read the unique window's CG bounds and AXPosition.
// Does not activate, click, close, enumerate applications or select another PID.
import AppKit
import ApplicationServices
import Foundation

private func rectJSON(_ rect: CGRect) -> [String: Double] {
    ["x": rect.origin.x, "y": rect.origin.y, "width": rect.width, "height": rect.height]
}
private func emit(_ value: [String: Any], status: Int32 = 0) -> Never {
    do {
        let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
        print(String(decoding: data, as: UTF8.self))
    } catch {
        print("{\"ok\":false,\"error\":\"json_encoding_failed\"}")
        exit(2)
    }
    exit(status)
}
private func reject(_ reason: String, _ details: [String: Any] = [:]) -> Never {
    emit(details.merging(["ok": false, "error": reason]) { _, new in new }, status: 1)
}
private func attribute(_ element: AXUIElement, _ name: String) -> (AXError, CFTypeRef?) {
    var value: CFTypeRef?
    let code = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    return (code, value)
}
private func windowBounds(_ id: CGWindowID, pid: Int32) -> CGRect? {
    guard let rows = CGWindowListCopyWindowInfo(.optionIncludingWindow, id) as? [[String: Any]],
          let row = rows.first(where: {
              ($0[kCGWindowNumber as String] as? NSNumber)?.uint32Value == id &&
              ($0[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == pid
          }), let bounds = row[kCGWindowBounds as String] as? NSDictionary else { return nil }
    return CGRect(dictionaryRepresentation: bounds)
}

private let args = CommandLine.arguments
if args.count == 2 && args[1] == "layout" {
    let screens: [[String: Any]] = NSScreen.screens.compactMap { screen in
        guard let number = screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber else { return nil }
        let id = CGDirectDisplayID(number.uint32Value)
        return ["id": id, "name": screen.localizedName, "quartz_bounds": rectJSON(CGDisplayBounds(id)),
                "appkit_frame": rectJSON(screen.frame), "backing_scale": screen.backingScaleFactor,
                "bounds": [CGDisplayBounds(id).minX, CGDisplayBounds(id).minY, CGDisplayBounds(id).width, CGDisplayBounds(id).height],
                "scale": screen.backingScaleFactor,
                "primary": id == CGMainDisplayID()]
    }
    emit(["ok": true, "operation": "layout", "screens": screens])
}
if args.count == 5 && args[1] == "pixel" {
    guard let x = Int(args[3]), let y = Int(args[4]), x >= 0, y >= 0,
          let data = try? Data(contentsOf: URL(fileURLWithPath: args[2])),
          data.starts(with: [137, 80, 78, 71, 13, 10, 26, 10]),
          let image = NSBitmapImageRep(data: data) else { reject("invalid_png_or_pixel") }
    guard x < image.pixelsWide, y < image.pixelsHigh,
          let sampled = image.colorAt(x: x, y: y),
          let components = sampled.cgColor.components,
          let space = image.cgImage?.colorSpace,
          // colorAt may label the original samples as generic calibrated RGB.
          // Reattach the PNG profile before converting, rather than converting
          // those samples from an unrelated generic profile.
          let sourceColor = CGColor(colorSpace: space, components: components),
          let color = sourceColor.converted(to: CGColorSpace(name: CGColorSpace.sRGB)!,
                                           intent: .defaultIntent, options: nil),
          let rgba = color.components, rgba.count == 4 else {
        reject("pixel_unavailable", ["width": image.pixelsWide, "height": image.pixelsHigh])
    }
    emit(["ok": true, "operation": "pixel", "x": x, "y": y,
          "width": image.pixelsWide, "height": image.pixelsHigh, "color_space": "sRGB",
          "rgba": rgba.map { Int((min(1, max(0, $0)) * 255).rounded()) }])
}
guard (args.count == 6 && args[1] == "move") || (args.count == 4 && args[1] == "inspect"),
      let pid = Int32(args[2]), pid > 0, !args[3].isEmpty else {
    reject("usage", ["usage": "multidisplay_setup layout | move PID exactTitle quartzX quartzY | inspect PID exactTitle | pixel PNG_PATH x y"])
}
private let title = args[3]
private let app = AXUIElementCreateApplication(pid)
AXUIElementSetMessagingTimeout(app, 2)
private let (windowsCode, windowsValue) = attribute(app, kAXWindowsAttribute)
guard windowsCode == .success, let windows = windowsValue as? [AXUIElement] else {
    reject("ax_windows_unavailable", ["pid": pid, "ax_windows_result": windowsCode.rawValue])
}
private var matches: [AXUIElement] = []
private var observedTitles: [String] = []
for window in windows {
    AXUIElementSetMessagingTimeout(window, 2)
    let (code, value) = attribute(window, kAXTitleAttribute)
    guard code == .success, let name = value as? String else {
        // An unreadable title cannot establish that the requested title is unique.
        reject("ax_title_unavailable", ["pid": pid, "ax_title_result": code.rawValue])
    }
    observedTitles.append(name)
    if name == title { matches.append(window) }
}
guard matches.count == 1 else {
    reject(matches.isEmpty ? "window_not_found" : "window_title_not_unique",
           ["pid": pid, "title": title, "matching_count": matches.count, "observed_titles": observedTitles])
}
private let window = matches[0]
private typealias ExactWindowID = @convention(c) (AXUIElement, UnsafeMutablePointer<CGWindowID>) -> AXError
private let framework = dlopen("/System/Library/Frameworks/ApplicationServices.framework/Frameworks/HIServices.framework/HIServices", RTLD_LAZY)
private let getWindowID = framework.flatMap { dlsym($0, "_AXUIElementGetWindow") }
    .map { unsafeBitCast($0, to: ExactWindowID.self) }
private var id: CGWindowID = 0
private var owner: pid_t = 0
private let idCode = getWindowID?(window, &id)
private let ownerCode = AXUIElementGetPid(window, &owner)
guard idCode == .success, id != 0, ownerCode == .success, owner == pid,
      let before = windowBounds(id, pid: pid) else {
    reject("window_identity_unproven", ["pid": pid, "title": title, "window_id": id,
           "ax_window_id_result": idCode?.rawValue ?? -1, "ax_owner_result": ownerCode.rawValue])
}
private let (titleCode, freshTitle) = attribute(window, kAXTitleAttribute)
guard titleCode == .success, freshTitle as? String == title else {
    reject("window_title_changed", ["pid": pid, "window_id": id, "ax_title_result": titleCode.rawValue])
}
if args[1] == "inspect" {
    let (code, value) = attribute(window, kAXPositionAttribute)
    var position = CGPoint.zero
    var read = false
    if code == .success, let value, CFGetTypeID(value) == AXValueGetTypeID() {
        let ax = unsafeBitCast(value, to: AXValue.self)
        if AXValueGetType(ax) == .cgPoint { read = AXValueGetValue(ax, .cgPoint, &position) }
    }
    emit(["ok": read, "operation": "inspect", "pid": pid, "window_id": id, "title": title,
          "bounds": [before.minX, before.minY, before.width, before.height],
          "ax_position": read ? [position.x, position.y] as Any : NSNull(),
          "ax_position_result": code.rawValue], status: read ? 0 : 1)
}
guard let x = Double(args[4]), let y = Double(args[5]), x.isFinite, y.isFinite else {
    reject("invalid_move_coordinates")
}
private var point = CGPoint(x: x, y: y)
guard let position = AXValueCreate(.cgPoint, &point) else { reject("position_encoding_failed") }
private let result = AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, position)
private var after = windowBounds(id, pid: pid)
private let until = ProcessInfo.processInfo.systemUptime + 1
while result == .success, let bounds = after,
      (abs(bounds.minX - x) >= 1 || abs(bounds.minY - y) >= 1),
      ProcessInfo.processInfo.systemUptime < until {
    Thread.sleep(forTimeInterval: 0.02)
    after = windowBounds(id, pid: pid)
}
emit(["ok": result == .success, "operation": "move", "pid": pid, "window_id": id, "title": title,
      "requested_position": ["x": x, "y": y], "before_bounds": rectJSON(before),
      "bounds": after.map { [$0.minX, $0.minY, $0.width, $0.height] as Any } ?? NSNull(), "ax_position_result": result.rawValue,
      "position_observed": after.map { abs($0.minX - x) < 1 && abs($0.minY - y) < 1 } ?? false],
     status: result == .success ? 0 : 1)
