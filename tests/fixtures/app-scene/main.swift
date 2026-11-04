import AppKit
import Foundation
import UniformTypeIdentifiers
func pointerEventTelemetry(_ event:NSEvent)->[String:Any] {
    let point=event.locationInWindow
    // AppKit can report unavailable coordinates during keyboard focus delivery.
    // Keep the event observable without inventing a usable point or invalid JSON.
    return ["type":event.type.rawValue,"window":event.windowNumber,
        "x":point.x.isFinite ? point.x as Any:NSNull(),
        "y":point.y.isFinite ? point.y as Any:NSNull(),
        "location_valid":point.x.isFinite && point.y.isFinite]
}
final class SceneWindow:NSWindow {
    override func constrainFrameRect(_ frameRect:NSRect,to screen:NSScreen?)->NSRect {frameRect}
}
final class SequenceMessage:NSButton {
    weak var controller:Delegate?
    override func rightMouseDown(with event:NSEvent) {
        controller?.openSequencePopup(self)
    }
}
final class GesturePad:NSView {
    weak var controller:Delegate?
    var tracking:NSTrackingArea?
    override func updateTrackingAreas() {
        if let tracking {removeTrackingArea(tracking)}
        tracking=NSTrackingArea(rect:.zero,options:[.activeAlways,.inVisibleRect,.mouseMoved,.mouseEnteredAndExited],owner:self,userInfo:nil)
        addTrackingArea(tracking!);super.updateTrackingAreas()
    }
    func record(_ kind:String,_ event:NSEvent) {
        let point=convert(event.locationInWindow,from:nil)
        controller?.gestureEvents.append(["kind":kind,"x":point.x,"y":point.y,"dx":event.deltaX,"dy":event.deltaY])
        if kind=="drag",controller?.interruptDrag == true {
            controller?.interruptDrag=false
            let obstacle=NSWindow(contentRect:window!.convertToScreen(frame),styleMask:.borderless,backing:.buffered,defer:false)
            obstacle.title="DTW Drag obstacle";obstacle.isReleasedWhenClosed=false;obstacle.level = .floating
            obstacle.backgroundColor = .systemOrange;controller?.windows.append(obstacle);obstacle.orderFront(nil)
        }
        controller?.write()
    }
    override func mouseMoved(with event:NSEvent){record("move",event)}
    override func mouseDown(with event:NSEvent){record("down",event)}
    override func mouseDragged(with event:NSEvent){record("drag",event)}
    override func mouseUp(with event:NSEvent){record("up",event)}
}
final class Delegate: NSObject, NSApplicationDelegate {
    let root=Bundle.main.bundleURL.deletingLastPathComponent()
    var windows:[NSWindow]=[]
    var counts=["main":0,"peer":0,"popup":0]
    var command=""
    var timer:Timer?
    var blocked=false
    var cancelled=0
    var newRequests=0
    var gestureEvents:[[String:Any]]=[]
    var pointerEvents:[[String:Any]]=[]
    var sequencePopup:NSWindow?
    var sequenceCounts=["right":0,"opened":0,"action":0,"base":0]
    var interruptDrag=false
    var panel:NSPanel?
    var panelKind=""
    var panelResult=""
    var opened=""
    var saved=""
    func make(_ name:String,_ frame:NSRect,_ color:NSColor,_ buttonFrame:NSRect)->NSWindow {
        let w=SceneWindow(contentRect:frame,styleMask:[.titled,.closable],backing:.buffered,defer:false)
        w.title="DTW Scene \(name)";w.isReleasedWhenClosed=false
        w.contentView!.wantsLayer=true;w.contentView!.layer!.backgroundColor=color.cgColor
        let b=NSButton(title:name,target:self,action:#selector(clicked(_:)));b.frame=buttonFrame;b.identifier=NSUserInterfaceItemIdentifier(name)
        w.contentView!.addSubview(b)
        if name=="main" {
            let message=SequenceMessage(title:"Sequence message",target:self,action:#selector(sequenceBase))
            message.controller=self;message.frame=NSRect(x:40,y:120,width:160,height:36)
            w.contentView!.addSubview(message)
        }
        if name=="main" {let field=NSTextField(frame:NSRect(x:40,y:60,width:380,height:28));field.stringValue="initial";field.identifier=NSUserInterfaceItemIdentifier("scene-input");w.contentView!.addSubview(field)}
        if name=="main" {let slider=NSSlider(value:0,minValue:0,maxValue:100,target:nil,action:nil);slider.frame=NSRect(x:40,y:230,width:420,height:28);w.contentView!.addSubview(slider)}
        if name=="main" {let pad=GesturePad(frame:NSRect(x:40,y:280,width:420,height:70));pad.controller=self;pad.wantsLayer=true;pad.layer!.backgroundColor=NSColor.systemPurple.cgColor;w.contentView!.addSubview(pad)}
        w.acceptsMouseMovedEvents=true
        windows.append(w);w.makeKeyAndOrderFront(nil);return w
    }
    func openSequencePopup(_ message:SequenceMessage) {
        sequenceCounts["right",default:0]+=1
        guard sequencePopup?.isVisible != true,let owner=message.window else {write();return}
        // The popup is a separate CG surface entirely inside the owner's frame.
        // Its action occupies the same screen point as the underlying message.
        let rect=owner.convertToScreen(message.convert(message.bounds,to:nil))
        let popup=NSWindow(contentRect:rect,styleMask:.borderless,backing:.buffered,defer:false)
        popup.title="DTW Sequence popup";popup.isReleasedWhenClosed=false
        popup.level = .popUpMenu;popup.backgroundColor = .systemGreen
        let action=NSButton(title:"Sequence action",target:self,action:#selector(sequenceAction))
        action.frame=NSRect(origin:.zero,size:rect.size);popup.contentView!.addSubview(action)
        sequencePopup=popup;windows.append(popup);popup.orderFront(nil)
        sequenceCounts["opened",default:0]+=1;write()
    }
    @objc func sequenceAction() {
        sequenceCounts["action",default:0]+=1;sequencePopup?.close();write()
    }
    @objc func sequenceBase() {sequenceCounts["base",default:0]+=1;write()}
    @objc func newWindow(){newRequests += 1;_ = make("main",NSRect(x:100,y:200,width:560,height:400),.systemBlue,NSRect(x:220,y:170,width:150,height:40));write()}
    @objc func openFile() {
        let open=NSOpenPanel();open.directoryURL=root;open.canChooseDirectories=false;open.allowsMultipleSelection=false
        panel=open;panelKind="open";panelResult="pending"
        open.beginSheetModal(for:windows.first(where:{$0.isVisible && $0.title.hasSuffix("main")})!){[weak self] response in
            self?.panelResult=response == .OK ? "ok":"cancel"
            self?.opened=open.url?.path ?? "";self?.write()
        };write()
    }
    @objc func saveFile() {
        let save=NSSavePanel();save.directoryURL=root;save.nameFieldStringValue="saved.txt";save.canCreateDirectories=false;save.allowedContentTypes=[.plainText];save.allowsOtherFileTypes=false;save.isExtensionHidden=false
        panel=save;panelKind="save";panelResult="pending"
        save.beginSheetModal(for:windows.first(where:{$0.isVisible && $0.title.hasSuffix("main")})!){[weak self] response in
            self?.panelResult=response == .OK ? "ok":"cancel"
            self?.saved=save.url?.path ?? ""
            if response == .OK,let url=save.url,url.deletingLastPathComponent().resolvingSymlinksInPath().appendingPathComponent(url.lastPathComponent).path==self?.root.resolvingSymlinksInPath().appendingPathComponent("saved.txt").path {try? "saved by native panel".write(to:url,atomically:true,encoding:.utf8)}
            self?.write()
        };write()
    }
    func applicationDidFinishLaunching(_ notification:Notification) {
        _=NSEvent.addLocalMonitorForEvents(matching:[.mouseMoved,.leftMouseDown,.leftMouseDragged,.leftMouseUp,.rightMouseDown,.rightMouseUp]){[weak self] event in
            self?.pointerEvents.append(pointerEventTelemetry(event));return event
        }
        _=NSEvent.addLocalMonitorForEvents(matching:.scrollWheel){[weak self] event in
            if let self,let window=event.window {
                let p=window.convertPoint(toScreen:event.locationInWindow)
                self.gestureEvents.append(["kind":"scroll","x":p.x,"y":NSScreen.screens.first!.frame.maxY-p.y,"dx":event.scrollingDeltaX,"dy":event.scrollingDeltaY]);self.write()
            }
            return event
        }
        let menu=NSMenu();let appItem=NSMenuItem();let appMenu=NSMenu();appMenu.addItem(withTitle:"Quit",action:#selector(NSApplication.terminate(_:)),keyEquivalent:"q");appItem.submenu=appMenu;menu.addItem(appItem)
        let fileItem=NSMenuItem();let file=NSMenu(title:"File");let new=NSMenuItem(title:"New",action:#selector(newWindow),keyEquivalent:"n");new.target=self;file.addItem(new);let open=NSMenuItem(title:"Open…",action:#selector(openFile),keyEquivalent:"o");open.target=self;file.addItem(open);let save=NSMenuItem(title:"Save…",action:#selector(saveFile),keyEquivalent:"s");save.target=self;file.addItem(save);fileItem.submenu=file;menu.addItem(fileItem)
        let editItem=NSMenuItem();let edit=NSMenu(title:"Edit");let selectAll=NSMenuItem(title:"Select All",action:#selector(NSText.selectAll(_:)),keyEquivalent:"a");selectAll.keyEquivalentModifierMask = .command;edit.addItem(selectAll);editItem.submenu=edit;menu.addItem(editItem);NSApp.mainMenu=menu
        _=make("main",NSRect(x:100,y:200,width:560,height:400),.systemBlue,NSRect(x:220,y:170,width:150,height:40))
        let peer=make("peer",NSRect(x:280,y:280,width:300,height:200),.systemRed,NSRect(x:40,y:90,width:150,height:40));peer.level = .floating
        NSApp.activate(ignoringOtherApps:true);peer.makeKeyAndOrderFront(nil);write()
        timer=Timer.scheduledTimer(withTimeInterval:0.05,repeats:true){[weak self] _ in self?.tick()}
        RunLoop.main.add(timer!,forMode:.common);RunLoop.main.add(timer!,forMode:.modalPanel);RunLoop.main.add(timer!,forMode:.eventTracking)
    }
    @objc func clicked(_ button:NSButton){let key=button.identifier!.rawValue;counts[key,default:0]+=1;write()}
    func tick(){
        if let next=try? String(contentsOf:root.appendingPathComponent("command"),encoding:.utf8),next != command {
            command=next
            if next=="popup" { let w=make("popup",NSRect(x:350,y:350,width:150,height:100),.systemGreen,NSRect(x:0,y:20,width:140,height:40));w.level = .popUpMenu }
            if next=="close_popup" {windows.filter{$0.title.hasSuffix("popup")}.forEach{$0.close()}}
            if next=="move_popup" {windows.last!.setFrameOrigin(NSPoint(x:500,y:500))}
            if next=="move_peer" {windows[1].setFrameOrigin(NSPoint(x:450,y:350))}
            if next.hasPrefix("move:") {
                let parts=next.split(separator:":");if parts.count==3,let x=Double(parts[1]),let y=Double(parts[2]),let w=windows.first(where:{$0.isVisible && $0.title.hasSuffix("main")}) {
                    w.setFrameOrigin(NSPoint(x:x,y:NSScreen.screens.first!.frame.maxY-y-w.frame.height))
                }
            }
            if next=="interrupt_drag" {interruptDrag=true}
            if next=="clear_obstacle" {windows.filter{$0.title=="DTW Drag obstacle"}.forEach{$0.close()}}
            if next=="block_quit" {blocked=true}
            if next=="allow_quit" {blocked=false}
            if next=="quit" {blocked=false;NSApp.terminate(nil)}
        }
        write()
    }
    func applicationShouldTerminate(_ sender:NSApplication)->NSApplication.TerminateReply {
        guard blocked else{return .terminateNow}
        let alert=NSAlert();alert.messageText="Unsaved disposable scene";alert.addButton(withTitle:"Keep Open")
        alert.beginSheetModal(for:windows[0]){[weak self] _ in self?.cancelled += 1; NSApp.reply(toApplicationShouldTerminate:false);self?.write()}
        write();return .terminateLater
    }
    func write(){
        let top=NSScreen.screens.first!.frame.maxY
        var buttons:[String:[Double]]=[:]
        var inputs:[String:String]=[:]
        var sliderValue=0.0
        func visit(_ view:NSView,_ window:NSWindow) {
            if let button=view as? NSButton,!button.title.isEmpty { let p=window.convertPoint(toScreen:button.convert(NSPoint(x:button.bounds.midX,y:button.bounds.midY),to:nil));buttons[button.title]=[p.x,top-p.y] }
            if let field=view as? NSTextField,field.identifier?.rawValue=="scene-input" {let p=window.convertPoint(toScreen:field.convert(NSPoint(x:field.bounds.midX,y:field.bounds.midY),to:nil));buttons["scene-input"]=[p.x,top-p.y];inputs[window.title]=field.currentEditor()?.string ?? field.stringValue}
            if let slider=view as? NSSlider {sliderValue=slider.doubleValue
                for (name,x) in [("slider-start",10.0),("slider-end",410.0)] {let p=window.convertPoint(toScreen:slider.convert(NSPoint(x:x,y:slider.bounds.midY),to:nil));buttons[name]=[p.x,top-p.y]}
            }
            if let pad=view as? GesturePad {
                for (name,x,y) in [("gesture-start",60.0,35.0),("gesture-end",340.0,35.0),("gesture-upper",60.0,60.0),("gesture-lower",340.0,10.0)] {let p=window.convertPoint(toScreen:pad.convert(NSPoint(x:x,y:y),to:nil));buttons[name]=[p.x,top-p.y]}
            }
            for child in view.subviews {visit(child,window)}
        }
        let surfaces=windows+windows.compactMap{ $0.attachedSheet }
        for window in surfaces {
            if let view=window.contentView {visit(view,window)}
            if let button=window.standardWindowButton(.closeButton) {let p=window.convertPoint(toScreen:button.convert(NSPoint(x:button.bounds.midX,y:button.bounds.midY),to:nil));buttons["Close "+window.title]=[p.x,top-p.y]}
        }
        let rows=surfaces.filter{$0.isVisible}.map{w->[String:Any] in
            var row:[String:Any]=["title":w.title,"id":w.windowNumber,"bounds":[w.frame.minX,top-w.frame.maxY,w.frame.width,w.frame.height],
                "accepts_mouse_moved":w.acceptsMouseMovedEvents,"ignores_mouse":w.ignoresMouseEvents,
                "tracking_areas":w.contentView!.subviews.compactMap{($0 as? GesturePad)?.trackingAreas.count}]
            if let b=w.contentView!.subviews.first as? NSButton {let p=w.convertPoint(toScreen:b.convert(NSPoint(x:b.bounds.midX,y:b.bounds.midY),to:nil));row["button"]=[p.x,top-p.y]}
            return row
        }
        let cg=(CGWindowListCopyWindowInfo([.optionOnScreenOnly,.excludeDesktopElements],0) as? [[String:Any]] ?? []).filter{($0[kCGWindowOwnerPID as String] as? Int32)==getpid()}.map{["id":$0[kCGWindowNumber as String] ?? 0,"layer":$0[kCGWindowLayer as String] ?? 0]}
        let pointer=CGEvent(source:nil)?.location
        let environment:[String:Any]=["pointer_screen":pointer.map{[$0.x,$0.y]} as Any? ?? NSNull(),
            "frontmost_pid":NSWorkspace.shared.frontmostApplication?.processIdentifier as Any? ?? NSNull(),
            "uptime":ProcessInfo.processInfo.systemUptime]
        let state:[String:Any]=["environment":environment,"sequence":sequenceCounts,"pointer_events":pointerEvents,"pressed_mouse_buttons":NSEvent.pressedMouseButtons,"slider":sliderValue,"panel":["kind":panelKind,"visible":panel?.isVisible ?? false,"result":panelResult,"opened":opened,"saved":saved],"gestures":gestureEvents,"buttons":buttons,"inputs":inputs,"new_requests":newRequests,"cancelled":cancelled,"cg":cg,"active":NSApp.isActive,"key":NSApp.keyWindow?.title ?? "", "pid":ProcessInfo.processInfo.processIdentifier,"counts":counts,"command":command,"windows":rows]
        if let data=try? JSONSerialization.data(withJSONObject:state){try? data.write(to:root.appendingPathComponent("state.json"),options:.atomic)}
    }
}
let delegate=Delegate();let app=NSApplication.shared;app.delegate=delegate;app.setActivationPolicy(.regular);app.run()
