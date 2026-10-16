// Read-only macOS reproducer: launch two live copies of this exact executable.
// Usage: sck-client WINDOW_ID [hold]. Prints dimensions, never image content.
import Foundation
import ScreenCaptureKit
import CoreGraphics

_ = CGMainDisplayID()
guard CommandLine.arguments.count >= 2,
      let id = UInt32(CommandLine.arguments[1]) else { exit(2) }
let done = DispatchSemaphore(value: 0)
SCShareableContent.getExcludingDesktopWindows(true, onScreenWindowsOnly: true) { content, error in
    guard let window = content?.windows.first(where: { $0.windowID == id }) else {
        print("window_unavailable"); done.signal(); return
    }
    print("shareable_callback")
    let filter = SCContentFilter(desktopIndependentWindow: window)
    let config = SCStreamConfiguration()
    config.width = 600; config.height = 400; config.showsCursor = false
    SCScreenshotManager.captureImage(contentFilter: filter, configuration: config) { image, error in
        print(image.map { "image_callback \($0.width)x\($0.height)" } ?? "capture_failed")
        done.signal()
    }
}
if done.wait(timeout: .now() + 12) != .success { print("capture_timeout") }
fflush(stdout)
if CommandLine.arguments.dropFirst(2).contains("hold") { sleep(60) }
