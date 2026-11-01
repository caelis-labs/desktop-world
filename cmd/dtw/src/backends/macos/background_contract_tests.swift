// Native.swift's Rust foreground bridge is linked only in the product binary.
// Pure controls must never activate a real application.
@_cdecl("dtw_set_front_process")
func dtwSetFrontProcessControlStub(_ pid: Int32, _ window: UInt32) -> Bool {
    preconditionFailure("Pure contracts must not invoke foreground input")
}

@_cdecl("dtw_front_process_matches")
func dtwFrontProcessMatchesControlStub(_ pid: Int32) -> Int32 {
    preconditionFailure("Pure contracts must not read the real WindowServer")
}

@_cdecl("dtw_front_process_pid")
func dtwFrontProcessPIDControlStub() -> Int32 {
    preconditionFailure("Pure contracts must not resolve the real foreground PID")
}

// Pure target-read recovery controls; no desktop, AX server or input events.
private func backgroundCheck(_ value: @autoclosure () -> Bool) { precondition(value()) }
private func busyAttribute(_ name: String = "AXModal") -> NativeFailure {
    failure("ax_unavailable", "busy \(name)", details:["attribute":name, "ax_error":AXError.cannotComplete.rawValue],
            attributeFailure:AXAttributeFailure(name:name,error:.cannotComplete))
}
private func expectedBackgroundError(_ code: String, _ body: () throws -> Void) -> NativeFailure {
    do { try body(); preconditionFailure("Expected \(code)") }
    catch let error as NativeFailure { backgroundCheck(error.code == code); return error }
    catch { preconditionFailure("Unexpected \(error)") }
}
private var backgroundReadCount = 0
try BackgroundInputValidation.waitForReadableTarget(remaining: { 1 }, check: {}, read: {
    backgroundReadCount += 1
}, wait: { _ in preconditionFailure("A readable target must not wait") })
backgroundCheck(backgroundReadCount == 1)

private var backgroundRemaining = 0.75
backgroundReadCount = 0
try BackgroundInputValidation.waitForReadableTarget(remaining: { backgroundRemaining }, check: {}, read: {
    backgroundReadCount += 1
    if backgroundReadCount == 1 { throw busyAttribute() }
}, wait: { interval in backgroundCheck(interval == 0.02); backgroundRemaining -= interval })
backgroundCheck(backgroundReadCount == 2)

backgroundRemaining = 0.05
backgroundReadCount = 0
private let exhaustedAX = expectedBackgroundError("ax_unavailable") {
    try BackgroundInputValidation.waitForReadableTarget(remaining: { backgroundRemaining }, check: {}, read: {
        backgroundReadCount += 1
        throw busyAttribute(backgroundReadCount == 1 ? "AXModal" : "AXSheets")
    }, wait: { backgroundRemaining -= $0 })
}
backgroundCheck(backgroundReadCount == 3)
backgroundCheck(exhaustedAX.attributeFailure?.name == "AXModal")

for permanent in [failure("target_changed", "closed"), failure("unsupported_modal", "sheet"),
                  failure("modal_state_unproven", "missing Boolean"),
                  failure("ax_unavailable", "invalid object", attributeFailure:AXAttributeFailure(name:"AXModal",error:.invalidUIElement)),
                  failure("ax_unavailable", "failure", attributeFailure:AXAttributeFailure(name:"AXModal",error:.failure))] {
    backgroundReadCount = 0
    _ = expectedBackgroundError(permanent.code) {
        try BackgroundInputValidation.waitForReadableTarget(remaining: { 1 }, check: {}, read: {
            backgroundReadCount += 1; throw permanent
        }, wait: { _ in preconditionFailure("Permanent failure must not wait") })
    }
    backgroundCheck(backgroundReadCount == 1)
}

// A modal appearing after the busy sample stops at the first proven change.
backgroundReadCount = 0
_ = expectedBackgroundError("unsupported_modal") {
    try BackgroundInputValidation.waitForReadableTarget(remaining: { 1 }, check: {}, read: {
        backgroundReadCount += 1
        if backgroundReadCount == 1 { throw busyAttribute() }
        throw failure("unsupported_modal", "new sheet")
    }, wait: { _ in })
}
backgroundCheck(backgroundReadCount == 2)

// A changed target before the first read never enters AX recovery.
_ = expectedBackgroundError("target_changed") {
    try BackgroundInputValidation.waitForReadableTarget(remaining: { 1 }, check: {
        throw failure("target_changed", "closed before read")
    }, read: { preconditionFailure("No read after target loss") }, wait: { _ in preconditionFailure("No wait after target loss") })
}

// The successful read itself may race a closing/replaced window.
private var backgroundTargetAlive = true
_ = expectedBackgroundError("target_changed") {
    try BackgroundInputValidation.waitForReadableTarget(remaining: { 1 }, check: {
        if !backgroundTargetAlive { throw failure("target_changed", "closed during read") }
    }, read: { backgroundTargetAlive = false }, wait: { _ in preconditionFailure("No retry after target change") })
}

// Local AX budget expiry retains the original typed cause; the action deadline
// and shutdown take precedence because check uses the original admission.
for code in ["deadline_exceeded", "native_stopping"] {
    backgroundReadCount = 0
    _ = expectedBackgroundError(code) {
        try BackgroundInputValidation.waitForReadableTarget(remaining: { 1 }, check: {
            if backgroundReadCount > 0 { throw failure(code, "admission ended") }
        }, read: { backgroundReadCount += 1; throw busyAttribute() }, wait: { _ in preconditionFailure("Ended admission cannot wait") })
    }
}
backgroundRemaining = 0.1
backgroundReadCount = 0
private let localExpiry = expectedBackgroundError("ax_unavailable") {
    try BackgroundInputValidation.waitForReadableTarget(remaining: { backgroundRemaining }, check: {}, read: {
        backgroundReadCount += 1
        if backgroundReadCount == 1 { throw busyAttribute() }
        backgroundRemaining = 0
        throw failure("deadline_exceeded", "local AX read budget")
    }, wait: { backgroundRemaining -= $0 })
}
backgroundCheck(localExpiry.attributeFailure?.error == .cannotComplete)

// Even a successful late read cannot extend the bounded recovery window.
backgroundRemaining = 0.1
backgroundReadCount = 0
private let lateSuccess = expectedBackgroundError("ax_unavailable") {
    try BackgroundInputValidation.waitForReadableTarget(remaining: { backgroundRemaining }, check: {}, read: {
        backgroundReadCount += 1
        if backgroundReadCount == 1 { throw busyAttribute() }
        backgroundRemaining = 0
    }, wait: { backgroundRemaining -= $0 })
}
backgroundCheck(lateSuccess.attributeFailure?.name == "AXModal")
print("Background target-read recovery contracts passed (15 pure scenarios)")


