import AppKit
import Foundation
let root=URL(fileURLWithPath:CommandLine.arguments.last!)
try! FileManager.default.createDirectory(at:root,withIntermediateDirectories:true)
let app=NSApplication.shared
app.setActivationPolicy(.regular)
final class Delegate:NSObject,NSApplicationDelegate {
 var panels:[NSWindow]=[];var count=0;var command="";var timer:Timer?;var events:[[String:Any]]=[];var monitor:Any?
 func save(){
  let data:[String:Any]=["clicks":count,"pid":ProcessInfo.processInfo.processIdentifier,"active":app.isActive,"key_window":app.keyWindow?.windowNumber ?? 0,"events":events]
  try? JSONSerialization.data(withJSONObject:data,options:[.sortedKeys]).write(to:root.appendingPathComponent("state.json"),options:.atomic)
 }
 @objc func clicked(){count+=1;save()}
 func panel(_ x:CGFloat,_ y:CGFloat,_ title:String,_ level:NSWindow.Level)->NSWindow {
  let w=NSWindow(contentRect:NSRect(x:x,y:y,width:300,height:200),styleMask:[.borderless],backing:.buffered,defer:false)
  w.title=title;w.level=level;w.backgroundColor = .windowBackgroundColor;w.isReleasedWhenClosed=false
  let label=NSTextField(labelWithString:title);label.frame=NSRect(x:30,y:140,width:240,height:30);w.contentView!.addSubview(label)
  let button=NSButton(title:"Count click",target:self,action:#selector(clicked));button.frame=NSRect(x:100,y:70,width:100,height:40);w.contentView!.addSubview(button)
  w.makeKeyAndOrderFront(nil);return w
 }
 func applicationDidFinishLaunching(_ notification:Notification){
  panels=[panel(200,300,"DTW base",.normal),panel(400,350,"DTW middle",.floating),panel(600,400,"DTW top",.modalPanel)]
  app.activate(ignoringOtherApps:true);save()
  monitor=NSEvent.addLocalMonitorForEvents(matching:[.leftMouseDown,.leftMouseUp]){[self] event in
   let pointer=CGEvent(source:nil)?.location ?? .zero
   events.append(["type":event.type == .leftMouseDown ? "down":"up","count":event.clickCount,"window":event.windowNumber,"active":app.isActive,"pointer":[pointer.x,pointer.y]])
   save();return event
  }
  timer=Timer.scheduledTimer(withTimeInterval:0.1,repeats:true){[self] _ in
   guard let text=try? String(contentsOf:root.appendingPathComponent("command"),encoding:.utf8),text != command else{return};command=text
   switch text.trimmingCharacters(in:.whitespacesAndNewlines){
    case "move":panels.last!.setFrameOrigin(NSPoint(x:650,y:400))
    case "restore":panels.last!.setFrameOrigin(NSPoint(x:600,y:400));panels.last!.orderFront(nil)
    case "close":panels.last!.orderOut(nil)
    case "quit":app.terminate(nil)
    default:break
   }
  }
 }
}
let delegate=Delegate();app.delegate=delegate;app.run()
