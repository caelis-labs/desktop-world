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


// Return proof must identify the original window, including same-PID borrows.
// Synthetic AX reads exercise the actual bounded poll without touching a desktop.
private var returnClock = 0.0
private var returnReads = 0
private var returnInvalidated = false
private func restoredFocusRead(pid: Int32 = 10, sameObject: Bool, sameID: Bool) throws -> Bool {
    try exactInputFocus(expectedPID: 10, focusedPID: { pid }, windowMatches: { sameObject && sameID })
}
for mismatch in [(false, true), (true, false), (false, false)] {
    returnClock = 0; returnReads = 0
    _ = expectedBackgroundError("focus_restore_unproven") {
        _ = try waitForRestoredFocus(remaining: { 0.05 - returnClock }, check: {}, invalidated: { false }, read: {
            returnReads += 1
            return try restoredFocusRead(sameObject: mismatch.0, sameID: mismatch.1)
        }, wait: { returnClock += $0 })
    }
    backgroundCheck(returnReads == 3)
}
returnClock = 0; returnReads = 0
private let delayedReturn = try waitForRestoredFocus(remaining: { 0.1 - returnClock }, check: {}, invalidated: { false }, read: {
    returnReads += 1
    return try restoredFocusRead(sameObject: returnReads >= 3, sameID: returnReads >= 3)
}, wait: { returnClock += $0 })
backgroundCheck(delayedReturn && returnReads == 3 && returnClock == 0.04)

// A positively absent original focused window only needs exact app identity.
// NativeStore labels that successful branch application_restored, not restored.
private let applicationOnlyReturn = try waitForRestoredFocus(remaining: { 1 }, check: {}, invalidated: { false }, read: {
    try exactInputFocus(expectedPID: 10, focusedPID: { 10 }, windowMatches: { true })
}, wait: { _ in preconditionFailure("Available application return must not wait") })
backgroundCheck(applicationOnlyReturn)

// Already ended epochs do not query the target or accidentally claim restored.
private let endedReturn = try waitForRestoredFocus(remaining: { 1 }, check: {}, invalidated: { true }, read: {
    preconditionFailure("No return read after external focus choice")
}, wait: { _ in preconditionFailure("No wait after external focus choice") })
backgroundCheck(!endedReturn)
returnInvalidated = false
private let changedDuringRead = try waitForRestoredFocus(remaining: { 1 }, check: {}, invalidated: { returnInvalidated }, read: {
    returnInvalidated = true
    return true
}, wait: { _ in preconditionFailure("Do not wait after losing ownership") })
backgroundCheck(!changedDuringRead)
returnInvalidated = false
private var returnChecks = 0
private let changedAfterRead = try waitForRestoredFocus(remaining: { 1 }, check: {
    returnChecks += 1
    if returnChecks == 2 { returnInvalidated = true }
}, invalidated: { returnInvalidated }, read: { true }, wait: { _ in preconditionFailure("Do not wait after proof") })
backgroundCheck(!changedAfterRead)
returnInvalidated = false
private let failedReadAfterExternalChange = try waitForRestoredFocus(remaining: { 1 }, check: {}, invalidated: { returnInvalidated }, read: {
    returnInvalidated = true
    throw failure("ax_unavailable", "Object invalidated by independent choice")
}, wait: { _ in preconditionFailure("No read retry after external choice") })
backgroundCheck(!failedReadAfterExternalChange)

// Lost or replaced original process/window proof stops immediately, never loops
// and never obtains a successful result merely because the frontmost PID agrees.
for code in ["focus_restore_failed", "window_unproven", "deadline_exceeded"] {
    _ = expectedBackgroundError(code) {
        _ = try waitForRestoredFocus(remaining: { 1 }, check: {
            throw failure(code, "Original return identity no longer proven")
        }, invalidated: { false }, read: { preconditionFailure("No read after identity/deadline failure") },
        wait: { _ in preconditionFailure("No retry after identity/deadline failure") })
    }
}
returnChecks = 0
_ = expectedBackgroundError("focus_restore_failed") {
    _ = try waitForRestoredFocus(remaining: { 1 }, check: {
        returnChecks += 1
        if returnChecks == 2 { throw failure("focus_restore_failed", "Original app replaced during AX read") }
    }, invalidated: { false }, read: { true }, wait: { _ in preconditionFailure("No retry after app replacement") })
}
backgroundCheck(returnChecks == 2)