// Post-raise modal snapshot expiry never grants a skipped child or another
// activation. A full subsequent focus read must still establish every proof.
private var modalAssertions = 0
private func modalCheck(_ condition: @autoclosure () -> Bool) {
    precondition(condition()); modalAssertions += 1
}
private let staleModalElement = failure("ax_unavailable", "AX role is invalid", details:["ax_error":-25202],
    attributeFailure:AXAttributeFailure(name:kAXRoleAttribute,error:.invalidUIElement))
private func modalFailure(_ body: () throws -> Void) -> NativeFailure {
    do { try body(); preconditionFailure("Expected modal validation failure") }
    catch let error as NativeFailure { return error }
    catch { preconditionFailure("Unexpected modal error: \(error)") }
}
private var modalProofReads = 0
private var modalTargetChecks = 0
private let modalReady: Int = try readModalSnapshot(phase: .postRaise, scope: .appChild,
    isTarget: false, read: { 7 }, parentContains: { preconditionFailure("Successful read needs no refresh") })
modalCheck(modalReady == 7)
for scope in [ModalReadScope.appWindow, .appChild, .targetChild] {
    for present in [false, true] {
        modalProofReads = 0; modalTargetChecks = 0
        let error = modalFailure {
            try readModalSnapshot(phase: .postRaise, scope: scope, isTarget: false,
                read: { throw staleModalElement }, checkTarget: { modalTargetChecks += 1 },
                parentContains: { modalProofReads += 1; return present })
        }
        modalCheck(error.modalSnapshotExpired == !present)
        modalCheck(error.code == staleModalElement.code && error.attributeFailure?.error == .invalidUIElement)
        modalCheck(error.details?["validation_scope"] as? String == scope.rawValue)
        modalCheck(error.details?["validation_phase"] as? String == "post_raise")
        modalCheck(modalProofReads == 1 && modalTargetChecks == 2)
    }
}
for (phase, scope, isTarget) in [
    (FocusValidationPhase.validation, ModalReadScope.appChild, false),
    (.preRaise, .appWindow, false), (.postRaise, .targetRoot, false),
    (.postRaise, .appWindow, true), (.postRaise, .appChild, true), (.postRaise, .targetChild, true),
] {
    let error = modalFailure {
        try readModalSnapshot(phase: phase, scope: scope, isTarget: isTarget,
            read: { throw staleModalElement },
            parentContains: { preconditionFailure("Target/pre-raise failure cannot refresh") })
    }
    modalCheck(!error.modalSnapshotExpired && error.attributeFailure?.error == .invalidUIElement)
}
for original in [failure("unsupported_modal", "Real modal"), failure("modal_state_unproven", "Missing modal state"),
                 failure("ax_unavailable", "Untyped invalid element"), busyAttribute()] {
    let error = modalFailure {
        try readModalSnapshot(phase: .postRaise, scope: .appChild, isTarget: false,
            read: { throw original }, parentContains: { preconditionFailure("Only typed invalid object may refresh") })
    }
    modalCheck(!error.modalSnapshotExpired && error.code == original.code)
}
for refreshError in [failure("children_unavailable", "Incomplete parent"),
                     failure("window_limit", "Parent exceeds complete read limit"),
                     failure("modal_state_unproven", "Parent changed during read")] {
    let error = modalFailure {
        try readModalSnapshot(phase: .postRaise, scope: .appChild, isTarget: false,
            read: { throw staleModalElement }, parentContains: { throw refreshError })
    }
    modalCheck(!error.modalSnapshotExpired && error.attributeFailure?.error == .invalidUIElement)
    modalCheck(error.details?["inventory_refresh_error"] as? String == refreshError.code)
}
for code in ["deadline_exceeded", "native_stopping", "app_changed", "target_changed", "stale_observation"] {
    for failAfterRead in [false, true] {
        modalTargetChecks = 0; modalProofReads = 0
        let error = modalFailure {
            try readModalSnapshot(phase: .postRaise, scope: .appChild, isTarget: false,
                read: { throw staleModalElement }, checkTarget: {
                    modalTargetChecks += 1
                    if modalTargetChecks == (failAfterRead ? 2 : 1) { throw failure(code, "Original target ended") }
                }, parentContains: { modalProofReads += 1; return false })
        }
        modalCheck(error.code == code && !error.modalSnapshotExpired)
        modalCheck(modalProofReads == (failAfterRead ? 1 : 0))
    }
    let error = modalFailure {
        try readModalSnapshot(phase: .postRaise, scope: .appChild, isTarget: false,
            read: { throw staleModalElement }, parentContains: { throw failure(code, "Refresh ended") })
    }
    modalCheck(error.code == code && !error.modalSnapshotExpired)
}
// A target-root AX failure before or after the parent copy is fatal, even
// though it shares the auxiliary object's raw invalidUIElement status.
for failAfterRead in [false, true] {
    modalTargetChecks = 0
    let targetError = failure("ax_unavailable", "Original target root invalid",
        attributeFailure: AXAttributeFailure(name:kAXRoleAttribute,error:.invalidUIElement))
    let error = modalFailure {
        try readModalSnapshot(phase: .postRaise, scope: .appChild, isTarget: false,
            read: { throw staleModalElement }, checkTarget: {
                modalTargetChecks += 1
                if modalTargetChecks == (failAfterRead ? 2 : 1) { throw targetError }
            }, parentContains: { false })
    }
    modalCheck(error.message == targetError.message && !error.modalSnapshotExpired)
}
private let expiredModalSnapshot = modalFailure {
    try readModalSnapshot(phase: .postRaise, scope: .appChild, isTarget: false,
        read: { throw staleModalElement }, parentContains: { false })
}
private var modalClock = 0.0
private var modalPolls = 0
private let recoveredModalFocus: Int = try waitForEstablishedFocus(remaining: { 0.75 - modalClock }, check: {}, read: {
    modalPolls += 1
    if modalPolls == 1 { throw expiredModalSnapshot }
    return 9
}, wait: { modalClock += $0 })
modalCheck(recoveredModalFocus == 9 && modalPolls == 2)
// Sustained churn is bounded by the original focus-read window. Preserve the
// first original AX error, including scope and native status, on exhaustion.
modalClock = 0; modalPolls = 0
private let churnError = modalFailure {
    let _: Int = try waitForEstablishedFocus(remaining: { 0.05 - modalClock }, check: {}, read: {
        modalPolls += 1; throw expiredModalSnapshot
    }, wait: { modalClock += $0 })
}
modalCheck(modalPolls == 3 && modalClock == 0.05)
modalCheck(churnError.message == expiredModalSnapshot.message && churnError.attributeFailure?.error == .invalidUIElement)
for fatal in [failure("unsupported_modal", "New real sheet"), failure("target_changed", "Original surface closed"),
              failure("ax_unavailable", "Target role invalid", attributeFailure:AXAttributeFailure(name:kAXRoleAttribute,error:.invalidUIElement))] {
    modalClock = 0; modalPolls = 0
    let error = modalFailure {
        let _: Int = try waitForEstablishedFocus(remaining: { 0.75 - modalClock }, check: {}, read: {
            modalPolls += 1
            if modalPolls == 1 { throw expiredModalSnapshot }
            throw fatal
        }, wait: { modalClock += $0 })
    }
    modalCheck(error.message == fatal.message && modalPolls == 2 && modalClock == 0.02)
}
modalPolls = 0
private let modalDeadline = modalFailure {
    let _: Int = try waitForEstablishedFocus(remaining: { 0.75 }, check: {
        if modalPolls > 0 { throw failure("deadline_exceeded", "Original action deadline") }
    }, read: { modalPolls += 1; throw expiredModalSnapshot },
       wait: { _ in preconditionFailure("Original deadline prevents another poll") })
}
modalCheck(modalDeadline.code == "deadline_exceeded" && modalPolls == 1)
print("Post-raise auxiliary snapshot expiry controls passed (\(modalAssertions) assertions; no AX or GUI reads)")


