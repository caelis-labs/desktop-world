// Pure input coordination controls: no AX, CGEvent, GUI or desktop permissions.
enum ControlError: Error { case expected }
final class InputEvents: @unchecked Sendable {
    private let lock = NSLock()
    private var entries: [String] = []
    func add(_ event: String) { lock.lock(); entries.append(event); lock.unlock() }
    func all() -> [String] { lock.lock(); defer { lock.unlock() }; return entries }
}
var assertions = 0
func expect(_ condition: @autoclosure () -> Bool, _ message: String) {
    precondition(condition(), message)
    assertions += 1
}

do {
    let gate = NativeInputGate()
    let events = InputEvents()
    let entered = DispatchSemaphore(value: 0)
    let release = DispatchSemaphore(value: 0)
    let finished = DispatchSemaphore(value: 0)
    DispatchQueue.global().async {
        let accepted = gate.withPair(press: { events.add("down") }, release: { events.add("up") }) {
            entered.signal()
            precondition(release.wait(timeout: .now() + 2) == .success)
            return true
        }
        precondition(accepted == true)
        finished.signal()
    }
    expect(entered.wait(timeout: .now() + 2) == .success, "pair entered")
    expect(events.all() == ["down"], "barrier is after down and before up")
    gate.closeAdmission()
    expect(gate.isClosing, "closing admission makes no claim that active input drained")
    let late = gate.withPair(press: { events.add("extra down") }, release: { events.add("extra up") }) { true }
    expect(late == nil, "closed gate refuses the entire new pair")
    expect(gate.withAdmission { events.add("late AX"); return true } == nil, "AX and pointer input share the closed admission")
    release.signal()
    expect(finished.wait(timeout: .now() + 2) == .success, "admitted release completed")
    expect(gate.quiesce(milliseconds: 100), "drain succeeds after release")
    expect(events.all() == ["down", "up"], "exactly one release and no replay")
    expect(gate.withAdmission { true } == nil, "quiesce is terminal")
}

do {
    let gate = NativeInputGate()
    let events = InputEvents()
    do {
        let _: Bool? = try gate.withPair(press: { events.add("down") }, release: { events.add("up") }) {
            throw ControlError.expected
        }
        preconditionFailure("expected injected operation failure")
    } catch ControlError.expected {} catch { preconditionFailure("unexpected error") }
    expect(events.all() == ["down", "up"], "throwing operation still releases once")
    expect(gate.quiesce(milliseconds: 0), "throwing operation releases active accounting")
}

do {
    let gate = NativeInputGate()
    expect(gate.quiesce(milliseconds: 0), "idle shutdown completes")
    expect(gate.quiesce(milliseconds: 0), "repeated shutdown is idempotent")
    expect(gate.withAdmission { true } == nil, "idle shutdown cannot reopen admission")
}
print("Native input controls passed: \(assertions) assertions; no GUI calls")
