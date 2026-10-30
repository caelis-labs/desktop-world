// Independent, read-only observer. It inventories and captures only caller-owned
// helper window IDs after kernel birth + WindowServer owner checks.
import AppKit
import CoreGraphics
import Darwin
import Foundation
import ImageIO
import CryptoKit
import ScreenCaptureKit

func birth(_ pid: Int32) -> UInt64? {
    var info = proc_bsdinfo()
    let size = Int32(MemoryLayout<proc_bsdinfo>.size)
    guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size) == size,
          info.pbi_pid == UInt32(pid), info.pbi_start_tvusec < 1_000_000 else { return nil }
    let (seconds, a) = info.pbi_start_tvsec.multipliedReportingOverflow(by: 1_000_000)
    let (value, b) = seconds.addingReportingOverflow(info.pbi_start_tvusec)
    return a || b || value == 0 ? nil : value
}
struct Identity: Decodable { let pid: Int32; let birth: UInt64; let windows: [UInt32] }
func snapshot(_ identity: Identity) -> [String: Any] {
    let before = birth(identity.pid)
    let rows = CGWindowListCopyWindowInfo(.optionAll, 0) as? [[String: Any]] ?? []
    let after = birth(identity.pid)
    let current = before == identity.birth && after == identity.birth
    let owned = rows.filter { row in
        current && (row[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == identity.pid &&
        identity.windows.contains((row[kCGWindowNumber as String] as? NSNumber)?.uint32Value ?? 0)
    }.map { row -> [String: Any] in
        let bounds = row[kCGWindowBounds as String] as? [String: Any] ?? [:]
        return ["id": row[kCGWindowNumber as String] ?? 0,
                "on_screen": row[kCGWindowIsOnscreen as String] as? Bool ?? false,
                "bounds": ["x": bounds["X"] ?? 0, "y": bounds["Y"] ?? 0,
                           "width": bounds["Width"] ?? 0, "height": bounds["Height"] ?? 0]]
    }
    return ["identity_matches": current, "windows": owned,
            "uptime": ProcessInfo.processInfo.systemUptime]
}
func coloredPixels(_ image: CGImage) -> [String: Any] {
    let width = image.width, height = image.height
    var bytes = [UInt8](repeating: 0, count: width * height * 4)
    guard let context = CGContext(data: &bytes, width: width, height: height, bitsPerComponent: 8,
                                  bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
        return ["colored_pixels": 0, "opaque_pixels": 0]
    }
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    var colored = 0, opaque = 0
    var histogram: [String: Int] = [:]
    for offset in stride(from: 0, to: bytes.count, by: 4) {
        if bytes[offset+3] < 128 { continue }
        opaque += 1
        let rgb = [Int(bytes[offset]), Int(bytes[offset+1]), Int(bytes[offset+2])]
        if rgb.max()! - rgb.min()! >= 40 {
            colored += 1
            let key = rgb.map { String($0 / 16) }.joined(separator: ",")
            histogram[key, default: 0] += 1
        }
    }
    return ["width": width, "height": height, "colored_pixels": colored, "opaque_pixels": opaque,
            "dominant_color_bin": histogram.max { $0.value < $1.value }?.key ?? ""]
}
// PNG comparison is offline and independent of DTW's capture implementation.
// CGImage cropping uses image pixel coordinates; only the static owned pad ROI
// is decoded to canonical premultiplied RGBA, never title bars or changing text.
func rgba(_ image: CGImage) throws -> [UInt8] {
    var bytes = [UInt8](repeating: 0, count: image.width * image.height * 4)
    let ok = bytes.withUnsafeMutableBytes { storage -> Bool in
        guard let context = CGContext(data: storage.baseAddress, width: image.width, height: image.height,
                                      bitsPerComponent: 8, bytesPerRow: image.width * 4,
                                      space: CGColorSpaceCreateDeviceRGB(),
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue) else { return false }
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        return true
    }
    guard ok else { throw NSError(domain: "rgba_decode_failed", code: 1) }
    return bytes
}
func compareROI(_ before: CGImage, _ after: CGImage, _ roi: CGRect) throws -> [String: Any] {
    guard before.width == after.width, before.height == after.height,
          roi.width > 0, roi.height > 0, roi == roi.integral,
          CGRect(x: 0, y: 0, width: before.width, height: before.height).contains(roi),
          let first = before.cropping(to: roi), let second = after.cropping(to: roi) else {
        throw NSError(domain: "roi_outside_equal_sized_images", code: 1)
    }
    let a = try rgba(first), b = try rgba(second)
    guard a.count == b.count else { throw NSError(domain: "roi_size_changed", code: 1) }
    var differentPixels = 0, differentChannels = 0, maximumDelta = 0, opaquePixels = 0
    for offset in stride(from: 0, to: a.count, by: 4) {
        var different = false
        if a[offset+3] >= 128 { opaquePixels += 1 }
        for channel in 0..<4 {
            let delta = abs(Int(a[offset+channel]) - Int(b[offset+channel]))
            if delta > 0 { different = true; differentChannels += 1 }
            maximumDelta = max(maximumDelta, delta)
        }
        if different { differentPixels += 1 }
    }
    return ["roi": [Int(roi.minX), Int(roi.minY), Int(roi.width), Int(roi.height)],
            "pixels": a.count / 4, "different_pixels": differentPixels, "different_channels": differentChannels,
            "maximum_channel_delta": maximumDelta, "baseline_opaque_pixels": opaquePixels,
            "before_rgba_sha256": SHA256.hash(data: Data(a)).map { String(format: "%02x", $0) }.joined(),
            "after_rgba_sha256": SHA256.hash(data: Data(b)).map { String(format: "%02x", $0) }.joined()]
}
func pngImage(_ path: String) throws -> CGImage {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
        throw NSError(domain: "png_decode_failed", code: 1)
    }
    return image
}
func pixelControls() throws -> [String: Any] {
    func image(changed: Int?) -> CGImage {
        var bytes = [UInt8](repeating: 255, count: 8 * 8 * 4)
        if let changed { bytes[changed * 4] = 0 }
        let provider = CGDataProvider(data: Data(bytes) as CFData)!
        return CGImage(width: 8, height: 8, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: 32,
                       space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)!
    }
    let first = image(changed: nil), roi = CGRect(x: 2, y: 2, width: 3, height: 3)
    let equal = try compareROI(first, first, roi)
    let inside = try compareROI(first, image(changed: 3 * 8 + 3), roi)
    let outside = try compareROI(first, image(changed: 7 * 8 + 7), roi)
    guard equal["different_pixels"] as? Int == 0, inside["different_pixels"] as? Int == 1,
          outside["different_pixels"] as? Int == 0 else { throw NSError(domain: "roi_control_failed", code: 1) }
    var rejected = false
    do { _ = try compareROI(first, first, CGRect(x: 7, y: 7, width: 3, height: 3)) } catch { rejected = true }
    guard rejected else { throw NSError(domain: "outside_roi_accepted", code: 1) }
    return ["pixel_controls_passed": true, "inside_change_pixels": inside["different_pixels"]!,
            "outside_change_pixels": outside["different_pixels"]!]
}

@main struct Observer {
    static func main() async {
        do {
            if CommandLine.arguments.count == 2 && CommandLine.arguments[1] == "--pixel-controls" {
                let data = try JSONSerialization.data(withJSONObject: pixelControls(), options: [.sortedKeys])
                FileHandle.standardOutput.write(data + Data("\n".utf8)); return
            }
            if CommandLine.arguments.count == 8 && CommandLine.arguments[1] == "--compare-roi" {
                let numbers = try CommandLine.arguments[4...7].map { text -> Int in
                    guard let value = Int(text) else { throw NSError(domain: "invalid_roi", code: 1) }; return value
                }
                let roi = CGRect(x: numbers[0], y: numbers[1], width: numbers[2], height: numbers[3])
                let result = try compareROI(pngImage(CommandLine.arguments[2]), pngImage(CommandLine.arguments[3]), roi)
                let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
                FileHandle.standardOutput.write(data + Data("\n".utf8)); return
            }
            guard CommandLine.arguments.count >= 2 else { throw NSError(domain: "missing_identity", code: 1) }
            let identity = try JSONDecoder().decode(Identity.self, from: Data(CommandLine.arguments[1].utf8))
            var result = snapshot(identity)
            if CommandLine.arguments.count > 2 {
                let path = CommandLine.arguments[2]
                var capture: [String: Any] = ["status": "unavailable"]
                do {
                    guard result["identity_matches"] as? Bool == true, identity.windows.count == 1,
                          let before = (result["windows"] as? [[String: Any]])?.first,
                          before["on_screen"] as? Bool == true else { throw NSError(domain: "owned_marker_not_visible", code: 1) }
                    guard CGPreflightScreenCaptureAccess() else { throw NSError(domain: "screen_capture_permission_unavailable", code: 1) }
                    let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
                    guard let window = content.windows.first(where: { $0.windowID == identity.windows[0] && $0.owningApplication?.processID == identity.pid }) else {
                        throw NSError(domain: "owned_helper_missing_from_capture_inventory", code: 1)
                    }
                    let filter = SCContentFilter(desktopIndependentWindow: window)
                    let config = SCStreamConfiguration()
                    config.width = 176; config.height = 128; config.showsCursor = false
                    config.ignoreShadowsSingleWindow = true
                    let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
                    let after = snapshot(identity)
                    guard after["identity_matches"] as? Bool == true,
                          let afterWindow = (after["windows"] as? [[String: Any]])?.first,
                          afterWindow["on_screen"] as? Bool == true,
                          NSDictionary(dictionary: before["bounds"] as? [String: Any] ?? [:]) ==
                          NSDictionary(dictionary: afterWindow["bounds"] as? [String: Any] ?? [:]) else {
                        throw NSError(domain: "marker_changed_during_capture", code: 1)
                    }
                    guard let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
                        throw NSError(domain: "png_encode_failed", code: 1)
                    }
                    try png.write(to: URL(fileURLWithPath: path))
                    capture = coloredPixels(image)
                    capture["status"] = "captured"; capture["path"] = path
                    capture["exact_owned_window"] = identity.windows[0]
                    result["after_capture"] = after
                } catch { capture["reason"] = String(describing: error) }
                result["capture"] = capture
            }
            let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
            FileHandle.standardOutput.write(data + Data("\n".utf8))
        } catch {
            let data = try! JSONSerialization.data(withJSONObject: ["error": String(describing: error)])
            FileHandle.standardOutput.write(data + Data("\n".utf8)); exit(1)
        }
    }
}
