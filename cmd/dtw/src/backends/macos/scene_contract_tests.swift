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
private let readyPoint = SurfacePointProof(target: cover, point: CGPoint(x:50,y:50), stack: [])
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