// Reproduce the original ordering directly: an old A read applied after exact
// B establishment wrongly looks like an external leave. Same-epoch serial alone
// cannot distinguish that stale result from a fresh observation.
private var oldSampleOrdering = FocusBorrowEpoch(initialPID: 10, targetPID: 20, targetWindow: 200)
oldSampleOrdering.activated(20); oldSampleOrdering.focusedWindow(200)
oldSampleOrdering.activated(10)
backgroundCheck(oldSampleOrdering.invalidated)

private var sampledEpoch = FocusBorrowEpoch(initialPID: 10, targetPID: 20, targetWindow: 200)
private let beforeEstablishment = sampledEpoch.sampleRevision
sampledEpoch.established(20, window: 200)
backgroundCheck(!sampledEpoch.sampled(10, window: 100, revision: beforeEstablishment))
backgroundCheck(!sampledEpoch.invalidated && sampledEpoch.reachedTarget && sampledEpoch.reachedWindow)
backgroundCheck(sampledEpoch.sampled(10, window: 100, revision: sampledEpoch.sampleRevision))
backgroundCheck(sampledEpoch.invalidated) // Fresh external choice remains authoritative.

// A delayed sample from before retarget cannot act on the new expected target,
// even when the old sample describes a third app rather than the initial app.
sampledEpoch = FocusBorrowEpoch(initialPID: 10, targetPID: 20, targetWindow: 200)
sampledEpoch.established(20, window: 200)
private let beforeRetarget = sampledEpoch.sampleRevision
backgroundCheck(sampledEpoch.retarget(10, window: 100))
backgroundCheck(sampledEpoch.sampleRevision != beforeRetarget)
backgroundCheck(!sampledEpoch.sampled(30, window: 300, revision: beforeRetarget))
backgroundCheck(!sampledEpoch.invalidated)
backgroundCheck(sampledEpoch.sampled(10, window: 100, revision: sampledEpoch.sampleRevision))
backgroundCheck(sampledEpoch.reachedTarget && sampledEpoch.reachedWindow)

// Same-PID sibling retargets advance the revision too, while a no-op retarget
// does not discard a legitimately in-flight sample of unchanged expectations.
sampledEpoch = FocusBorrowEpoch(initialPID: 20, targetPID: 20, targetWindow: 200)
sampledEpoch.established(20, window: 200)
private let beforeSiblingRetarget = sampledEpoch.sampleRevision
backgroundCheck(sampledEpoch.retarget(20, window: 201))
backgroundCheck(!sampledEpoch.sampled(20, window: 200, revision: beforeSiblingRetarget))
backgroundCheck(!sampledEpoch.invalidated)
sampledEpoch.established(20, window: 201)
private let unchangedRevision = sampledEpoch.sampleRevision
backgroundCheck(sampledEpoch.retarget(20, window: 201) && sampledEpoch.sampleRevision == unchangedRevision)
backgroundCheck(sampledEpoch.sampled(20, window: 200, revision: unchangedRevision) && sampledEpoch.invalidated)

// Notifications are not revision-filtered. Preserve actual app/window
// leave-return events and never rearm an invalidated epoch on establishment.
for appChange in [true, false] {
    sampledEpoch = FocusBorrowEpoch(initialPID: 10, targetPID: 20, targetWindow: 200)
    sampledEpoch.established(20, window: 200)
    if appChange { sampledEpoch.activated(30); sampledEpoch.activated(20) }
    else { sampledEpoch.focusedWindow(201); sampledEpoch.focusedWindow(200) }
    backgroundCheck(sampledEpoch.invalidated)
    let invalidRevision = sampledEpoch.sampleRevision
    sampledEpoch.established(20, window: 200)
    backgroundCheck(sampledEpoch.invalidated && sampledEpoch.sampleRevision == invalidRevision)
    backgroundCheck(!sampledEpoch.retarget(20, window: 200))
    backgroundCheck(!sampledEpoch.sampled(20, window: 200, revision: invalidRevision))
}
sampledEpoch = FocusBorrowEpoch(initialPID: 20, targetPID: 20, targetWindow: 201)
private let beforeSiblingEstablishment = sampledEpoch.sampleRevision
sampledEpoch.established(20, window: 201)
backgroundCheck(!sampledEpoch.sampled(20, window: 200, revision: beforeSiblingEstablishment))
backgroundCheck(!sampledEpoch.invalidated)
backgroundCheck(sampledEpoch.sampled(20, window: 200, revision: sampledEpoch.sampleRevision))
backgroundCheck(sampledEpoch.invalidated)
// Notifications can establish an expected PID/window before the caller's
// explicit established() readback. Those phase transitions expire older reads
// too; repeated confirmations of the same state must not starve fresh samples.
sampledEpoch = FocusBorrowEpoch(initialPID: 10, targetPID: 20, targetWindow: 200)
private let beforeActivationNotification = sampledEpoch.sampleRevision
sampledEpoch.activated(20)
backgroundCheck(!sampledEpoch.sampled(10, window: 100, revision: beforeActivationNotification))
backgroundCheck(!sampledEpoch.invalidated)
private let afterActivationNotification = sampledEpoch.sampleRevision
sampledEpoch.activated(20)
backgroundCheck(sampledEpoch.sampleRevision == afterActivationNotification)
backgroundCheck(sampledEpoch.sampled(10, window: 100, revision: afterActivationNotification))
backgroundCheck(sampledEpoch.invalidated)

sampledEpoch = FocusBorrowEpoch(initialPID: 20, targetPID: 20, targetWindow: 201)
private let beforeWindowNotification = sampledEpoch.sampleRevision
sampledEpoch.focusedWindow(201)
backgroundCheck(!sampledEpoch.sampled(20, window: 200, revision: beforeWindowNotification))
backgroundCheck(!sampledEpoch.invalidated)
private let afterWindowNotification = sampledEpoch.sampleRevision
sampledEpoch.focusedWindow(201)
backgroundCheck(sampledEpoch.sampleRevision == afterWindowNotification)
backgroundCheck(sampledEpoch.sampled(20, window: 200, revision: afterWindowNotification))
backgroundCheck(sampledEpoch.invalidated)
print("Focus sampling revision controls passed (9 pure scenarios)")

