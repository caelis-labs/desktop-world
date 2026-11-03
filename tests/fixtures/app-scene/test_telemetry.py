#!/usr/bin/env python3
"""Headless numeric regression for Scene telemetry; creates no windows or input."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


@unittest.skipUnless(sys.platform == "darwin", "AppKit fixture requires macOS")
class PointerTelemetryTests(unittest.TestCase):
    def test_unavailable_coordinates_remain_observable_and_json_safe(self):
        # Compile the actual fixture definitions without its GUI entry point.
        source = Path(__file__).with_name("main.swift").read_text()
        entry = "let delegate=Delegate();let app=NSApplication.shared;"
        self.assertEqual(source.count(entry), 1)
        source = source.split(entry)[0]
        checks = r'''
let cases: [(Double,Double)] = [(0,0),(-12.5,74),(.nan,.nan),
    (.infinity,-.infinity),(23,.nan),(.nan,-8)]
for (x,y) in cases {
    // Construct an event object only; never post it or launch NSApplication.
    let event=NSEvent.mouseEvent(with:.leftMouseDown,location:NSPoint(x:x,y:y),
        modifierFlags:[],timestamp:0,windowNumber:42,context:nil,
        eventNumber:7,clickCount:1,pressure:1)!
    let row=pointerEventTelemetry(event)
    precondition(row["type"] as? UInt == NSEvent.EventType.leftMouseDown.rawValue)
    precondition(row["window"] as? Int == 42)
    precondition(row["location_valid"] as? Bool == (x.isFinite && y.isFinite))
    for (key,value) in [("x",x),("y",y)] {
        if value.isFinite {
            precondition((row[key] as? NSNumber)?.doubleValue == value)
        } else {
            precondition(row[key] is NSNull, "unavailable coordinate must not become zero")
        }
    }
    let state:[String:Any] = ["pointer_events":[row],
        "inputs":["scene":"independently observed text"],"command":"allow_quit"]
    precondition(JSONSerialization.isValidJSONObject(state))
    let data=try JSONSerialization.data(withJSONObject:state)
    let decoded=try JSONSerialization.jsonObject(with:data) as! [String:Any]
    precondition((decoded["inputs"] as? [String:String])?["scene"] == "independently observed text")
    precondition(decoded["command"] as? String == "allow_quit")
}
print("6 coordinate cases passed: finite/zero/NaN/infinities/partial invalid; state JSON preserved")
'''
        with tempfile.TemporaryDirectory(prefix="dtw-scene-telemetry-") as name:
            root = Path(name)
            main = root / "main.swift"
            main.write_text(source + checks)
            binary = root / "telemetry-tests"
            subprocess.run(["xcrun", "swiftc", "-warnings-as-errors", "-module-cache-path",
                            str(root / "module-cache"), str(main), "-o", str(binary)],
                           check=True, timeout=90)
            result = subprocess.run([str(binary)], check=True, text=True,
                                    capture_output=True, timeout=10)
            self.assertIn("6 coordinate cases passed", result.stdout)
            print(result.stdout.strip())


if __name__ == "__main__":
    unittest.main()
