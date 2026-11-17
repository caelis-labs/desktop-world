// Pure AX-result controls: create references, but issue no AX/GUI requests.
private func checkTextPressCapabilities() throws {
    // A focusable select-only ComboBox must execute Press to open its list.
    var expanded = false
    var focused = false
    let selectOnly = textInputReceiver(role: kAXComboBoxRole, editableValue: { false })
    let capability = textPressCapability(textReceiver: selectOnly, press: true, focusWritable: { true })
    switch capability {
    case .action?: expanded = true
    case .focus?: focused = true
    default: preconditionFailure("Select-only ComboBox lost Press")
    }
    precondition(expanded && !focused && !selectOnly)
    for role in [kAXTextFieldRole, kAXTextAreaRole, kAXComboBoxRole] {
        let receiver = textInputReceiver(role: role, editableValue: { true })
        precondition(receiver)
        guard case .focus? = textPressCapability(textReceiver: receiver, press: true, focusWritable: { true }) else {
            preconditionFailure("Editable text control did not prefer exact receiver focus")
        }
        guard case .action? = textPressCapability(textReceiver: receiver, press: true, focusWritable: { false }) else {
            preconditionFailure("Unavailable focus lost the existing Press fallback")
        }
    }
    let button = textInputReceiver(role: kAXButtonRole) {
        preconditionFailure("Action roles must not query text writability")
    }
    precondition(!button)
    guard case .action? = textPressCapability(textReceiver: button, press: true, focusWritable: {
        preconditionFailure("Non-text controls must not query focus writability")
    }) else { preconditionFailure("Action control lost Press") }
    precondition(textPressCapability(textReceiver: false, press: false, focusWritable: { true }) == nil)
    do {
        _ = try textInputReceiver(role: kAXComboBoxRole) { throw failure("deadline_exceeded", "original read failure") }
        preconditionFailure("Editable capability read error was swallowed")
    } catch let error as NativeFailure { precondition(error.code == "deadline_exceeded") }
}
try checkTextPressCapabilities()

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
    var visualEpoch = FocusEpoch(initialPID: 123, targetPID: 123)
    visualEpoch.focusedWindow(10); visualEpoch.focusedWindow(20)
    precondition(!visualEpoch.invalidated, "visual application scope depends on AX window callbacks")
    visualEpoch.activated(456); visualEpoch.activated(123)
    precondition(visualEpoch.invalidated, "leave/return revived visual foreground ownership")
}
try checkAXArrayCompatibility()
print("AX count/range compatibility, malformed arrays, budgets and visual route authority controls passed; no AX/GUI requests")