private let matchingFrontProcess = try frontProcessMatch(1)
private let differentFrontProcess = try frontProcessMatch(0)
backgroundCheck(matchingFrontProcess && !differentFrontProcess)
for unavailable: Int32 in [-1, 2, Int32.min] {
    _ = expectedBackgroundError("focus_unproven") { _ = try frontProcessMatch(unavailable) }
}
print("Fresh WindowServer foreground proof controls passed (5 pure scenarios)")


@_cdecl("dtw_scroll_anchor_check")
func testScrollAnchorCheck() -> UnsafeMutablePointer<CChar>? { nil }
@_cdecl("dtw_scroll_anchor_free")
func testScrollAnchorFree(_ value: UnsafeMutablePointer<CChar>) { preconditionFailure("pure geometry controls never allocate a guard response") }
@_cdecl("dtw_scroll_anchor_posted")
func testScrollAnchorPosted() { preconditionFailure("pure geometry controls never post a wheel") }

private let scrollWindow = CGRect(x: -1200, y: -100, width: 800, height: 600)
private let scrollElement = CGRect(x: -1250, y: -50, width: 200, height: 80)
private let scrollGeometry = try ScrollAnchorGeometry(element: scrollElement, window: scrollWindow)
backgroundCheck(scrollGeometry.point == CGPoint(x: -1125, y: -10))
backgroundCheck(scrollGeometry.matches(element: scrollElement, window: scrollWindow, point: scrollGeometry.point))
backgroundCheck(!scrollGeometry.matches(element: scrollElement.offsetBy(dx: 0, dy: 1), window: scrollWindow, point: scrollGeometry.point))
backgroundCheck(!scrollGeometry.matches(element: scrollElement, window: scrollWindow.offsetBy(dx: 1, dy: 0), point: scrollGeometry.point))
backgroundCheck(!scrollGeometry.matches(element: scrollElement, window: scrollWindow, point: .zero))
private let boundScrollRequest: [String: Any] = ["bounds": [-1200.0, -100.0, 800.0, 600.0], "screen_x": -1125.0, "screen_y": -10.0]
private let keptScrollPoint = try fixedScrollPoint(boundScrollRequest, current: scrollWindow)
backgroundCheck(keptScrollPoint == scrollGeometry.point)
_ = expectedBackgroundError("stale_observation") { _ = try fixedScrollPoint(boundScrollRequest, current: scrollWindow.offsetBy(dx: 20, dy: 0)) }
_ = expectedBackgroundError("element_not_visible") { _ = try ScrollAnchorGeometry(element: CGRect(x: 0, y: 0, width: 10, height: 10), window: scrollWindow) }
private let legacyCenter = try fixedScrollPoint([:], current: scrollWindow)
backgroundCheck(legacyCenter == CGPoint(x: scrollWindow.midX, y: scrollWindow.midY))
print("Scroll anchor geometry preserves fixed negative points through focus; pure controls only")


// System AX global absence does not erase independently proven app-local AX
// focus. These controls use only synthetic process/window values, no desktop.
private var globalCandidateReads = 0
private func resolveFocusControl(result: AXError, value: Int32? = 10, candidate: Int32? = 10,
                                 front: Int32 = 1, generation: UInt64? = 42) throws -> FocusApplication<Int32>? {
    try resolveFocusedApplication(system: { try systemFocusValue(result, element: value) }, candidate: {
        globalCandidateReads += 1; return candidate
    }, pid: { $0 }, birth: { _ in generation }, frontmost: { _ in try frontProcessMatch(front) })
}
globalCandidateReads = 0
private let normalGlobalFocus = try resolveFocusControl(result: .success)
backgroundCheck(normalGlobalFocus?.pid == 10 && normalGlobalFocus?.generation == 42)
backgroundCheck(normalGlobalFocus?.fromCandidate == false && globalCandidateReads == 0)
for unavailable in [AXError.cannotComplete, .noValue] {
    globalCandidateReads = 0
    var windowReads = 0
    let matched = try exactInputFocus(expectedPID: 10, focusedApplication: {
        try resolveFocusControl(result: unavailable)
    }, windowMatches: { app in
        windowReads += 1
        return try exactFocusedSurface(requested: app, requestedWindow: 123,
            focused: app, equal: ==, windowID: { _ in 123 })
    })
    backgroundCheck(matched && globalCandidateReads == 2 && windowReads == 1)
    let wrongWindow = try exactInputFocus(expectedPID: 10, focusedApplication: {
        try resolveFocusControl(result: unavailable)
    }, windowMatches: { _ in
        try exactFocusedSurface(requested: 1, requestedWindow: 123,
            focused: 2, equal: ==, windowID: { _ in 123 })
    })
    backgroundCheck(!wrongWindow) // Same PID/CG ID never substitutes for AX object identity.

    globalCandidateReads = 0
    let notFrontmost = try exactInputFocus(expectedPID: 10, focusedApplication: {
        try resolveFocusControl(result: unavailable, front: 0)
    }, windowMatches: { _ in preconditionFailure("A negative PSN proof cannot read/accept a window") })
    backgroundCheck(!notFrontmost && globalCandidateReads == 1)

    let original = expectedBackgroundError(unavailable == .noValue ? "focus_unproven" : "ax_unavailable") {
        _ = try resolveFocusControl(result: unavailable, candidate: nil)
    }
    backgroundCheck(original.attributeFailure?.name == kAXFocusedApplicationAttribute)
    backgroundCheck(original.attributeFailure?.error == unavailable)

    for native in [-1, -2, -3, -4, -5, -6] as [Int32] {
        let missingWS = expectedBackgroundError("focus_unproven") {
            _ = try resolveFocusControl(result: unavailable, front: native)
        }
        backgroundCheck(missingWS.focusAbsence != .application && missingWS.focusAbsence != .window)
    }
}

// A successful System AX read that conflicts with WindowServer is not a reason
// to try a different candidate, even if that candidate would be convenient.
globalCandidateReads = 0
_ = expectedBackgroundError("focus_unproven") { _ = try resolveFocusControl(result: .success, candidate: 20, front: 0) }
backgroundCheck(globalCandidateReads == 0)
for invalid in [AXError.apiDisabled, .attributeUnsupported, .notImplemented, .invalidUIElement, .failure] {
    globalCandidateReads = 0
    let kept = expectedBackgroundError("ax_unavailable") { _ = try resolveFocusControl(result: invalid) }
    backgroundCheck(kept.attributeFailure?.error == invalid && globalCandidateReads == 0)
}
globalCandidateReads = 0
_ = expectedBackgroundError("focus_unproven") { _ = try resolveFocusControl(result: .success, value: nil) }
backgroundCheck(globalCandidateReads == 0) // A malformed success is not positive noValue.
for original in [busyAttribute(kAXFocusedWindowAttribute), failure("native_stopping", "original cancel"),
                 failure("deadline_exceeded", "original deadline")] {
    let kept = expectedBackgroundError(original.code) {
        let _: FocusApplication<Int32>? = try resolveFocusedApplication(system: { throw original },
            candidate: { preconditionFailure("Only the global typed AX failure can select a candidate") },
            pid: { $0 }, birth: { _ in 42 }, frontmost: { _ in true })
    }
    backgroundCheck(kept.message == original.message)
}

