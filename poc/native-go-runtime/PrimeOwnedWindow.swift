// A single bounded activation diagnostic for an owned test App only. It waits
// for a quiet physical input interval, never injects input, and restores the
// original frontmost application. It prints PID/timing metadata only.
import AppKit
import ApplicationServices

guard CommandLine.arguments.count == 3,
      let pid = pid_t(CommandLine.arguments[1]) else {
  fputs("usage: PrimeOwnedWindow OWNED_PID EXPECTED_BUNDLE_PATH\n", stderr)
  exit(2)
}
let expected = URL(fileURLWithPath: CommandLine.arguments[2]).standardizedFileURL
guard let target = NSWorkspace.shared.runningApplications.first(where: { $0.processIdentifier == pid }),
      target.bundleURL?.standardizedFileURL == expected else {
  fputs("target PID does not match the owned bundle\n", stderr)
  exit(2)
}
func inputAge() -> TimeInterval {
  let types: [CGEventType] = [.keyDown, .leftMouseDown, .rightMouseDown,
    .otherMouseDown, .mouseMoved, .scrollWheel]
  return types.map { CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: $0) }
    .filter { $0 >= 0 }.min() ?? 1e9
}
let quietDeadline = Date().addingTimeInterval(3)
while inputAge() < 0.6 && Date() < quietDeadline {
  RunLoop.current.run(until: Date().addingTimeInterval(0.02))
}
guard inputAge() >= 0.6 else {
  print("{\"result\":\"user_active\"}")
  exit(0)
}
guard let prior = NSWorkspace.shared.frontmostApplication,
      prior.processIdentifier != pid else {
  fputs("frontmost app unavailable or already target\n", stderr)
  exit(2)
}
let started = ProcessInfo.processInfo.systemUptime
let requested = target.activate(options: [.activateAllWindows])
RunLoop.current.run(until: Date().addingTimeInterval(0.15))
let seenTarget = NSWorkspace.shared.frontmostApplication?.processIdentifier == pid
let interrupted = inputAge() < 0.15
let restoredRequest = prior.activate(options: [])
let restoreDeadline = Date().addingTimeInterval(1)
while NSWorkspace.shared.frontmostApplication?.processIdentifier != prior.processIdentifier &&
      Date() < restoreDeadline {
  RunLoop.current.run(until: Date().addingTimeInterval(0.02))
}
let restored = NSWorkspace.shared.frontmostApplication?.processIdentifier == prior.processIdentifier
let ms = Int((ProcessInfo.processInfo.systemUptime - started) * 1000)
let result: [String: Any] = ["result":"completed", "target_pid":pid,
  "prior_pid":prior.processIdentifier, "activate_requested":requested,
  "target_seen_front":seenTarget, "input_during_interval":interrupted,
  "restore_requested":restoredRequest, "restored":restored, "total_ms":ms]
let data = try JSONSerialization.data(withJSONObject: result, options:[.sortedKeys])
print(String(decoding:data, as:UTF8.self))
if !restored { exit(1) }
