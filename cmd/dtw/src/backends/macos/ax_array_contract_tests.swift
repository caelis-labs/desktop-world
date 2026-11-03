// Pure AX-result controls: create references, but issue no AX/GUI requests.
private func checkAXArrayCompatibility() throws {
    let children = [AXUIElementCreateApplication(123), AXUIElementCreateApplication(456)]
    let array = children as CFArray
    var copies = 0
    func copy() -> CFArray { copies += 1; return array }
    func rejects(_ code: String, _ body: () throws -> Void) {
        do { try body(); preconditionFailure("unproven AX result accepted") }
        catch let error as NativeFailure { precondition(error.code == code) }
        catch { preconditionFailure("unexpected AX error: \(error)") }
    }
    let fastCount = try resolvedAXChildCount(result: .success, count: 2, copy: copy)
    precondition(fastCount == 2)
    precondition(copies == 0)
    for error: AXError in [.failure, .attributeUnsupported, .noValue, .notImplemented, .illegalArgument] {
        let count = try resolvedAXChildCount(result: error, count: 0, copy: copy)
        precondition(count == 2)
        let range = try resolvedAXChildRange(result: error, value: nil, index: 1, count: 17, copy: copy)
        precondition(range.count == 1 && CFEqual(range[0], children[1]))
    }
    precondition(copies == 10)
    for error: AXError in [.invalidUIElement, .cannotComplete, .apiDisabled] {
        rejects("children_unavailable") { _ = try resolvedAXChildCount(result: error, count: 0, copy: copy) }
        rejects("children_unavailable") { _ = try resolvedAXChildRange(result: error, value: nil, index: 0, count: 1, copy: copy) }
    }
    precondition(copies == 10, "transport/identity errors retried through array copy")
    rejects("children_unavailable") { _ = try resolvedAXChildRange(result: .success, value: [] as CFArray, index: 0, count: 1, copy: copy) }
    precondition(copies == 10, "empty successful range hid a count/range race")
    rejects("children_unavailable") { _ = try resolvedAXChildRange(result: .failure, value: nil, index: 2, count: 1, copy: copy) }
    rejects("children_unavailable") {
        _ = try resolvedAXChildRange(result: .failure, value: nil, index: 0, count: 1) { ["impostor"] as CFArray }
    }
    rejects("children_unavailable") { _ = try checkedAXChildren("wrong type" as CFString, result: .success, required: false) }
    rejects("children_unavailable") { _ = try checkedAXChildren(nil, result: .failure, required: false) }
    rejects("modal_state_unproven") { _ = try checkedAXChildren(nil, result: .noValue, required: true) }
    let empty = try checkedAXChildren(nil, result: .attributeUnsupported, required: false)
    precondition(CFArrayGetCount(empty) == 0)
    rejects("children_unavailable") {
        _ = try checkedAXChildren(Array(repeating: NSNull(), count: 32_769) as CFArray, result: .success, required: false)
    }
    rejects("deadline_exceeded") {
        _ = try resolvedAXChildCount(result: .failure, count: 0) { throw failure("deadline_exceeded", "original deadline") }
    }
    for code in ["children_unavailable", "ax_unavailable", "semantic_window_unavailable", "modal_state_unproven"] {
        precondition(requiresVisualInput(failure(code, "capability unavailable")))
    }
    for code in ["target_changed", "stale_window", "app_changed", "cancelled", "permission_required", "deadline_exceeded"] {
        precondition(!requiresVisualInput(failure(code, "original authority error")))
    }
    var visualEpoch = FocusBorrowEpoch(initialPID: 123, targetPID: 123)
    visualEpoch.focusedWindow(10); visualEpoch.focusedWindow(20)
    precondition(!visualEpoch.invalidated, "visual application scope depends on AX window callbacks")
    visualEpoch.activated(456); visualEpoch.activated(123)
    precondition(visualEpoch.invalidated, "leave/return revived visual foreground ownership")
}
try checkAXArrayCompatibility()
print("AX count/range compatibility, malformed arrays, budgets and visual route authority controls passed; no AX/GUI requests")
