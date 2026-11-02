import AppKit
import Foundation
final class Delegate: NSObject, NSApplicationDelegate {
    let root=Bundle.main.bundleURL.deletingLastPathComponent()
    var windows:[NSWindow]=[]
    var counts=["main":0,"peer":0,"popup":0]
    var command=""
    var timer:Timer?
    var blocked=false
    var cancelled=0
    var newRequests=0
    func make(_ name:String,_ frame:NSRect,_ color:NSColor,_ buttonFrame:NSRect)->NSWindow {
        let w=NSWindow(contentRect:frame,styleMask:[.titled,.closable],backing:.buffered,defer:false)
        w.title="DTW Scene \(name)";w.isReleasedWhenClosed=false
        w.contentView!.wantsLayer=true;w.contentView!.layer!.backgroundColor=color.cgColor
        let b=NSButton(title:name,target:self,action:#selector(clicked(_:)));b.frame=buttonFrame;b.identifier=NSUserInterfaceItemIdentifier(name)
        w.contentView!.addSubview(b)
        if name=="main" {let field=NSTextField(frame:NSRect(x:40,y:60,width:380,height:28));field.stringValue="initial";field.identifier=NSUserInterfaceItemIdentifier("scene-input");w.contentView!.addSubview(field)}
        windows.append(w);w.makeKeyAndOrderFront(nil);return w
    }
    @objc func newWindow(){newRequests += 1;_ = make("main",NSRect(x:100,y:200,width:560,height:400),.systemBlue,NSRect(x:220,y:170,width:150,height:40));write()}
    func applicationDidFinishLaunching(_ notification:Notification) {
        let menu=NSMenu();let appItem=NSMenuItem();let appMenu=NSMenu();appMenu.addItem(withTitle:"Quit",action:#selector(NSApplication.terminate(_:)),keyEquivalent:"q");appItem.submenu=appMenu;menu.addItem(appItem)
        let fileItem=NSMenuItem();let file=NSMenu(title:"File");let new=NSMenuItem(title:"New",action:#selector(newWindow),keyEquivalent:"n");new.target=self;file.addItem(new);fileItem.submenu=file;menu.addItem(fileItem)
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
        func visit(_ view:NSView,_ window:NSWindow) {
            if let button=view as? NSButton,!button.title.isEmpty { let p=window.convertPoint(toScreen:button.convert(NSPoint(x:button.bounds.midX,y:button.bounds.midY),to:nil));buttons[button.title]=[p.x,top-p.y] }
            if let field=view as? NSTextField,field.identifier?.rawValue=="scene-input" {let p=window.convertPoint(toScreen:field.convert(NSPoint(x:field.bounds.midX,y:field.bounds.midY),to:nil));buttons["scene-input"]=[p.x,top-p.y];inputs[window.title]=field.currentEditor()?.string ?? field.stringValue}
            for child in view.subviews {visit(child,window)}
        }
        let surfaces=windows+windows.compactMap{ $0.attachedSheet }
        for window in surfaces {
            if let view=window.contentView {visit(view,window)}
            if let button=window.standardWindowButton(.closeButton) {let p=window.convertPoint(toScreen:button.convert(NSPoint(x:button.bounds.midX,y:button.bounds.midY),to:nil));buttons["Close "+window.title]=[p.x,top-p.y]}
        }
        let rows=surfaces.filter{$0.isVisible}.map{w->[String:Any] in
            var row:[String:Any]=["title":w.title,"id":w.windowNumber]
            if let b=w.contentView!.subviews.first as? NSButton {let p=w.convertPoint(toScreen:b.convert(NSPoint(x:b.bounds.midX,y:b.bounds.midY),to:nil));row["button"]=[p.x,top-p.y]}
            return row
        }
        let cg=(CGWindowListCopyWindowInfo([.optionOnScreenOnly,.excludeDesktopElements],0) as? [[String:Any]] ?? []).filter{($0[kCGWindowOwnerPID as String] as? Int32)==getpid()}.map{["id":$0[kCGWindowNumber as String] ?? 0,"layer":$0[kCGWindowLayer as String] ?? 0]}
        let state:[String:Any]=["buttons":buttons,"inputs":inputs,"new_requests":newRequests,"cancelled":cancelled,"cg":cg,"active":NSApp.isActive,"key":NSApp.keyWindow?.title ?? "", "pid":ProcessInfo.processInfo.processIdentifier,"counts":counts,"command":command,"windows":rows]
        if let data=try? JSONSerialization.data(withJSONObject:state){try? data.write(to:root.appendingPathComponent("state.json"),options:.atomic)}
    }
}
let delegate=Delegate();let app=NSApplication.shared;app.delegate=delegate;app.setActivationPolicy(.regular);app.run()
