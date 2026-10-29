// Append after Input.swift + Native.swift. Pure rows only, no desktop/AX reads.
private func surfaceCheck(_ ok: @autoclosure () -> Bool) { precondition(ok()) }
private func surfaceRow(_ id: UInt32, pid: Int32 = 10, layer: Int = 0, width: Double = 100) -> [String: Any] {
    [kCGWindowNumber as String:id, kCGWindowOwnerPID as String:pid, kCGWindowLayer as String:layer,
     kCGWindowBounds as String:CGRect(x:0,y:0,width:width,height:100).dictionaryRepresentation,
     kCGWindowIsOnscreen as String:true, kCGWindowName as String:"owned title"]
}
private let (surfaces, coverage) = try surfaceInventory([surfaceRow(1), surfaceRow(2), surfaceRow(3, pid:11), surfaceRow(4, layer:3), surfaceRow(5, width:0)], pid:10)
surfaceCheck(surfaces.count == 2 && surfaces[0].0.id == 1 && surfaces[1].0.id == 2)
surfaceCheck(coverage["scope"] as? String == "same_pid_layer_zero_surfaces")
let counts = coverage["diagnostics"] as! [String:Int]
surfaceCheck(counts["cg_app_raw"] == 4 && counts["supported_count"] == 2)
surfaceCheck(counts["excluded_non_layer_zero"] == 1 && counts["excluded_zero_area"] == 1)
private let empty = try surfaceInventory([], pid:10)
surfaceCheck(empty.0.isEmpty)
private var invalid = surfaceRow(1)
invalid.removeValue(forKey: kCGWindowBounds as String)
for rows in [[surfaceRow(1),surfaceRow(1)], [invalid], [[kCGWindowOwnerPID as String:Int32(10)]]] {
    do { _ = try surfaceInventory(rows, pid:10); preconditionFailure("Malformed/duplicate surface accepted") }
    catch let failure as NativeFailure {
        surfaceCheck(failure.code == "window_inventory_unproven")
        let data = try JSONSerialization.data(withJSONObject:failure.json)
        surfaceCheck(!String(decoding:data,as:UTF8.self).contains("owned title"))
    }
}
print("System surface contracts passed (no AX or GUI reads)")

// Readiness distinguishes a temporal web stub from persistent custom decoration.
private var ordinary = SemanticReadinessSample()
ordinary.add(role:kAXButtonRole,children:0,depth:2,parentRole:kAXGroupRole)
surfaceCheck(ordinary.decision(manualOptIn:false,elapsed:0) == .ready)
surfaceCheck(ordinary.decision(manualOptIn:true,elapsed:0) == .ready)
surfaceCheck(ordinary.decision(manualOptIn:true,elapsed:3) == .ready)
private var shallow = ordinary
shallow.add(role:kAXUnknownRole,children:0,depth:2,parentRole:kAXGroupRole)
surfaceCheck(shallow.decision(manualOptIn:false,elapsed:0) == .pending)
surfaceCheck(shallow.decision(manualOptIn:false,elapsed:0.31) == .opaque)
surfaceCheck(shallow.decision(manualOptIn:true,elapsed:0.31) == .opaque)
private var decoration = ordinary
decoration.add(role:kAXUnknownRole,children:0,depth:8,parentRole:kAXGroupRole)
surfaceCheck(decoration.decision(manualOptIn:false,elapsed:0) == .ready)
private var web = shallow
web.add(role:"AXWebArea",children:1,depth:4,parentRole:kAXScrollAreaRole)
surfaceCheck(web.decision(manualOptIn:false,elapsed:0.1) == .ready)
print("Per-query native readiness decisions passed")

private var contentStub = SemanticReadinessSample()
contentStub.add(role:kAXGroupRole,children:0,depth:1,parentRole:kAXWindowRole)
surfaceCheck(contentStub.decision(manualOptIn:false,elapsed:0) == .pending)
surfaceCheck(contentStub.decision(manualOptIn:false,elapsed:0.31) == .emptyContent)
private var titleDecoration = SemanticReadinessSample()
titleDecoration.add(role:kAXGroupRole,children:0,depth:2,parentRole:kAXButtonRole)
surfaceCheck(titleDecoration.decision(manualOptIn:false,elapsed:0) == .ready)
private var liveContent = SemanticReadinessSample()
liveContent.add(role:kAXGroupRole,children:1,depth:1,parentRole:kAXWindowRole)
surfaceCheck(liveContent.decision(manualOptIn:false,elapsed:0) == .ready)
surfaceCheck(liveContent.decision(manualOptIn:true,elapsed:0) == .ready)
private var emptyWeb = SemanticReadinessSample()
emptyWeb.add(role:"AXWebArea",children:0,depth:3,parentRole:kAXScrollAreaRole)
surfaceCheck(emptyWeb.decision(manualOptIn:false,elapsed:0) == .pending)
print("Empty window content root stays pending; titlebar decoration does not")

