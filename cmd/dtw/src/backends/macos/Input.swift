import Foundation

// Retain the source until private and session event progress reach every
// posted release and both state tables report its touched keys/buttons up.
// An initially empty key table proves nothing
// while the events are still queued. Getter work is asynchronous: a stalled
// CoreGraphics read must not trap the parent-death shutdown thread.
final class NativePendingInput: @unchecked Sendable {
    private let owner: AnyObject
    private let counters: [(UInt32) -> UInt32]
    private let released: (Set<UInt16>, Set<UInt32>) -> Bool
    private let probe: (@escaping () -> Void) -> Void
    private let baselines: [[UInt32: UInt32]]
    private let lock = NSLock()
    private var expected: [UInt32: UInt32] = [:]
    private var keys: Set<UInt16> = []
    private var buttons: Set<UInt32> = []
    private var version: UInt64 = 0
    private var verified: UInt64?
    private var probing = false

    init(owner: AnyObject, eventTypes: [UInt32], counters: [(UInt32) -> UInt32],
         released: @escaping (Set<UInt16>, Set<UInt32>) -> Bool,
         probe: @escaping (@escaping () -> Void) -> Void = { work in
             DispatchQueue.global(qos: .userInitiated).async(execute: DispatchWorkItem(block: work))
         }) {
        precondition(!counters.isEmpty)
        self.owner = owner
        self.counters = counters
        self.released = released
        self.probe = probe
        self.baselines = counters.map { counter in
            Dictionary(uniqueKeysWithValues: eventTypes.map { ($0, counter($0)) })
        }
    }

    // Called inside admission immediately before the corresponding post.
    // Ordinary downs only add a held key; modifier downs also count as
    // flagsChanged, since WindowServer uses that type for both modifier edges.
    func posted(eventType: UInt32?, key: UInt16? = nil, button: UInt32? = nil) {
        lock.lock(); defer { lock.unlock() }
        if let eventType {
            precondition(baselines.allSatisfy { $0[eventType] != nil })
            expected[eventType, default: 0] &+= 1
        }
        if let key { keys.insert(key) }
        if let button { buttons.insert(button) }
        version &+= 1
        verified = nil
    }

    func isReleased() -> Bool {
        lock.lock()
        if version == 0 || verified == version { lock.unlock(); return true }
        if probing { lock.unlock(); return false }
        probing = true
        let observedVersion = version
        let counts = expected, heldKeys = keys, heldButtons = buttons
        lock.unlock()
        probe { [self] in
            let progress = counters.enumerated().allSatisfy { index, counter in
                counts.allSatisfy { type, count in
                    counter(type) &- baselines[index][type]! >= count
                }
            }
            let clear = progress && released(heldKeys, heldButtons)
            lock.lock()
            if clear && version == observedVersion { verified = observedVersion }
            probing = false
            lock.unlock()
        }
        lock.lock(); defer { lock.unlock() }
        return verified == version
    }
}

// Closing admission and draining already admitted input are separate steps.
// No AX, capture, or event-loop work is needed to close this gate. In particular,
// a stuck read cannot prevent the shutdown thread from refusing another input.
final class NativeInputGate: @unchecked Sendable {
    private let lock = NSLock()
    private let active = DispatchGroup()
    private var closing = false
    private var cleanupFailed = false
    private var pending: [ObjectIdentifier: NativePendingInput] = [:]