// Only typed missing focus/busy AX is eligible for the existing read-only poll.
for transient in [missingFocus(.window), busyAttribute("AXFocusedWindow")] {
    returnClock = 0; returnReads = 0
    let restored = try waitForRestoredFocus(remaining: { 0.1 - returnClock }, check: {}, invalidated: { false }, read: {
        returnReads += 1
        if returnReads == 1 { throw transient }
        return true
    }, wait: { returnClock += $0 })
    backgroundCheck(restored && returnReads == 2)
}
returnClock = 0
_ = expectedBackgroundError("ax_unavailable") {
    _ = try waitForRestoredFocus(remaining: { 0.03 - returnClock }, check: {}, invalidated: { false }, read: {
        throw busyAttribute("AXFocusedWindow")
    }, wait: { returnClock += $0 })
}
returnClock = 0
_ = expectedBackgroundError("focus_restore_unproven") {
    _ = try waitForRestoredFocus(remaining: { 0.03 - returnClock }, check: {}, invalidated: { false }, read: {
        returnClock = 0.04
        return true
    }, wait: { _ in preconditionFailure("Late proof cannot wait") })
}
returnClock = 0
_ = expectedBackgroundError("focus_restore_unproven") {
    _ = try waitForRestoredFocus(remaining: { 0.03 - returnClock }, check: {}, invalidated: { false }, read: {
        try restoredFocusRead(pid: 20, sameObject: true, sameID: true)
    }, wait: { returnClock += $0 })
}
returnClock = 0
_ = expectedBackgroundError("focus_restore_unproven") {
    _ = try waitForRestoredFocus(remaining: { 0.03 - returnClock }, check: {}, invalidated: { false }, read: {
        var pidReads = 0
        return try exactInputFocus(expectedPID: 10, focusedPID: {
            pidReads += 1; return pidReads == 1 ? 10 : 20
        }, windowMatches: { true })
    }, wait: { returnClock += $0 })
}
print("Exact focus restoration contracts passed (19 pure scenarios)")


// The return snapshot is not published if any exact original-window read or
// process proof changes while the initial AX capture is in progress.
try validateFocusSnapshot(check: {}, read: { true })
_ = expectedBackgroundError("focus_unproven") {
    try validateFocusSnapshot(check: {}, read: { false })
}
private var snapshotChecks = 0
_ = expectedBackgroundError("app_changed") {
    try validateFocusSnapshot(check: {
        snapshotChecks += 1
        if snapshotChecks == 2 { throw failure("app_changed", "Birth stamp changed during snapshot") }
    }, read: { true })
}
backgroundCheck(snapshotChecks == 2)
_ = expectedBackgroundError("ax_unavailable") {
    try validateFocusSnapshot(check: {}, read: { throw busyAttribute("AXFocusedWindow") })
}
print("Original focus snapshot controls passed (4 pure scenarios)")


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


