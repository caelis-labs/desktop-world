// Run by concatenating Input.swift and this file, then swiftc the resulting main.swift.
// Pure event planning only; no event source, GUI connection, input or focus.
private var inputContractAssertions = 0
func check(_ value: @autoclosure () -> Bool, _ message: String) {
    if !value() { fatalError(message) }
    inputContractAssertions += 1
}
let chord = dtwChordPlan(["META", "SHIFT", "A"])!
check(chord.count == 3, "all chord members retained")
check(chord[0].downFlags == .maskCommand && chord[0].upFlags.isEmpty, "command release clears only own modifier")
check(chord[1].downFlags == [.maskCommand, .maskShift] && chord[1].upFlags == .maskCommand, "reverse modifier release preserves earlier modifier")
check(chord[2].code == 0, "A key maps to native keycode")
check(dtwChordPlan(["CTRL", "RETURN"]) != nil, "named keys and aliases")
check(dtwChordPlan(["Cmd", "Meta", "A"]) == nil, "duplicate aliased modifier refused")
check(dtwChordPlan(["A", "B"]) == nil && dtwChordPlan(["A", "CMD"]) == nil, "one primary key last")
check(dtwChordPlan(["Unrecognized"]) == nil && dtwChordPlan([]) == nil, "unknown and empty chords refused")
check(dtwUnicodeUnits("中") == [0x4E2D], "Chinese UTF16 preserved")
check(dtwUnicodeUnits("😀") == [0xD83D, 0xDE00], "surrogate pair preserved")
check(dtwTextScalars("a\r\nb\rc\n") == Array("a\nb\rc\n".unicodeScalars), "CRLF is one Return and lone CR/LF are preserved")
check(dtwTextScalars("\r\n\r\n").count == 2, "consecutive CRLF retain exactly two Return pairs")
check(dtwWheelSteps(NSNumber(value:100)) == 100 && dtwWheelSteps(NSNumber(value:-100)) == -100, "signed wheel-step limits")
check(dtwWheelSteps(NSNumber(value:101)) == nil && dtwWheelSteps(NSNumber(value:-101)) == nil, "wheel steps outside public bounds refused")
check(dtwWheelSteps(NSNumber(value:1.5)) == nil && dtwWheelSteps(NSNumber(value:true)) == nil, "fractional and boolean steps refused")
print("Native keyboard contracts passed (no input posted)")

// A blocked read is not held input. Closing while preflight is outstanding
// drains immediately, and its eventual completion cannot admit an action.
private final class PreflightControl: @unchecked Sendable {
    let began = DispatchSemaphore(value: 0)
    let resume = DispatchSemaphore(value: 0)
    let finished = DispatchSemaphore(value: 0)
    let gate = NativeInputGate()
    private let lock = NSLock()
    private var sent = false
    func effect() { lock.lock(); sent = true; lock.unlock() }
    func effectWasSent() -> Bool { lock.lock(); defer { lock.unlock() }; return sent }
}
private let preflight = PreflightControl()
DispatchQueue.global().async {
    preflight.began.signal()
    precondition(preflight.resume.wait(timeout: .now() + 2) == .success)
    let admitted = preflight.gate.withAdmission { preflight.effect(); return true }
    precondition(admitted == nil)
    preflight.finished.signal()
}
check(preflight.began.wait(timeout: .now() + 2) == .success, "preflight is blocked")
check(preflight.gate.quiesce(milliseconds: 0), "blocked read has no held input to drain")
preflight.resume.signal()
check(preflight.finished.wait(timeout: .now() + 2) == .success, "read completion returns after shutdown")
check(!preflight.effectWasSent(), "no late side effect after admission closes")
print("Read-only preflight does not block input drain")