    func register(_ input: NativePendingInput) -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard !closing else { return false }
        pending[ObjectIdentifier(input)] = input
        return true
    }

    func withAdmission<T>(_ operation: () throws -> T) rethrows -> T? {
        lock.lock()
        guard !closing else { lock.unlock(); return nil }
        active.enter()
        lock.unlock()
        defer { active.leave() }
        return try operation()
    }

    // Keep the same paired-input path in production and deterministic tests.
    // The release runs once even when the work between the events throws.
    func withPair<T>(press: () -> Void, release: () -> Void,
                     between: () throws -> T) rethrows -> T? {
        try withAdmission {
            press()
            defer { release() }
            return try between()
        }
    }

    var isClosing: Bool { lock.lock(); defer { lock.unlock() }; return closing }

    // Closing admission alone makes no cleanup claim. quiesce additionally
    // verifies release within a budget and preserves any failed verification.
    func closeAdmission() {
        lock.lock(); closing = true; lock.unlock()
    }

    func finish(_ input: NativePendingInput, timeout: TimeInterval = 0.2,
                now: () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
                pause: (TimeInterval) -> Void = { Thread.sleep(forTimeInterval: $0) }) -> Bool {
        drain([input], until: now() + timeout, now: now, pause: pause)
    }

    private func drain(_ inputs: [NativePendingInput], until: TimeInterval,
                       now: () -> TimeInterval, pause: (TimeInterval) -> Void) -> Bool {
        while true {
            for input in inputs {
                if input.isReleased() {
                    lock.lock(); pending.removeValue(forKey: ObjectIdentifier(input)); lock.unlock()
                }
            }
            lock.lock()
            if cleanupFailed { lock.unlock(); return false }
            // Another normal/shutdown drainer may already have proven and
            // retired this source. Do not poison a successful drain using a
            // stale failed probe from before that acknowledgement.
            if !inputs.contains(where: { pending[ObjectIdentifier($0)] != nil }) {
                lock.unlock(); return true
            }
            let left = until - now()
            if left <= 0 {
                cleanupFailed = true; closing = true; lock.unlock()
                return false
            }
            lock.unlock()
            pause(min(0.005, left))
        }
    }

    func quiesce(milliseconds: UInt32,
                 now: () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
                 pause: (TimeInterval) -> Void = { Thread.sleep(forTimeInterval: $0) }) -> Bool {
        let until = now() + Double(milliseconds) / 1000
        closeAdmission()
        guard active.wait(timeout: .now() + .milliseconds(Int(milliseconds))) == .success else {
            lock.lock(); cleanupFailed = true; lock.unlock()
            return false
        }
        lock.lock(); let inputs = Array(pending.values); lock.unlock()
        return drain(inputs, until: until, now: now, pause: pause)
    }
}

private let nativeInputGate = NativeInputGate()

// nil means admission was refused before the closure ran. Once admitted, an
// AX call may already have effects; quiescing cannot cancel or replay it.
func dtwWithInputAdmission<T>(_ operation: () throws -> T) rethrows -> T? {
    try nativeInputGate.withAdmission(operation)
}

func dtwWithPairedInput(press: () -> Void, release: () -> Void,
                       between: () -> Void) -> Bool {
    nativeInputGate.withPair(press: press, release: release) {
        between()
        return true
    } ?? false
}

// Called by the worker's ordinary shutdown thread, never a signal handler.
// This is a bounded drain, not a claim that delivered input can be undone.
@_cdecl("dtw_input_quiesce")
public func dtwInputQuiesce(_ timeoutMilliseconds: UInt32) -> Bool {
    nativeInputGate.quiesce(milliseconds: min(timeoutMilliseconds, 400))
}

// Narrow keyboard donor: CUA 0.38.0 input/keyboard.rs (85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19).
// Only key names, Unicode CGEvents and reverse modifier release are carried over.
import ApplicationServices

struct NativeKeyPair {
    let down: CGEvent
    let up: CGEvent
}

