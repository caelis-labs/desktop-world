// Append after Input.swift + Native.swift. Pure rows only, no desktop/AX reads.
private func surfaceCheck(_ ok: @autoclosure () -> Bool) { precondition(ok()) }
private func surfaceRow(_ id: UInt32, pid: Int32 = 10, layer: Int = 0, width: Double = 100) -> [String: Any] {
    [kCGWindowNumber as String:id, kCGWindowOwnerPID as String:pid, kCGWindowLayer as String:layer,
     kCGWindowBounds as String:CGRect(x:0,y:0,width:width,height:100).dictionaryRepresentation,
     kCGWindowIsOnscreen as String:true, kCGWindowName as String:"owned title"]
}
private let (surfaces, coverage) = try surfaceInventory([surfaceRow(1), surfaceRow(2), surfaceRow(3, pid:11), surfaceRow(4, layer:3), surfaceRow(5, width:0)], pid:10)
surfaceCheck(surfaces.count == 3 && surfaces[0].0.id == 1 && surfaces[1].0.id == 2 && surfaces[2].0.id == 4)
surfaceCheck(coverage["scope"] as? String == "same_pid_application_surfaces")
let counts = coverage["diagnostics"] as! [String:Int]
surfaceCheck(counts["cg_app_raw"] == 4 && counts["supported_count"] == 3)
surfaceCheck(counts["excluded_unavailable_surface"] == 0 && counts["excluded_zero_area"] == 1)
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

// Live sheet/menu ownership uses object ancestry, never a matching name/frame.
private let surfaceParents = [3: 2, 2: 1, 5: 4, 4: 0, 1: 0]
surfaceCheck(boundedAncestor(3, is: 2, equal: ==) { surfaceParents[$0] })
surfaceCheck(!boundedAncestor(3, is: 4, equal: ==) { surfaceParents[$0] })
surfaceCheck(!boundedAncestor(3, is: 2, equal: ==) { $0 == 3 ? 4 : surfaceParents[$0] })
surfaceCheck(!boundedAncestor(3, is: 1, limit: 2, equal: ==) { surfaceParents[$0] })
surfaceCheck(!boundedAncestor(3, is: 1, equal: ==) { $0 == 3 ? 2 : 3 })
do {
    _ = try boundedAncestor(3, is: 1, equal: ==) { _ -> Int? in throw failure("native_stopping", "cancelled") }
    preconditionFailure("Ancestry read error was swallowed")
} catch let error as NativeFailure { surfaceCheck(error.code == "native_stopping") }

// Full text pages preserve UTF-16 offsets across emoji and CJK. Offset inside
// a pair is a non-consuming range refusal, not a malformed replacement string.
private let pageText = "A😀中文Z"
private let pageOne = try textPage(pageText, offset: 0, limit: 2)
surfaceCheck(pageOne.0 == "A" && pageOne.1 == 1)
private let pageTwo = try textPage(pageText, offset: 1, limit: 2)
surfaceCheck(pageTwo.0 == "😀" && pageTwo.1 == 3)
private let pageThree = try textPage(pageText, offset: 3, limit: 3)
surfaceCheck(pageThree.0 == "中文Z" && pageThree.1 == nil)
private let endPage = try textPage(pageText, offset: 6, limit: 2)
surfaceCheck(endPage.0.isEmpty && endPage.1 == nil)
for offset in [2, 7] {
    do { _ = try textPage(pageText, offset: offset, limit: 2); preconditionFailure("Invalid range accepted") }
    catch let error as NativeFailure { surfaceCheck(error.code == "invalid_text_range") }
}
surfaceCheck(associatedGeometry(CGRect(x: -900, y: -30, width: 420, height: 300),
                                CGRect(x: -900, y: -30, width: 420, height: 300)))
surfaceCheck(!associatedGeometry(CGRect(x: -900, y: -30, width: 420, height: 300),
                                 CGRect(x: -890, y: -30, width: 420, height: 300)))
print("Surface ancestry, replacement/cancellation, negative-origin geometry and UTF-16 pages passed")

// Unattributed foreign AX hit failures are partial discovery evidence, never
// a reason to discard proven same-process windows. Same-app read failures and
// caller termination retain their exact errors instead of entering that path.
private let foreignHitUnavailable = failure("association_unavailable", "foreign hit busy",
    attributeFailure: AXAttributeFailure(name: "AXElementAtPosition", error: .cannotComplete))
