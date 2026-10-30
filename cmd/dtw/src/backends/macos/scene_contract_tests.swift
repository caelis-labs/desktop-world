// Append after Input.swift, Native.swift, Scene.swift. No desktop/AX/SCK reads.
private let cursorLevel = Int(CGWindowLevelForKey(.cursorWindow))
private let cursorExecutable = windowServerExecutable
precondition(isSystemCursor(layer: cursorLevel, executable: { cursorExecutable }))
precondition(!isSystemCursor(layer: cursorLevel, executable: { "/Applications/Fake.app/Contents/MacOS/WindowServer" }))
precondition(!isSystemCursor(layer: cursorLevel, executable: { nil }))
precondition(!isSystemCursor(layer: cursorLevel - 1, executable: { cursorExecutable }))
private let cover = Window(pid:1,id:1,layer:9,rank:0,rect:CGRect(x:0,y:0,width:100,height:100))
precondition(transparentCovers([cover], permitted:{true}, sample:{_ in 0}))
precondition(!transparentCovers([cover], permitted:{true}, sample:{_ in 1}))
precondition(!transparentCovers([cover], permitted:{true}, sample:{_ in nil}))
private var samples = 0
precondition(!transparentCovers(Array(repeating:cover,count:9),permitted:{true},sample:{_ in samples += 1; return 0}))
precondition(samples == 0)
private var closing = false
precondition(!transparentCovers([cover],permitted:{!closing},sample:{_ in closing = true; return 0}))
samples = 0
precondition(!transparentCovers([cover],permitted:{false},sample:{_ in samples += 1;return 0}))
precondition(samples == 0)

private var clock: TimeInterval = 0
private var waits = 0
precondition(!dtwWaitForScene(until:0.025,stopping:{false},now:{clock},wait:{interval in
    waits += 1; precondition(interval <= 0.01); clock += interval; return false
}))
precondition(waits == 3 && clock == 0.025)
waits = 0
precondition(!dtwWaitForScene(until:1,stopping:{true},now:{0},wait:{_ in waits += 1;return true}))
precondition(waits == 0)
closing = false
precondition(!dtwWaitForScene(until:1,stopping:{closing},now:{0},wait:{_ in closing = true;return true}))
precondition(dtwWaitForScene(until:1,stopping:{false},now:{0},wait:{_ in true}))
print("Point-alpha proof and bounded cancellation controls passed; no SCK/GUI calls")

// Deterministic first-point readiness: closures only, no OS/window/input calls.
private let readyPoint = SurfacePointProof(target: cover, point: CGPoint(x:50,y:50), stack: [], overlayCandidates: [])
private func pointFailure(_ body: () throws -> SurfacePointProof) -> String {
    do { _ = try body(); preconditionFailure("Expected point readiness rejection") }
    catch let error as PointReadinessFailure { return error.result["error"] as? String ?? "missing_error" }
    catch { preconditionFailure("Unexpected point readiness error: \(error)") }
}
private var pointOrder: [String] = []
private var pointAttempts = 0
clock = 0
let settledPoint = try waitForFirstPoint(deadline: 5, stopping: { false }, now: { clock }, validate: { until in
    precondition(until == 0.75)
    pointOrder.append("validate"); return nil
}, prove: { until in
    precondition(until == 0.75)
    pointOrder.append("proof"); pointAttempts += 1
    return pointAttempts == 3 ? readyPoint : nil
}, wait: { interval in
    precondition(interval > 0 && interval <= 0.015)
    pointOrder.append("wait"); clock += interval
})
// A caller can proceed only after proof and the final original-target check.
precondition(settledPoint.target.id == cover.id)
pointOrder.append("dispatch")
precondition(pointOrder == ["validate","proof","validate","wait",
    "validate","proof","validate","wait","validate","proof","validate","dispatch"])

clock = 0
precondition(pointFailure {
    try waitForFirstPoint(deadline: 5, stopping: { false }, now: { clock }, validate: { _ in nil },
                         prove: { until in precondition(until == 0.75); return nil },
                         wait: { clock += $0 })
} == "target_occluded")
precondition(abs(clock - 0.75) < 0.000001)
clock = 0
precondition(pointFailure {
    try waitForFirstPoint(deadline: 0.025, stopping: { false }, now: { clock }, validate: { _ in nil },
                         prove: { until in precondition(until == 0.025); return nil },
                         wait: { clock += $0 })
} == "deadline_exceeded")
precondition(clock == 0.025)

pointAttempts = 0
precondition(pointFailure {
    try waitForFirstPoint(deadline: 1, stopping: { true }, now: { 0 },
                         validate: { _ in preconditionFailure("Cancelled validation ran") },
                         prove: { _ in pointAttempts += 1; return readyPoint },
                         wait: { _ in preconditionFailure("Cancelled readiness waited") })
} == "native_stopping")
precondition(pointAttempts == 0)

clock = 0
precondition(pointFailure {
    try waitForFirstPoint(deadline: 1, stopping: { false }, now: { clock },
                         validate: { _ in clock = 0.8; return nil },
                         prove: { _ in preconditionFailure("Validation exceeded readiness budget") })
} == "target_occluded")
clock = 0
precondition(pointFailure {
    try waitForFirstPoint(deadline: 0.025, stopping: { false }, now: { clock }, validate: { _ in nil },
                         prove: { _ in clock = 0.03; return readyPoint })
} == "deadline_exceeded")
closing = false
precondition(pointFailure {
    try waitForFirstPoint(deadline: 1, stopping: { closing }, now: { 0 }, validate: { _ in nil },
                         prove: { _ in closing = true; return readyPoint })
} == "native_stopping")
closing = false
precondition(pointFailure {
    try waitForFirstPoint(deadline: 1, stopping: { closing }, now: { 0 },
                         validate: { _ in closing = true; return nil },
                         prove: { _ in preconditionFailure("Cancelled validation admitted proof") })
} == "native_stopping")