// Process replacement must fail both inside the PSN snapshot and across the
// intervening app-window read. Missing birth proof is never a cached identity.
private var focusBirthReads = 0
_ = expectedBackgroundError("focus_unproven") {
    _ = try resolveFocusedApplication(system: { Int32(10) }, candidate: { nil }, pid: { $0 }, birth: { _ in
        focusBirthReads += 1; return focusBirthReads == 1 ? 42 : 43
    }, frontmost: { _ in true })
}
backgroundCheck(focusBirthReads == 2)
_ = expectedBackgroundError("focus_unproven") { _ = try resolveFocusControl(result: .noValue, generation: nil) }
focusBirthReads = 0
_ = expectedBackgroundError("focus_unproven") {
    _ = try exactInputFocus(expectedPID: 10, focusedApplication: {
        focusBirthReads += 1
        return try resolveFocusControl(result: .noValue, generation: focusBirthReads == 1 ? 42 : 43)
    }, windowMatches: { _ in true })
}
for original in [busyAttribute(kAXFocusedWindowAttribute), missingFocus(.window),
                 failure("focus_unproven", "malformed app window"), failure("deadline_exceeded", "window read exhausted request")] {
    globalCandidateReads = 0
    let kept = expectedBackgroundError(original.code) {
        _ = try exactInputFocus(expectedPID: 10, focusedApplication: {
            try resolveFocusControl(result: .noValue)
        }, windowMatches: { _ in throw original })
    }
    backgroundCheck(kept.message == original.message && globalCandidateReads == 1)
}

// The observer can retain only app identity after an app-window failure. No CG
// ordering can create reachedWindow; fresh exact AX and epoch revision remain
// required. A later identity or deadline failure suppresses the sample entirely.
private let observerApp = FocusApplication(element: Int32(10), pid: 10, generation: 42, fromCandidate: true)
private var pidOnlyEpoch = FocusBorrowEpoch(initialPID: 20, targetPID: 10, targetWindow: 123)
private let observerAppCurrent = try focusedApplicationStillCurrent(observerApp,
    birth: { _ in 42 }, frontmost: { _ in true })
backgroundCheck(observerAppCurrent)
backgroundCheck(pidOnlyEpoch.sampled(10, window: nil, revision: pidOnlyEpoch.sampleRevision))
backgroundCheck(pidOnlyEpoch.reachedTarget && !pidOnlyEpoch.reachedWindow && !pidOnlyEpoch.invalidated)
private let observerWindowRevision = pidOnlyEpoch.sampleRevision
backgroundCheck(pidOnlyEpoch.sampled(10, window: 123, revision: observerWindowRevision))
backgroundCheck(pidOnlyEpoch.reachedWindow)
private let lostObserverApp = try focusedApplicationStillCurrent(observerApp,
    birth: { _ in 42 }, frontmost: { _ in false })
backgroundCheck(!lostObserverApp)
focusBirthReads = 0
private let changedObserverApp = try focusedApplicationStillCurrent(observerApp, birth: { _ in
    focusBirthReads += 1; return focusBirthReads == 1 ? 42 : 43
}, frontmost: { _ in true })
backgroundCheck(!changedObserverApp)
_ = expectedBackgroundError("deadline_exceeded") {
    _ = try focusedApplicationStillCurrent(observerApp,
        birth: { _ in throw failure("deadline_exceeded", "same observer budget") }, frontmost: { _ in true })
}
print("Typed System AX absence uses exact WindowServer/app-window proof; no CG window guessing or action retries")

// Reproduce the resident-cache failure: System AX is absent, the cached app is
// A, and stable WindowServer identity is B. Only the fresh B candidate can be
// used, then birth and exact app/window checks still apply normally.
private var freshForegroundReads = 0
for unavailable in [AXError.cannotComplete, .noValue] {
    let cachedA = try resolveFocusedApplication(system: {
        try systemFocusValue(unavailable, element: Optional<Int32>.none)
    }, candidate: { Int32(10) }, pid: { $0 }, birth: { _ in 42 }, frontmost: { $0 == 20 })
    backgroundCheck(cachedA == nil)
    freshForegroundReads = 0
    let freshB = try resolveFocusedApplication(system: {
        try systemFocusValue(unavailable, element: Optional<Int32>.none)
    }, candidate: {
        try foregroundCandidatePID(expectedPID:nil, fresh:{
            freshForegroundReads += 1; return try frontProcessPID(20)
        })
    }, pid: { $0 }, birth: { _ in 42 }, frontmost: { $0 == 20 })
    backgroundCheck(freshB?.pid == 20 && freshB?.generation == 42 && freshB?.fromCandidate == true)
    backgroundCheck(freshForegroundReads == 1)

    // A known target A must remain false when B is frontmost. It must not turn
    // into B's ownership just because a fresh unknown-app resolver is available.
    let knownA = try resolveFocusedApplication(system: {
        try systemFocusValue(unavailable, element: Optional<Int32>.none)
    }, candidate: {
        try foregroundCandidatePID(expectedPID:10, fresh:{
            preconditionFailure("Known target must not resolve another foreground application")
        })
    }, pid: { $0 }, birth: { _ in 42 }, frontmost: { $0 == 20 })
    backgroundCheck(knownA == nil)
}
for result in [0,-1,-2,-3,-4,-5,-6,-7,-8,-9,-10] as [Int32] {
    let kept = expectedBackgroundError("focus_unproven") { _ = try frontProcessPID(result) }
    backgroundCheck(kept.details?["native_result"] as? Int32 == result)
    backgroundCheck(kept.focusAbsence == nil) // No lookup error invents an absent/closed app.
}
for original in [failure("native_stopping", "cancel during foreground lookup"),
                 failure("deadline_exceeded", "foreground lookup exceeded original deadline"),
                 failure("focus_unproven", "unstable PSN roundtrip", details:["native_result":-9])] {
    let kept = expectedBackgroundError(original.code) {
        let _: FocusApplication<Int32>? = try resolveFocusedApplication(system: {
            try systemFocusValue(.noValue, element: Optional<Int32>.none)
        }, candidate: {
            try foregroundCandidatePID(expectedPID:nil, fresh:{throw original})
        }, pid: { $0 }, birth: { _ in preconditionFailure("Failed lookup cannot admit birth/window reads") },
           frontmost: { _ in preconditionFailure("Failed lookup cannot admit input") })
    }
    backgroundCheck(kept.message == original.message)
}
print("Fresh foreground PID replaces stale cache only for typed global AX absence; known-target and original-error boundaries remain strict")



