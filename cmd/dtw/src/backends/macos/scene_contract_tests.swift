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