// WindowServer may briefly omit its front PSN after activation. Only read-side
// restoration polling can recover; pre-action capture remains fail-before-input.
for transient: Int32 in [-3, -4, -5, -6] {
    returnClock = 0; returnReads = 0
    let restored = try waitForRestoredFocus(remaining: { 0.06 - returnClock }, check: {}, invalidated: { false }, read: {
        returnReads += 1
        return try frontProcessMatch(returnReads == 1 ? transient : 1)
            && restoredFocusRead(sameObject: true, sameID: true)
    }, wait: { returnClock += $0 })
    backgroundCheck(restored && returnReads == 2 && returnClock == 0.02)
    let beforeAction = expectedBackgroundError("focus_unproven") {
        try validateFocusSnapshot(check: { _ = try frontProcessMatch(transient) }, read: {
            preconditionFailure("Unproven foreground cannot publish a return snapshot")
        })
    }
    backgroundCheck(beforeAction.focusAbsence == .frontProcess)
    _ = expectedBackgroundError("focus_unproven") {
        _ = try focusedBeforeEstablishment { try frontProcessMatch(transient) }
    }
}
returnClock = 0; returnReads = 0
private let persistentFrontAbsence = expectedBackgroundError("focus_restore_unproven") {
    _ = try waitForRestoredFocus(remaining: { 0.03 - returnClock }, check: {}, invalidated: { false }, read: {
        returnReads += 1
        return try frontProcessMatch(returnReads == 1 ? -3 : -4)
    }, wait: { returnClock += $0 })
}
backgroundCheck(returnReads == 2 && returnClock == 0.03 && persistentFrontAbsence.focusAbsence == .frontProcess)
backgroundCheck((persistentFrontAbsence.details?["details"] as? [String: Any])?["native_result"] as? Int32 == -3)

// A proven PID is insufficient if the exact AX object/window is still wrong;
// valid mismatch after transient absence clears the obsolete unavailable cause.
returnClock = 0; returnReads = 0
private let delayedExactFrontReturn = try waitForRestoredFocus(remaining: { 0.1 - returnClock }, check: {}, invalidated: { false }, read: {
    returnReads += 1
    let result: Int32 = returnReads == 1 ? -3 : (returnReads == 2 ? 0 : 1)
    return try frontProcessMatch(result)
        && restoredFocusRead(sameObject: returnReads == 4, sameID: returnReads == 4)
}, wait: { returnClock += $0 })
backgroundCheck(delayedExactFrontReturn && returnReads == 4)
returnClock = 0; returnReads = 0
private let persistentFrontMismatch = expectedBackgroundError("focus_restore_unproven") {
    _ = try waitForRestoredFocus(remaining: { 0.03 - returnClock }, check: {}, invalidated: { false }, read: {
        returnReads += 1
        return try frontProcessMatch(returnReads == 1 ? -3 : 0)
    }, wait: { returnClock += $0 })
}
backgroundCheck(persistentFrontMismatch.focusAbsence == nil)

// Permanent query errors do not acquire the transient read-only retry path.
for permanent: Int32 in [-1, -2, 2] {
    returnReads = 0
    let error = expectedBackgroundError("focus_restore_unproven") {
        _ = try waitForRestoredFocus(remaining: { 1 }, check: {}, invalidated: { false }, read: {
            returnReads += 1; return try frontProcessMatch(permanent)
        }, wait: { _ in preconditionFailure("Permanent WindowServer failure cannot retry") })
    }
    backgroundCheck(returnReads == 1 && error.focusAbsence == nil)
}
// Deadline/cancel/replacement checks win even when the preceding read reports
// transient absence, and a user's external focus choice ends the return.
for code in ["deadline_exceeded", "native_stopping", "focus_restore_failed"] {
    returnChecks = 0
    _ = expectedBackgroundError(code) {
        _ = try waitForRestoredFocus(remaining: { 1 }, check: {
            returnChecks += 1
            if returnChecks == 2 { throw failure(code, "Return proof ended during native read") }
        }, invalidated: { false }, read: { try frontProcessMatch(-3) },
        wait: { _ in preconditionFailure("Ended return cannot reread") })
    }
    backgroundCheck(returnChecks == 2)
}
returnInvalidated = false
private let frontAbsenceAfterExternalChoice = try waitForRestoredFocus(remaining: { 1 }, check: {}, invalidated: { returnInvalidated }, read: {
    returnInvalidated = true; return try frontProcessMatch(-3)
}, wait: { _ in preconditionFailure("No WindowServer reread after external choice") })
backgroundCheck(!frontAbsenceAfterExternalChoice)
print("Post-activation WindowServer absence controls passed (read-only; no GUI access)")