func dtwTrackInput(source: CGEventSource, eventTypes: [CGEventType]) -> NativePendingInput? {
    let state = source.sourceStateID
    guard state != .combinedSessionState && state != .hidSystemState && state != .privateState else { return nil }
    // The private table can advance before the combined session table. Session
    // progress is a second condition, never a replacement for source identity:
    // unrelated physical input may also contribute to the session counters.
    let states: [CGEventSourceStateID] = [state, .combinedSessionState]
    let input = NativePendingInput(owner: source, eventTypes: eventTypes.map(\.rawValue),
        counters: states.map { state in
            { CGEventSource.counterForEventType(state, eventType: CGEventType(rawValue: $0)!) }
        },
        released: { keys, buttons in
            states.allSatisfy { state in
                dtwInputStateReleased(keys: keys, buttons: buttons,
                    keyState: { CGEventSource.keyState(state, key: $0) },
                    buttonState: { CGEventSource.buttonState(state, button: CGMouseButton(rawValue: $0)!) },
                    flags: { CGEventSource.flagsState(state) })
            }
        })
    return nativeInputGate.register(input) ? input : nil
}

func dtwFinishInput(_ input: NativePendingInput) -> Bool { nativeInputGate.finish(input) }

func dtwModifierFlag(code: CGKeyCode) -> CGEventFlags? {
    switch code {
    case 55: return .maskCommand
    case 56: return .maskShift
    case 58: return .maskAlternate
    case 59: return .maskControl
    default: return nil
    }
}

func dtwInputStateReleased(keys: Set<UInt16>, buttons: Set<UInt32>,
                           keyState: (UInt16) -> Bool, buttonState: (UInt32) -> Bool,
                           flags: () -> CGEventFlags) -> Bool {
    guard keys.allSatisfy({ !keyState($0) }), buttons.allSatisfy({ !buttonState($0) }) else { return false }
    let touchedModifiers = keys.reduce(CGEventFlags()) { $0.union(dtwModifierFlag(code: $1) ?? []) }
    return touchedModifiers.isEmpty || flags().intersection(touchedModifiers).isEmpty
}

func dtwKeyProgressType(code: CGKeyCode, isUp: Bool) -> CGEventType? {
    if dtwModifierFlag(code: code) != nil { return .flagsChanged }
    return isUp ? .keyUp : nil
}

func dtwPostKey(_ event: CGEvent, input: NativePendingInput) {
    let code = CGKeyCode(event.getIntegerValueField(.keyboardEventKeycode))
    let type = dtwKeyProgressType(code: code, isUp: event.type == .keyUp)
    input.posted(eventType: type?.rawValue, key: code)
    event.post(tap: .cghidEventTap)
}

func dtwKeyCode(_ key: String) -> CGKeyCode? {
    let named: [String: CGKeyCode] = [
        "return":36,"enter":36,"tab":48,"space":49,"delete":51,"backspace":51,
        "escape":53,"esc":53,"home":115,"pageup":116,"forwarddelete":117,"del":117,
        "end":119,"pagedown":121,"left":123,"arrowleft":123,"right":124,"arrowright":124,
        "down":125,"arrowdown":125,"up":126,"arrowup":126,
        "f1":122,"f2":120,"f3":99,"f4":118,"f5":96,"f6":97,"f7":98,"f8":100,
        "f9":101,"f10":109,"f11":103,"f12":111,
        "a":0,"s":1,"d":2,"f":3,"h":4,"g":5,"z":6,"x":7,"c":8,"v":9,"b":11,
        "q":12,"w":13,"e":14,"r":15,"y":16,"t":17,"1":18,"2":19,"3":20,"4":21,
        "6":22,"5":23,"=":24,"9":25,"7":26,"-":27,"8":28,"0":29,
        "]":30,"o":31,"u":32,"[":33,"i":34,"p":35,"l":37,"j":38,"'":39,"k":40,
        ";":41,"\\":42,",":43,"/":44,"n":45,"m":46,".":47,"`":50
    ]
    return named[key.lowercased().replacingOccurrences(of: "_", with: "")]
}

func dtwModifier(_ key: String) -> (CGKeyCode, CGEventFlags)? {
    switch key.lowercased() {
    case "cmd", "command", "meta", "super": return (55, .maskCommand)
    case "shift": return (56, .maskShift)
    case "option", "alt": return (58, .maskAlternate)
    case "control", "ctrl": return (59, .maskControl)
    default: return nil
    }
}