private var routingClock: TimeInterval = 0
private var routingPauses = 0
check(dtwSettleInput(seconds:0.04,deadline:1,now:{routingClock},stopping:{false},pause:{duration in
    check(duration <= 0.01, "routing polls at most 10 ms apart")
    routingClock += duration; routingPauses += 1
}), "pointer routing wait completes without input admission")
check(routingClock == 0.04 && routingPauses == 4, "bounded routing interval")
routingPauses = 0
check(!dtwSettleInput(seconds:0.04,deadline:0.01,now:{0},stopping:{false},pause:{_ in routingPauses += 1}), "no wait beyond call deadline")
check(routingPauses == 0, "insufficient budget fails before waiting")
private var routingClosed = false
check(!dtwSettleInput(seconds:0.04,deadline:1,now:{0},stopping:{routingClosed},pause:{_ in routingClosed = true}), "shutdown interrupts pointer routing")
print("Pointer routing settle is bounded and cancellable")

// The production tracker is exercised with independent WindowServer progress;
// these controls create no CGEventSource and never post an event.
private final class PendingModel: @unchecked Sendable {
    let lock = NSLock()
    var counts: [UInt32: UInt32] = [:]
    var heldKeys: Set<UInt16> = []
    var heldButtons: Set<UInt32> = []
    var heldFlags: CGEventFlags = []
    func count(_ type: UInt32) -> UInt32 { lock.lock(); defer { lock.unlock() }; return counts[type, default: 0] }
    func see(_ type: UInt32, _ count: UInt32) { lock.lock(); counts[type] = count; lock.unlock() }
    func hold(key: UInt16, _ held: Bool) { lock.lock(); if held { heldKeys.insert(key) } else { heldKeys.remove(key) }; lock.unlock() }
    func hold(button: UInt32, _ held: Bool) { lock.lock(); if held { heldButtons.insert(button) } else { heldButtons.remove(button) }; lock.unlock() }
    func flags(_ value: CGEventFlags) { lock.lock(); heldFlags = value; lock.unlock() }
    func released(_ keys: Set<UInt16>, _ buttons: Set<UInt32>) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return dtwInputStateReleased(keys: keys, buttons: buttons,
                                     keyState: heldKeys.contains, buttonState: heldButtons.contains,
                                     flags: { heldFlags })
    }
    func input(owner: AnyObject = NSObject()) -> NativePendingInput {
        NativePendingInput(owner: owner, eventTypes: [1,2,3,4], counters: [count], released: released,
                           probe: { $0() })
    }
}

private func dualInput(_ source: PendingModel, _ session: PendingModel) -> NativePendingInput {
    NativePendingInput(owner: source, eventTypes: [1,2,3,4], counters: [source.count, session.count],
                       released: { source.released($0, $1) && session.released($0, $1) },
                       probe: { $0() })
}

check(dtwKeyProgressType(code: 0, isUp: false) == nil && dtwKeyProgressType(code: 0, isUp: true) == .keyUp,
      "Unicode keys count actual up posts")
for code: CGKeyCode in [55,56,58,59] {
    check(dtwKeyProgressType(code: code, isUp: false) == .flagsChanged && dtwKeyProgressType(code: code, isUp: true) == .flagsChanged,
          "both modifier edges progress through flagsChanged")
}

do {
    let gate = NativeInputGate(), model = PendingModel()
    let input = model.input()
    check(gate.register(input), "register source before any input")
    input.posted(eventType: nil, key: 0)
    input.posted(eventType: 1, key: 0)
    check(!input.isReleased(), "empty key table cannot prove an undelivered up")
    var clock: TimeInterval = 0
    check(gate.finish(input, now: { clock }, pause: { interval in
        clock += interval
        if clock >= 0.03 { model.see(1, 1) }
    }), "normal completion waits for source-specific release progress")
    check(clock >= 0.03, "a queued down beyond ten milliseconds cannot cause premature handoff")
    check(!gate.isClosing && gate.withAdmission { true } == true, "normal drain does not close future input")
}

do {
    let gate = NativeInputGate(), model = PendingModel()
    let tracked = model.input()
    check(gate.register(tracked), "register chord source")
    tracked.posted(eventType: 2, key: 55)
    tracked.posted(eventType: nil, key: 0)
    tracked.posted(eventType: 1, key: 0)
    tracked.posted(eventType: 2, key: 55)
    model.see(1, 1); model.see(2, 1)
    check(!tracked.isReleased(), "ordinary key up cannot stand in for missing modifier release")
    model.see(2, 2); model.hold(key: 55, true)
    check(!tracked.isReleased(), "seen release counters alone do not prove the held key lifted")
    model.hold(key: 55, false)
    check(gate.finish(tracked), "chord needs both its release counters and its own released key states")
}

