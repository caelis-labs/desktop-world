// Appended to Native.swift in one temporary main.swift. These controls exercise
// inventory classification and serialization: no AX requests, desktop reads or input dispatch.
private var inventoryAssertions = 0
private func expect(_ value: @autoclosure () -> Bool, _ message: String) {
    precondition(value(), message)
    inventoryAssertions += 1
}
private func expectFailure(_ diagnostics: WindowInventoryDiagnostics, _ messagePart: String) {
    do {
        try diagnostics.requireProven()
        preconditionFailure("unproven inventory was accepted")
    } catch let error as NativeFailure {
        expect(error.code == "window_inventory_unproven", "wrong inventory error")
        expect(error.message.contains(messagePart), "diagnostic reason absent: \(messagePart)")
        expect(error.message.utf8.count < 2048, "fixed diagnostic exceeded bound")
    } catch { preconditionFailure("unexpected error: \(error)") }
}
private func row(id: UInt32 = 10, pid: Int32 = 100, layer: Int = 0) -> [String: Any] {
    [kCGWindowNumber as String: id, kCGWindowOwnerPID as String: pid,
     kCGWindowLayer as String: layer,
     kCGWindowBounds as String: CGRect(x: 10, y: 20, width: 200, height: 100).dictionaryRepresentation,
     kCGWindowIsOnscreen as String: true,
     kCGWindowName as String: "DO_NOT_INCLUDE_THIS_TITLE"]
}
private func classified(pid: Int32 = 100, role: String? = kAXWindowRole,
                        subrole: String? = kAXStandardWindowSubrole,
                        identifier: String? = nil) throws -> WindowClassification {
    try classifyWindow(actualPID: pid, expectedPID: 100,
                       role: { role }, subrole: { subrole }, identifier: { identifier })
}
private func checkInventory() throws {
    var empty = WindowInventoryDiagnostics()
    let (none, excluded) = appCGInventory([], pid: 100, diagnostics: &empty)
    expect(none.isEmpty && excluded.isEmpty, "empty CG inventory changed")
    try empty.requireProven()

    var peer = WindowInventoryDiagnostics()
    let (peerRows, _) = appCGInventory([row(pid: 200)], pid: 100, diagnostics: &peer)
    expect(peerRows.isEmpty && peer.counts["cg_app_raw"] == 0, "other app entered inventory")
    try peer.requireProven()

    var missingAX = WindowInventoryDiagnostics()
    let (owned, _) = appCGInventory([row(), row(id: 11), row(id: 12, pid: 200)],
                                  pid: 100, diagnostics: &missingAX)
    expect(owned.count == 2 && missingAX.counts["cg_app_raw"] == 2, "wrong scoped CG count")
    expectFailure(missingAX, "ax_window_count=0")
    expectFailure(missingAX, "window_server_count=2")

    // More CG surfaces than AX windows does not itself prove a missing window.
    var decoration = missingAX
    decoration.counts["ax_window_count"] = 1
    decoration.counts["supported_count"] = 1
    try decoration.requireProven()

    var outsideScope = WindowInventoryDiagnostics()
    var nonLayer = row(layer: 3)
    nonLayer.removeValue(forKey: kCGWindowBounds as String)
    let (outsideRows, outsideIDs) = appCGInventory([nonLayer], pid: 100, diagnostics: &outsideScope)
    expect(outsideRows.isEmpty && outsideIDs == [10], "explicit layer exclusion lost ID")
    expect(outsideScope.counts["cg_non_layer_zero"] == 1, "layer exclusion count absent")
    expect(outsideScope.counts["cg_invalid_metadata"] == 0, "out-of-scope bounds treated as unproven")
    try outsideScope.requireProven()

    for field in [kCGWindowNumber, kCGWindowLayer, kCGWindowBounds] {
        var malformed = row()
        malformed.removeValue(forKey: field as String)
        var invalid = WindowInventoryDiagnostics()
        let (records, _) = appCGInventory([malformed], pid: 100, diagnostics: &invalid)
        expect(records.isEmpty, "malformed row became actionable")
        expectFailure(invalid, "cg_invalid_metadata=1")
    }
    var zeroSize = row()
    zeroSize[kCGWindowBounds as String] = CGRect(x: 0, y: 0, width: 0, height: 1).dictionaryRepresentation
    var invalidBounds = WindowInventoryDiagnostics()
    _ = appCGInventory([zeroSize], pid: 100, diagnostics: &invalidBounds)
    expectFailure(invalidBounds, "cg_invalid_metadata=1")

    let standard = try classified()
    expect(standard == .supported, "standard window rejected")
    for classification in [try classified(role: kAXMenuRole),
                           try classified(subrole: kAXDialogSubrole),
                           try classified(identifier: "open-panel"),
                           try classified(identifier: "save-panel")] {
        expect(classification == .excluded, "explicit unsupported surface was not excluded")
        var unsupported = WindowInventoryDiagnostics()
        unsupported.counts["ax_window_count"] = 1
        unsupported.increment(classification.rawValue)
        try unsupported.requireProven()
    }
    for (classification, reason) in [
        (try classified(pid: 200), "ax_pid_unproven"),
        (try classified(role: nil), "ax_role_unproven"),
        (try classified(subrole: nil), "ax_subrole_unproven"),
        (try classified(subrole: ""), "ax_subrole_unproven")
    ] {
        var mixed = decoration // One other candidate already proved successfully.
        mixed.increment(classification.rawValue)
        expectFailure(mixed, "\(reason)=1")
    }
    // Exclusion/PID rejection keeps the original short-circuit AX call behavior.
    var attributeReads = 0
    let wrongPID = try classifyWindow(actualPID: 200, expectedPID: 100,
        role: { attributeReads += 1; return kAXWindowRole },
        subrole: { attributeReads += 1; return kAXStandardWindowSubrole },
        identifier: { attributeReads += 1; return nil })
    expect(wrongPID == .pidUnproven && attributeReads == 0, "PID mismatch read another app's attributes")
    let excludedRole = try classifyWindow(actualPID: 100, expectedPID: 100,
        role: { kAXMenuRole }, subrole: { attributeReads += 1; return nil },
        identifier: { attributeReads += 1; return nil })
    expect(excludedRole == .excluded && attributeReads == 0, "excluded role triggered extra AX reads")
    // Preserve specific upstream failures instead of rewriting them to inventory errors.
    for code in ["deadline_exceeded", "permission_denied", "unsupported_modal", "ax_unavailable"] {
        do {
            _ = try classifyWindow(actualPID: 100, expectedPID: 100,
                role: { throw failure(code, "original") }, subrole: { nil }, identifier: { nil })
            preconditionFailure("read failure was swallowed")
        } catch let error as NativeFailure {
            expect(error.code == code && error.message == "original", "original error was replaced")
        }
    }

    var missingID = decoration
    missingID.exactIDFailure(.success) // SPI succeeded but produced ID zero.
    expect(missingID.axErrors.isEmpty, "ID zero fabricated an AX error")
    expectFailure(missingID, "exact_id_zero=1")
    var unavailable = decoration
    unavailable.exactIDFailure(.cannotComplete)
    expectFailure(unavailable, "exact_id_unproven=1")
    expect(unavailable.axErrors == [AXError.cannotComplete.rawValue], "SPI error code lost")
    for _ in 0..<128 { unavailable.exactIDFailure(.invalidUIElement) }
    expect(unavailable.axErrors.count == 8, "AX error samples are unbounded")
    expect(unavailable.counts["exact_id_unproven"] == 129, "AX errors beyond sample cap not counted")
    expectFailure(unavailable, "exact_id_unproven=129")
    for reason in ["cg_owner_unmatched", "duplicate_id"] {
        var mixed = decoration
        mixed.increment(reason)
        expectFailure(mixed, "\(reason)=1")
    }
    do { try missingAX.requireProven() }
    catch let error as NativeFailure {
        expect(!error.message.contains("DO_NOT_INCLUDE_THIS_TITLE"), "window title leaked into diagnostic")
        expect(!error.message.contains("200"), "peer PID leaked into diagnostic")
        let inventory = error.details?["inventory"] as? [String: Any]
        let counters = inventory?["diagnostics"] as? [String: Int]
        expect(counters?["ax_window_count"] == 0, "structured AX count missing")
        expect(counters?["window_server_count"] == missingAX.counts["window_server_count"], "structured CG count missing")
    }
}
try checkInventory()
private func checkSemanticSerialization() throws {
    expect(semanticRole(kAXButtonRole) == "button", "native button role was not normalized")
    expect(semanticRole(kAXCheckBoxRole) == "checkbox", "native checkbox role was not normalized")
    expect(semanticRole(kAXTextFieldRole) == "text_field", "native text field role was not normalized")
    for (native, expected) in [(NSAccessibility.Role.link.rawValue, "link"), (kAXTableRole, "table"),
                               (kAXRowRole, "row"), (kAXCellRole, "cell"), (kAXListRole, "list"),
                               (kAXTabGroupRole, "tab_group"), (kAXPopUpButtonRole, "pop_up_button"),
                               (kAXMenuItemRole, "menu_item"), (kAXHeadingRole, "heading")] {
        expect(semanticRole(native) == expected, "known native role was not normalized: \(expected)")
    }
    expect(semanticRole(kAXRadioButtonRole, subrole: NSAccessibility.Subrole.tabButtonSubrole.rawValue) == "tab",
           "native tab subrole was not normalized")
    expect(semanticRole(kAXButtonRole, subrole: "unrecognized") == "button", "unknown subrole changed role")
    expect(semanticRole("provider-specific role") == "unknown", "unknown role was guessed from prose")
    // Creating an opaque AX object makes no attribute requests or desktop reads.
    let native = AXUIElementCreateApplication(0)
    for enabled: Bool? in [nil, false, true] {
        let element = NativeElement(id: 1, element: native, label: "display-only text",
                                    role: "button", name: "Action", value: "current value",
                                    enabled: enabled, canPress: enabled == true, canSetValue: false)
        let bytes = try JSONSerialization.data(withJSONObject: element.json)
        let wire = try JSONSerialization.jsonObject(with: bytes) as! [String: Any]
        expect(wire["role"] as? String == "button", "semantic role was derived from label")
        expect(wire["name"] as? String == "Action", "semantic name lost its own attribute")
        expect(wire["value"] as? String == "current value", "semantic value lost its own attribute")
        expect(wire["label"] as? String == "display-only text", "legacy label changed")
        if let enabled { expect(wire["enabled"] as? Bool == enabled, "enabled Boolean changed") }
        else { expect(wire["enabled"] is NSNull, "unknown enabled state was fabricated") }
        expect(wire["actions"] as? [String] == (enabled == true ? ["press"] : []),
               "element advertises an unavailable action")
        expect(wire["can_press"] as? Bool == (enabled == true), "legacy can_press disagrees with actions")
    }
    let unnamed = NativeElement(id: 2, element: native, label: "AXGroup", role: "group",
                                name: nil, value: nil, enabled: nil, canPress: false, canSetValue: false).json
    expect(unnamed["name"] is NSNull && unnamed["value"] is NSNull,
           "missing semantic values must remain null, without label inference")
}
try checkSemanticSerialization()
let missingPixelTarget = dtwValidatePixelTarget([
    "deadline_unix_ms": UInt64((Date().timeIntervalSince1970 + 5) * 1_000)
])
expect(missingPixelTarget?["error"] as? String == "invalid_request",
       "pixel guard accepted a legacy request without a typed target")
