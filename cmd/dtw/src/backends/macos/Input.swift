import Foundation

// Closing admission and draining already admitted input are separate steps.
// No AX, capture, or event-loop work is needed to close this gate. In particular,
// a stuck read cannot prevent the shutdown thread from refusing another input.
final class NativeInputGate: @unchecked Sendable {
    private let lock = NSLock()
    private let active = DispatchGroup()
    private var closing = false

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

    func quiesce(milliseconds: UInt32) -> Bool {
        lock.lock()
        closing = true
        lock.unlock()
        return active.wait(timeout: .now() + .milliseconds(Int(milliseconds))) == .success
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
