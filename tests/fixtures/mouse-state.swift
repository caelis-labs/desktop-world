// Read-only observer. This fixture never creates or posts an input event.
import Foundation
import CoreGraphics

func emit(_ event: String, _ down: Bool) {
    let value: [String: Any] = ["event": event, "down": down,
        "monotonic_ns": DispatchTime.now().uptimeNanoseconds,
        "source": "combinedSessionState", "button": "left"]
    let data = try! JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    FileHandle.standardOutput.write(data + Data([10]))
}

guard CommandLine.arguments.count == 2,
      let milliseconds = UInt64(CommandLine.arguments[1]),
      (100...15_000).contains(milliseconds) else {
    FileHandle.standardError.write(Data("usage: mouse-state DURATION_MS (100..15000)\n".utf8))
    exit(64)
}
let deadline = DispatchTime.now().uptimeNanoseconds + milliseconds * 1_000_000
var previous = CGEventSource.buttonState(.combinedSessionState, button: .left)
emit("ready", previous)
var transitions = 0
while DispatchTime.now().uptimeNanoseconds < deadline {
    let current = CGEventSource.buttonState(.combinedSessionState, button: .left)
    if current != previous {
        emit(current ? "down" : "up", current)
        previous = current
        transitions += 1
        if transitions >= 32 { emit("transition_limit", current); exit(2) }
    }
    Thread.sleep(forTimeInterval: 0.001)
}
emit("end", CGEventSource.buttonState(.combinedSessionState, button: .left))
