// Append after Input.swift + Native.swift. Pure rows only, no desktop/AX reads.
private var executionScopeControlReply: String?
private var executionScopeControlActive = false
private var executionScopeControlChecks = 0
private var executionScopeControlFrees = 0
@_cdecl("dtw_control_execution_scope_check")
func executionScopeCheckControlStub() -> UnsafeMutablePointer<CChar>? {
    executionScopeControlChecks += 1
    return executionScopeControlReply.flatMap { strdup($0) }
}
@_cdecl("dtw_control_execution_scope_free")
func executionScopeFreeControlStub(_ response: UnsafeMutablePointer<CChar>) {
    executionScopeControlFrees += 1
    free(response)
}
@_cdecl("dtw_control_execution_scope_active")
func executionScopeActiveControlStub() -> Bool { executionScopeControlActive }

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
// a reason to discard proven same-process windows. Caller termination,
// permissions and failures of already retained proofs remain strict.
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
for original in [failure("association_unavailable", "unclassified association failure"),
                 failure("ax_unavailable", "untyped failure"),
                 failure("ax_unavailable", "AX API disabled", attributeFailure:
                    AXAttributeFailure(name: kAXParentAttribute, error: .apiDisabled)),
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
private func desktopFocus(_ current: Int, windows: [Int], foreign: Int? = nil) throws -> Bool {
    try windowlessFocusSurface(current, pid: 10, windows: windows,
        owner: { $0 == foreign ? 20 : 10 },
        role: { $0 == 1 || $0 == 2 ? kAXScrollAreaRole : kAXWindowRole }, equal: ==)
}
sheetFocusCheck(try desktopFocus(1, windows: [1]))
sheetFocusCheck(try desktopFocus(1, windows: [1, 2]))
sheetFocusCheck(try !desktopFocus(1, windows: []))
sheetFocusCheck(try !desktopFocus(1, windows: [2]))
sheetFocusCheck(try !desktopFocus(1, windows: [1, 3]))
sheetFocusCheck(try !desktopFocus(3, windows: [3]))
sheetFocusCheck(try !desktopFocus(1, windows: [1], foreign: 1))
sheetFocusCheck(try !desktopFocus(1, windows: [1, 2], foreign: 2))
sheetFocusCheck(try !desktopFocus(1, windows: Array(repeating: 1, count: 129)))
do {
    _ = try windowlessFocusSurface(1, pid: 10, windows: [1], owner: { _ in 10 },
        role: { _ -> String? in throw failure("ax_unavailable", "unreadable desktop") }, equal: ==)
    preconditionFailure("Unreadable desktop was accepted")
} catch let error as NativeFailure { sheetFocusCheck(error.code == "ax_unavailable") }
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
// Activating a sheet raises its parent, but a parent readback alone never
// proves focus of the exact requested sheet.
let sheetParentRaise = try dialogActivation(2)
let requestedSheetExact = try dialogFocused(2, 2)
sheetFocusCheck(sheetParentRaise == 1 && requestedSheetExact)
sheetFocusCheck(try !dialogFocused(2, sheetParentRaise!))
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
print("Sheet activation, exact focus and main-window notification controls passed")

// The resident execution epoch follows app activation, while exact logical
// ancestry permits sheets/popups and rejects an unrelated same-app window.
private var executionEpoch = FocusBorrowEpoch(initialPID: 20, targetPID: 10)
executionEpoch.established(10, window: 100)
executionEpoch.focusedWindow(200)
surfaceCheck(!executionEpoch.invalidated)
private let executionParents = [2: 1, 3: 2, 4: 0, 1: 0, 5: 1]
private func executionFamily(_ surface: Int, owner: (Int) throws -> Int32 = { _ in 10 }) throws -> Bool {
    try executionFocusFamilyMatches(surface, anchor: 1, pid: 10, equal: ==,
                                    owner: owner, parent: { executionParents[$0] })
}
sheetFocusCheck(try executionFamily(1))
sheetFocusCheck(try executionFamily(2))
sheetFocusCheck(try executionFamily(3))
sheetFocusCheck(try executionFamily(5))
sheetFocusCheck(try !executionFamily(4))
sheetFocusCheck(try !executionFamily(2, owner: { $0 == 1 ? 11 : 10 }))
sheetFocusCheck(try !executionFamily(3, owner: { $0 == 2 ? 11 : 10 }))
sheetFocusCheck(try !executionFocusFamilyMatches(3, anchor: 1, pid: 10, equal: ==,
    owner: { _ in 10 }, parent: { $0 == 3 ? 2 : 3 }))
for code in ["native_stopping", "deadline_exceeded", "ax_unavailable"] {
    do {
        _ = try executionFocusFamilyMatches(3, anchor: 1, pid: 10, equal: ==,
            owner: { _ in 10 }, parent: { _ -> Int? in throw failure(code, "unreadable execution ancestry") })
        preconditionFailure("Unknown ancestry authorized execution focus")
    } catch let error as NativeFailure { surfaceCheck(error.code == code) }
}
executionEpoch.activated(20)
executionEpoch.activated(10)
surfaceCheck(executionEpoch.invalidated)
surfaceCheck(!executionEpoch.retarget(10)) // Leave/return cannot create another borrow.
private var siblingEpoch = FocusBorrowEpoch(initialPID: 20, targetPID: 10)
siblingEpoch.established(10, window: 100)
if try !executionFamily(4) { siblingEpoch.invalidate() }
siblingEpoch.established(10, window: 100)
surfaceCheck(siblingEpoch.invalidated) // Same-app sibling leave/return is sticky too.

// Restoration validates birth before and after the exact retained window read.
// Reused PIDs and changed roots never reach the activation boundary.
private var restoreWindowReads = 0
sheetFocusCheck(try !executionRestoreIdentityCurrent(pid: 20, generation: 700,
    birth: { _ in 701 }, windowMatches: { restoreWindowReads += 1; return true }))
surfaceCheck(restoreWindowReads == 0)
sheetFocusCheck(try !executionRestoreIdentityCurrent(pid: 20, generation: 700,
    birth: { _ in 700 }, windowMatches: { false }))
private var restoreBirthReads = 0
sheetFocusCheck(try !executionRestoreIdentityCurrent(pid: 20, generation: 700, birth: { _ in
    restoreBirthReads += 1; return restoreBirthReads == 1 ? 700 : 701
}, windowMatches: { true }))
surfaceCheck(restoreBirthReads == 2)
sheetFocusCheck(try executionRestoreIdentityCurrent(pid: 20, generation: 700,
    birth: { _ in 700 }, windowMatches: { true }))
do {
    _ = try executionRestoreIdentityCurrent(pid: 20, generation: 700, birth: { _ in 700 },
        windowMatches: { throw failure("ax_unavailable", "Original exact root cannot be proven") })
    preconditionFailure("Unknown prior window permitted restoration")
} catch let error as NativeFailure { surfaceCheck(error.code == "ax_unavailable") }
print("Execution focus modal family, irreversible external invalidation and PID-birth restoration controls passed")

// Each payload guard reads anew, preserving denial details and freeing every
// producer-owned JSON response. Unknown/malformed proof cannot authorize input.
try admitExecutionScope()
try admitExecutionScope()
surfaceCheck(executionScopeControlChecks == 2 && executionScopeControlFrees == 0)
executionScopeControlReply = "{\"error\":\"focus_changed\",\"message\":\"Resident epoch invalidated\"}"
do {
    try admitExecutionScope()
    preconditionFailure("Resident epoch denial admitted another payload")
} catch let error as NativeFailure {
    surfaceCheck(error.code == "focus_changed" && error.message == "Resident epoch invalidated")
}
surfaceCheck(executionScopeControlChecks == 3 && executionScopeControlFrees == 1)
for reply in ["{invalid", "{}", "{\"error\":\"focus_changed\"}"] {
    executionScopeControlReply = reply
    do { try admitExecutionScope(); preconditionFailure("Malformed guard evidence admitted input") }
    catch let error as NativeFailure { surfaceCheck(error.code == "execution_scope_unavailable") }
}
surfaceCheck(executionScopeControlChecks == 6 && executionScopeControlFrees == 4)
executionScopeControlReply = nil
surfaceCheck(!executionScopeActive())
executionScopeControlActive = true
surfaceCheck(executionScopeActive())
executionScopeControlActive = false

// Only confirmed exact lifecycle outcomes retire the anchor. Neither an
// unknown quit receipt nor closing a descendant popup has terminal authority.
surfaceCheck(confirmedExecutionTerminal(operation: "quit_app", evidence: "confirmed", postcondition: "application_exited",
    sameApp: true, closedWindow: nil, anchorWindow: 100) == "application_exited")
surfaceCheck(confirmedExecutionTerminal(operation: "close_window", evidence: "confirmed", postcondition: "window_closed",
    sameApp: true, closedWindow: 100, anchorWindow: 100) == "window_closed")
for evidence in [nil, "unverified", "unknown"] as [String?] {
    surfaceCheck(confirmedExecutionTerminal(operation: "quit_app", evidence: evidence, postcondition: "application_exited",
        sameApp: true, closedWindow: nil, anchorWindow: 100) == nil)
}
surfaceCheck(confirmedExecutionTerminal(operation: "quit_app", evidence: "confirmed", postcondition: "application_exited",
    sameApp: false, closedWindow: nil, anchorWindow: 100) == nil)
surfaceCheck(confirmedExecutionTerminal(operation: "close_window", evidence: "confirmed", postcondition: "window_closed",
    sameApp: true, closedWindow: 200, anchorWindow: 100) == nil)
surfaceCheck(confirmedExecutionTerminal(operation: "quit_app", evidence: "confirmed", postcondition: "application_running",
    sameApp: true, closedWindow: nil, anchorWindow: 100) == nil)
private let terminalRestored = settledExecutionTerminal("application_exited", originalCurrent: { true })
surfaceCheck(terminalRestored["focus"] as? String == "restored" && terminalRestored["error"] == nil)
private let terminalForeign = settledExecutionTerminal("window_closed", originalCurrent: { false })
surfaceCheck(terminalForeign["focus"] as? String == "changed_externally" && terminalForeign["terminal"] as? String == "window_closed")
private let terminalUnknown = settledExecutionTerminal("application_exited", originalCurrent: {
    throw failure("focus_unproven", "Original exact focus cannot be proven")
})
surfaceCheck(terminalUnknown["focus"] as? String == "changed_externally" && terminalUnknown["error"] == nil)
surfaceCheck(terminalUnknown["terminal_focus"] as? String == "unproven")
print("Execution payload guard replies and confirmed quit/root-close settlement passed; no AX or GUI reads")

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

// A real cold Electron shape: a nested container shell and only typed native
// window buttons. It must not become ready simply by adding more group levels.
private var coldElectronShell = SemanticReadinessSample()
coldElectronShell.add(role:kAXWindowRole,children:4,depth:0,parentRole:"")
for depth in 1...5 {
    coldElectronShell.add(role:kAXGroupRole,children:depth == 5 ? 2 : 1,depth:depth,
                          parentRole:depth == 1 ? kAXWindowRole : kAXGroupRole)
}
for _ in 0..<2 { coldElectronShell.add(role:kAXGroupRole,children:0,depth:6,parentRole:kAXGroupRole) }
for subrole in [kAXCloseButtonSubrole,kAXMinimizeButtonSubrole,kAXZoomButtonSubrole,kAXFullScreenButtonSubrole] {
    surfaceCheck(semanticWindowDecoration(role:kAXButtonRole,subrole:subrole))
    coldElectronShell.add(role:kAXButtonRole,children:0,depth:1,parentRole:kAXWindowRole,
                          decoration:semanticWindowDecoration(role:kAXButtonRole,subrole:subrole))
}
coldElectronShell.add(role:kAXGroupRole,children:0,depth:2,parentRole:kAXButtonRole,decoration:true)
surfaceCheck(coldElectronShell.shellOnly)
surfaceCheck(coldElectronShell.decision(manualOptIn:true,elapsed:0.007,materializationRemaining:2.993) == .pending)
// The final read can complete just as the original attempt expires. Keep the
// elapsed attempt in the decision rather than reclassifying the shell as ready.
for now in [13.0,13.001] {
    surfaceCheck(coldElectronShell.decision(manualOptIn:true,elapsed:now-10,
        materializationRemaining:13-now) == .emptyContent)
}
surfaceCheck(coldElectronShell.decision(manualOptIn:false,elapsed:0.007,materializationRemaining:2.993) == .ready)
surfaceCheck(coldElectronShell.decision(manualOptIn:true,elapsed:0.007,materializationRemaining:2.993,scanComplete:false) == .ready)
private var materializedElectron = coldElectronShell
materializedElectron.add(role:"AXWebArea",children:9,depth:7,parentRole:kAXGroupRole)
surfaceCheck(materializedElectron.decision(manualOptIn:true,elapsed:2.3,materializationRemaining:2.993) == .ready)
for role in [kAXButtonRole,kAXTextFieldRole,kAXStaticTextRole] {
    var nativeContent = coldElectronShell
    nativeContent.add(role:role,children:0,depth:7,parentRole:kAXGroupRole)
    surfaceCheck(!nativeContent.shellOnly)
    surfaceCheck(nativeContent.decision(manualOptIn:true,elapsed:0.007,materializationRemaining:2.993) == .ready)
}
surfaceCheck(!semanticWindowDecoration(role:kAXButtonRole,subrole:nil))
surfaceCheck(!semanticWindowDecoration(role:kAXButtonRole,subrole:"AXUnknown"))
surfaceCheck(!semanticWindowDecoration(role:kAXButtonRole,subrole:"AXToolbarButton"))
surfaceCheck(!semanticWindowDecoration(role:kAXTextFieldRole,subrole:kAXCloseButtonSubrole))
surfaceCheck(semanticMaterializationUntil(attemptedAt:10,now:10.007) == 13)
surfaceCheck(semanticMaterializationUntil(attemptedAt:10,now:12.3) == 13)
surfaceCheck(semanticMaterializationUntil(attemptedAt:10,now:13) == nil)
surfaceCheck(semanticMaterializationUntil(attemptedAt:nil,now:10) == nil)
// A later query retains the expired attempt for classification, but has no
// wait budget. The same shell cannot become complete merely by getting older.
private let cachedManualUntil = 10.0 + semanticReadinessBudget
for queriedAt in [13.0,14.0,100.0] {
    surfaceCheck(semanticMaterializationUntil(attemptedAt:10,now:queriedAt) == nil)
    surfaceCheck(coldElectronShell.decision(manualOptIn:true,elapsed:0.007,
        materializationRemaining:cachedManualUntil-queriedAt) == .emptyContent)
    var cachedNativeContent = coldElectronShell
    cachedNativeContent.add(role:kAXTextFieldRole,children:0,depth:7,parentRole:kAXGroupRole)
    surfaceCheck(cachedNativeContent.decision(manualOptIn:true,elapsed:0.007,
        materializationRemaining:cachedManualUntil-queriedAt) == .ready)
}
print("Cold accepted Manual shell waits without repoking; native content and fixed attempt budgets remain usable")

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
// Missing input focus remains a typed refusal; cleanup's fresh ownership
// branch has its own controls and does not reinterpret input failures.
for absent in [FocusAbsence.application, .window] {
    focusPhaseCheck(missingFocus(absent).json.count == 2, "Typed focus absence remains private")
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
private let recoveredInventory: [Int] = try retryCompleteAXRead(original: inventoryBusy,
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
    let _: [Int] = try retryCompleteAXRead(original: inventoryBusy,
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
        let _: [Int] = try retryCompleteAXRead(original: inventoryBusy, remaining: { 0.75 },
            check: { throw failure(code, "caller cause") },
            read: { preconditionFailure("Read continued after caller stop") }, wait: { _ in })
        preconditionFailure("Caller error lost")
    } catch let error as NativeFailure { surfaceCheck(error.code == code && error.message == "caller cause") }
}
for original in [failure("ax_unavailable", "target object expired", attributeFailure: AXAttributeFailure(name: kAXRoleAttribute, error: .invalidUIElement)),
                 failure("ax_unavailable", "untyped failure"), failure("target_changed", "target changed")] {
    do {
        let _: [Int] = try retryCompleteAXRead(original: inventoryBusy, remaining: { 0.75 },
            check: {}, read: { throw original }, wait: { _ in })
        preconditionFailure("A non-busy error was retried")
    } catch let error as NativeFailure { surfaceCheck(error.code == original.code && error.message == original.message) }
}
inventoryTime = 0
do {
    let _: [Int] = try retryCompleteAXRead(original: inventoryBusy,
        remaining: { 0.25 - inventoryTime }, check: {}, read: {
            inventoryTime = 0.25
            throw failure("deadline_exceeded", "local read window expired")
        }, wait: { inventoryTime += $0 })
    preconditionFailure("Local recovery expiry falsely succeeded")
} catch let error as NativeFailure { surfaceCheck(error.message == inventoryBusy.message) }
private var inventoryChecks = 0
do {
    let _: [Int] = try retryCompleteAXRead(original: inventoryBusy, remaining: { 0.75 }, check: {
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
sheetFocusCheck(try modalInputRequiresForeground(role: kAXSheetRole) { preconditionFailure("Sheet modal state is structural") })
sheetFocusCheck(try modalInputRequiresForeground(role: kAXWindowRole) { true })
sheetFocusCheck(try !modalInputRequiresForeground(role: kAXWindowRole) { false })
sheetFocusCheck(try !modalInputRequiresForeground(role: kAXMenuRole) { preconditionFailure("Menu must not manufacture AXModal") })
do {
    _ = try modalInputRequiresForeground(role: kAXWindowRole) { nil }
    preconditionFailure("Missing modal state became background support")
} catch let error as NativeFailure { surfaceCheck(error.code == "modal_state_unproven") }
do {
    _ = try modalInputRequiresForeground(role: kAXWindowRole) { throw inventoryBusy }
    preconditionFailure("Typed modal read failure was swallowed")
} catch let error as NativeFailure { surfaceCheck(error.message == inventoryBusy.message && error.attributeFailure?.error == .cannotComplete) }
print("Associated-menu routing and modal keyboard restriction controls passed")

// A tracking/cursor need cannot authorize activation. Exercise both admission
// checks (already foreground, then lost foreground) and unreadable native proof
// with an activation counter; no desktop or AX access is involved.
private var activationAttempts = 0
if try activationRequired(allowed: false, isForeground: { true }) { activationAttempts += 1 }
surfaceCheck(activationAttempts == 0)
do {
    if try activationRequired(allowed: false, isForeground: { false }) { activationAttempts += 1 }
    preconditionFailure("Inactive hover gained activation permission")
} catch let error as NativeFailure { surfaceCheck(error.code == "foreground_required") }
surfaceCheck(activationAttempts == 0)
for allowed in [false, true] {
    for code in ["focus_unproven", "ax_unavailable", "modal_state_unproven"] {
        do {
            if try activationRequired(allowed: allowed, isForeground: {
                throw failure(code, "original unreadable proof")
            }) { activationAttempts += 1 }
            preconditionFailure("Unreadable proof gained activation permission")
        } catch let error as NativeFailure {
            surfaceCheck(error.code == code && error.message == "original unreadable proof")
        }
    }
}
surfaceCheck(activationAttempts == 0)
if try activationRequired(allowed: true, isForeground: { false }) { activationAttempts += 1 }
surfaceCheck(activationAttempts == 1)
print("Foreground admission controls passed (inactive hover and unreadable proof never activate)")

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


// Exact scene bootstrap cannot spend the selected app's budget on foreign
// association discovery. Only a selected foreign surface needs that proof.
private let ownSelected: (record: CGRecord, title: String, association: (birth: UInt64, root: Int)?) =
    try selectedSurfaceCandidate([surfaceRow(90)], id: 90, pid: 10,
        birth: { _ in preconditionFailure("Own app birth belongs to caller verification") },
        associate: { _ in preconditionFailure("Own surface must never probe foreign AX/SCK") },
        refresh: { _ in preconditionFailure("Own exact lookup must not scan a second inventory") })
surfaceCheck(ownSelected.record.id == 90 && ownSelected.association == nil)
private var selectedBirthPIDs: [Int32] = []
private var selectedAssociatedIDs: [CGWindowID] = []
private var selectedRefreshIDs: [CGWindowID] = []
private let foreignSelected = try selectedSurfaceCandidate([surfaceRow(91, pid: 20)], id: 91, pid: 10,
    birth: { selectedBirthPIDs.append($0); return 700 },
    associate: { selectedAssociatedIDs.append($0.id); return 901 },
    refresh: { selectedRefreshIDs.append($0); return [surfaceRow(91, pid: 20)] })
surfaceCheck(foreignSelected.record.id == 91 && foreignSelected.association?.root == 901 && foreignSelected.association?.birth == 700)
surfaceCheck(selectedBirthPIDs == [20,20] && selectedAssociatedIDs == [91] && selectedRefreshIDs == [91])

private func selectedFailure(_ code: String, _ read: () throws -> Void) {
    do { try read(); preconditionFailure("Selected surface failure was accepted") }
    catch let error as NativeFailure { surfaceCheck(error.code == code) }
    catch { preconditionFailure("Unexpected selected surface error") }
}
for (rows, expectedCode) in [([], "target_changed"), ([surfaceRow(92)], "target_changed"),
                           ([surfaceRow(91),surfaceRow(91)], "window_unproven"),
                           ([invalid], "window_unproven")] {
    selectedFailure(expectedCode) {
        let _: (CGRecord, String, (UInt64, Int)?) = try selectedSurfaceCandidate(rows, id: 91, pid: 10,
            birth: { _ in preconditionFailure("Invalid exact rows must fail before birth lookup") },
            associate: { _ in preconditionFailure("Invalid exact rows must fail before association") },
            refresh: { _ in preconditionFailure("Invalid exact rows must fail before refresh") })
    }
}
for births: [UInt64?] in [[nil], [700,701], [700,nil]] {
    var index = 0
    selectedFailure("stale_window") {
        _ = try selectedSurfaceCandidate([surfaceRow(91, pid: 20)], id: 91, pid: 10,
            birth: { _ in defer { index += 1 }; return births[index] },
            associate: { _ in 901 }, refresh: { _ in [surfaceRow(91, pid: 20)] })
    }
}
selectedFailure("stale_window") {
    let _: (CGRecord, String, (UInt64, Int)?) = try selectedSurfaceCandidate([surfaceRow(91, pid: 20)], id: 91, pid: 10,
        birth: { _ in 700 }, associate: { _ in nil },
        refresh: { _ in preconditionFailure("Unproven relation cannot publish a selected target") })
}
private var hiddenSelectedRow = surfaceRow(91, pid: 20)
hiddenSelectedRow[kCGWindowIsOnscreen as String] = false
for rows in [[], [surfaceRow(91, pid: 21)], [surfaceRow(91, pid: 20, width: 101)], [hiddenSelectedRow]] {
    selectedFailure("target_changed") {
        _ = try selectedSurfaceCandidate([surfaceRow(91, pid: 20)], id: 91, pid: 10,
            birth: { _ in 700 }, associate: { _ in 901 }, refresh: { _ in rows })
    }
}
for code in ["native_stopping", "deadline_exceeded", "permission_denied", "association_unavailable"] {
    selectedFailure(code) {
        let _: (CGRecord, String, (UInt64, Int)?) = try selectedSurfaceCandidate([surfaceRow(91, pid: 20)], id: 91, pid: 10,
            birth: { _ in 700 }, associate: { _ in throw failure(code, "selected association failed") },
            refresh: { _ in preconditionFailure("Selected proof failure cannot fall back to another surface") })
    }
}
// Repeated binds always require a new association proof, even for the same ID.
selectedFailure("stale_window") {
    let _: (CGRecord, String, (UInt64, Int)?) = try selectedSurfaceCandidate([surfaceRow(91, pid: 20)], id: 91, pid: 10,
        birth: { _ in 700 }, associate: { _ in nil }, refresh: { _ in [] })
}
print("Selected surface bootstrap skips foreign probes for own windows and re-proves only the requested association")

// Scoped pixel proof keeps the app/actual owner/point identity across the
// helper boundary. The callbacks below allocate JSON only; no native UI work.
private let proofApp = NativeApp(identity: "pure.associated.proof", pid: 10, generation: 50)
private let proofRequest: [String: Any] = ["operation": "prove_associated_surface", "app": proofApp.json,
    "surface_owner": ["pid": 20, "generation": 700], "window_id": 91,
    "bounds": [0.0,0.0,100.0,100.0], "screen_x": 40.0, "screen_y": 50.0]
private let proofSnapshot = try AssociatedPointSnapshot(proofRequest)
private var proofAppChecks = 0
private var proofOwnerReads: [Int32] = []
try proofSnapshot.validate(checkApp: { app in surfaceCheck(app == proofApp); proofAppChecks += 1 },
    birth: { proofOwnerReads.append($0); return 700 },
    read: { id in surfaceCheck(id == 91); return [surfaceRow(91, pid: 20)] })
surfaceCheck(proofAppChecks == 2 && proofOwnerReads == [20,20])
for rows in [[], [surfaceRow(92, pid: 20)], [surfaceRow(91, pid: 21)],
             [surfaceRow(91, pid: 20, width: 101)], [hiddenSelectedRow]] {
    selectedFailure("target_changed") {
        try proofSnapshot.validate(checkApp: { _ in }, birth: { _ in 700 }, read: { _ in rows })
    }
}
for before: UInt64? in [nil, 701] {
    selectedFailure("target_changed") {
        try proofSnapshot.validate(checkApp: { _ in }, birth: { _ in before },
            read: { _ in preconditionFailure("Unproven owner cannot authorize a surface read") })
    }
}
private var proofBirthReads = 0
selectedFailure("target_changed") {
    try proofSnapshot.validate(checkApp: { _ in }, birth: { _ in
        proofBirthReads += 1; return proofBirthReads == 1 ? 700 : 701
    }, read: { _ in [surfaceRow(91, pid: 20)] })
}
proofAppChecks = 0
selectedFailure("app_changed") {
    try proofSnapshot.validate(checkApp: { _ in
        proofAppChecks += 1
        if proofAppChecks == 2 { throw failure("app_changed", "Logical app changed across exact row read") }
    }, birth: { _ in 700 }, read: { _ in [surfaceRow(91, pid: 20)] })
}
private var outsideProofRequest = proofRequest
outsideProofRequest["screen_x"] = 101.0
selectedFailure("invalid_point") { _ = try AssociatedPointSnapshot(outsideProofRequest) }
private var invalidProofOwner = proofRequest
invalidProofOwner["surface_owner"] = ["pid":20,"generation":0]
selectedFailure("invalid_request") { _ = try AssociatedPointSnapshot(invalidProofOwner) }
for code in ["native_stopping", "deadline_exceeded", "target_changed"] {
    var checks = 0
    selectedFailure(code) {
        _ = try checkedAssociatedPoint(check: {
            checks += 1
            if checks == 2 { throw failure(code, "Proof expired before publication") }
        }, prove: { true })
    }
    surfaceCheck(checks == 2)
}
private var falseProofChecks = 0
private let falseProofResult = try checkedAssociatedPoint(check: { falseProofChecks += 1 }, prove: { false })
surfaceCheck(!falseProofResult && falseProofChecks == 2)

private var proofReply = "{\"proven\":true}"
private var proofCallbackCalls = 0
private var proofCallbackFrees = 0
private func controlAssociationCallback(_ input: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>? {
    proofCallbackCalls += 1
    let value = try! JSONSerialization.jsonObject(with: Data(String(cString: input).utf8)) as! [String: Any]
    surfaceCheck(value["operation"] as? String == "prove_associated_surface")
    surfaceCheck(value["window_id"] as? Int == 91)
    return strdup(proofReply)
}
private func controlAssociationRelease(_ value: UnsafeMutablePointer<CChar>) {
    proofCallbackFrees += 1; free(value)
}
private let controlAssociationProver = AssociatedSurfaceProver()
selectedFailure("association_unavailable") { _ = try controlAssociationProver.prove(proofRequest) }
controlAssociationProver.set(controlAssociationCallback, nil)
selectedFailure("association_unavailable") { _ = try controlAssociationProver.prove(proofRequest) }
surfaceCheck(proofCallbackCalls == 0 && proofCallbackFrees == 0)
controlAssociationProver.set(controlAssociationCallback, controlAssociationRelease)
private let callbackTrue = try controlAssociationProver.prove(proofRequest)
surfaceCheck(callbackTrue && proofCallbackCalls == 1 && proofCallbackFrees == 1)
proofReply = "{\"proven\":false}"
private let callbackFalse = try controlAssociationProver.prove(proofRequest)
surfaceCheck(!callbackFalse && proofCallbackCalls == 2 && proofCallbackFrees == 2)
for code in ["native_scene_busy", "deadline_exceeded", "native_stopping", "target_changed", "shutdown_failed"] {
    proofReply = "{\"error\":\"\(code)\",\"message\":\"original helper cause\"}"
    selectedFailure(code) { _ = try controlAssociationProver.prove(proofRequest) }
}
for reply in ["{}", "{\"proven\":1}", "{\"proven\":true,\"cleanup_error\":{\"error\":\"shutdown_failed\"}}", "broken"] {
    proofReply = reply
    selectedFailure("invalid_response") { _ = try controlAssociationProver.prove(proofRequest) }
}
surfaceCheck(proofCallbackCalls == proofCallbackFrees)
controlAssociationProver.set(nil, nil)
selectedFailure("association_unavailable") { _ = try controlAssociationProver.prove(proofRequest) }
print("Scoped association proof requires its helper hook, retains exact identities and releases every producer-owned reply")

// Only the resident guard of an already leased Scene skips pixel acquisition.
// It still requires a fresh matching root and the original owner birth twice.
private var ordinaryPixelReads = 0
private let ordinaryPixelProof = try AssociationProofScope.full.provePixels {
    ordinaryPixelReads += 1; return true
}
surfaceCheck(ordinaryPixelProof && ordinaryPixelReads == 1)
private let ordinaryPixelFailure = try AssociationProofScope.full.provePixels {
    ordinaryPixelReads += 1; return false
}
surfaceCheck(!ordinaryPixelFailure && ordinaryPixelReads == 2)
private let guardedPixelProof = try AssociationProofScope.sceneScrollGuard.provePixels {
    preconditionFailure("Resident guard attempted SCK while its Scene caller holds the scene lease")
}
surfaceCheck(guardedPixelProof)
selectedFailure("association_unavailable") {
    _ = try AssociationProofScope.full.provePixels { throw failure("association_unavailable", "original point proof error") }
}
private var guardRootReads = 0
private let retainedGuardRoot = try retainedAssociationMatches(901, ownerBirth: 700,
    birth: { 700 }, root: { guardRootReads += 1; return 901 }, equal: ==)
surfaceCheck(retainedGuardRoot && guardRootReads == 1)
for current: Int? in [nil, 902] {
    let matches = try retainedAssociationMatches(901, ownerBirth: 700, birth: { 700 }, root: { current }, equal: ==)
    surfaceCheck(!matches)
}
for stamps: [UInt64?] in [[nil], [701], [700,nil], [700,701]] {
    var index = 0
    let matches = try retainedAssociationMatches(901, ownerBirth: 700, birth: {
        defer { index += 1 }; return stamps[index]
    }, root: { 901 }, equal: ==)
    surfaceCheck(!matches)
}
selectedFailure("ax_unavailable") {
    _ = try retainedAssociationMatches(901, ownerBirth: 700, birth: { 700 }, root: { () -> Int? in
        throw failure("ax_unavailable", "Fresh retained relation is unreadable",
                      attributeFailure: AXAttributeFailure(name: "AXElementAtPosition", error: .cannotComplete))
    }, equal: ==)
}
// Existing geometry scope has no permission to accept a moved associated
// surface merely because its retained AX object continues to exist.
private let guardedAnchorGeometry = try ScrollAnchorGeometry(element: CGRect(x: 10, y: 10, width: 20, height: 20),
    window: CGRect(x: 0, y: 0, width: 100, height: 100))
surfaceCheck(!guardedAnchorGeometry.matches(element: CGRect(x: 10, y: 10, width: 20, height: 20),
    window: CGRect(x: 1, y: 0, width: 100, height: 100), point: guardedAnchorGeometry.point))
print("Resident scroll guard preserves fresh root/birth/geometry and cannot acquire the caller's scene lease")

// AXWindowID cannotComplete is typed private evidence, never inferred from
// the public message. No failed read is an identity or permission to dispatch.
private func identityReadFailure(_ result: AXError, id: CGWindowID = 0) -> NativeFailure {
    do {
        _ = try requireAXWindowID(result, id)
        preconditionFailure("Unproven identity unexpectedly accepted")
    } catch let error as NativeFailure { return error }
    catch { preconditionFailure("Unexpected identity error") }
}
private let typedBusyIdentity = identityReadFailure(.cannotComplete)
surfaceCheck(typedBusyIdentity.code == "window_unproven")
surfaceCheck(typedBusyIdentity.attributeFailure?.name == "AXWindowID"
    && typedBusyIdentity.attributeFailure?.error == .cannotComplete)
surfaceCheck(Set(typedBusyIdentity.json.keys) == Set(["error", "message"]))
private let provenIdentityID = try requireAXWindowID(.success, 777)
surfaceCheck(provenIdentityID == 777)
for result in [AXError.success, .apiDisabled, .invalidUIElement, .failure] {
    let error = identityReadFailure(result)
    surfaceCheck(error.code == "window_unproven" && error.attributeFailure == nil)
}
print("Exact window identity rejects zero IDs and native failures without treating them as absence")

// Telegram can return AXFailure for AXParent while its WindowServer window
// stays usable. Exercise every stage of the optional proof and retain exact
// native diagnostics without reusing an earlier proof or authorizing a surface.
for attribute in ["AXElementAtPosition", kAXParentAttribute, kAXRoleAttribute, "AXPID", "AXMessagingTimeout"] {
    for result in [AXError.failure, .cannotComplete, .invalidUIElement] {
        let original = failure("ax_unavailable", "original optional AX cause",
            attributeFailure: AXAttributeFailure(name: attribute, error: result))
        let (owned, _) = try surfaceInventory([surfaceRow(10), surfaceRow(11)], pid: 10)
        var checks = 0
        let proof: (Int?, Int32?) = try unprovenAssociationProbe(check: { checks += 1 }, read: { throw original })
        surfaceCheck(owned.map { $0.0.id } == [10, 11] && proof.0 == nil
            && proof.1 == result.rawValue && checks == 1)
        // This same AX failure is fatal when checking an existing association.
        do {
            _ = try retainedAssociationMatches(901, ownerBirth: 700, birth: { 700 },
                root: { () -> Int? in throw original }, equal: ==)
            preconditionFailure("A retained surface lost its ownership guard")
        } catch let error as NativeFailure {
            surfaceCheck(error.code == original.code && error.message == original.message
                && error.attributeFailure?.name == attribute && error.attributeFailure?.error == result)
        }
        for stop in ["app_changed", "native_stopping", "deadline_exceeded", "permission_required"] {
            do {
                let _: (Int?, Int32?) = try unprovenAssociationProbe(check: { throw failure(stop, "original hard cause") },
                    read: { throw original })
                preconditionFailure("Optional degradation masked caller/permission/identity failure")
            } catch let error as NativeFailure { surfaceCheck(error.code == stop && error.message == "original hard cause") }
        }
    }
}
print("Optional AX failure/busy/expired ancestry preserves owned CG surfaces; retained ownership and hard errors remain strict")

// Scoped waits must use exact native absence, not incomplete app inventory,
// visibility, zero-area animation, title matching, or a different owner.
let waitAbsent = try exactWindowPresent([], id: 90, owner: 10)
surfaceCheck(!waitAbsent)
let waitZeroPresent = try exactWindowPresent([surfaceRow(90, width: 0)], id: 90, owner: 10)
surfaceCheck(waitZeroPresent)
var waitHidden = surfaceRow(90)
waitHidden[kCGWindowIsOnscreen as String] = false
let waitHiddenPresent = try exactWindowPresent([waitHidden], id: 90, owner: 10)
surfaceCheck(waitHiddenPresent)
for rows in [[surfaceRow(90, pid: 11)], [surfaceRow(91)], [surfaceRow(90), surfaceRow(90)], [[:]]] {
    do { _ = try exactWindowPresent(rows, id: 90, owner: 10); preconditionFailure("Unproven absence accepted") }
    catch let error as NativeFailure { surfaceCheck(["target_changed", "window_inventory_unproven"].contains(error.code)) }
}
// Nested sheets choose the leaf; unrelated siblings remain two candidates.
let waitParents: [Int: Int] = [2: 1, 3: 2, 4: 1, 5: 0]
let waitEqual: (Int, Int) -> Bool = { $0 == $1 }
let waitParent: (Int) -> Int? = { waitParents[$0] }
let waitDescendant: (Int, Int) -> Bool = { child, parent in
    boundedAncestor(child, is: parent, equal: waitEqual, parent: waitParent)
}
let waitTerminal = terminalModalRoots([2, 3], equal: waitEqual, descendant: waitDescendant)
surfaceCheck(waitTerminal == [3])
let waitAmbiguous = terminalModalRoots([3, 4], equal: waitEqual, descendant: waitDescendant)
surfaceCheck(waitAmbiguous == [3, 4])
do {
    _ = try terminalModalRoots([2, 3], equal: ==) { _, _ -> Bool in
        throw failure("permission_denied", "original ancestry permission failure")
    }
    preconditionFailure("Ancestry failure became an empty dialog list")
} catch let error as NativeFailure { surfaceCheck(error.code == "permission_denied") }
surfaceCheck(windowProbePending(failure("ax_unavailable", "busy", attributeFailure: AXAttributeFailure(name: "AXWindows", error: .cannotComplete))) == "ax_busy")
for error in [failure("permission_denied", "original"), failure("app_changed", "birth changed"),
              failure("native_stopping", "cancelled"),
              failure("ax_unavailable", "expired target", attributeFailure: AXAttributeFailure(name: "AXRole", error: .invalidUIElement))] {
    surfaceCheck(windowProbePending(error) == nil)
}
print("Scoped window condition contracts passed (no AX or GUI reads)")

let waitDeadline = try windowProbeFailureReason(failure("deadline_exceeded", "original deadline"), checkCaller: {})
surfaceCheck(waitDeadline == nil)
let waitHard = try windowProbeFailureReason(failure("permission_denied", "original permission"), checkCaller: {})
surfaceCheck(waitHard == nil)
for code in ["deadline_exceeded", "native_stopping", "app_changed"] {
    do {
        _ = try windowProbeFailureReason(failure("ax_unavailable", "busy", attributeFailure: AXAttributeFailure(name: "AXWindows", error: .cannotComplete)),
            checkCaller: { throw failure(code, "original caller failure") })
        preconditionFailure("Pending window read masked original caller failure")
    } catch let error as NativeFailure { surfaceCheck(error.code == code && error.message == "original caller failure") }
}

// Closing the final window or an associated service can end its process. The
// same helper guards BOTH identities; absence still needs the exact CG query.
for expected in [UInt64(100), UInt64(200)] {
    var checkedLive = false
    try verifyWindowWaitProcess(expected: expected, stamp: expected, gone: { false }, verifyLive: { checkedLive = true })
    surfaceCheck(checkedLive)
    try verifyWindowWaitProcess(expected: expected, stamp: nil, gone: { true }, verifyLive: { preconditionFailure("Exited process was verified live") })
    for stamp in [UInt64?.none, expected + 1] {
        do {
            try verifyWindowWaitProcess(expected: expected, stamp: stamp, gone: { false }, verifyLive: {})
            preconditionFailure("Missing or reused process identity accepted")
        } catch let error as NativeFailure { surfaceCheck(error.code == "app_changed") }
    }
    do {
        try verifyWindowWaitProcess(expected: expected, stamp: expected, gone: { true },
            verifyLive: { throw failure("permission_denied", "original permission") })
        preconditionFailure("Process exit hid a permission failure")
    } catch let error as NativeFailure { surfaceCheck(error.code == "permission_denied") }
}
print("Closed-window waits require original process identity or positive death plus exact CG absence")

surfaceCheck(windowProbePending(failure("window_unproven", "busy exact id", attributeFailure: AXAttributeFailure(name: "AXWindowID", error: .cannotComplete))) == "ax_busy")
surfaceCheck(windowProbePending(failure("window_unproven", "zero exact id", attributeFailure: AXAttributeFailure(name: "AXWindowID", error: .success))) == nil)

private let waitGrowingSheet = try readyModalSurface([surfaceRow(90, width: 0)], id: 90)
private let waitHiddenSheet = try readyModalSurface([waitHidden], id: 90)
private let waitVisibleSheet = try readyModalSurface([surfaceRow(90)], id: 90)
surfaceCheck(waitGrowingSheet == nil && waitHiddenSheet == nil && waitVisibleSheet?.id == 90)
for row in [invalid, surfaceRow(90, width: -1), surfaceRow(90, width: Double.infinity), surfaceRow(91)] {
    do { _ = try readyModalSurface([row], id: 90); preconditionFailure("Invalid sheet surface became pending") }
    catch let error as NativeFailure { surfaceCheck(["window_unproven", "target_changed"].contains(error.code)) }
}

// An unnamed editable control publishes its native role as a fallback label.
// Copying that returned label into a selector must find the same control.
private let unnamedTextField = SemanticTextFields(role: kAXTextFieldRole, title: "", value: "", description: "")
surfaceCheck(unnamedTextField.label == "AXTextField" && unnamedTextField.name == "" && unnamedTextField.value == "")
precondition(unnamedTextField.matches("AXTextField"), "selector failed to match the returned AXTextField label")
surfaceCheck(unnamedTextField.matches("axtextfield"))
surfaceCheck(!unnamedTextField.matches("text_field"))
private let absentTextAttributes = SemanticTextFields(role: kAXTextFieldRole, title: nil, value: nil, description: nil)
surfaceCheck(absentTextAttributes.matches("AXTextField"))
surfaceCheck(absentTextAttributes.name == nil && absentTextAttributes.value == nil)

// Unnamed standard chrome is identified by native subrole, never array order.
for (subrole, name) in [("AXCloseButton", "Close window"), ("AXMinimizeButton", "Minimize window"),
                        ("AXZoomButton", "Zoom window"), ("AXFullScreenButton", "Full screen window")] {
    let chrome = SemanticTextFields(role: kAXButtonRole, title: nil, value: nil, description: "", subrole: subrole)
    surfaceCheck(chrome.name == name && chrome.label == name && chrome.matches(name))
}
surfaceCheck(SemanticTextFields(role: kAXButtonRole, title: "关闭", value: nil, description: nil,
    subrole: "AXCloseButton").name == "关闭")
surfaceCheck(SemanticTextFields(role: kAXButtonRole, title: nil, value: nil, description: "关闭窗口",
    subrole: "AXCloseButton").name == "关闭窗口")
surfaceCheck(SemanticTextFields(role: kAXTextFieldRole, title: nil, value: nil, description: nil,
    subrole: "AXCloseButton").name == nil)
surfaceCheck(SemanticTextFields(role: kAXButtonRole, title: nil, value: nil, description: nil,
    subrole: "AXCustomButton").label == kAXButtonRole)

// A new/raised target surface crossing the original is distinct from a
// pre-existing overlay, another app's window, and a missing original owner.
private func orderRow(_ id: CGWindowID, _ pid: Int32) -> CGRecord {
    CGRecord(id: id, pid: pid, bounds: CGRect(x: 0, y: 0, width: 100, height: 100), onscreen: true)
}
private let originalOrder = [orderRow(1, 20), orderRow(2, 10), orderRow(3, 20)]
private let alreadyAbove = targetWindowsAbove(originalOrder, original: 2, owner: 10, target: 20)!
surfaceCheck(alreadyAbove == [1])
surfaceCheck(targetWindowsAbove([orderRow(4, 30)] + originalOrder, original: 2, owner: 10, target: 20) == alreadyAbove)
surfaceCheck(targetWindowsAbove([orderRow(3, 20)] + originalOrder.prefix(2), original: 2, owner: 10, target: 20)!.subtracting(alreadyAbove) == [3])
surfaceCheck(targetWindowsAbove([orderRow(4, 20)] + originalOrder, original: 2, owner: 10, target: 20)!.subtracting(alreadyAbove) == [4])
surfaceCheck(targetWindowsAbove(originalOrder, original: 2, owner: 99, target: 20) == nil)
surfaceCheck(targetWindowsAbove([orderRow(3, 20)], original: 2, owner: 10, target: 20) == nil)
surfaceCheck(!originalAboveOtherApps(originalOrder, original: 2, owner: 10))
surfaceCheck(originalAboveOtherApps([orderRow(4, 10), orderRow(2, 10), orderRow(3, 20)], original: 2, owner: 10))
surfaceCheck(!originalAboveOtherApps(originalOrder, original: 2, owner: 99))

// A menu's accessibility focus is not keyboard foreground. Passive order return
// needs the original actual foreground and exact local window, including a
// second foreground proof after the potentially blocking AX read.
private var passiveFrontReads = 0
private let passiveSameOwner = try passiveOrderOwner(requested: 42, requestedWindow: 42,
    frontmost: { passiveFrontReads += 1; return true }, focusedWindow: { 42 },
    equal: ==, windowID: { CGWindowID($0) })
surfaceCheck(passiveSameOwner)
surfaceCheck(passiveFrontReads == 2)
private let passiveOtherForeground = try passiveOrderOwner(requested: 42, requestedWindow: 42,
    frontmost: { false }, focusedWindow: { preconditionFailure("Do not query a different foreground owner") },
    equal: ==, windowID: { CGWindowID($0) })
surfaceCheck(!passiveOtherForeground)
private let passiveOtherWindow = try passiveOrderOwner(requested: 42, requestedWindow: 42,
    frontmost: { true }, focusedWindow: { 43 }, equal: ==, windowID: { CGWindowID($0) })
surfaceCheck(!passiveOtherWindow)
private let passiveMissingWindow = try passiveOrderOwner(requested: 42, requestedWindow: 42,
    frontmost: { true }, focusedWindow: { nil }, equal: ==, windowID: { CGWindowID($0) })
surfaceCheck(!passiveMissingWindow)
passiveFrontReads = 0
private let passiveOwnerChangedDuringRead = try passiveOrderOwner(requested: 42, requestedWindow: 42,
    frontmost: { passiveFrontReads += 1; return passiveFrontReads == 1 }, focusedWindow: { 42 },
    equal: ==, windowID: { CGWindowID($0) })
surfaceCheck(!passiveOwnerChangedDuringRead)
surfaceCheck(passiveFrontReads == 2)

// Public composite labels are matchable verbatim. Separately returned fields
// never create an implicit whitespace-joined match, nor an extra role alias.
private let orderNumber = SemanticTextFields(role: kAXTextFieldRole, title: "Order", value: "Number", description: nil)
surfaceCheck(orderNumber.label == "Order | Number")
surfaceCheck(orderNumber.matches("Order | Number"))
surfaceCheck(orderNumber.matches("order") && orderNumber.matches("NUMBER"))
surfaceCheck(!orderNumber.matches("Order Number"))
surfaceCheck(!orderNumber.matches("AXTextField"))
private let describedText = SemanticTextFields(role: kAXTextFieldRole, title: "", value: "", description: "Café")
surfaceCheck(describedText.name == "Café" && describedText.matches("CAFE"))
private let spacedText = SemanticTextFields(role: kAXTextFieldRole, title: "  retained spaces  ", value: nil, description: nil)
surfaceCheck(spacedText.matches("  RETAINED SPACES  "))
surfaceCheck(orderNumber.matches(nil))
surfaceCheck(!semanticTextContains(String(repeating: "a", count: 4_096) + "needle", needle: "needle"))
surfaceCheck(semanticTextContains(String(repeating: "a", count: 300) + "needle", needle: "NEEDLE"))
print("Semantic selectors match public fallback/composite labels without concatenating fields; no AX or GUI reads")

// Shared search coverage, independent of published preview size and page size.
private var nativeSearchBudget = NativeTextSearchBudget()
private var nativeButtonMatch = NativeNodeTextMatch(matched: false)
nativeButtonMatch.inspect("Commit note", needle: "Commit note", budget: &nativeSearchBudget)
private var unrelatedDocumentMatch = NativeNodeTextMatch(matched: false)
unrelatedDocumentMatch.inspect(String(repeating: "x", count: 20_971), needle: "Commit note", budget: &nativeSearchBudget)
surfaceCheck(nativeButtonMatch.matched && !nativeButtonMatch.incomplete)
surfaceCheck(!unrelatedDocumentMatch.matched && !unrelatedDocumentMatch.incomplete)
surfaceCheck(nativeSearchBudget.remaining == 1_048_576 - 11 - 20_971)

// A second eligible node after the old prefix must remain a second match.
private var lateDuplicateMatch = NativeNodeTextMatch(matched: false)
lateDuplicateMatch.inspect(String(repeating: "x", count: 4_096) + "Commit note", needle: "Commit note", budget: &nativeSearchBudget)
surfaceCheck(lateDuplicateMatch.matched && !lateDuplicateMatch.incomplete)
surfaceCheck([nativeButtonMatch, unrelatedDocumentMatch, lateDuplicateMatch].filter(\.matched).count == 2)

private var exhaustedSearchBudget = NativeTextSearchBudget(remaining: 4_096)
private var unsearchedDuplicateMatch = NativeNodeTextMatch(matched: false)
unsearchedDuplicateMatch.inspect(String(repeating: "x", count: 4_096) + "Commit note", needle: "Commit note", budget: &exhaustedSearchBudget)
surfaceCheck(!unsearchedDuplicateMatch.matched && unsearchedDuplicateMatch.incomplete)
surfaceCheck(exhaustedSearchBudget.remaining == 0)
private var subsequentNodeMatch = NativeNodeTextMatch(matched: false)
subsequentNodeMatch.inspect("Commit note", needle: "Commit note", budget: &exhaustedSearchBudget)
surfaceCheck(!subsequentNodeMatch.matched && subsequentNodeMatch.incomplete)

// A later public description can prove membership after an earlier field
// exhausts native search; the unsearched suffix then cannot change membership.
private var previewProvenMatch = unsearchedDuplicateMatch
previewProvenMatch.includePreview(SemanticTextFields(role: kAXTextAreaRole, title: nil, value: nil, description: "Commit note"), needle: "Commit note")
surfaceCheck(previewProvenMatch.matched && !previewProvenMatch.incomplete)
private var compositePreviewMatch = NativeNodeTextMatch(matched: false)
compositePreviewMatch.includePreview(orderNumber, needle: "Order | Number")
surfaceCheck(compositePreviewMatch.matched && !compositePreviewMatch.incomplete)

private var titleSearchBudget = NativeTextSearchBudget(remaining: 11)
private var titleProvenMatch = NativeNodeTextMatch(matched: false)
titleProvenMatch.inspect("Commit note", needle: "Commit note", budget: &titleSearchBudget)
titleProvenMatch.inspect(String(repeating: "x", count: 2_000_000), needle: "Commit note", budget: &titleSearchBudget)
surfaceCheck(titleProvenMatch.matched && !titleProvenMatch.incomplete && titleSearchBudget.remaining == 0)
private var prefixSearchBudget = NativeTextSearchBudget(remaining: 11)
private var prefixProvenMatch = NativeNodeTextMatch(matched: false)
prefixProvenMatch.inspect("Commit note" + String(repeating: "x", count: 100), needle: "Commit note", budget: &prefixSearchBudget)
surfaceCheck(prefixProvenMatch.matched && !prefixProvenMatch.incomplete)

private var unicodeSearchBudget = NativeTextSearchBudget(remaining: 8)
private var unicodeMatch = NativeNodeTextMatch(matched: false)
unicodeMatch.inspect("Cafe\u{301}🙂中", needle: "CAFÉ🙂中", budget: &unicodeSearchBudget)
surfaceCheck(unicodeMatch.matched && !unicodeMatch.incomplete && unicodeSearchBudget.remaining == 0)
private var splitScalarBudget = NativeTextSearchBudget(remaining: 1)
private var splitScalarMatch = NativeNodeTextMatch(matched: false)
splitScalarMatch.inspect("🙂", needle: "🙂", budget: &splitScalarBudget)
surfaceCheck(!splitScalarMatch.matched && splitScalarMatch.incomplete)
print("Native selector text search proves full fields, late duplicates and bounded unknown suffixes; no AX or GUI reads")

// Automatic app startup must preserve the user's foreground and leave surfaces
// shareable; hiding/minimizing is not a substitute for a nonactivating launch.
private let automaticOpen = applicationOpenConfiguration()
surfaceCheck(!automaticOpen.activates && !automaticOpen.hides)
surfaceCheck(!automaticOpen.createsNewApplicationInstance)
surfaceCheck(!automaticOpen.allowsRunningApplicationSubstitution)
surfaceCheck(!automaticOpen.addsToRecentItems)

// Missing AXWindows is a read-only semantic gap. The same native error remains
// a hard modal refusal outside that exact phase; unrelated failures stay hard.
for missing in [AXError.attributeUnsupported, .noValue] {
    let unavailable = failure("modal_state_unproven", "Required AX inventory is unavailable",
        attributeFailure: AXAttributeFailure(name: kAXWindowsAttribute, error: missing))
    do { let _: Int = try readOnlySemantics { throw unavailable }; preconditionFailure("read did not fail") }
    catch let error as NativeFailure { surfaceCheck(error.code == "semantic_window_unavailable" && error.attributeFailure?.error == missing) }
    surfaceCheck(unavailable.code == "modal_state_unproven")
}
for original in [failure("modal_state_unproven", "Real modal state is unknown"),
                 failure("permission_required", "AX denied"), failure("app_changed", "PID reused"),
                 failure("cancelled", "Canceled"),
                 failure("modal_state_unproven", "Sheets unavailable", attributeFailure: AXAttributeFailure(name: "AXSheets", error: .noValue))] {
    do { let _: Int = try readOnlySemantics { throw original }; preconditionFailure("hard error vanished") }
    catch let error as NativeFailure { surfaceCheck(error.code == original.code && error.message == original.message) }
}

// AX action reply budgeting is separate from attribute reads. These injected
// calls never create an AX object, operate the desktop, or wait in real time.
private let pressDeadline = try Deadline(["deadline_unix_ms": UInt64((Date().timeIntervalSince1970 + 30) * 1_000)])
private var pressClock = pressDeadline.uptime - 10
private var pressTimeouts: [Float] = []
private var pressDispatches = 0
private let modalPressReply = try performSemanticPress(deadline: pressDeadline, now: { pressClock }, setTimeout: {
    pressTimeouts.append($0); return .success
}, dispatch: {
    pressDispatches += 1
    let modalInitialization = 3.2 // Cold Open/Save initialization exceeded the old 2 s cap in DSH.
    pressClock += modalInitialization
    return Double(pressTimeouts.last!) > modalInitialization ? .success : .cannotComplete
})
surfaceCheck(pressTimeouts == [10] && pressDispatches == 1)
surfaceCheck(modalPressReply["dispatch"] as? String == "dispatched")
surfaceCheck(modalPressReply["evidence"] as? String == "unverified" && modalPressReply["error"] == nil)

// A short caller deadline always bounds the one action timeout.
pressClock = pressDeadline.uptime - 0.075
pressTimeouts = []; pressDispatches = 0
private let shortPressReply = try performSemanticPress(deadline: pressDeadline, now: { pressClock }, setTimeout: {
    pressTimeouts.append($0); return .success
}, dispatch: {
    pressDispatches += 1; pressClock = pressDeadline.uptime
    return .cannotComplete
})
surfaceCheck(pressTimeouts.count == 1 && abs(pressTimeouts[0] - 0.075) < 0.00001 && pressDispatches == 1)
surfaceCheck(shortPressReply["dispatch"] as? String == "unknown" && shortPressReply["error"] as? String == "deadline_exceeded")

// An AX acknowledgement remains original provider evidence at the deadline;
// Core owns the separate context timeout/cancellation conclusion.
pressClock = pressDeadline.uptime - 0.1
pressDispatches = 0
private let latePressReply = try performSemanticPress(deadline: pressDeadline, now: { pressClock }, setTimeout: { _ in .success }, dispatch: {
    pressDispatches += 1; pressClock = pressDeadline.uptime + 0.1
    return .success
})
surfaceCheck(pressDispatches == 1 && latePressReply["dispatch"] as? String == "dispatched")
surfaceCheck(latePressReply["evidence"] as? String == "unverified" && latePressReply["error"] == nil)

// Neither timeout nor a different native error is positive acceptance evidence.
for axError in [AXError.cannotComplete, .attributeUnsupported, .actionUnsupported,
                .invalidUIElement, .failure, .notImplemented, .illegalArgument, .apiDisabled] {
    pressClock = pressDeadline.uptime - 5; pressDispatches = 0
    let reply = try performSemanticPress(deadline: pressDeadline, now: { pressClock }, setTimeout: { _ in .success }, dispatch: {
        pressDispatches += 1; return axError
    })
    surfaceCheck(pressDispatches == 1 && reply["dispatch"] as? String == "unknown")
    surfaceCheck(reply["evidence"] as? String == "unverified" && reply["error"] as? String == "ax_action_failed")
    surfaceCheck((reply["message"] as? String)?.contains(String(axError.rawValue)) == true)
}

// All throwing paths are before the native call; none may be mislabelled as
// post-dispatch refusal by press()'s outer not_dispatched error handler.
for phase in ["already_expired", "timeout_configuration", "expired_during_configuration", "admission_closed"] {
    pressClock = pressDeadline.uptime - (phase == "already_expired" ? 0 : 5)
    pressTimeouts = []; pressDispatches = 0
    do {
        _ = try performSemanticPress(deadline: pressDeadline, now: { pressClock }, setTimeout: {
            pressTimeouts.append($0)
            if phase == "expired_during_configuration" { pressClock = pressDeadline.uptime }
            return phase == "timeout_configuration" ? .invalidUIElement : .success
        }, dispatch: {
            surfaceCheck(phase == "admission_closed")
            // The closed gate returns nil without executing any native action.
            return nil
        })
        preconditionFailure("AXPress admitted after a pre-dispatch refusal")
    } catch let error as NativeFailure {
        let expected = phase == "timeout_configuration" ? "ax_unavailable" : phase == "admission_closed" ? "closed" : "deadline_exceeded"
        surfaceCheck(error.code == expected && pressDispatches == 0)
        surfaceCheck(pressTimeouts.count == (phase == "already_expired" ? 0 : 1))
    }
}
print("Semantic AXPress uses one bounded action reply and preserves unknown/late provider evidence; no AX or GUI reads")

// A live equivalent alias proves the retained surface without reading an
// expired sheet-parent proxy. A different native surface still fails closed.
sheetFocusCheck(try exactFocusedSurface(requested: 1, requestedWindow: 100, focused: 6,
    equal: { ($0 == 1 || $0 == 6) && ($1 == 1 || $1 == 6) },
    windowID: { element in
        guard element == 6 else { throw failure("window_unproven", "expired proxy") }
        return 100
    }))
private func inlineIdentity(role: String? = kAXTextFieldRole, parent: String? = kAXApplicationRole,
    owner: Int32 = 10, parentOwner: Int32 = 10, window: CGWindowID = 100,
    requested: CGWindowID = 100) -> Bool {
    standaloneTextControlIdentity(role: role, parentRole: parent, pid: 10, owner: owner,
        parentOwner: parentOwner, window: window, requested: requested)
}
surfaceCheck(inlineIdentity())
surfaceCheck(!inlineIdentity(role: kAXButtonRole))
surfaceCheck(!inlineIdentity(role: nil))
surfaceCheck(!inlineIdentity(parent: kAXWindowRole))
surfaceCheck(!inlineIdentity(parent: nil))
surfaceCheck(!inlineIdentity(owner: 20))
surfaceCheck(!inlineIdentity(parentOwner: 20))
surfaceCheck(!inlineIdentity(window: 0))
surfaceCheck(!inlineIdentity(requested: 101))
print("Standalone native text receiver controls passed; no desktop access")

// Only expired modal identity plus an admitted action can return to a known
// live app window. Missing evidence, minimization and arbitrary siblings cannot.
surfaceCheck(expiredAXWindowIdentity(.illegalArgument, id: 0))
surfaceCheck(expiredAXWindowIdentity(.invalidUIElement, id: 0))
for status in [AXError.success, .cannotComplete, .apiDisabled, .failure, .noValue] {
    surfaceCheck(!expiredAXWindowIdentity(status, id: 0))
}
surfaceCheck(!expiredAXWindowIdentity(.illegalArgument, id: 100))
private func modalReturn(admitted: Bool = true, modal: Bool = true, anchor: CGWindowID = 100,
    candidate: CGWindowID = 200, known: Set<CGWindowID> = [100, 200],
    onscreen: Bool = false, expired: Bool = true) -> Bool {
    closedModalReturnAllowed(admitted: admitted, modal: modal, anchor: anchor,
        candidate: candidate, known: known, onscreen: onscreen, expired: expired)
}
surfaceCheck(modalReturn())
surfaceCheck(!modalReturn(admitted: false))
surfaceCheck(!modalReturn(modal: false))
surfaceCheck(!modalReturn(anchor: 0))
surfaceCheck(!modalReturn(candidate: 0))
surfaceCheck(!modalReturn(candidate: 100))
surfaceCheck(!modalReturn(known: [100]))
surfaceCheck(!modalReturn(onscreen: true))
surfaceCheck(!modalReturn(expired: false))
print("Controlled modal dismissal return controls passed; no desktop access")

// Original-owner confirmation can re-read a typed native gap without changing
// the nominated owner, activating anything or replaying input.
private var originalConfirmationReads = 0
private var originalConfirmationWaits = 0
private let originalConfirmed: Bool = try waitForEstablishedFocus(
    remaining: { 1 }, check: {}, read: {
        originalConfirmationReads += 1
        return try frontProcessMatch(originalConfirmationReads == 1 ? -3 : 1)
    }, wait: { _ in originalConfirmationWaits += 1 })
surfaceCheck(originalConfirmed && originalConfirmationReads == 2 && originalConfirmationWaits == 1)
private var changedOwnerWaits = 0
do {
    let _: Bool = try waitForEstablishedFocus(remaining: { 1 }, check: {}, read: {
        guard try frontProcessMatch(0) else { throw failure("focus_changed", "Original owner changed") }
        return true
    }, wait: { _ in changedOwnerWaits += 1 })
    preconditionFailure("Proved external choice was retried")
} catch let error as NativeFailure {
    surfaceCheck(error.code == "focus_changed" && changedOwnerWaits == 0)
}
print("Original-owner confirmation recovers only typed read gaps; no activation/input")
