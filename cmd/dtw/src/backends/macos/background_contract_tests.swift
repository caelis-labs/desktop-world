// Native.swift's Rust foreground bridge is linked only in the product binary.
// Pure controls must never activate a real application.
@_cdecl("dtw_set_front_process")
func dtwSetFrontProcessControlStub(_ pid: Int32, _ window: UInt32) -> Bool {
    preconditionFailure("Pure contracts must not invoke foreground input")
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