// One issued AXRaise may be unconfirmed while exact focus is already valid.
// Only cannotComplete can reach the existing read-only proof without another
// activation. No key is admitted until that proof has actually returned.
private var raiseActivations = 0
private let acknowledgedRaise = try activationAfterRaise(.success) { raiseActivations += 1 }
backgroundCheck(acknowledgedRaise == nil && raiseActivations == 1)
raiseActivations = 0
private let unconfirmedRaiseControl = try activationAfterRaise(.cannotComplete) { raiseActivations += 1 }
backgroundCheck(unconfirmedRaiseControl == .cannotComplete && raiseActivations == 0)
for rejected in [AXError.apiDisabled, .actionUnsupported, .invalidUIElement, .noValue, .notImplemented, .failure] {
    let original = expectedBackgroundError("focus_failed") {
        _ = try activationAfterRaise(rejected) { preconditionFailure("Rejected raise cannot activate") }
    }
    backgroundCheck(original.message == "AXRaise did not confirm the requested window" && original.details == nil)
}
private let activationFailure = expectedBackgroundError("native_stopping") {
    _ = try activationAfterRaise(.success) { throw failure("native_stopping", "activation admission closed") }
}
backgroundCheck(activationFailure.message == "activation admission closed")

private var raiseProofClock = 0.0
private var raiseProofReads = 0
private var keysAfterRaiseProof = 0
private let provenAfterUnconfirmedRaise: Int = try verifyAfterRaise(unconfirmedRaiseControl) {
    try waitForEstablishedFocus(remaining: { 0.05 - raiseProofClock }, check: {}, read: {
        raiseProofReads += 1
        return raiseProofReads == 2 ? 7 : nil
    }, wait: { raiseProofClock += $0 })
}
keysAfterRaiseProof += 1
backgroundCheck(provenAfterUnconfirmedRaise == 7 && raiseProofReads == 2 && raiseProofClock == 0.02)
backgroundCheck(keysAfterRaiseProof == 1 && raiseActivations == 0)
raiseProofClock = 0; raiseProofReads = 0; keysAfterRaiseProof = 0
private let unresolvedRaiseProof = expectedBackgroundError("focus_unproven") {
    let _: Int = try verifyAfterRaise(unconfirmedRaiseControl) {
        try waitForEstablishedFocus(remaining: { 0.05 - raiseProofClock }, check: {}, read: {
            raiseProofReads += 1; return nil
        }, wait: { raiseProofClock += $0 })
    }
    keysAfterRaiseProof += 1
}
backgroundCheck(raiseProofReads == 3 && keysAfterRaiseProof == 0 && raiseActivations == 0)
backgroundCheck((unresolvedRaiseProof.details?["unconfirmed_raise"] as? [String: Any])?["ax_error"] as? Int32 == AXError.cannotComplete.rawValue)
backgroundCheck(unresolvedRaiseProof.message.contains("AXRaise unconfirmed: AXError=-25204"))
for original in [failure("native_stopping", "caller cancelled"), failure("deadline_exceeded", "original deadline"),
                 failure("app_changed", "process generation changed"), failure("stale_observation", "original root replaced"),
                 failure("target_blocked_by_modal", "new modal blocks target"), busyAttribute(kAXFocusedWindowAttribute),
                 missingFocus(.window)] {
    let kept = expectedBackgroundError(original.code) {
        let _: Int = try verifyAfterRaise(unconfirmedRaiseControl) { throw original }
        keysAfterRaiseProof += 1
    }
    backgroundCheck(keysAfterRaiseProof == 0 && kept.message.hasPrefix(original.message))
    backgroundCheck(kept.attributeFailure?.name == original.attributeFailure?.name)
    backgroundCheck(kept.attributeFailure?.error == original.attributeFailure?.error)
    backgroundCheck(kept.focusAbsence == original.focusAbsence && kept.modalSnapshotExpired == original.modalSnapshotExpired)
}
private let ordinaryProofFailure = expectedBackgroundError("focus_unproven") {
    let _: Int = try verifyAfterRaise(acknowledgedRaise) { throw failure("focus_unproven", "ordinary proof failure") }
}
backgroundCheck(ordinaryProofFailure.message == "ordinary proof failure" && ordinaryProofFailure.details == nil)
print("Unconfirmed AXRaise permits only bounded exact readback, retains original cause, and never repeats activation or admits failed-proof keys")


// Continuing an operation must inspect today's focus. The old AX root is deliberately
// unreadable, while the current root can be a replacement for the same logical
// window. Input refs keep their separate exact-object proof.
private let cleanupApp = NativeApp(identity: "cleanup.control", pid: 10, generation: 42)
private let cleanupApplication = FocusApplication(element: 100, pid: Int32(10), generation: UInt64(42), fromCandidate: true)
private var obsoleteRootReads = 0
private var currentRootReads = 0
private func cleanupRootID(_ root: Int) throws -> CGWindowID {
    if root == 1 {
        obsoleteRootReads += 1
        throw failure("window_unproven", "original AX root no longer readable")
    }
    precondition(root == 2)
    currentRootReads += 1
    return 777
}
private let ownsReplacementRoot = try borrowedFocusStillOwned(app: cleanupApp, windowID: 777,
    invalidated: { false }, focusedApplication: { cleanupApplication },
    currentWindowID: { _ in try cleanupRootID(2) })
backgroundCheck(ownsReplacementRoot && obsoleteRootReads == 0 && currentRootReads == 1)
// 999 is the associated helper's actual CG surface; the captured logical focus
// ID is 777. Their coincidence is neither assumed nor required.
private let wrongAssociatedFocus = try borrowedFocusStillOwned(app: cleanupApp, windowID: 777,
    invalidated: { false }, focusedApplication: { cleanupApplication }, currentWindowID: { _ in 999 })
backgroundCheck(!wrongAssociatedFocus && obsoleteRootReads == 0)
private let replacedInputRef = try exactFocusedSurface(requested: 1, requestedWindow: 777,
    focused: 2, equal: ==, windowID: cleanupRootID)
backgroundCheck(!replacedInputRef && obsoleteRootReads == 0) // Input never rebinds to the new root.

for currentID: CGWindowID? in [nil, 778] {
    let owned = try borrowedFocusStillOwned(app: cleanupApp, windowID: 777,
        invalidated: { false }, focusedApplication: { cleanupApplication }, currentWindowID: { _ in currentID })
    backgroundCheck(!owned)
}
private let missingAppOwnership = try borrowedFocusStillOwned(app: cleanupApp, windowID: 777,
    invalidated: { false }, focusedApplication: { Optional<FocusApplication<Int>>.none },
    currentWindowID: { _ in preconditionFailure("No focused app means no current window read") })
backgroundCheck(!missingAppOwnership)
for other in [FocusApplication(element: 200, pid: Int32(20), generation: UInt64(42), fromCandidate: false),
              FocusApplication(element: 100, pid: Int32(10), generation: UInt64(43), fromCandidate: true)] {
    let owned = try borrowedFocusStillOwned(app: cleanupApp, windowID: 777,
        invalidated: { false }, focusedApplication: { other },
        currentWindowID: { _ in preconditionFailure("Another app or reused PID cannot own the lease") })
    backgroundCheck(!owned)
}
private var cleanupAppReads = 0
private let reusedDuringCurrentRead = try borrowedFocusStillOwned(app: cleanupApp, windowID: 777,
    invalidated: { false }, focusedApplication: {
        cleanupAppReads += 1
        return FocusApplication(element: 100, pid: 10, generation: cleanupAppReads == 1 ? 42 : 43, fromCandidate: true)
    }, currentWindowID: { _ in 777 })
