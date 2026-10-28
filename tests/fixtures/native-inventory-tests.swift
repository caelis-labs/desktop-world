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
print("Native inventory controls passed (\(inventoryAssertions) assertions; no GUI access)")