do {
    let gate = NativeInputGate(), model = PendingModel()
    var owner: NSObject? = NSObject()
    weak var witness: NSObject?
    witness = owner
    var input: NativePendingInput? = model.input(owner: owner!)
    input!.posted(eventType: 1, key: 0)
    check(gate.register(input!), "pending source is registered")
    owner = nil; input = nil
    check(witness != nil, "gate keeps the event source alive independently of the native call stack")
    model.see(1, 1)
    check(gate.quiesce(milliseconds: 100), "shutdown drains queued input with no active AX or input closure")
    check(witness == nil, "acknowledged source is released after shutdown drain")
    check(gate.withAdmission { true } == nil, "successful shutdown cannot reopen input")
}

do {
    let gate = NativeInputGate(), model = PendingModel()
    let tracked = model.input()
    check(gate.register(tracked), "register cancellable source")
    let entered = DispatchSemaphore(value: 0), resume = DispatchSemaphore(value: 0), ended = DispatchSemaphore(value: 0)
    DispatchQueue.global().async {
        let _: Bool? = gate.withPair(press: { tracked.posted(eventType: nil, key: 0) },
                                    release: { tracked.posted(eventType: 1, key: 0) }) {
            entered.signal()
            precondition(resume.wait(timeout: .now() + 2) == .success)
            return true
        }
        ended.signal()
    }
    check(entered.wait(timeout: .now() + 2) == .success, "cancel control has posted a down")
    gate.closeAdmission()
    check(gate.isClosing, "closing admission is separate from verifying the admitted up post")
    resume.signal()
    check(ended.wait(timeout: .now() + 2) == .success, "admitted up returns once without replay")
    var clock: TimeInterval = 0
    check(gate.quiesce(milliseconds: 100, now: { clock }, pause: { interval in
        clock += interval
        if clock >= 0.025 { model.see(1, 1) }
    }), "cancel drain independently waits for the queued up to reach WindowServer")
    check(clock >= 0.025 && gate.withAdmission { true } == nil, "cancel waits for progress and keeps admission closed")
}

do {
    let gate = NativeInputGate(), model = PendingModel()
    let input = model.input()
    check(gate.register(input), "register stalled source")
    input.posted(eventType: 1, key: 0)
    var clock: TimeInterval = 0
    check(!gate.finish(input, timeout: 0.02, now: { clock }, pause: { clock += $0 }), "missing release acknowledgement has a hard cleanup budget")
    check(clock == 0.02 && gate.isClosing, "timeout closes admission")
    check(!gate.register(model.input()) && gate.withAdmission { true } == nil, "cleanup failure cannot admit a new source or input")
    model.see(1, 1)
    check(!gate.quiesce(milliseconds: 100), "later progress cannot erase the first cleanup failure")
}

do {
    let gate = NativeInputGate(), model = PendingModel()
    model.see(1, UInt32.max - 1)
    let input = model.input()
    check(gate.register(input), "register counter-wrap control")
    for _ in 0..<3 { input.posted(eventType: 1, key: 0) }
    model.see(1, 0)
    check(!input.isReleased(), "two wraparound releases cannot satisfy three posts")
    model.see(1, 1)
    check(gate.finish(input), "counter progress uses bounded wrapping subtraction")
}

do {
    let gate = NativeInputGate(), model = PendingModel()
    let mouse = model.input()
    check(gate.register(mouse), "register private mouse source")
    mouse.posted(eventType: nil, button: 0)
    mouse.posted(eventType: 3, button: 0)
    check(!mouse.isReleased(), "empty button table cannot prove queued mouse-up delivery")
    model.see(3, 1); model.hold(button: 0, true)
    check(!mouse.isReleased(), "mouse-up counter requires released button state too")
    model.hold(button: 0, false)
    check(gate.finish(mouse), "private mouse release can be acknowledged")
    let wheel = model.input()
    check(gate.register(wheel), "register private wheel source")
    wheel.posted(eventType: 4)
    check(!wheel.isReleased(), "wheel without held keys still needs seen-event progress")
    model.see(4, 1)
    check(gate.finish(wheel), "wheel source lives until its one posted event is seen")
}