private var associationChecks = 0
private let failedAssociation: (Int?, Int32?) = try unprovenAssociationProbe(check: {
    associationChecks += 1
}, read: { throw foreignHitUnavailable })
surfaceCheck(failedAssociation.0 == nil && failedAssociation.1 == AXError.cannotComplete.rawValue && associationChecks == 1)
private let provenAssociation = try unprovenAssociationProbe(check: {
    preconditionFailure("Successful proof was treated as unavailable")
}, read: { 41 })
surfaceCheck(provenAssociation.0 == 41 && provenAssociation.1 == nil)
private let absentAssociation: (Int?, Int32?) = try unprovenAssociationProbe(check: {
    preconditionFailure("A successful unrelated hit was treated as an error")
}, read: { nil })
surfaceCheck(absentAssociation.0 == nil && absentAssociation.1 == nil)
for original in [failure("ax_unavailable", "same-app AX read failed",
                        attributeFailure: AXAttributeFailure(name: kAXRoleAttribute, error: .cannotComplete)),
                 failure("association_unavailable", "unclassified association failure"),
                 failure("target_changed", "exact surface changed")] {
    do {
        let _: (Int?, Int32?) = try unprovenAssociationProbe(check: {
            preconditionFailure("A target error entered optional recovery")
        }, read: { throw original })
        preconditionFailure("A target error was swallowed")
    } catch let error as NativeFailure {
        surfaceCheck(error.code == original.code && error.message == original.message)
    }
}
for code in ["deadline_exceeded", "native_stopping", "app_changed"] {
    do {
        let _: (Int?, Int32?) = try unprovenAssociationProbe(check: {
            throw failure(code, "original caller/target cause")
        }, read: { throw foreignHitUnavailable })
        preconditionFailure("Caller/target failure was downgraded")
    } catch let error as NativeFailure {
        surfaceCheck(error.code == code && error.message == "original caller/target cause")
    }
}
print("Optional association discovery preserves target and caller errors; no AX or GUI reads")

// The standard Rust bridge now serializes window scope explicitly. Cold AX
// materialization must retain the same bounded read path as omitted scope.
private let implicitWindowQuery = try NativeQuery(["text": "body"])
private let explicitWindowQuery = try NativeQuery(["text": "body", "scope": ["kind": "window"]])
surfaceCheck(implicitWindowQuery.isWindowScope && explicitWindowQuery.isWindowScope)
for kind in ["menu", "subtree", "region"] {
    let scopedQuery = try NativeQuery(["scope": ["kind": kind]])
    surfaceCheck(!scopedQuery.isWindowScope)
}

// Chrome's file chooser reports its AXSheet as AXFocusedWindow. Activation
// goes through the exact parent AXWindow, while focus proof keeps the sheet.
private func sheetFocusCheck(_ condition: Bool) { surfaceCheck(condition) }
private let dialogRoles = [1: kAXWindowRole, 2: kAXSheetRole, 3: kAXSheetRole,
                           4: kAXWindowRole, 5: kAXApplicationRole]
private let dialogParents = [2: 1, 3: 2, 1: 5, 4: 5]
private let dialogIDs: [Int: CGWindowID] = [1: 100, 2: 200, 3: 300, 4: 400]
private func dialogActivation(_ surface: Int) throws -> Int? {
    try resolveActivationWindow(surface, pid: 10, owner: { _ in 10 },
        role: { dialogRoles[$0] }, parent: { dialogParents[$0] })
}
sheetFocusCheck(try dialogActivation(1) == 1)
sheetFocusCheck(try dialogActivation(2) == 1)
sheetFocusCheck(try dialogActivation(3) == 1)
private func dialogFocused(_ requested: Int, _ actual: Int) throws -> Bool {
    try exactFocusedSurface(requested: requested, requestedWindow: dialogIDs[requested]!,
                            focused: actual, equal: ==, windowID: { dialogIDs[$0]! })
}
sheetFocusCheck(try dialogFocused(2, 2))
sheetFocusCheck(try !dialogFocused(2, 1)) // Parent-focused provider form remains unproven.
sheetFocusCheck(try !dialogFocused(2, 3)) // A new nested sheet is a distinct target.
sheetFocusCheck(try !dialogFocused(2, 4))
sheetFocusCheck(try !exactFocusedSurface(requested: 2, requestedWindow: 200,
                                    focused: 2, equal: ==, windowID: { _ in 201 }))
private let foreignActivation = try resolveActivationWindow(2, pid: 10,
    owner: { $0 == 1 ? 20 : 10 }, role: { dialogRoles[$0] }, parent: { dialogParents[$0] })
sheetFocusCheck(foreignActivation == nil)
private let cycleActivation = try resolveActivationWindow(2, pid: 10,
    owner: { _ in 10 }, role: { _ in kAXSheetRole }, parent: { _ in 2 })
sheetFocusCheck(cycleActivation == nil)
do {
    _ = try resolveActivationWindow(2, pid: 10, owner: { _ in 10 },
        role: { dialogRoles[$0] }, parent: { _ -> Int? in throw failure("native_stopping", "original cleanup stop") })
    preconditionFailure("Activation ancestry swallowed cancellation")
} catch let error as NativeFailure { sheetFocusCheck(error.code == "native_stopping" && error.message == "original cleanup stop") }
// Returning to a captured sheet raises its parent, but a parent readback alone
// never proves restoration of that exact original sheet.
let previousSheetRaise = try dialogActivation(2)
let previousSheetExact = try dialogFocused(2, 2)
sheetFocusCheck(previousSheetRaise == 1 && previousSheetExact)
sheetFocusCheck(try !dialogFocused(2, previousSheetRaise!))
private var sheetEpoch = FocusBorrowEpoch(initialPID: 10, targetPID: 10, targetWindow: 200)
sheetEpoch.established(10, window: 200)
func sheetNotification(_ name: String, objectID: CGWindowID, sampledID: CGWindowID) {
    if notificationNamesFocusedWindow(name) { sheetEpoch.focusedWindow(objectID) }
    else { _ = sheetEpoch.sampled(10, window: sampledID, revision: sheetEpoch.sampleRevision) }
}
sheetNotification(kAXMainWindowChangedNotification, objectID: 100, sampledID: 200)
sheetFocusCheck(!sheetEpoch.invalidated)
sheetNotification(kAXFocusedWindowChangedNotification, objectID: 400, sampledID: 400)
sheetFocusCheck(sheetEpoch.invalidated)
sheetNotification(kAXFocusedWindowChangedNotification, objectID: 200, sampledID: 200)
sheetFocusCheck(sheetEpoch.invalidated) // A genuine leave-return is never rearmed.
print("Sheet activation, exact focus/return and main-window notification controls passed")

