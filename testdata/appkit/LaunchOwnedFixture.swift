// One-shot Launch Services launcher for an owned POC AppKit bundle. It does not
// activate the application; the fixture must also use its --background mode.
import AppKit

guard CommandLine.arguments.count >= 2 else {
  fputs("usage: LaunchOwnedFixture OWNED_APP_PATH [--reuse-registration] [APP_ARGS...]\n", stderr)
  exit(2)
}
let url = URL(fileURLWithPath: CommandLine.arguments[1])
let reuseRegistration = CommandLine.arguments.dropFirst(2).first == "--reuse-registration"
let configuration = NSWorkspace.OpenConfiguration()
configuration.activates = false
configuration.createsNewApplicationInstance = !reuseRegistration
configuration.addsToRecentItems = false
configuration.arguments = Array(CommandLine.arguments.dropFirst(reuseRegistration ? 3 : 2))
var finished = false
var launchedPID: pid_t = 0
var launchError: Error?
NSWorkspace.shared.openApplication(at: url, configuration: configuration) { app, error in
  launchedPID = app?.processIdentifier ?? 0
  launchError = error
  finished = true
}
let deadline = Date().addingTimeInterval(8)
while !finished && Date() < deadline { RunLoop.current.run(until: Date().addingTimeInterval(0.05)) }
if let launchError { fputs("launch failed: \(launchError)\n", stderr); exit(1) }
if !finished || launchedPID <= 0 { fputs("launch did not return a PID\n", stderr); exit(1) }
print(launchedPID)