// Multiple shutdown callers share the first failed verification, including
// when the originally admitted pair eventually releases after that timeout.
do {
    let gate = NativeInputGate(), events = InputEvents()
    let began = DispatchSemaphore(value: 0), resume = DispatchSemaphore(value: 0), ended = DispatchSemaphore(value: 0)
    DispatchQueue.global().async {
        let _: Bool? = gate.withPair(press: { events.add("down") }, release: { events.add("up") }) {
            began.signal()
            precondition(resume.wait(timeout: .now() + 2) == .success)
            return true
        }
        ended.signal()
    }
    check(began.wait(timeout: .now() + 2) == .success, "held input is active before shutdown")
    let laterStarted = DispatchSemaphore(value: 0), laterFinished = DispatchSemaphore(value: 0)
    let laterResult = InputEvents()
    DispatchQueue.global().async {
        laterStarted.signal()
        laterResult.add(gate.quiesce(milliseconds: 500) ? "clean" : "failed")
        laterFinished.signal()
    }
    check(laterStarted.wait(timeout: .now() + 2) == .success, "another shutdown caller is active")
    check(!gate.quiesce(milliseconds: 10), "active input exceeding the shutdown budget fails verification")
    check(gate.isClosing && gate.withAdmission { true } == nil, "active timeout keeps input closed")
    resume.signal()
    check(ended.wait(timeout: .now() + 2) == .success, "late pair still posts its original up once")
    check(laterFinished.wait(timeout: .now() + 2) == .success, "the other shutdown caller finishes after release")
    check(laterResult.all() == ["failed"], "a concurrent shutdown cannot turn the earlier timeout into clean exit")
    check(events.all() == ["down", "up"], "late release is paired and never replayed")
    check(!gate.quiesce(milliseconds: 100), "later successful active drain cannot erase the first shutdown timeout")
    check(!gate.quiesce(milliseconds: 0), "repeated completed shutdown still exposes its original failure")
}

// Simulate a blocking native getter on the probe thread, not an active input
// interval. Shutdown must still return within its bound and remain failed.
do {
    let gate = NativeInputGate(), model = PendingModel()
    let began = DispatchSemaphore(value: 0), resume = DispatchSemaphore(value: 0), ended = DispatchSemaphore(value: 0)
    let input = NativePendingInput(owner: model, eventTypes: [1], counters: [model.count, { type in
        if model.count(2) == 1 {
            began.signal()
            precondition(resume.wait(timeout: .now() + 2) == .success)
            ended.signal()
        }
        return model.count(type)
    }], released: model.released)
    check(gate.register(input), "register native-getter control")
    input.posted(eventType: 1, key: 0); model.see(1, 1); model.see(2, 1)
    check(!input.isReleased(), "getters run asynchronously")
    check(began.wait(timeout: .now() + 2) == .success, "native getter is blocked")
    let before = ProcessInfo.processInfo.systemUptime
    check(!gate.quiesce(milliseconds: 20), "blocked CoreGraphics getter cannot block shutdown")
    check(ProcessInfo.processInfo.systemUptime - before < 0.3, "shutdown does not join an unresponsive getter")
    model.see(1, 1); model.see(2, 0); resume.signal()
    check(ended.wait(timeout: .now() + 2) == .success, "controlled getter exits")
    check(!gate.quiesce(milliseconds: 100), "late getter completion cannot clear sticky shutdown failure")
}