// Native string writability alone does not advertise text editing on Chrome's
// structural/action nodes. The same role gate is used before setter dispatch.
for role in [kAXGroupRole, kAXToolbarRole, kAXButtonRole, kAXPopUpButtonRole,
             kAXCheckBoxRole, kAXSliderRole, kAXStaticTextRole, kAXWindowRole, ""] {
    surfaceCheck(!supportsStringValueEditing(role))
}
for role in [kAXTextFieldRole, kAXTextAreaRole, kAXComboBoxRole] {
    surfaceCheck(supportsStringValueEditing(role))
}
print("String setter excludes structural/action/value controls")

// Inject focus reads; never query system-wide AX or activate an application.
private var focusAssertions = 0
private func focusCheck(_ ok: @autoclosure () -> Bool, _ message: String) {
    precondition(ok(), message)
    focusAssertions += 1
}
private func focusFailure(_ code: String, _ body: () throws -> Bool) {
    do { _ = try body(); preconditionFailure("Unproven focus was accepted") }
    catch let error as NativeFailure { focusCheck(error.code == code, "Focus error was replaced: \(error.code)") }
    catch { preconditionFailure("Unexpected focus error: \(error)") }
}
private var focusReads = 0
private var windowReads = 0
private let stableFocus = try exactInputFocus(expectedPID: 10, focusedPID: {
    focusReads += 1; return 10
}, windowMatches: { windowReads += 1; return true })
focusCheck(stableFocus && focusReads == 2 && windowReads == 1, "Stable focus requires two fresh application reads")
focusReads = 0; windowReads = 0
private let otherApp = try exactInputFocus(expectedPID: 10, focusedPID: {
    focusReads += 1; return 20
}, windowMatches: { windowReads += 1; return true })
focusCheck(!otherApp && focusReads == 1 && windowReads == 0, "Another foreground app must reject a stale target window")
private var livePID: Int32 = 10
private let switchedDuringWindowRead = try exactInputFocus(expectedPID: 10, focusedPID: {
    livePID
}, windowMatches: { livePID = 20; return true })
focusCheck(!switchedDuringWindowRead, "Focus changed while the target AX window query was in flight")
private let changedWindow = try exactInputFocus(expectedPID: 10, focusedPID: { 10 }, windowMatches: { false })
focusCheck(!changedWindow, "A sibling window in the same process must not match")
for unavailable: Int32? in [nil, 0, -1] {
    focusFailure("focus_unproven") {
        try exactInputFocus(expectedPID: 10, focusedPID: { unavailable }, windowMatches: { true })
    }
}
private var focusAvailable = true
focusFailure("focus_unproven") {
    try exactInputFocus(expectedPID: 10, focusedPID: { focusAvailable ? 10 : nil },
                        windowMatches: { focusAvailable = false; return true })
}
focusFailure("ax_unavailable") {
    try exactInputFocus(expectedPID: 10, focusedPID: { throw failure("ax_unavailable", "original AX failure") },
                        windowMatches: { true })
}
focusFailure("window_unproven") {
    try exactInputFocus(expectedPID: 10, focusedPID: { 10 },
                        windowMatches: { throw failure("window_unproven", "original window identity failure") })
}
focusReads = 0
focusFailure("deadline_exceeded") {
    try exactInputFocus(expectedPID: 10, focusedPID: {
        focusReads += 1
        if focusReads == 2 { throw failure("deadline_exceeded", "deadline elapsed during focus read") }
        return 10
    }, windowMatches: { true })
}
print("Fresh input focus controls passed (\(focusAssertions) assertions; no AX or GUI reads)")

