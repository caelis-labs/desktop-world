// Append after Input.swift, Native.swift, Scene.swift; never calls native input.
private var calls: [String] = []
private var blocked = false
private let stopped = dtwClickSequence(count:2,validateNext:{
    calls.append("validate")
    return blocked ? ["error":"stale_observation","pointer_moved":true] : nil
},pair:{index in
    calls.append("down\(index)");blocked=true;calls.append("up\(index)");return true
},between:{calls.append("between")})
precondition(stopped?["error"] as? String == "stale_observation")
precondition(calls == ["down1","up1","between","validate"])
private var normal: [String] = []
precondition(dtwClickSequence(count:2,validateNext:{normal.append("validate");return nil},pair:{i in normal.append("down\(i)");normal.append("up\(i)");return true},between:{normal.append("between")}) == nil)
precondition(normal == ["down1","up1","between","validate","down2","up2"])
print("Paired multi-click boundaries passed; no GUI input")