// Initial exact focus incurs no restoration debt even if input later closes
// that sheet. In-place cleanup still runs the same conservative pointer path.
private func alreadyAtSurface(previousPID: Int32 = 10, generation: UInt64 = 1,
                              previous: Int? = 2, previousID: CGWindowID? = 200,
                              target: Int? = 2, targetID: CGWindowID = 200) -> Bool {
    sameFocusDestination(previousApp: (previousPID, generation), previousWindow: previous,
        previousWindowID: previousID, targetApp: (10, 1), targetWindow: target,
        targetWindowID: targetID, equal: ==)
}
surfaceCheck(alreadyAtSurface())
surfaceCheck(!alreadyAtSurface(previousPID: 20))
surfaceCheck(!alreadyAtSurface(generation: 2))
surfaceCheck(!alreadyAtSurface(previous: 1, previousID: 100)) // Same PID sibling/parent.
surfaceCheck(!alreadyAtSurface(previous: 3)) // Reused WindowID is not the same AX object.
surfaceCheck(!alreadyAtSurface(previousID: 201))
surfaceCheck(!alreadyAtSurface(previous: nil, previousID: nil))
surfaceCheck(!alreadyAtSurface(target: nil))
private var inPlace = FocusReturnObligation(matchesOriginal: alreadyAtSurface())
private var pointerCleanups = 0
sheetFocusCheck(try inPlace.releaseInPlace { pointerCleanups += 1 })
surfaceCheck(pointerCleanups == 1 && !inPlace.required)
inPlace.retarget(matchesOriginal: true)
surfaceCheck(!inPlace.required)
inPlace.retarget(matchesOriginal: false)
surfaceCheck(inPlace.required)
sheetFocusCheck(try !inPlace.releaseInPlace { preconditionFailure("A real borrow skipped exact restoration") })
inPlace.retarget(matchesOriginal: true)
surfaceCheck(inPlace.required) // An earlier real borrow cannot be cleared by repeated focus.
private let existingSheet = FocusReturnObligation(matchesOriginal: true)
do {
    _ = try existingSheet.releaseInPlace { throw failure("pointer_restore_failed", "original pointer cleanup") }
    preconditionFailure("In-place focus swallowed pointer cleanup failure")
} catch let error as NativeFailure {
    surfaceCheck(error.code == "pointer_restore_failed" && error.message == "original pointer cleanup")
}
print("Exact in-place focus, real-borrow retention and independent pointer cleanup controls passed")

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

// The first filtered empty page may be a cold asynchronous provider result.
// Re-read only within the original Enhanced attempt's window, without a poke.
surfaceCheck(emptySemanticRetryUntil(filtered:true,complete:true,matches:0,
    enhancedAttemptedAt:10,now:10.2) == 13)
for (filtered, complete, matches, attempted, now) in [
    (false, true, 0, Optional(10.0), 10.2),
    (true, false, 0, Optional(10.0), 10.2),
    (true, true, 1, Optional(10.0), 10.2),
    (true, true, 0, Optional<Double>.none, 10.2),
    (true, true, 0, Optional(10.0), 13.0),
] {
    surfaceCheck(emptySemanticRetryUntil(filtered:filtered,complete:complete,matches:matches,
        enhancedAttemptedAt:attempted,now:now) == nil)
}
var materializationChecks = 0
let coldMaterializationPending = try semanticMaterializationPending(until:13,now:10.2,populated:false,
    check:{materializationChecks += 1})
let populatedMaterializationPending = try semanticMaterializationPending(until:13,now:12.3,populated:true,
    check:{materializationChecks += 1})
let nativeMaterializationPending = try semanticMaterializationPending(until:13,now:13,populated:false,
    check:{materializationChecks += 1}) // A native window need not grow WebArea.
let ordinaryMaterializationPending = try semanticMaterializationPending(until:nil,now:10,populated:false,
    check:{preconditionFailure("Ordinary matching/native query must not enter the wait")})
surfaceCheck(coldMaterializationPending && !populatedMaterializationPending
    && !nativeMaterializationPending && !ordinaryMaterializationPending)
