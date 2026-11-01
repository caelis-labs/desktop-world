import AppKit
import ApplicationServices
import Foundation

// Owned GUI acceptance app. All text, event logs and saved files stay in the
// explicit output directory; no clipboard, external document or network use.
let output = URL(fileURLWithPath: CommandLine.arguments.last!, isDirectory: true)
// Optional owned-fixture setup only. The default preserves native_input_probe.
let fixtureConfiguration = (try? Data(contentsOf: output.appendingPathComponent("fixture.json")))
    .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] } ?? [:]
let fixtureApplication = NSApplication.shared
fixtureApplication.setActivationPolicy(.regular)
final class FlippedDocument: NSView { override var isFlipped: Bool { true } }
final class ClickPad: NSView {
    var emit: ((String, NSEvent) -> Void)?
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func draw(_ dirty: NSRect) {
        NSColor.systemOrange.setFill(); bounds.fill()
        ("Click pad" as NSString).draw(at:NSPoint(x:16,y:bounds.midY),withAttributes:[.font:NSFont.systemFont(ofSize:18)])
    }
    override func mouseDown(with event: NSEvent) { emit?("left_down", event) }
    override func mouseUp(with event: NSEvent) { emit?("left_up", event) }
    override func rightMouseDown(with event: NSEvent) { emit?("right_down", event) }
    override func rightMouseUp(with event: NSEvent) { emit?("right_up", event) }
}
final class Delegate: NSObject, NSApplicationDelegate, NSTextFieldDelegate {
    var window: NSWindow!
    var field: NSTextField!
    var scroll: NSScrollView!
    var pad: ClickPad!
    var timer: Timer?
    var monitor: Any?
    var command = ""
    var saved = ""
    var saves = 0
    var eventCount = 0
    var lastPublishedUptime: TimeInterval = 0
    var padCounts = ["left_down":0,"left_up":0,"right_down":0,"right_up":0,"double_down":0]
    var events: [[String:Any]] = []
    var eventFile: FileHandle!
    var axHidden = false
    var coverAfterFirst = false
    var overlay: NSWindow?
    // Opt-in owned application-surface probe. Keep the closed object retained
    // to distinguish a dead surface from a lingering WindowServer backing row.
    var applicationSurface: NSWindow?
    var applicationSurfaceID: Int?
    var applicationSurfacePad: ClickPad?
    var applicationSurfaceCounts = ["left_down":0,"left_up":0,"right_down":0,"right_up":0]
    var secondaryWindow: NSWindow?
    var secondaryWindowID: Int?
    var closedWindowIDs: Set<Int> = []
    var windowCloseObserver: NSObjectProtocol?
    var secondaryField: NSTextField?
    var desktopFile: FileHandle?
    var focusFile: FileHandle?
    var activationObserver: NSObjectProtocol?
    func journal(_ value: [String:Any], to file: FileHandle?) {
        guard let file, let data = try? JSONSerialization.data(withJSONObject:value,options:[.sortedKeys]) else { return }
        try? file.write(contentsOf:data + Data("\n".utf8))
    }
    let keys: [CGKeyCode] = [0,1,2,3,4,5,6,7,8,9,11,12,13,14,15,16,17,36,48,55,56,58,59]
    var text: String { (field.currentEditor() as? NSTextView)?.string ?? field.stringValue }
    func record(_ kind: String, _ event: NSEvent) {
        eventCount += 1
        var entry: [String:Any] = ["sequence":eventCount,"kind":kind,"timestamp":event.timestamp,
            "uptime":ProcessInfo.processInfo.systemUptime,"key_code": [.keyDown,.keyUp,.flagsChanged].contains(event.type) ? Int(event.keyCode) : -1,
            "characters": event.type == .keyDown || event.type == .keyUp ? event.characters ?? "" : "",
            "flags":event.modifierFlags.rawValue,"click_count":event.type == .leftMouseDown || event.type == .rightMouseDown ? event.clickCount : 0]
        entry["window_number"] = event.windowNumber
        entry["location_in_window"] = [event.locationInWindow.x, event.locationInWindow.y]
        if event.type == .scrollWheel {
            entry["delta"] = [event.deltaX, event.deltaY]
            entry["scrolling_delta"] = [event.scrollingDeltaX, event.scrollingDeltaY]
            entry["precise"] = event.hasPreciseScrollingDeltas
        }
        events.append(entry); events = Array(events.suffix(64))
        if let data = try? JSONSerialization.data(withJSONObject:entry,options:[.sortedKeys]) {
            try? eventFile.write(contentsOf:data + Data("\n".utf8))
        }
        // The 20 ms snapshot timer publishes state. Re-encoding the rolling
        // journal for every down/up would block AppKit's AX server under text
        // input; the append-only event journal above still retains every event.
    }
    func saveState() {
        guard field != nil else { return }
        let primaryHeight = CGDisplayBounds(CGMainDisplayID()).height
        let point = window.convertPoint(toScreen:pad.convert(NSPoint(x:pad.bounds.midX,y:pad.bounds.midY),to:nil))
        let frame = window.frame
        let cursor = NSEvent.mouseLocation
        let localCursor = window.convertPoint(fromScreen: cursor)
        let hit = window.contentView?.hitTest(localCursor)
        let scrollFrame = window.convertToScreen(scroll.convert(scroll.bounds, to:nil))
        // AppKit's isVisible can precede WindowServer publication. Query only
        // this fixture's window; readiness must also prove its exact owner.
        let ownPID = ProcessInfo.processInfo.processIdentifier
        let ownWindow = window.windowNumber
        let rows = CGWindowListCopyWindowInfo(.optionIncludingWindow, CGWindowID(ownWindow)) as? [[String:Any]]
        let ownRows = rows?.filter {
            ($0[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == ownPID &&
            ($0[kCGWindowNumber as String] as? NSNumber)?.intValue == ownWindow
        } ?? []
        let windowServer: [String:Any] = [
            "query_succeeded":rows != nil, "owner_pid":ownPID, "window_id":ownWindow,
            "present":ownRows.count == 1,
            "on_screen":ownRows.first?[kCGWindowIsOnscreen as String] as? NSNumber ?? NSNull()
        ]
        let publishedUptime = max(ProcessInfo.processInfo.systemUptime, lastPublishedUptime.nextUp)
        lastPublishedUptime = publishedUptime
        let frontmostPID = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0
        let desktop: [String:Any] = ["uptime":publishedUptime,"frontmost_pid":frontmostPID,
                                     "pid":ownPID,"key_window":fixtureApplication.keyWindow?.windowNumber ?? 0,
                                     "pointer_screen":[cursor.x,primaryHeight-cursor.y]]
        journal(desktop,to:desktopFile)
        var value: [String:Any] = ["pid":ProcessInfo.processInfo.processIdentifier,"ready":true,"command":command,
            "frontmost_pid":frontmostPID,
            "published_uptime":publishedUptime,
            "window_server":windowServer,
            "active":fixtureApplication.isActive,"key_window":fixtureApplication.keyWindow?.windowNumber ?? 0,
            "text":text,"field_value":field.stringValue,"saved":saved,"saves":saves,
            "events_count":eventCount,"events":events,"pad":padCounts,"ax_hidden":axHidden,
            "cover_after_first":coverAfterFirst,"overlay_window":overlay?.windowNumber ?? 0,
            "overlay_visible":overlay?.isVisible ?? false,
            "keys_down_global":keys.filter { CGEventSource.keyState(.combinedSessionState,key:$0) }.map(Int.init),
            "modifier_flags_global":CGEventSource.flagsState(.combinedSessionState).rawValue,
            "buttons_down_global":[CGMouseButton.left, .right].filter { CGEventSource.buttonState(.combinedSessionState,button:$0) }.map { Int($0.rawValue) },
            "scroll_y":scroll.contentView.bounds.origin.y,
            "pointer_screen":[cursor.x, primaryHeight-cursor.y],
            "pointer_window":[localCursor.x,localCursor.y],
            "pointer_view":hit.map { String(describing:type(of:$0)) } ?? "none",
            "scroll_screen_bounds":[scrollFrame.minX,primaryHeight-scrollFrame.maxY,scrollFrame.width,scrollFrame.height],
            "window_bounds":[frame.minX,primaryHeight-frame.maxY,frame.width,frame.height],
            "pad_screen_point":[point.x,primaryHeight-point.y],
            "closed_window_ids":closedWindowIDs.sorted(),
            "windows":[["id":window.windowNumber,"title":window.title,"visible":window.isVisible]]]
        if let secondaryWindow, let secondaryField {
            value["windows"] = [["id":window.windowNumber,"title":window.title,"visible":window.isVisible],
                                ["id":secondaryWindowID ?? secondaryWindow.windowNumber,"title":secondaryWindow.title,"visible":secondaryWindow.isVisible]]
            value["secondary_text"] = (secondaryField.currentEditor() as? NSTextView)?.string ?? secondaryField.stringValue
        }
        if let surface = applicationSurface, let surfacePad = applicationSurfacePad,
           let surfaceID = applicationSurfaceID {
            let surfacePoint = surface.convertPoint(toScreen:surfacePad.convert(
                NSPoint(x:surfacePad.bounds.midX,y:surfacePad.bounds.midY),to:nil))
            let surfaceFrame = surface.frame
            let surfaceRows = CGWindowListCopyWindowInfo(.optionIncludingWindow,CGWindowID(surfaceID)) as? [[String:Any]]
            let surfaceRow = surfaceRows?.first {
                ($0[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == ownPID &&
                ($0[kCGWindowNumber as String] as? NSNumber)?.intValue == surfaceID
            }
            value["application_surface"] = ["id":surfaceID,"title":surface.title,"visible":surface.isVisible,
                "level":surface.level.rawValue,"window_server_present":surfaceRow != nil,
                "window_server_layer":surfaceRow?[kCGWindowLayer as String] as? NSNumber ?? NSNull(),
                "bounds":[surfaceFrame.minX,primaryHeight-surfaceFrame.maxY,surfaceFrame.width,surfaceFrame.height],
                "pad_screen_point":[surfacePoint.x,primaryHeight-surfacePoint.y],"pad":applicationSurfaceCounts]
        }
        try? JSONSerialization.data(withJSONObject:value,options:[.sortedKeys]).write(to:output.appendingPathComponent("state.json"),options:.atomic)
    }
    func openApplicationSurface() {
        guard applicationSurface == nil else { return } // One setup, no hidden reopen.
        let surface = NSWindow(contentRect:NSRect(x:window.frame.minX+60,y:window.frame.minY+70,width:260,height:180),
                               styleMask:[.titled,.closable],backing:.buffered,defer:false)
        surface.title = fixtureConfiguration["surface_title"] as? String ?? "DTW Owned Raised Surface"
        surface.isReleasedWhenClosed=false;surface.level = .floating
        let surfacePad = ClickPad(frame:NSRect(origin:.zero,size:surface.contentView!.bounds.size))
        surfacePad.setAccessibilityLabel("Owned Surface Click Pad")
        surfacePad.emit = { [weak self] kind,event in
            guard let self else { return }
            self.applicationSurfaceCounts[kind,default:0] += 1
            self.record("surface_"+kind,event)
        }
        surface.contentView=surfacePad
        applicationSurface=surface;applicationSurfacePad=surfacePad
        surface.orderFront(nil);applicationSurfaceID=surface.windowNumber
    }
    @objc func saveDocument(_ sender: Any?) {
        saved = text; saves += 1
        try? saved.write(to:output.appendingPathComponent("saved.txt"),atomically:true,encoding:.utf8)
        saveState()
    }
    func applicationDidBecomeActive(_ n: Notification) { saveState() }
    func applicationDidResignActive(_ n: Notification) { saveState() }
    func applicationDidFinishLaunching(_ n: Notification) {
        try! FileManager.default.createDirectory(at:output,withIntermediateDirectories:true)
        _ = FileManager.default.createFile(atPath:output.appendingPathComponent("events.jsonl").path,contents:nil)
        eventFile = try! FileHandle(forWritingTo:output.appendingPathComponent("events.jsonl"))
        if fixtureConfiguration["background_probe"] as? Bool == true {
            for name in ["desktop.jsonl", "focus.jsonl"] {
                _ = FileManager.default.createFile(atPath:output.appendingPathComponent(name).path,contents:nil)
            }
            desktopFile = try! FileHandle(forWritingTo:output.appendingPathComponent("desktop.jsonl"))
            focusFile = try! FileHandle(forWritingTo:output.appendingPathComponent("focus.jsonl"))
            activationObserver = NSWorkspace.shared.notificationCenter.addObserver(
                forName:NSWorkspace.didActivateApplicationNotification,object:nil,queue:.main) { [weak self] notification in
                    guard let self, let app = notification.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication else { return }
                    self.journal(["uptime":ProcessInfo.processInfo.systemUptime,"frontmost_pid":app.processIdentifier,
                                  "app":app.bundleIdentifier ?? ""],to:self.focusFile)
                }
        }
        let menu = NSMenu()
        let appItem = NSMenuItem(title:"Fixture",action:nil,keyEquivalent:"");appItem.submenu=NSMenu(title:"Fixture");menu.addItem(appItem)
        let file = NSMenuItem(title:"File",action:nil,keyEquivalent:""); menu.addItem(file)
        let commands = NSMenu(); let save = NSMenuItem(title:"Save",action:#selector(saveDocument),keyEquivalent:"s")
        save.target=self; save.keyEquivalentModifierMask = .command; commands.addItem(save); file.submenu=commands
        if fixtureConfiguration["background_probe"] as? Bool == true {
            let close = NSMenuItem(title:"Close Window",action:#selector(NSWindow.performClose(_:)),keyEquivalent:"w")
            close.keyEquivalentModifierMask = .command
            commands.addItem(close) // nil target follows the real key-window responder chain.
        }
        let edit = NSMenuItem(title:"Edit",action:nil,keyEquivalent:"");let editing=NSMenu(title:"Edit")
        let selectAll=NSMenuItem(title:"Select All",action:#selector(NSText.selectAll(_:)),keyEquivalent:"a")
        selectAll.keyEquivalentModifierMask = .command;editing.addItem(selectAll);edit.submenu=editing;menu.addItem(edit)
        fixtureApplication.mainMenu=menu
        if fixtureConfiguration["all_mouse_events"] as? Bool == true {
            NSEvent.addLocalMonitorForEvents(matching:[.leftMouseDown,.leftMouseUp,.rightMouseDown,.rightMouseUp]) { [weak self] event in
                self?.record("observed_mouse_\(event.type.rawValue)", event)
                return event
            }
        }
        let defaultX: CGFloat = output.lastPathComponent == "peer" ? 730 : 120
        let x = (fixtureConfiguration["x"] as? NSNumber).map { CGFloat($0.doubleValue) } ?? defaultX
        window = NSWindow(contentRect:NSRect(x:x,y:210,width:540,height:420),styleMask:[.titled,.closable],backing:.buffered,defer:false)
        window.title=fixtureConfiguration["title"] as? String ?? "DTW Input Target";window.isReleasedWhenClosed=false
        let label=NSTextField(labelWithString:"DTW owned native input fixture");label.frame=NSRect(x:20,y:380,width:480,height:25);window.contentView!.addSubview(label)
        field=NSTextField(frame:NSRect(x:20,y:335,width:350,height:28));field.delegate=self;field.stringValue="Replace this seed"
        field.setAccessibilityLabel("Fixture Text");field.identifier=NSUserInterfaceItemIdentifier("fixture-text");window.contentView!.addSubview(field)
        let button=NSButton(title:"Save",target:self,action:#selector(saveDocument));button.frame=NSRect(x:395,y:332,width:110,height:34);window.contentView!.addSubview(button)
        scroll=NSScrollView(frame:NSRect(x:20,y:25,width:300,height:280));scroll.hasVerticalScroller=true
        let document=FlippedDocument(frame:NSRect(x:0,y:0,width:280,height:1600))
        for index in 0..<70 { let line=NSTextField(labelWithString:"Owned scroll line \(index)");line.frame=NSRect(x:10,y:index*22,width:260,height:20);document.addSubview(line) }
        scroll.documentView=document;window.contentView!.addSubview(scroll)
        pad=ClickPad(frame:NSRect(x:350,y:50,width:165,height:230));pad.setAccessibilityLabel("Fixture Click Pad")
        pad.emit={ [weak self] kind,event in
            guard let self else{return};self.padCounts[kind,default:0]+=1
            if kind == "left_down" && event.clickCount == 2 { self.padCounts["double_down"]! += 1 }
            self.record(kind,event)
            if kind == "left_down" && event.clickCount == 1 && self.coverAfterFirst && self.overlay == nil {
                let cover = NSWindow(contentRect:self.window.frame,styleMask:[.borderless],backing:.buffered,defer:false)
                cover.title="DTW Blocking Overlay";cover.isReleasedWhenClosed=false
                cover.level = .floating
                let blocker=ClickPad(frame:NSRect(origin:.zero,size:cover.contentView!.bounds.size))
                blocker.emit={ [weak self] action,event in
                    guard let self else{return};self.padCounts["overlay_"+action,default:0]+=1;self.record("overlay_"+action,event)
                };cover.contentView=blocker;self.overlay=cover;cover.orderFront(nil);self.saveState()
            }
        };window.contentView!.addSubview(pad)
        monitor=NSEvent.addLocalMonitorForEvents(matching:[.keyDown,.keyUp,.flagsChanged,.scrollWheel]) { [weak self] event in
            let kind: String
            switch event.type {case .keyDown:kind="key_down";case .keyUp:kind="key_up";case .flagsChanged:kind="flags_changed";default:kind="scroll"}
            self?.record(kind,event);return event
        }
        if fixtureConfiguration["secondary_window"] as? Bool == true {
            let other = NSWindow(contentRect:NSRect(x:x+40,y:80,width:480,height:100),styleMask:[.titled,.closable],backing:.buffered,defer:false)
            other.title="DTW Input Secondary";other.isReleasedWhenClosed=false
            let otherField=NSTextField(frame:NSRect(x:20,y:35,width:430,height:28))
            otherField.setAccessibilityLabel("Secondary Fixture Text")
            other.contentView!.addSubview(otherField);other.makeFirstResponder(otherField);other.orderFront(nil)
            secondaryWindow=other;secondaryWindowID=other.windowNumber;secondaryField=otherField
        }
        if fixtureConfiguration["background_probe"] as? Bool == true {
            windowCloseObserver = NotificationCenter.default.addObserver(forName:NSWindow.willCloseNotification,
                object:nil,queue:.main) { [weak self] notification in
                    guard let self, let closing = notification.object as? NSWindow,
                          closing === self.window || closing === self.secondaryWindow ||
                          closing === self.applicationSurface else { return }
                    let id = closing.windowNumber
                    self.closedWindowIDs.insert(id); self.eventCount += 1
                    let entry: [String:Any] = ["sequence":self.eventCount,"kind":"window_closed",
                        "uptime":ProcessInfo.processInfo.systemUptime,"window_number":id]
                    self.journal(entry,to:self.eventFile)
                    self.events.append(entry); self.events=Array(self.events.suffix(64))
                }
        }
        window.makeKeyAndOrderFront(nil);window.makeFirstResponder(field);fixtureApplication.activate()
        timer=Timer.scheduledTimer(withTimeInterval:0.02,repeats:true) { [self] _ in
            if let next=try? String(contentsOf:output.appendingPathComponent("command"),encoding:.utf8),next != command {
                command=next
                switch next.trimmingCharacters(in:.whitespacesAndNewlines) {
                case "front":
                    window.makeKeyAndOrderFront(nil)
                    fixtureApplication.activate()
                case "front_secondary":
                    secondaryWindow?.makeKeyAndOrderFront(nil)
                case "park_pointer":
                    let frame=window.frame, height=CGDisplayBounds(CGMainDisplayID()).height
                    _=CGWarpMouseCursorPosition(CGPoint(x:frame.midX,y:height-frame.midY))
                    _=CGAssociateMouseAndMouseCursorPosition(1)
                case "reset":window.endEditing(for:field);field.stringValue="";window.makeFirstResponder(field)
                case "reset_secondary":
                    if let secondaryWindow, let secondaryField {
                        secondaryWindow.endEditing(for:secondaryField);secondaryField.stringValue=""
                        secondaryWindow.makeFirstResponder(secondaryField)
                    }
                case "application_surface_open":openApplicationSurface()
                case "application_surface_close":applicationSurface?.close()
                case "cover_after_first":coverAfterFirst=true
                case "ax_empty":fixtureApplication.setAccessibilityWindows([]);axHidden=true
                case "ax_restore":fixtureApplication.setAccessibilityWindows([window!]);axHidden=false
                case "quit":fixtureApplication.terminate(nil)
                default:break
                }
            }
            saveState()
        }
        saveState()
    }
}
let delegate=Delegate();fixtureApplication.delegate=delegate;fixtureApplication.run()