backgroundCheck(!reusedDuringCurrentRead && cleanupAppReads == 2)

private var cleanupEpochEnded = false
private let endedBeforeRead = try borrowedFocusStillOwned(app: cleanupApp, windowID: 777,
    invalidated: { true }, focusedApplication: { () -> FocusApplication<Int>? in
        preconditionFailure("An ended epoch does not start another read")
    }, currentWindowID: { _ in preconditionFailure("No window read after the epoch ends") })
backgroundCheck(!endedBeforeRead)
private let endedDuringRead = try borrowedFocusStillOwned(app: cleanupApp, windowID: 777,
    invalidated: { cleanupEpochEnded }, focusedApplication: { cleanupApplication }, currentWindowID: { _ in
        cleanupEpochEnded = true; return 777
    })
backgroundCheck(!endedDuringRead)
cleanupEpochEnded = false
private let endedDuringFailedRead = try borrowedFocusStillOwned(app: cleanupApp, windowID: 777,
    invalidated: { cleanupEpochEnded }, focusedApplication: { cleanupApplication }, currentWindowID: { _ in
        cleanupEpochEnded = true; throw busyAttribute(kAXFocusedWindowAttribute)
    })
backgroundCheck(!endedDuringFailedRead)

// Only an actual noValue result proves no current window; bad/unsupported/busy
// current reads remain unresolved and must not be treated as a closed target.
private let absentCurrent: Int? = try focusedWindowValue(.noValue, element: nil)
backgroundCheck(absentCurrent == nil)
_ = expectedBackgroundError("focus_unproven") {
    let _: Int? = try focusedWindowValue(.success, element: nil)
}
for native in [AXError.cannotComplete, .apiDisabled, .attributeUnsupported, .invalidUIElement] {
    let kept = expectedBackgroundError("ax_unavailable") {
        _ = try borrowedFocusStillOwned(app: cleanupApp, windowID: 777,
            invalidated: { false }, focusedApplication: { cleanupApplication }, currentWindowID: { _ in
                try focusedWindowValue(native, element: Optional<CGWindowID>.none)
            })
    }
    backgroundCheck(kept.attributeFailure?.name == kAXFocusedWindowAttribute && kept.attributeFailure?.error == native)
}
for original in [failure("permission_denied", "current permission denied"),
                 failure("window_unproven", "current exact identity unavailable"),
                 failure("focus_unproven", "malformed current window"),
                 failure("deadline_exceeded", "original cleanup deadline")] {
    let kept = expectedBackgroundError(original.code) {
        _ = try borrowedFocusStillOwned(app: cleanupApp, windowID: 777,
            invalidated: { false }, focusedApplication: { cleanupApplication }, currentWindowID: { _ in throw original })
    }
    backgroundCheck(kept.message == original.message)
}

// The same ownership proof protects continuing a script's borrow. Updating a
// new target after an AX root replacement must retain the existing scope if
// a current read or observer setup fails.
private struct RetargetBorrowControl {
    var currentWindow: CGWindowID
}
private let originalBorrowControl = RetargetBorrowControl(currentWindow: 777)
private var publishedBorrowControl: RetargetBorrowControl? = originalBorrowControl
private var retargetEpochCalls = 0
private let originalObsoleteReads = obsoleteRootReads
publishedBorrowControl = try retargetFocusBorrow(originalBorrowControl, owned: {
    try borrowedFocusStillOwned(app: cleanupApp, windowID: 777, invalidated: { false },
        focusedApplication: { cleanupApplication }, currentWindowID: { _ in try cleanupRootID(2) })
}, retarget: { retargetEpochCalls += 1; return true }, update: { next in
    next.currentWindow = 778
})
backgroundCheck(publishedBorrowControl?.currentWindow == 778 && retargetEpochCalls == 1 && obsoleteRootReads == originalObsoleteReads)
for original in [busyAttribute(kAXFocusedWindowAttribute), failure("window_unproven", "current identity unreadable"),
                 failure("deadline_exceeded", "current read deadline"), failure("permission_denied", "current AX denied")] {
    publishedBorrowControl = originalBorrowControl
    retargetEpochCalls = 0
    let kept = expectedBackgroundError(original.code) {
        publishedBorrowControl = try retargetFocusBorrow(originalBorrowControl, owned: {
            try borrowedFocusStillOwned(app: cleanupApp, windowID: 777, invalidated: { false },
                focusedApplication: { cleanupApplication }, currentWindowID: { _ in throw original })
        }, retarget: { retargetEpochCalls += 1; return true }, update: { _ in
            preconditionFailure("No publication/update after a current-read error")
        })
    }
    backgroundCheck(kept.message == original.message && retargetEpochCalls == 0)
    backgroundCheck(publishedBorrowControl?.currentWindow == 777)
}
publishedBorrowControl = originalBorrowControl
_ = expectedBackgroundError("deadline_exceeded") {
    publishedBorrowControl = try retargetFocusBorrow(originalBorrowControl, owned: { true }, retarget: {
        throw failure("deadline_exceeded", "observer retarget deadline")
    }, update: { _ in preconditionFailure("Observer setup failure cannot publish a new borrow") })
}
backgroundCheck(publishedBorrowControl?.currentWindow == 777)
publishedBorrowControl = try retargetFocusBorrow(originalBorrowControl, owned: { false }, retarget: {
    preconditionFailure("A proven ended lease cannot retarget its epoch")
}, update: { _ in preconditionFailure("A proven ended lease cannot publish a continuation") })
backgroundCheck(publishedBorrowControl == nil)
private let externallyEndedRetarget = try retargetFocusBorrow(originalBorrowControl, owned: { true }, retarget: { false },
    update: { _ in preconditionFailure("An ended observer epoch cannot publish a continuation") })
backgroundCheck(externallyEndedRetarget == nil)
print("Retarget publishes only after current ownership and observer setup succeed")

// A cold background attempt can exhaust its opt-in clock before the script
// brings the exact same window forward. One existing explicit-focus scope can
// then wait read-only; ordinary controls and fresh content never spend it.
private var focusColdShell = SemanticReadinessSample()
focusColdShell.add(role:kAXWindowRole, children:2, depth:0, parentRole:"")
focusColdShell.add(role:kAXGroupRole, children:0, depth:2, parentRole:kAXGroupRole)
focusColdShell.add(role:kAXButtonRole, children:0, depth:1, parentRole:kAXWindowRole, decoration:true)
backgroundCheck(focusColdShell.shellOnly)
backgroundCheck(focusColdShell.decision(manualOptIn:true, elapsed:0.002,
    materializationRemaining:-20) == .emptyContent)