// Post-activation settling polls only injected reads, with a simulated clock.
// Ordinary input focus above deliberately continues to propagate every AX error.
private var focusReadinessAssertions = 0
private func focusReadinessCheck(_ ok: @autoclosure () -> Bool, _ message: String) {
    precondition(ok(), message)
    focusReadinessAssertions += 1
}
private func focusReadinessFailure(_ expected: NativeFailure, _ body: () throws -> Int) {
    do { _ = try body(); preconditionFailure("Unproven focus readiness was accepted") }
    catch let actual as NativeFailure {
        focusReadinessCheck(NSDictionary(dictionary: actual.json).isEqual(to: expected.json)
                            && actual.attributeFailure?.name == expected.attributeFailure?.name
                            && actual.attributeFailure?.error == expected.attributeFailure?.error
                            && actual.focusAbsence == expected.focusAbsence,
                            "Focus readiness replaced the original error")
    } catch { preconditionFailure("Unexpected readiness error: \(error)") }
}
private let busyFocusAX = failure("ax_unavailable", "first opaque diagnostic", details: ["marker": 7],
                                 attributeFailure: AXAttributeFailure(name: kAXWindowsAttribute, error: .cannotComplete))
private let laterBusyFocusAX = failure("ax_unavailable", "later opaque diagnostic",
                                      attributeFailure: AXAttributeFailure(name: kAXFocusedWindowAttribute, error: .cannotComplete))
private let missingReadyFocus = failure("focus_unproven", "The exact requested window did not become the focused window")
private var readinessClock: TimeInterval = 0
private var readinessReads = 0
private var readinessChecks = 0
private var readinessWaits: [TimeInterval] = []
private let settledFocus: Int = try waitForEstablishedFocus(remaining: { 0.75 - readinessClock }, check: {
    readinessChecks += 1
}, read: {
    readinessReads += 1
    if readinessReads == 1 { throw busyFocusAX }
    return 10
}, wait: { readinessWaits.append($0); readinessClock += $0 })
focusReadinessCheck(settledFocus == 10 && readinessReads == 2 && readinessChecks == 4
                    && readinessWaits == [0.02], "Transient AX failure can settle with read-only probes")

readinessClock = 0; readinessReads = 0; readinessWaits = []
focusReadinessFailure(busyFocusAX) {
    try waitForEstablishedFocus(remaining: { 0.03 - readinessClock }, check: {}, read: {
        readinessReads += 1
        throw readinessReads == 1 ? busyFocusAX : laterBusyFocusAX
    }, wait: { readinessWaits.append($0); readinessClock += $0 })
}
focusReadinessCheck(readinessReads == 2 && readinessWaits.count == 2
                    && readinessWaits.allSatisfy { $0 > 0 && $0 <= 0.02 }
                    && abs(readinessClock - 0.03) < 0.000_001,
                    "Readiness exhaustion is bounded and retains the first unresolved native error")

readinessClock = 0; readinessReads = 0
focusReadinessFailure(missingReadyFocus) {
    try waitForEstablishedFocus(remaining: { 0.04 - readinessClock }, check: {}, read: {
        readinessReads += 1
        if readinessReads == 1 { throw busyFocusAX }
        return nil
    }, wait: { readinessClock += $0 })
}
focusReadinessCheck(readinessReads == 2, "A recovered AX read with false focus clears the stale transient error")

for fatal in [
    failure("ax_unavailable", "AX array AXWindows failed: -25204"),
    failure("ax_unavailable", "wrong native status", attributeFailure: AXAttributeFailure(name: kAXWindowsAttribute, error: .failure)),
    failure("ax_unavailable", "invalid element", attributeFailure: AXAttributeFailure(name: kAXWindowsAttribute, error: .invalidUIElement)),
    failure("ax_unavailable", "malformed successful array", attributeFailure: AXAttributeFailure(name: kAXWindowsAttribute, error: .success)),
    failure("permission_denied", "AX permission denied"),
    failure("target_changed", "Application identity was replaced"),
] {
    readinessReads = 0; readinessWaits = []
    focusReadinessFailure(fatal) {
        try waitForEstablishedFocus(remaining: { 0.75 }, check: {}, read: {
            readinessReads += 1; throw fatal
        }, wait: { readinessWaits.append($0) })
    }
    focusReadinessCheck(readinessReads == 1 && readinessWaits.isEmpty,
                        "Permanent or untyped failures must not be retried")
}

for terminal in [failure("native_stopping", "Closing admission"),
                 failure("deadline_exceeded", "Request expired"),
                 failure("target_changed", "Process identity changed")] {
    for transientRead in [false, true] {
        readinessChecks = 0; readinessReads = 0; readinessWaits = []
        focusReadinessFailure(terminal) {
            try waitForEstablishedFocus(remaining: { 0.75 }, check: {
                readinessChecks += 1
                if readinessChecks == 2 { throw terminal }
            }, read: {
                readinessReads += 1
                if transientRead { throw busyFocusAX }
                return 10
            }, wait: { readinessWaits.append($0) })
        }
        focusReadinessCheck(readinessReads == 1 && readinessWaits.isEmpty,
                            "Post-read terminal state overrides both successful and transient reads")
    }
}