struct NativeKeyDescriptor {
    let code: CGKeyCode
    let downFlags: CGEventFlags
    let upFlags: CGEventFlags
}

func dtwChordPlan(_ keys: [String]) -> [NativeKeyDescriptor]? {
    guard !keys.isEmpty, keys.count <= 5 else { return nil }
    var modifiers: [(CGKeyCode, CGEventFlags)] = []
    var primary: CGKeyCode?
    for key in keys {
        if let modifier = dtwModifier(key) {
            guard primary == nil, !modifiers.contains(where: { $0.0 == modifier.0 }) else { return nil }
            modifiers.append(modifier)
        } else {
            guard primary == nil, let code = dtwKeyCode(key) else { return nil }
            primary = code
        }
    }
    var plan: [NativeKeyDescriptor] = []
    var flags: CGEventFlags = []
    for (code, flag) in modifiers {
        let previous = flags; flags.insert(flag)
        plan.append(NativeKeyDescriptor(code: code, downFlags: flags, upFlags: previous))
    }
    if let code = primary { plan.append(NativeKeyDescriptor(code: code, downFlags: flags, upFlags: flags)) }
    return plan.isEmpty ? nil : plan
}

// Allocate every release before admitting any key down. The caller posts all
// down events and releases exactly that prefix in reverse order on every exit.
func dtwChord(_ keys: [String], source: CGEventSource) -> [NativeKeyPair]? {
    guard let plan = dtwChordPlan(keys) else { return nil }
    var pairs: [NativeKeyPair] = []
    for item in plan {
        guard let down = CGEvent(keyboardEventSource: source, virtualKey: item.code, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: item.code, keyDown: false) else { return nil }
        down.flags = item.downFlags; up.flags = item.upFlags
        pairs.append(NativeKeyPair(down: down, up: up))
    }
    return pairs
}

func dtwUnicodeUnits(_ scalar: Unicode.Scalar) -> [UInt16] { Array(String(scalar).utf16) }

// A Windows-style newline denotes one Return, not two native key pairs.
func dtwTextScalars(_ text: String) -> [Unicode.Scalar] {
    Array(text.replacingOccurrences(of: "\r\n", with: "\n").unicodeScalars)
}

func dtwWheelSteps(_ value: Any?) -> Int32? {
    guard let number = value as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
          let steps = Int32(number.stringValue), (-100...100).contains(steps) else { return nil }
    return steps
}

func dtwUnicodePair(_ scalar: Unicode.Scalar, source: CGEventSource) -> NativeKeyPair? {
    let code: CGKeyCode = scalar == "\n" || scalar == "\r" ? 36 : (scalar == "\t" ? 48 : 0)
    guard let down = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true),
          let up = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: false) else { return nil }
    if code == 0 {
        let units = dtwUnicodeUnits(scalar)
        units.withUnsafeBufferPointer { bytes in
            down.keyboardSetUnicodeString(stringLength: bytes.count, unicodeString: bytes.baseAddress!)
            up.keyboardSetUnicodeString(stringLength: bytes.count, unicodeString: bytes.baseAddress!)
        }
    }
    down.flags = []; up.flags = []
    return NativeKeyPair(down: down, up: up)
}

func dtwInputStopping() -> Bool { nativeInputGate.isClosing }

// Pointer warping is asynchronous in WindowServer. Allow the same short
// routing interval as pixel click before revalidating and posting a wheel.
// This wait holds no admitted input and stops promptly on shutdown/deadline.
func dtwSettleInput(seconds: TimeInterval, deadline: TimeInterval,
                    now: () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
                    stopping: () -> Bool = dtwInputStopping,
                    pause: (TimeInterval) -> Void = { Thread.sleep(forTimeInterval: $0) }) -> Bool {
    let until = now() + seconds
    guard until < deadline else { return false }
    while !stopping() {
        let current = now()
        guard current < deadline else { return false }
        if current >= until { return true }
        pause(min(0.01, until-current, deadline-current))
    }
    return false
}