// Private-source progress can lead session progress. Both counters have their
// own baseline; private acknowledgement plus an initially empty session table
// must not release input ownership before the session has seen those events.
do {
    let gate = NativeInputGate(), source = PendingModel(), session = PendingModel()
    source.see(1, 10); source.see(2, 20)
    session.see(1, 100); session.see(2, 1000)
    let input = dualInput(source, session)
    check(gate.register(input), "register dual-stage keyboard acknowledgement")
    input.posted(eventType: 2, key: 55)
    input.posted(eventType: nil, key: 0)
    input.posted(eventType: 1, key: 0)
    input.posted(eventType: 2, key: 55)
    source.see(1, 11); source.see(2, 22)
    check(!input.isReleased(), "private acknowledgement and initially empty combined state cannot finish")
    session.see(1, 101); session.see(2, 1001)
    check(!input.isReleased(), "combined ordinary up cannot replace the pending modifier release")
    session.see(2, 1002); session.hold(key: 55, true)
    check(!input.isReleased(), "both counter floors still require combined key release")
    session.hold(key: 55, false); session.flags(.maskCommand)
    check(!input.isReleased(), "combined key-state up still requires the touched modifier flag to clear")
    session.flags([]); source.hold(key: 0, true)
    check(!input.isReleased(), "private key state remains a required condition")
    source.hold(key: 0, false); source.flags(.maskCommand)
    check(!input.isReleased(), "private touched modifier flags remain a required condition")
    source.flags([]); session.flags(.maskAlternate)
    session.hold(key: 12, true); session.hold(button: 1, true)
    check(gate.finish(input), "unrelated physical key, button and modifier state does not become owned input")
    check(!gate.isClosing, "normal dual-stage acknowledgement keeps admission available")
}

do {
    let gate = NativeInputGate(), source = PendingModel(), session = PendingModel()
    let input = dualInput(source, session)
    check(gate.register(input), "register session-lag cancellation control")
    input.posted(eventType: 1, key: 0); source.see(1, 1)
    var clock: TimeInterval = 0
    check(gate.quiesce(milliseconds: 100, now: { clock }, pause: { interval in
        clock += interval
        if clock >= 0.035 { session.see(1, 1) }
    }), "shutdown waits for independent combined release progress")
    check(clock >= 0.035 && gate.isClosing, "shutdown cannot mistake initial empty combined state for completion")
}

do {
    let gate = NativeInputGate(), source = PendingModel(), session = PendingModel()
    let input = dualInput(source, session)
    check(gate.register(input), "register missing combined acknowledgement")
    input.posted(eventType: 1, key: 0); source.see(1, 1)
    var clock: TimeInterval = 0
    check(!gate.finish(input, timeout: 0.02, now: { clock }, pause: { clock += $0 }),
          "private success cannot conceal a combined acknowledgement timeout")
    session.see(1, 1)
    check(!gate.quiesce(milliseconds: 100), "late combined acknowledgement cannot erase cleanup failure")
    check(gate.withAdmission { true } == nil, "dual-stage cleanup timeout permanently closes input")
}

do {
    let gate = NativeInputGate(), source = PendingModel(), session = PendingModel()
    let mouse = dualInput(source, session)
    check(gate.register(mouse), "register dual-stage mouse acknowledgement")
    mouse.posted(eventType: nil, button: 0); mouse.posted(eventType: 3, button: 0)
    source.see(3, 1)
    check(!mouse.isReleased(), "private mouse-up and initial combined empty state cannot finish")
    session.see(3, 1); session.hold(button: 0, true)
    check(!mouse.isReleased(), "combined mouse-up counter still requires combined button release")
    session.hold(button: 0, false); source.hold(button: 0, true)
    check(!mouse.isReleased(), "private mouse button release remains required")
    source.hold(button: 0, false)
    check(gate.finish(mouse), "mouse ownership ends only after both stages acknowledge release")
    let wheel = dualInput(source, session)
    check(gate.register(wheel), "register dual-stage wheel acknowledgement")
    wheel.posted(eventType: 4); source.see(4, 1)
    check(!wheel.isReleased(), "private wheel progress does not substitute for combined delivery")
    session.see(4, 1)
    check(gate.finish(wheel), "wheel ownership ends after both event counters advance")
}
print("Native input contracts passed: \(inputContractAssertions) assertions; dual-stage progress, lifetime, cancellation, mouse and bounded shutdown; no input posted")