surfaceCheck(materializationChecks == 3)
for code in ["native_stopping", "deadline_exceeded", "target_changed", "ax_unavailable"] {
    do {
        _ = try semanticMaterializationPending(until:13,now:13,populated:false,
            check:{throw failure(code, "original control cause")})
        preconditionFailure("Materialization replaced a terminal cause with an empty result")
    } catch let error as NativeFailure { surfaceCheck(error.code == code && error.message == "original control cause") }
}
print("Cold filtered-empty materialization controls passed; no AX or GUI reads")

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
private func controlExactInputFocus(expectedPID: Int32, focusedPID: () throws -> Int32?,
                                    windowMatches: () throws -> Bool) throws -> Bool {
    try exactInputFocus(expectedPID: expectedPID, focusedApplication: {
        guard let pid = try focusedPID(), pid > 0 else { throw missingFocus(.application) }
        return FocusApplication(element: pid, pid: pid, generation: 1, fromCandidate: false)
    }, windowMatches: { _ in try windowMatches() })
}
private let stableFocus = try controlExactInputFocus(expectedPID: 10, focusedPID: {
    focusReads += 1; return 10
}, windowMatches: { windowReads += 1; return true })
focusCheck(stableFocus && focusReads == 2 && windowReads == 1, "Stable focus requires two fresh application reads")
focusReads = 0; windowReads = 0
private let otherApp = try controlExactInputFocus(expectedPID: 10, focusedPID: {
    focusReads += 1; return 20
}, windowMatches: { windowReads += 1; return true })
focusCheck(!otherApp && focusReads == 1 && windowReads == 0, "Another foreground app must reject a stale target window")
private var livePID: Int32 = 10
private let switchedDuringWindowRead = try controlExactInputFocus(expectedPID: 10, focusedPID: {
    livePID
}, windowMatches: { livePID = 20; return true })
focusCheck(!switchedDuringWindowRead, "Focus changed while the target AX window query was in flight")
private let changedWindow = try controlExactInputFocus(expectedPID: 10, focusedPID: { 10 }, windowMatches: { false })
focusCheck(!changedWindow, "A sibling window in the same process must not match")
for unavailable: Int32? in [nil, 0, -1] {
    focusFailure("focus_unproven") {
        try controlExactInputFocus(expectedPID: 10, focusedPID: { unavailable }, windowMatches: { true })
    }
}
private var focusAvailable = true
focusFailure("focus_unproven") {
    try controlExactInputFocus(expectedPID: 10, focusedPID: { focusAvailable ? 10 : nil },
                        windowMatches: { focusAvailable = false; return true })
}
focusFailure("ax_unavailable") {
    try controlExactInputFocus(expectedPID: 10, focusedPID: { throw failure("ax_unavailable", "original AX failure") },
                        windowMatches: { true })
}
focusFailure("window_unproven") {
    try controlExactInputFocus(expectedPID: 10, focusedPID: { 10 },
                        windowMatches: { throw failure("window_unproven", "original window identity failure") })
}
focusReads = 0
focusFailure("deadline_exceeded") {
    try controlExactInputFocus(expectedPID: 10, focusedPID: {
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
    try controlExactInputFocus(expectedPID: 10, focusedPID: { throw busyFocusAX }, windowMatches: { true })
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
    try controlExactInputFocus(expectedPID: 10, focusedPID: { nil }, windowMatches: { true })
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
    try controlExactInputFocus(expectedPID: 10, focusedPID: { nil }, windowMatches: { true })
}
focusPhaseFailure(missingFocus(.window)) {
    try controlExactInputFocus(expectedPID: 10, focusedPID: { 10 }, windowMatches: { throw missingFocus(.window) })
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

// All-list retained backing surfaces must not mint live refs. A false onscreen
// flag alone does not exclude a surface which exact WindowServer lookup proves.
private var offscreenSurface = surfaceRow(8)
offscreenSurface.removeValue(forKey: kCGWindowIsOnscreen as String)
private var exactReads: [UInt32] = []
private let liveSurfaces = try surfaceInventory([surfaceRow(7), offscreenSurface, surfaceRow(9, layer: 25)], pid: 10) { record in
    exactReads.append(record.id)
    return record.id == 7 ? nil : record
}
surfaceCheck(exactReads == [7, 8, 9])
surfaceCheck(liveSurfaces.0.map { $0.0.id } == [8, 9])
surfaceCheck(!liveSurfaces.0[0].0.onscreen)
surfaceCheck((liveSurfaces.1["diagnostics"] as! [String:Int])["excluded_unavailable_surface"] == 1)
do {
    _ = try surfaceInventory([surfaceRow(7)], pid: 10) { _ in throw failure("deadline_exceeded", "original") }
    preconditionFailure("Exact inventory failure became absence")
} catch let error as NativeFailure { surfaceCheck(error.code == "deadline_exceeded" && error.message == "original") }
do {
    _ = try surfaceInventory([surfaceRow(7)], pid: 10) { record in
        CGRecord(id: record.id, pid: 11, bounds: record.bounds, onscreen: true)
    }
    preconditionFailure("Changed owner became a live reference")
} catch let error as NativeFailure { surfaceCheck(error.code == "target_changed") }
print("Application surfaces preserve overlays, reject retained stores and check exact owners")

// Exercise the production exact-query parser, not only the resolver seam.
// Nonempty malformed data may not claim a complete inventory by disappearing.
private let expectedSurface = cgRecord(surfaceRow(70))!
private let absentExactSurface = try exactSurfaceRecord([], expected: expectedSurface)
surfaceCheck(absentExactSurface == nil)
private let provenExactSurface = try exactSurfaceRecord([surfaceRow(70, layer: 25)], expected: expectedSurface)
surfaceCheck(provenExactSurface?.id == expectedSurface.id)
for missing in [kCGWindowNumber, kCGWindowOwnerPID, kCGWindowLayer, kCGWindowBounds] {
    var row = surfaceRow(70)
    row.removeValue(forKey: missing as String)
    do {
        _ = try exactSurfaceRecord([row], expected: expectedSurface)
        preconditionFailure("Incomplete exact row became unavailable")
    } catch let error as NativeFailure { surfaceCheck(error.code == "window_inventory_unproven") }
}
for rows in [[surfaceRow(70), surfaceRow(70)], [surfaceRow(70), [:]], [surfaceRow(70, width: 0)]] {
    do {
        _ = try exactSurfaceRecord(rows, expected: expectedSurface)
        preconditionFailure("Invalid exact rows became unavailable")
    } catch let error as NativeFailure { surfaceCheck(error.code == "window_inventory_unproven") }
}
for row in [surfaceRow(71), surfaceRow(70, pid: 11)] {
    do {
        _ = try exactSurfaceRecord([row], expected: expectedSurface)
        preconditionFailure("Changed exact identity became unavailable")
    } catch let error as NativeFailure { surfaceCheck(error.code == "target_changed") }
}
print("Exact inventory distinguishes empty lookup from malformed or changed metadata")

// Real filesystem aliases: a matching label or matching bytes is insufficient.
// No process/desktop reads are needed to prove the normalization boundary.
private func checkExecutableAliases() throws {
    let fm = FileManager.default
    let root = fm.temporaryDirectory.appendingPathComponent("dtw-executable-alias-" + UUID().uuidString)
    defer { try? fm.removeItem(at: root) }
    let bundle = root.appendingPathComponent("Browser.app")
    let executable = bundle.appendingPathComponent("Contents/MacOS/Browser")
    let alias = root.appendingPathComponent("Browser.app.bundle/Contents/MacOS/Browser")
    let impostor = root.appendingPathComponent("Other.app/Contents/MacOS/Browser")
    for file in [executable, alias, impostor] {
        try fm.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
    }
    try Data("owned binary".utf8).write(to: executable)
    try fm.linkItem(at: executable, to: alias)
    try Data("owned binary".utf8).write(to: impostor)
    let candidate = (bundle: bundle.path, executable: executable.path)
    surfaceCheck(canonicalAppExecutable(alias.path, registered: { candidate }) == executable.path)
    surfaceCheck(canonicalAppExecutable(alias.path, registered: { nil }) == alias.path)
    surfaceCheck(canonicalAppExecutable(alias.path, registered: {
        (root.appendingPathComponent("Other.app").path, impostor.path)
    }) == alias.path)
    surfaceCheck(canonicalAppExecutable(alias.path, registered: {
        (root.appendingPathComponent("Wrong.app").path, executable.path)
    }) == alias.path)
    try fm.removeItem(at: executable)
    surfaceCheck(canonicalAppExecutable(alias.path, registered: { candidate }) == alias.path)
    for kernel in [impostor.path, "/usr/bin/tool",
                   "/Applications/Browser.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper"] {
        surfaceCheck(canonicalAppExecutable(kernel, registered: {
            preconditionFailure("ordinary bundle/helper/non-app must not consult registered URLs")
        }) == kernel)
    }
}
try checkExecutableAliases()
print("Executable alias contracts passed (real hard link, impostor, missing file and nested helper)")


// Retry only complete read-only inventories after a typed cannotComplete. Each
// attempt constructs a new array; a failed partial snapshot is never merged.
private let inventoryBusy = failure("ax_unavailable", "original AXRole busy",
    attributeFailure: AXAttributeFailure(name: kAXRoleAttribute, error: .cannotComplete))
private var inventoryTime = 0.0
private var inventoryReads = 0
private let recoveredInventory: [Int] = try retryCompleteInventory(original: inventoryBusy,
    remaining: { 0.75 - inventoryTime }, check: {}, read: {
        inventoryReads += 1
        if inventoryReads == 1 {
            let partial = [1, 2]; surfaceCheck(partial.count == 2)
            throw inventoryBusy
        }
        return [3, 4]
    }, wait: { inventoryTime += $0 })
surfaceCheck(recoveredInventory == [3, 4] && inventoryReads == 2)
inventoryTime = 0
private var lastInventoryError = ""
do {
    let _: [Int] = try retryCompleteInventory(original: inventoryBusy,
        remaining: { 0.25 - inventoryTime }, check: {}, read: {
            inventoryTime += 0.1
            lastInventoryError = "AXWindows busy at \(inventoryTime)"
            throw failure("ax_unavailable", lastInventoryError,
                          attributeFailure: AXAttributeFailure(name: kAXWindowsAttribute, error: .cannotComplete))
        }, wait: { inventoryTime += $0 })
    preconditionFailure("Busy inventory falsely became empty")
} catch let error as NativeFailure {
    surfaceCheck(error.message == lastInventoryError && error.attributeFailure?.name == kAXWindowsAttribute)
}
for code in ["native_stopping", "deadline_exceeded", "app_changed"] {
    do {
        let _: [Int] = try retryCompleteInventory(original: inventoryBusy, remaining: { 0.75 },
            check: { throw failure(code, "caller cause") },
            read: { preconditionFailure("Read continued after caller stop") }, wait: { _ in })
        preconditionFailure("Caller error lost")
    } catch let error as NativeFailure { surfaceCheck(error.code == code && error.message == "caller cause") }
}
for original in [failure("ax_unavailable", "target object expired", attributeFailure: AXAttributeFailure(name: kAXRoleAttribute, error: .invalidUIElement)),
                 failure("ax_unavailable", "untyped failure"), failure("target_changed", "target changed")] {
    do {
        let _: [Int] = try retryCompleteInventory(original: inventoryBusy, remaining: { 0.75 },
            check: {}, read: { throw original }, wait: { _ in })
        preconditionFailure("A non-busy error was retried")
    } catch let error as NativeFailure { surfaceCheck(error.code == original.code && error.message == original.message) }
}
inventoryTime = 0
do {
    let _: [Int] = try retryCompleteInventory(original: inventoryBusy,
        remaining: { 0.25 - inventoryTime }, check: {}, read: {
            inventoryTime = 0.25
            throw failure("deadline_exceeded", "local read window expired")
        }, wait: { inventoryTime += $0 })
    preconditionFailure("Local recovery expiry falsely succeeded")
} catch let error as NativeFailure { surfaceCheck(error.message == inventoryBusy.message) }
private var inventoryChecks = 0
do {
    let _: [Int] = try retryCompleteInventory(original: inventoryBusy, remaining: { 0.75 }, check: {
        inventoryChecks += 1
        if inventoryChecks == 3 { throw failure("app_changed", "after fresh snapshot") }
    }, read: { [7] }, wait: { _ in })
    preconditionFailure("Replaced app published a fresh array")
} catch let error as NativeFailure { surfaceCheck(error.code == "app_changed" && error.message == "after fresh snapshot") }
print("Complete inventory resampling controls passed (typed busy only; caller and original errors retained)")

// A native menu has no AXWindow; only an already proven association can select
// the existing strict visible-pixel fallback. No focus error is generalized.
surfaceCheck(coordinateMenuSurface(role: kAXMenuRole, associated: true))
surfaceCheck(!coordinateMenuSurface(role: kAXMenuRole, associated: false))
for role in [kAXWindowRole, kAXSheetRole, kAXMenuBarRole, kAXGroupRole] {
    surfaceCheck(!coordinateMenuSurface(role: role, associated: true))
}
surfaceCheck(!coordinateMenuSurface(role: nil, associated: true))
sheetFocusCheck(try modalKeyboardRequiresForeground(role: kAXSheetRole) { preconditionFailure("Sheet modal state is structural") })
sheetFocusCheck(try modalKeyboardRequiresForeground(role: kAXWindowRole) { true })
sheetFocusCheck(try !modalKeyboardRequiresForeground(role: kAXWindowRole) { false })
sheetFocusCheck(try !modalKeyboardRequiresForeground(role: kAXMenuRole) { preconditionFailure("Menu must not manufacture AXModal") })
do {
    _ = try modalKeyboardRequiresForeground(role: kAXWindowRole) { nil }
    preconditionFailure("Missing modal state became background support")
} catch let error as NativeFailure { surfaceCheck(error.code == "modal_state_unproven") }
do {
    _ = try modalKeyboardRequiresForeground(role: kAXWindowRole) { throw inventoryBusy }
    preconditionFailure("Typed modal read failure was swallowed")
} catch let error as NativeFailure { surfaceCheck(error.message == inventoryBusy.message && error.attributeFailure?.error == .cannotComplete) }
print("Associated-menu routing and modal keyboard preflight controls passed")

// FilePanel-shaped control: the large directory precedes the filename field.
// The ROI excludes the known native viewport, so no directory child is loaded.
private let filenameROI = CGRect(x: -980, y: 10, width: 200, height: 40)
private let directoryViewport = CGRect(x: -1000, y: 80, width: 500, height: 700)
private let filenameRect = CGRect(x: -950, y: 15, width: 120, height: 24)
private var regionFrontier = [0]
private var regionVisited = 0
private var regionFound: [Int] = []
while let node = regionFrontier.popLast() {
    regionVisited += 1
    switch node {
    case 0: regionFrontier.append(contentsOf: [2, 1])
    case 1:
        if regionExcludesScrollContents(role: kAXScrollAreaRole, bounds: directoryViewport, region: filenameROI) { continue }
        regionFrontier.append(contentsOf: Array(3..<10_003))
    case 2:
        if visibleRegionIntersection(element: filenameRect, region: filenameROI, viewports: []) != nil { regionFound.append(node) }
    default: preconditionFailure("Out-of-region directory contents were traversed")
    }
}
surfaceCheck(regionVisited == 3 && regionFound == [2])
surfaceCheck(!regionExcludesScrollContents(role: kAXGroupRole, bounds: directoryViewport, region: filenameROI))
surfaceCheck(!regionExcludesScrollContents(role: kAXScrollAreaRole, bounds: nil, region: filenameROI))
surfaceCheck(!regionExcludesScrollContents(role: kAXScrollAreaRole, bounds: .zero, region: filenameROI))
surfaceCheck(!regionExcludesScrollContents(role: kAXScrollAreaRole, bounds: filenameRect, region: filenameROI))
surfaceCheck(!regionExcludesScrollContents(role: kAXScrollAreaRole, bounds: CGRect(x: CGFloat.infinity, y: 0, width: 3, height: 3), region: filenameROI))
surfaceCheck(visibleRegionIntersection(element: filenameRect, region: filenameROI, viewports: [directoryViewport]) == nil)
private let viewport = CGRect(x: -1000, y: 0, width: 500, height: 60)
private let regionWindow = CGRect(x: -1000, y: 0, width: 600, height: 900)
private let regionProof = RegionElementProof(region: filenameROI, window: regionWindow,
    element: filenameRect, viewports: [(11, viewport)])
surfaceCheck(visibleRegionIntersection(element: filenameRect, region: filenameROI, viewports: [viewport]) == filenameRect)
surfaceCheck(regionProof.matches(regionProof, equal: ==))
for changed in [
    RegionElementProof(region: filenameROI, window: regionWindow, element: filenameRect.offsetBy(dx: 1, dy: 0), viewports: [(11, viewport)]),
    RegionElementProof(region: filenameROI, window: regionWindow, element: filenameRect, viewports: [(11, viewport.offsetBy(dx: 0, dy: 1))]),
    RegionElementProof(region: filenameROI, window: regionWindow, element: filenameRect, viewports: [(12, viewport)]),
    RegionElementProof(region: filenameROI, window: regionWindow, element: filenameRect, viewports: []),
    RegionElementProof(region: filenameROI, window: regionWindow.offsetBy(dx: 1, dy: 0), element: filenameRect, viewports: [(11, viewport)]),
] { surfaceCheck(!regionProof.matches(changed, equal: ==)) }
print("Visible-region FilePanel shape, unknown/overlapping clipping and retained geometry controls passed")

// A region-selected row can exceed its viewport. The scroll point belongs to
// the proven visible intersection while the full row/window stay in the guard.
private let oversizedRow = CGRect(x: -950, y: -100, width: 120, height: 200)
private let regionScroll = try ScrollAnchorGeometry(element: oversizedRow, window: regionWindow,
                                                    clipping: [filenameROI, viewport])
surfaceCheck(regionScroll.point == CGPoint(x: -890, y: 30))
surfaceCheck(regionScroll.element == oversizedRow && regionScroll.window == regionWindow)
surfaceCheck(filenameROI.contains(regionScroll.point) && viewport.contains(regionScroll.point))
surfaceCheck(!regionScroll.matches(element: oversizedRow.offsetBy(dx: 1, dy: 0), window: regionWindow, point: regionScroll.point))
private let ordinaryScroll = try ScrollAnchorGeometry(element: oversizedRow, window: regionWindow)
surfaceCheck(ordinaryScroll.point == CGPoint(x: -890, y: 50))
print("Region semantic wheel anchor uses visible intersection and retains full geometry proof")


// Same child and CG surface can survive reparenting while the AX window root
// changes. A fresh subtree may use the new root; the old ref cannot derive it.
private let retainedSubtree = RetainedSubtree(element: 7, root: 101)
private var liveSubtreeParent = 101
sheetFocusCheck(try retainedSubtree.checkedElement(in: liveSubtreeParent, equal: ==) == 7)
liveSubtreeParent = 202
do {
    _ = try retainedSubtree.checkedElement(in: liveSubtreeParent, equal: ==)
    preconditionFailure("Reparenting the same child rebound an old subtree ref")
} catch let error as NativeFailure { surfaceCheck(error.code == "stale_observation") }
private let refreshedSubtree = RetainedSubtree(element: 7, root: liveSubtreeParent)
sheetFocusCheck(try refreshedSubtree.checkedElement(in: liveSubtreeParent, equal: ==) == 7)
// Equal retained region geometry does not override the root-object obligation.
private let regionSubtree = RetainedSubtree(element: regionProof, root: 101)
private let survivingRegionProof = try regionSubtree.checkedElement(in: 101, equal: ==)
surfaceCheck(survivingRegionProof.matches(regionProof, equal: ==))
do {
    _ = try regionSubtree.checkedElement(in: 202, equal: ==)
    preconditionFailure("Matching region geometry replaced the original AX root")
} catch let error as NativeFailure { surfaceCheck(error.code == "stale_observation") }
print("Derived subtrees retain their original AX root across reparenting and region proofs")


// Association root A -> B keeps the app/CG owner and CGWindowID but gets a new
// generation. Both caches must be reconciled with that final publication.
private struct CacheControlBinding { let target: NativeTarget; let root: Int }
private let cacheApp = NativeApp(identity: "pure.cache.control", pid: 123, generation: 10)
private let cacheOld = NativeTarget(app: cacheApp, nativeID: 42, generation: 100)
private let cacheNew = NativeTarget(app: cacheApp, nativeID: 42, generation: 101)
private let cacheSibling = NativeTarget(app: cacheApp, nativeID: 43, generation: 102)
private let cacheSlot = WindowSlot(app: cacheApp, id: 42)
private let siblingSlot = WindowSlot(app: cacheApp, id: 43)
private var publishedTargets = [cacheSlot: cacheOld, siblingSlot: cacheSibling]
private var semanticCache = [cacheSlot: CacheControlBinding(target: cacheOld, root: 1),
                            siblingSlot: CacheControlBinding(target: cacheSibling, root: 3)]
private var observationCache = ["old": CacheControlBinding(target: cacheOld, root: 1),
                               "sibling": CacheControlBinding(target: cacheSibling, root: 3)]
publishedTargets[cacheSlot] = cacheNew
semanticCache = currentWindowBindings(semanticCache, windows: publishedTargets) { $0.target }
observationCache = currentWindowBindings(observationCache, windows: publishedTargets) { $0.target }
surfaceCheck(semanticCache[cacheSlot] == nil && observationCache["old"] == nil)
surfaceCheck(semanticCache[siblingSlot]?.target == cacheSibling && observationCache["sibling"]?.target == cacheSibling)
surfaceCheck(publishedTargets[cacheSlot] != cacheOld && publishedTargets[cacheSlot] == cacheNew)
// The freshly discovered root can now populate the cache directly; no second
// observe is needed to clear a stale A object left behind by windows().
semanticCache[cacheSlot] = CacheControlBinding(target: cacheNew, root: 2)
observationCache["fresh"] = CacheControlBinding(target: cacheNew, root: 2)
semanticCache = currentWindowBindings(semanticCache, windows: publishedTargets) { $0.target }
observationCache = currentWindowBindings(observationCache, windows: publishedTargets) { $0.target }
surfaceCheck(semanticCache[cacheSlot]?.root == 2 && observationCache["fresh"]?.root == 2)
surfaceCheck(observationCache["old"] == nil)
print("Association generation publication retires old roots/bindings and admits the fresh target")


// Two direct modal siblings are not a mutual exclusion cycle: only the exact
// focused modal may ignore its sibling. An ordinary window never inherits it.
private func modalPolicy(_ target: Int, focus: Int?, ui: Bool? = nil, direct: Bool = true) throws -> Bool {
    try permitsOtherModal(descendant: false, applicationModal: { true }, activeModalTarget: {
        try permitsActiveModalSibling(targetIsModal: { target == 1 || target == 2 },
            targetIsDirectAppChild: { direct }, exactFocusedRoot: { focus == target },
            focusedUIWithinTarget: { ui })
    })
}
for focused in [1, 2] {
    sheetFocusCheck(try modalPolicy(focused, focus: focused))
    for requested in [1, 2, 3] where requested != focused {
        do {
            _ = try modalPolicy(requested, focus: focused)
            preconditionFailure("A nonfocused/ordinary window inherited modal access")
        } catch let error as NativeFailure { surfaceCheck(error.code == "target_blocked_by_modal") }
    }
}
for (target, focused, ui, direct) in [(3, 3 as Int?, nil as Bool?, true),
                                    (1, nil, nil, true), (1, 1, false, true), (1, 1, nil, false)] {
    do { _ = try modalPolicy(target, focus: focused, ui: ui, direct: direct); preconditionFailure("Incomplete modal proof accepted") }
    catch let error as NativeFailure { surfaceCheck(error.code == "target_blocked_by_modal") }
}
sheetFocusCheck(try modalPolicy(1, focus: 1, ui: true))
sheetFocusCheck(try modalPolicy(1, focus: 1, ui: nil))
do {
    _ = try permitsOtherModal(descendant: true,
        applicationModal: { preconditionFailure("Attached child must reject before sibling inspection") },
        activeModalTarget: { preconditionFailure("Focus cannot override an attached child modal") })
    preconditionFailure("Active parent bypassed a child sheet")
} catch let error as NativeFailure { surfaceCheck(error.code == "target_blocked_by_modal") }
sheetFocusCheck(try !permitsOtherModal(descendant: false, applicationModal: { false },
    activeModalTarget: { preconditionFailure("Another window's attached sheet needs no focus exemption") }))
private var modalFocus = 1
private var focusedChild = 7
private let modalControlParents = [7: 1, 8: 1, 9: 2]
private func currentModalProof() throws -> Bool {
    try permitsActiveModalSibling(targetIsModal: { true }, targetIsDirectAppChild: { true },
        exactFocusedRoot: { modalFocus == 1 }, focusedUIWithinTarget: {
            boundedAncestor(focusedChild, is: 1, equal: ==) { modalControlParents[$0] }
        })
}
sheetFocusCheck(try currentModalProof())
focusedChild = 8 // A different control in the same exact modal is valid.
try recheckActiveModalSibling(true, proof: currentModalProof)
modalFocus = 2
do { try recheckActiveModalSibling(true, proof: currentModalProof); preconditionFailure("Focus changed across modal validation") }
catch let error as NativeFailure { surfaceCheck(error.code == "focus_changed") }
modalFocus = 1; focusedChild = 9
do { try recheckActiveModalSibling(true, proof: currentModalProof); preconditionFailure("Readable contradictory focused UI was ignored") }
catch let error as NativeFailure { surfaceCheck(error.code == "focus_changed") }
try recheckActiveModalSibling(false) { preconditionFailure("Ordinary paths must not add focus reads") }
for original in [inventoryBusy, failure("modal_state_unproven", "malformed typed modal/focus attribute"),
                 failure("native_stopping", "cancelled"), failure("deadline_exceeded", "expired")] {
    do {
        _ = try permitsActiveModalSibling(targetIsModal: { throw original },
            targetIsDirectAppChild: { true }, exactFocusedRoot: { true }, focusedUIWithinTarget: { nil })
        preconditionFailure("A typed proof failure was converted to permission")
    } catch let error as NativeFailure { surfaceCheck(error.code == original.code && error.message == original.message) }
    do { try recheckActiveModalSibling(true) { throw original }; preconditionFailure("Final proof error lost") }
    catch let error as NativeFailure { surfaceCheck(error.code == original.code && error.message == original.message) }
}
print("Focused sibling modal policy preserves descendant blocking and rechecks root/UI evidence")
