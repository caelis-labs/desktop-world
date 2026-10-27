import AppKit
import Foundation
let root=URL(fileURLWithPath:CommandLine.arguments.last!)
let app=NSApplication.shared
app.setActivationPolicy(.regular)
final class TestWindow:NSWindow {
 var ready=true;var slow=false
 override func accessibilityChildren()->[Any]? {
  if slow { Thread.sleep(forTimeInterval:2.0) }
  return ready ? super.accessibilityChildren() : []
 }
}
final class Delegate:NSObject,NSApplicationDelegate {
 var windows:[TestWindow]=[];var count=0;var started=0;var slowAction=false;var sheet:NSWindow?;var command="";var timer:Timer?;var label:NSTextField!
 func save(){try! JSONSerialization.data(withJSONObject:["pid":ProcessInfo.processInfo.processIdentifier,"clicks":count,"actions_started":started,"active":app.isActive,"key_window":app.keyWindow?.windowNumber ?? 0,"command":command,"windows":windows.map{["id":$0.windowNumber,"visible":$0.isVisible,"title":$0.title,"x":$0.frame.minX,"y":$0.frame.minY]}],options:[.sortedKeys]).write(to:root.appendingPathComponent("state.json"),options:.atomic)}
 @objc func clicked(){started+=1;save();if slowAction {Thread.sleep(forTimeInterval:2.0)};count+=1;label.stringValue="Count: \(count)";save()}
 func make(_ title:String,_ x:CGFloat)->TestWindow {
  let w=TestWindow(contentRect:NSRect(x:x,y:300,width:320,height:220),styleMask:[.titled,.closable],backing:.buffered,defer:false)
  w.title=title;w.isReleasedWhenClosed=false
  let button=NSButton(title:"Increment",target:self,action:#selector(clicked));button.frame=NSRect(x:100,y:70,width:120,height:40);w.contentView!.addSubview(button)
  let text=NSTextField(labelWithString:"Count: 0");text.frame=NSRect(x:80,y:150,width:200,height:24);w.contentView!.addSubview(text)
  if label==nil {label=text}
  w.makeKeyAndOrderFront(nil);return w
 }
 func applicationDidFinishLaunching(_ n:Notification){
  windows=[make("DTW Native Target",200),make("DTW Native Peer",600)]
  windows[0].makeKeyAndOrderFront(nil);app.activate(ignoringOtherApps:true);save()
  timer=Timer.scheduledTimer(withTimeInterval:0.05,repeats:true){[self] _ in
   guard let next=try? String(contentsOf:root.appendingPathComponent("command"),encoding:.utf8),next != command else{return}
   command=next
   switch next.trimmingCharacters(in:.whitespacesAndNewlines) {
    case "move":windows[0].setFrameOrigin(NSPoint(x:250,y:300))
    case "restore":windows[0].setFrameOrigin(NSPoint(x:200,y:300));windows[0].orderFront(nil)
    case "occlude":windows[1].setFrameOrigin(windows[0].frame.origin);windows[1].orderFront(nil)
    case "uncover":windows[1].setFrameOrigin(NSPoint(x:600,y:300));windows[0].makeKeyAndOrderFront(nil);app.activate(ignoringOtherApps:true)
    case "hide":windows[0].orderOut(nil)
    case "show":windows[0].orderFront(nil)
    case "duplicate":windows[1].title="DTW Native Target"
    case "close":windows[0].close()
    case "not_ready":windows[0].ready=false
    case "ready":windows[0].ready=true
    case "floating_peer":windows[1].level = .floating
    case "normal_peer":windows[1].level = .normal
    case "sheet":
     let child=NSWindow(contentRect:NSRect(x:0,y:0,width:280,height:120),styleMask:[.titled],backing:.buffered,defer:false)
     let button=NSButton(title:"Out of scope action",target:self,action:#selector(clicked));button.frame=NSRect(x:30,y:30,width:220,height:40);child.contentView!.addSubview(button)
     sheet=child;windows[0].beginSheet(child)
    case "dismiss_sheet":if let child=sheet {windows[0].endSheet(child);child.orderOut(nil);sheet=nil}
    case "slow_action":slowAction=true
    case "fast_action":slowAction=false
    case "slow_ax":windows[0].slow=true
    case "fast_ax":windows[0].slow=false
    case "quit":app.terminate(nil)
    default:break
   }
   save()
  }
 }
}
let delegate=Delegate();app.delegate=delegate;app.run()