readinessReads = 0
focusReadinessFailure(missingReadyFocus) {
    try waitForEstablishedFocus(remaining: { 0 }, check: {}, read: {
        readinessReads += 1; return 10
    }, wait: { _ in preconditionFailure("Exhausted readiness must not wait") })
}
focusReadinessCheck(readinessReads == 0, "No read starts after the readiness budget expires")
readinessClock = 0
focusReadinessFailure(missingReadyFocus) {
    try waitForEstablishedFocus(remaining: { 0.75 - readinessClock }, check: {
        readinessClock = 0.76
    }, read: {
        readinessReads += 1; return 10
    }, wait: { _ in preconditionFailure("A late preflight must not start a wait") })
}
focusReadinessCheck(readinessReads == 0, "Slow identity verification cannot admit a read after the readiness cutoff")
readinessClock = 0
focusReadinessFailure(missingReadyFocus) {
    try waitForEstablishedFocus(remaining: { 0.75 - readinessClock }, check: {}, read: {
        readinessClock = 0.76; return 10
    }, wait: { _ in preconditionFailure("A late read must not start another wait") })
}
focusReadinessFailure(busyFocusAX) {
    readinessClock = 0
    return try waitForEstablishedFocus(remaining: { 0.75 - readinessClock }, check: {}, read: {
        readinessClock = 0.76; throw busyFocusAX
    }, wait: { _ in preconditionFailure("A late error must not start another wait") })
}
focusFailure("ax_unavailable") {
    try exactInputFocus(expectedPID: 10, focusedPID: { throw busyFocusAX }, windowMatches: { true })
}
// The explicit focus/input request authorizes one establishment after target
// validation; existing focus is optional in that phase, never proof of success.
private var focusPhaseAssertions = 0
private func focusPhaseCheck(_ ok: @autoclosure () -> Bool, _ message: String) {
    precondition(ok(), message)
    focusPhaseAssertions += 1
}
private func focusPhaseFailure(_ expected: NativeFailure, _ body: () throws -> Bool) {
    do { _ = try body(); preconditionFailure("A focus phase hid a non-recoverable failure") }
    catch let actual as NativeFailure {
        focusPhaseCheck(NSDictionary(dictionary: actual.json).isEqual(to: expected.json)
                        && actual.attributeFailure?.error == expected.attributeFailure?.error
                        && actual.focusAbsence == expected.focusAbsence,
                        "Focus phase changed the original failure")
    } catch { preconditionFailure("Unexpected focus phase error: \(error)") }
}
private let alreadyFocused = try focusedBeforeEstablishment { true }
private let otherFocused = try focusedBeforeEstablishment { false }
focusPhaseCheck(alreadyFocused && !otherFocused, "Initial exact/other focus retains its meaning")
for absent in [FocusAbsence.application, .window] {
    let initialMatch = try focusedBeforeEstablishment { throw missingFocus(absent) }
    focusPhaseCheck(!initialMatch, "Missing initial focus requires establishment, not success")
    focusPhaseCheck(missingFocus(absent).json.count == 2, "Typed focus absence is private, not a public schema change")
}
private let initialAbsentApp = try focusedBeforeEstablishment {
    try exactInputFocus(expectedPID: 10, focusedPID: { nil }, windowMatches: { true })
}
focusPhaseCheck(!initialAbsentApp, "The real missing focused-app reader can proceed to establishment")
for fatal in [
    busyFocusAX,
    failure("permission_denied", "Permission denied"),
    failure("target_changed", "Target changed"),
    failure("deadline_exceeded", "Request expired"),
    failure("focus_unproven", "System-wide focused application is unavailable; refresh before further input"),
] {
    focusPhaseFailure(fatal) { try focusedBeforeEstablishment { throw fatal } }
}
focusPhaseFailure(missingFocus(.application)) {
    try exactInputFocus(expectedPID: 10, focusedPID: { nil }, windowMatches: { true })
}
focusPhaseFailure(missingFocus(.window)) {
    try exactInputFocus(expectedPID: 10, focusedPID: { 10 }, windowMatches: { throw missingFocus(.window) })
}