private func checkFocusBorrowEpoch() {
    var epoch = FocusBorrowEpoch(initialPID: 10, targetPID: 20)
    epoch.activated(10)
    expect(!epoch.invalidated, "Initial foreground sample prematurely invalidated focus establishment")
    epoch.activated(20)
    expect(epoch.reachedTarget && !epoch.invalidated, "Expected activation was not recorded")
    expect(epoch.retarget(20), "Repeated focus of the same app lost its original borrow")
    epoch.activated(30); epoch.activated(20)
    expect(epoch.invalidated, "Leave then return incorrectly regained foreground ownership")
    expect(!epoch.retarget(20), "Repeated explicit focus revived an invalidated epoch")
    expect(!epoch.retarget(10), "Cleanup revived an invalidated epoch")

    var beforeArrival = FocusBorrowEpoch(initialPID: 10, targetPID: 20)
    beforeArrival.activated(30); beforeArrival.activated(20)
    expect(beforeArrival.invalidated, "External activation during establishment was forgotten")

    var restored = FocusBorrowEpoch(initialPID: 10, targetPID: 20)
    restored.activated(20)
    expect(restored.retarget(10), "Owned restoration could not change expected foreground")
    restored.activated(20); restored.activated(10)
    expect(restored.reachedTarget && !restored.invalidated, "Owned restoration invalidated its own epoch")
    restored.activated(30); restored.activated(10)
    expect(restored.invalidated, "User activation during restoration was forgotten")

    var newBorrow = FocusBorrowEpoch(initialPID: 30, targetPID: 20)
    newBorrow.activated(20)
    expect(newBorrow.retarget(30) && newBorrow.targetPID == 30,
           "New explicit focus did not retain the user's new return target")
}
private func checkWindowFocusBorrowEpoch() {
    var epoch = FocusBorrowEpoch(initialPID: 10, targetPID: 20, targetWindow: 100)
    epoch.focusedWindow(200) // Old key window before the requested focus reaches 100.
    expect(!epoch.invalidated, "Old key window prematurely invalidated focus establishment")
    epoch.activated(20); epoch.focusedWindow(100)
    expect(epoch.reachedWindow, "Exact target arrival was not recorded")
    expect(epoch.retarget(20, window: 100), "Repeated exact window focus ended the borrow")
    epoch.focusedWindow(200); epoch.focusedWindow(100)
    expect(epoch.invalidated, "Sibling window leave/return revived the borrowed foreground")
    expect(!epoch.retarget(20, window: 100), "Window epoch became reusable after external change")

    var owned = FocusBorrowEpoch(initialPID: 10, targetPID: 20, targetWindow: 100)
    owned.activated(20); owned.focusedWindow(100)
    expect(owned.retarget(20, window: 200), "An explicit sibling focus could not update the expected window")
    owned.focusedWindow(100); owned.focusedWindow(200)
    expect(owned.reachedWindow && !owned.invalidated, "Owned sibling transition was treated as external")
    owned.focusedWindow(100); owned.focusedWindow(200)
    expect(owned.invalidated, "A later external sibling transition was forgotten")
}
checkWindowFocusBorrowEpoch()
private func checkBackgroundStepScope() throws {
    let app = NativeApp(identity: "owned", pid: 100, generation: 5)
    let target = NativeTarget(app: app, nativeID: 10, generation: 7)
    let milliseconds = UInt64((Date().timeIntervalSince1970 + 10) * 1_000)
    let context = try RequestContext(["deadline_unix_ms": milliseconds])
    let scope = BackgroundInputValidation(target: target, deadlineMilliseconds: milliseconds,
                                          keyboard: true, context: context, window: nil)
    expect(scope.matches(target: target, deadlineMilliseconds: milliseconds, keyboard: true),
           "Same admitted input action could not reuse its validation context")
    expect(!scope.matches(target: target, deadlineMilliseconds: milliseconds + 1, keyboard: true),
           "A later deadline extended an admitted action")
    expect(!scope.matches(target: target, deadlineMilliseconds: milliseconds, keyboard: false),
           "Pointer action reused keyboard validation")
    let replaced = NativeTarget(app: app, nativeID: 10, generation: 8)
    expect(!scope.matches(target: replaced, deadlineMilliseconds: milliseconds, keyboard: true),
           "A replacement window reused prior validation")
    expect(scope.context === context && scope.context.deadline.uptime == context.deadline.uptime,
           "Background step re-created the original monotonic deadline")
}
checkFocusBorrowEpoch()
try checkBackgroundStepScope()
print("Native inventory controls passed (\(inventoryAssertions) assertions; no GUI access)")