private var focusReadinessControl = FocusSemanticReadiness()
focusReadinessControl.arm(now:100)
focusReadinessControl.arm(now:101) // Same epoch cannot slide its deadline.
backgroundCheck(focusReadinessControl.until == 103)
private var readinessAudits = 0
private let focusReadinessUntil = try focusReadinessControl.consume(shellOnly:focusColdShell.shellOnly,
    complete:true, now:100.1, deadline:120, audit:{ until in
        readinessAudits += 1; backgroundCheck(until == 103); return true
    })
backgroundCheck(focusReadinessUntil == 103 && readinessAudits == 1)
backgroundCheck(focusColdShell.decision(manualOptIn:true, elapsed:0.002,
    materializationRemaining:focusReadinessUntil!-100.1) == .pending)
private var focusedWebContent = focusColdShell
focusedWebContent.add(role:"AXWebArea", children:5, depth:7, parentRole:kAXGroupRole)
backgroundCheck(focusedWebContent.decision(manualOptIn:true, elapsed:0.01,
    materializationRemaining:2.8) == .ready)
backgroundCheck(focusColdShell.decision(manualOptIn:true, elapsed:3,
    materializationRemaining:0) == .emptyContent)
focusReadinessControl.arm(now:200)
private let reusedFocusReadiness = try focusReadinessControl.consume(shellOnly:true, complete:true,
    now:101, deadline:120, audit:{ _ in preconditionFailure("Consumed focus must not repeat its audit or wait") })
backgroundCheck(reusedFocusReadiness == nil && focusReadinessControl.until == 103)

for contentRole in [kAXTextFieldRole, kAXStaticTextRole, "AXWebArea"] {
    var content = focusColdShell
    content.add(role:contentRole, children:contentRole == "AXWebArea" ? 1 : 0, depth:7, parentRole:kAXGroupRole)
    var opportunity = FocusSemanticReadiness(); opportunity.arm(now:100)
    backgroundCheck(content.decision(manualOptIn:true, elapsed:0.002, materializationRemaining:-20) == .ready)
    let extra = try opportunity.consume(shellOnly:content.shellOnly, complete:true, now:100.1,
        deadline:120, audit:{ _ in preconditionFailure("Native controls and mature web content return immediately") })
    backgroundCheck(extra == nil && !opportunity.consumed)
}
private var incompleteFocusScan = FocusSemanticReadiness(); incompleteFocusScan.arm(now:100)
private let incompleteFocusWait = try incompleteFocusScan.consume(shellOnly:true, complete:false,
    now:100.1, deadline:120, audit:{ _ in preconditionFailure("A truncated tree cannot prove a complete cold shell") })
backgroundCheck(incompleteFocusWait == nil && !incompleteFocusScan.consumed)
for now in [103.0, 104.0] {
    var opportunity = FocusSemanticReadiness(); opportunity.arm(now:100)
    let extra = try opportunity.consume(shellOnly:true, complete:true, now:now, deadline:120,
        audit:{ _ in preconditionFailure("An expired foreground opportunity does not read or wait") })
    backgroundCheck(extra == nil && opportunity.consumed)
}
private var limitedFocusReadiness = FocusSemanticReadiness(); limitedFocusReadiness.arm(now:100)
private let limitedFocusUntil = try limitedFocusReadiness.consume(shellOnly:true, complete:true,
    now:100.1, deadline:100.2, audit:{ until in backgroundCheck(until == 100.2); return true })
backgroundCheck(limitedFocusUntil == 100.2)
private var expiredCallerReadiness = FocusSemanticReadiness(); expiredCallerReadiness.arm(now:100)
private let expiredCallerWait = try expiredCallerReadiness.consume(shellOnly:true, complete:true,
    now:100.2, deadline:100.2, audit:{ _ in preconditionFailure("No work may outlive the caller deadline") })
backgroundCheck(expiredCallerWait == nil)

// Consume before the existing exact-current proof. A different current window,
// reused PID, or external epoch ends only the optional wait; no activation is
// requested. Permission/cancellation/deadline errors retain their original type.
for (focus, window, epochEnded) in [(cleanupApplication, CGWindowID(888), false),
    (FocusApplication(element:100, pid:Int32(10), generation:UInt64(43), fromCandidate:false), CGWindowID(777), false),
    (cleanupApplication, CGWindowID(777), true)] {
    var opportunity = FocusSemanticReadiness(); opportunity.arm(now:100)
    let extra = try opportunity.consume(shellOnly:true, complete:true, now:100.1, deadline:120, audit:{ _ in
        try borrowedFocusStillOwned(app:cleanupApp, windowID:777, invalidated:{epochEnded},
            focusedApplication:{focus}, currentWindowID:{_ in window})
    })
    backgroundCheck(extra == nil && opportunity.consumed)
    opportunity.arm(now:102)
    backgroundCheck(opportunity.until == 103)
}
for original in [failure("native_stopping", "cancelled before readiness focus proof"),
                 failure("deadline_exceeded", "original observation deadline"),
                 failure("permission_denied", "current focus permission denied"),
                 failure("target_changed", "current process identity changed")] {
    var opportunity = FocusSemanticReadiness(); opportunity.arm(now:100)
    let kept = expectedBackgroundError(original.code) {
        _ = try opportunity.consume(shellOnly:true, complete:true, now:100.1, deadline:120,
            audit:{ _ in throw original })
    }
    backgroundCheck(kept.message == original.message && opportunity.consumed)
    let again = try opportunity.consume(shellOnly:true, complete:true, now:100.2, deadline:120,
        audit:{ _ in preconditionFailure("A failed proof cannot reopen this epoch's wait") })
    backgroundCheck(again == nil)
}
private var retargetReadiness = FocusSemanticReadiness(); retargetReadiness.arm(now:100)
retargetReadiness.discard(); retargetReadiness.arm(now:101)
private let retargetWait = try retargetReadiness.consume(shellOnly:true, complete:true, now:101.1,
    deadline:120, audit:{ _ in preconditionFailure("A retarget cannot inherit or renew the old opportunity") })
backgroundCheck(retargetWait == nil)
private var absentFocusReadiness: FocusSemanticReadiness?
absentFocusReadiness?.arm(now:100)
backgroundCheck(absentFocusReadiness == nil)
print("Explicit-focus cold-shell wait is single-use, bounded and read-only; warm content, missing scopes and original stop/focus errors stay strict")


// An actual successful System AX snapshot contradicted by WindowServer has
// private provenance. Generic focus errors and birth replacement do not.
private let typedSnapshotMismatch = expectedBackgroundError("focus_unproven") {
    _ = try resolveFocusedApplication(system:{ 100 },
        candidate:{ () -> Int? in preconditionFailure("No source selection after successful AX") },
        pid:{ _ in 10 }, birth:{ _ in 42 }, frontmost:{ _ in false })
}
backgroundCheck(typedSnapshotMismatch.foregroundSnapshotMismatch)
backgroundCheck(typedSnapshotMismatch.details?["ax_pid"] as? Int32 == 10)
print("System AX/WindowServer disagreement retains typed mismatch evidence")
