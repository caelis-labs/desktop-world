// Concatenated with Native.swift by the native inventory controls. No desktop reads or input.
private func checkLaunchPlacement() {
    let initial: Int32 = 10
    let target: Int32 = 20
    let other: Int32 = 30
    var history = LaunchActivationHistory(initialPID: initial)
    precondition(history.permits(current: initial, target: target, restorationRequested: false))
    precondition(history.permits(current: target, target: target, restorationRequested: false),
                 "A fresh WindowServer sample can precede the target activation notification")
    precondition(!history.permits(current: other, target: target, restorationRequested: false))
    history.activated(target)
    precondition(history.permits(current: target, target: target, restorationRequested: false))
    precondition(!history.permits(current: initial, target: target, restorationRequested: false),
                 "An unattributed return to the initial app should stop placement")
    history.activated(initial)
    precondition(history.permits(current: initial, target: target, restorationRequested: true))
    history.activated(target)
    precondition(!history.permits(current: target, target: target, restorationRequested: true),
                 "The launch must not reclaim a second foreground transition")

    var external = LaunchActivationHistory(initialPID: initial)
    external.activated(other)
    external.activated(initial)
    precondition(!external.permits(current: initial, target: target, restorationRequested: false),
                 "A switch away and back invalidates launch placement")

    func launchRow(_ id: UInt32, _ pid: Int32, layer: Int = 0, onscreen: Bool = true,
                   alpha: Double = 1) -> [String: Any] {
        [kCGWindowNumber as String: id, kCGWindowOwnerPID as String: pid,
         kCGWindowLayer as String: layer, kCGWindowIsOnscreen as String: onscreen,
         kCGWindowBounds as String: CGRect(x: 10, y: 20, width: 200, height: 100).dictionaryRepresentation,
         kCGWindowAlpha as String: alpha]
    }
    let first = launchRow(101, target)
    let second = launchRow(102, target)
    let witness = launchRow(103, initial)
    let helper = launchRow(104, other)
    let original = LaunchWindowOrder.Window(id: 103, pid: initial)
    let mixed = LaunchWindowOrder([first, witness, helper, second])
    let expected = LaunchWindowOrder([witness, first, helper, second]).windows
    precondition(mixed.placing(target, behind: original) == expected,
                 "Only the launch window above the witness should cross it")
    let multiple = LaunchWindowOrder([first, second, witness, helper])
    precondition(multiple.placing(target, behind: original)
                 == LaunchWindowOrder([witness, first, second, helper]).windows,
                 "Multiple windows retain their relative order")
    let already = LaunchWindowOrder([helper, witness, first, second])
    precondition(already.placing(target, behind: original) == already.windows)
    let unrelatedAbove = LaunchWindowOrder([first, helper, witness, second])
    precondition(unrelatedAbove.placing(target, behind: original) == nil,
                 "An unrelated window above the witness forbids raising it")
    precondition(LaunchWindowOrder([first, helper]).placing(target, behind: original) == nil)
    precondition(LaunchWindowOrder([first, first, witness]).placing(target, behind: original) == nil)
    var unknown = helper
    unknown.removeValue(forKey: kCGWindowOwnerPID as String)
    precondition(LaunchWindowOrder([first, unknown, witness]).placing(target, behind: original) == nil,
                 "An unknown window owner must not disappear from placement proof")
    let excluded = LaunchWindowOrder([launchRow(105, target, layer: 3),
                                      launchRow(106, target, onscreen: false),
                                      launchRow(107, target, alpha: 0), witness])
    precondition(excluded.windows == [original])
    print("native launch placement controls passed")
}

checkLaunchPlacement()
