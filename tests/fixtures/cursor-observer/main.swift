// Independent, read-only system cursor oracle. No AppKit activation or input.
import CoreGraphics
import Foundation

let path = CommandLine.arguments[1]
FileManager.default.createFile(atPath: path, contents: nil)
let file = try FileHandle(forWritingTo: URL(fileURLWithPath: path))
var stopped = false
let lock = NSLock()
DispatchQueue.global().async {
    _ = readLine()
    lock.lock()
    stopped = true
    lock.unlock()
}
print("ready")
fflush(stdout)
repeat {
    let time = Date().timeIntervalSince1970 * 1000
    let row: [String: Any]
    if let point = CGEvent(source: nil)?.location {
        row = ["unix_ms": time, "x": point.x, "y": point.y]
    } else {
        row = ["unix_ms": time, "error": "cursor_read_unavailable"]
    }
    let data = try JSONSerialization.data(withJSONObject: row, options: [.sortedKeys])
    file.write(data)
    file.write(Data([10]))
    Thread.sleep(forTimeInterval: 0.002)
    lock.lock()
    let done = stopped
    lock.unlock()
    if done { break }
} while true
try file.close()