for absent in [FocusAbsence.application, .window] {
    readinessClock = 0; readinessReads = 0
    let result: Int = try waitForEstablishedFocus(remaining: { 0.75 - readinessClock }, check: {}, read: {
        readinessReads += 1
        if readinessReads == 1 { throw missingFocus(absent) }
        return 10
    }, wait: { readinessClock += $0 })
    focusReadinessCheck(result == 10 && readinessReads == 2, "Post-activation missing focus can settle without reactivation")
}
private let missingFocusSequences: [([NativeFailure?], NativeFailure)] = [
    ([missingFocus(.application), missingFocus(.application)], missingFocus(.application)),
    ([missingFocus(.window), missingFocus(.window)], missingFocus(.window)),
    ([busyFocusAX, missingFocus(.application)], missingFocus(.application)),
    ([missingFocus(.application), missingFocus(.window)], missingFocus(.window)),
    ([missingFocus(.application), busyFocusAX], busyFocusAX),
    ([missingFocus(.application), nil], missingReadyFocus),
]
for (sequence, expected) in missingFocusSequences {
    readinessClock = 0; readinessReads = 0
    focusReadinessFailure(expected) {
        try waitForEstablishedFocus(remaining: { 0.04 - readinessClock }, check: {}, read: {
            let error = sequence[readinessReads]
            readinessReads += 1
            if let error { throw error }
            return nil
        }, wait: { readinessClock += $0 })
    }
    focusReadinessCheck(readinessReads == 2, "Settling retains the current unresolved cause after recovery/absence transitions")
}
print("Focus phase separation controls passed (\(focusPhaseAssertions) assertions; no AX or GUI reads)")
print("Post-activation focus readiness controls passed (\(focusReadinessAssertions) assertions; no AX or GUI reads)")

private var descriptionAssertions = 0
private func descriptionCheck(_ ok: @autoclosure () -> Bool, _ message: String) {
    precondition(ok(), message)
    descriptionAssertions += 1
}
private func descriptionFailure(_ attribute: String = kAXDescriptionAttribute,
                                _ code: AXError = .failure) -> NativeFailure {
    failure("ax_unavailable", "opaque diagnostic; never parsed",
            attributeFailure: AXAttributeFailure(name: attribute, error: code))
}
for (caseName, alreadyMatched, incomplete) in [
    ("role-only keeps known node", true, false),
    ("title/value match keeps known node", true, false),
    ("unknown description could match text", false, true),
] {
    let result = try optionalDescription(alreadyMatched: alreadyMatched) { throw descriptionFailure() }
    descriptionCheck(result.text == nil && result.incompleteMatch == incomplete, caseName)
}
private var descriptionMatched = false
private let readableDescription = try optionalDescription(alreadyMatched: descriptionMatched) {
    descriptionMatched = true
    return "matching description"
}
descriptionCheck(readableDescription.text == "matching description" && descriptionMatched
                 && !readableDescription.incompleteMatch, "description can establish the text match")
private let absentDescription = try optionalDescription(alreadyMatched: false) { nil }
descriptionCheck(absentDescription.text == nil && !absentDescription.incompleteMatch,
                 "legitimately absent description does not imply an unreadable match")
for fatal in [
    descriptionFailure(kAXTitleAttribute), descriptionFailure(kAXValueAttribute),
    descriptionFailure(kAXDescriptionAttribute, .invalidUIElement),
    descriptionFailure(kAXDescriptionAttribute, .cannotComplete),
    descriptionFailure(kAXDescriptionAttribute, .notImplemented),
    failure("deadline_exceeded", "Request deadline elapsed"),
    failure("target_changed", "Identity was replaced"),
    failure("ax_unavailable", "AX attribute AXDescription failed: -25200"),
] {
    do {
        _ = try optionalDescription(alreadyMatched: true) { throw fatal }
        preconditionFailure("Fatal or unrelated attribute error was hidden")
    } catch let actual as NativeFailure {
        descriptionCheck(actual.code == fatal.code && actual.message == fatal.message
                         && actual.attributeFailure?.name == fatal.attributeFailure?.name
                         && actual.attributeFailure?.error == fatal.attributeFailure?.error,
                         "native failure evidence propagates unchanged")
    }
}
private enum DescriptionControlError: Error { case unexpected }
do {
    _ = try optionalDescription(alreadyMatched: true) { throw DescriptionControlError.unexpected }
    preconditionFailure("Unexpected error was hidden")
} catch DescriptionControlError.unexpected {
    descriptionCheck(true, "non-native errors propagate")
}
descriptionCheck(descriptionFailure().json.count == 2,
                 "typed native evidence does not change the public error schema")
print("Optional AXDescription controls passed (\(descriptionAssertions) assertions; no AX reads)")