precondition(pointFailure {
    try waitForFirstPoint(deadline: 1, stopping: { false }, now: { 0 },
                         validate: { _ in ["error":"target_changed"] },
                         prove: { _ in preconditionFailure("Invalid original target admitted proof") },
                         wait: { _ in preconditionFailure("Invalid target was retried") })
} == "target_changed")
private var pointTargetMoved = false
precondition(pointFailure {
    try waitForFirstPoint(deadline: 1, stopping: { false }, now: { 0 },
                         validate: { _ in pointTargetMoved ? ["error":"stale_observation"] : nil },
                         prove: { _ in pointTargetMoved = true; return readyPoint },
                         wait: { _ in preconditionFailure("Moved target was retried") })
} == "stale_observation")
precondition(pointFailure {
    try waitForFirstPoint(deadline: 1, stopping: { false }, now: { 0 }, validate: { _ in nil },
                         prove: { _ in throw PointReadinessFailure(result: ["error":"stale_observation"]) },
                         wait: { _ in preconditionFailure("Invalid proof target was retried") })
} == "stale_observation")
print("First-point readiness bounds, post-read guards and target invalidation passed; no GUI input")


// Private pointer presentation is optional and reports only its supplied post.
// No event is generated, and a callback can unregister itself without deadlock.
private var pointerPresentations: [(Double, Double, Int32)] = []
dtwSetPointerPresenter { x, y, kind in pointerPresentations.append((x, y, kind)) }
for kind: Int32 in 0...3 { dtwPresentPostedPointer(CGPoint(x: 10, y: 20), kind: kind) }
precondition(pointerPresentations.count == 4)
precondition(pointerPresentations.map { $0.2 } == [0, 1, 2, 3])
precondition(pointerPresentations.allSatisfy { $0.0 == 10 && $0.1 == 20 })
dtwPresentPostedPointer(CGPoint(x: Double.nan, y: 20), kind: 0)
dtwPresentPostedPointer(CGPoint(x: 10, y: 20), kind: 4)
precondition(pointerPresentations.count == 4)
dtwSetPointerPresenter { _, _, _ in dtwSetPointerPresenter(nil) }
dtwPresentPostedPointer(CGPoint(x: 10, y: 20), kind: 1)
dtwPresentPostedPointer(CGPoint(x: 10, y: 20), kind: 2)
precondition(pointerPresentations.count == 4)

// An exact helper window is excluded only while kernel birth and WindowServer
// ownership agree. Sibling windows, spoofed owners and unproven reuse stay.
private let overlayCandidate = SceneOverlayCandidate(pid: 7, birth: 700, windows: [70])
private let overlayWindow = Window(pid: 7, id: 70, layer: 99, rank: 0, rect: cover.rect)
private let helperSibling = Window(pid: 7, id: 71, layer: 99, rank: 1, rect: cover.rect)
private let spoofedOwner = Window(pid: 8, id: 70, layer: 99, rank: 0, rect: cover.rect)
private var overlayReadOrder: [String] = []
private let excluded = withoutVerifiedOverlays([overlayCandidate], birth: { pid in
    overlayReadOrder.append("birth:\(pid)"); return 700
}, read: {
    overlayReadOrder.append("windows"); return [overlayWindow, helperSibling, cover]
})
precondition(overlayReadOrder == ["birth:7", "windows", "birth:7"])
precondition(excluded.map(\.id) == [71, 1])
precondition(withoutVerifiedOverlays([overlayCandidate], birth: { _ in 700 }, read: { [spoofedOwner] }).count == 1)
for stamps: [UInt64?] in [[nil, 700], [700, nil], [700, 701], [701, 701]] {
    var reads = stamps.makeIterator()
    precondition(withoutVerifiedOverlays([overlayCandidate], birth: { _ in reads.next()! },
                                        read: { [overlayWindow] }).count == 1)
}
precondition(withoutVerifiedOverlays([], birth: { _ in preconditionFailure("No candidate needs a birth read") },
                                    read: { [overlayWindow] }).count == 1)
let parsedOverlay = dtwSceneOverlayCandidates(["_overlay_candidates": [["pid":7, "birth":700, "windows":[70]]]])
precondition(parsedOverlay.count == 1 && parsedOverlay[0].windows == [70])
for request: [String: Any] in [[:], ["_overlay_candidates": [["pid":7, "windows":[70]]]],
    ["_overlay_candidates": [["pid":7, "birth":false, "windows":[70]]]],
    ["_overlay_candidates": [["pid":7, "birth":700, "windows":[true]]]],
    ["_overlay_candidates": [["pid":7, "birth":700, "windows":[70.5]]]],
    ["_overlay_candidates": [["pid":7, "birth":700, "windows":[]]]]] {
    precondition(dtwSceneOverlayCandidates(request).isEmpty)
}
// A previously excluded surface returns to the final hit stack if its birth
// proof changes. Reusing the candidate list does not reuse its old validation.
private let initiallyClear = withoutVerifiedOverlays([overlayCandidate], birth: { _ in 700 }, read: { [overlayWindow, cover] })
private let finallyUnproven = withoutVerifiedOverlays([overlayCandidate], birth: { _ in 701 }, read: { [overlayWindow, cover] })
precondition(hitStack(initiallyClear, id: cover.id, point: CGPoint(x: 50, y: 50)).isEmpty)
precondition(!hitStack(finallyUnproven, id: cover.id, point: CGPoint(x: 50, y: 50)).isEmpty)
print("Pointer presenter and exact overlay identity controls passed; no GUI input")
