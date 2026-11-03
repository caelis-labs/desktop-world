#!/usr/bin/env python3
"""Native-delivery research on a fresh owned Chrome profile, not product acceptance.

No CDP, inspector, user window or HID input. Missing effects are recorded, never
replayed. Run with cursor_audit.py; passing posts alone do not prove capability.
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import signal
import subprocess
import threading
import time
import uuid

from standard_agent_probe import process_table, require, wait_until, write_json
from app_client import AppClient

HTML = b'''<!doctype html><title>Owned directed input</title>
<style>#pad{position:absolute;left:100px;top:100px;width:500px;height:180px;background:orange}
#pad:hover{background:green}#text{position:absolute;left:100px;top:330px;width:550px;font:24px monospace}</style>
<div id="pad"></div><div id="text">Independent native input should select these words.</div>
<script>
function send(data){fetch('/event',{method:'POST',body:JSON.stringify(data)})}
send({kind:'ready',screenX,screenY,outerWidth,outerHeight,innerWidth,innerHeight});
for(const kind of ['mousemove','mousedown','mouseup','click','dragstart','dragend'])
document.addEventListener(kind,e=>send({kind,trusted:e.isTrusted,x:e.clientX,y:e.clientY,target:e.target.id,
 selected:String(getSelection()),hover:document.querySelector('#pad').matches(':hover')}));
document.addEventListener('selectionchange',()=>send({kind:'selection',selected:String(getSelection())}));
</script>'''


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--dtw',type=Path,help='Test normal apps/exec instead of the separate raw-event experiment')
    parser.add_argument('--transport',choices=['public','skylight'],default='skylight')
    parser.add_argument('--chrome',type=Path,default=Path('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'))
    args=parser.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=False);out.chmod(0o700)
    title='Owned directed input '+uuid.uuid4().hex[:10]
    events=[];lock=threading.Lock()
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200);self.send_header('Content-Type','text/html');self.end_headers();self.wfile.write(HTML.replace(b'Owned directed input',title.encode()))
        def do_POST(self):
            row=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            with lock:
                events.append(row);write_json(out/'events.json',events)
            self.send_response(204);self.end_headers()
        def log_message(self,*_):pass
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
    thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
    helper=out/'directed-pointer'
    subprocess.run(['xcrun','swiftc','-module-cache-path',str(out/'swift-cache'),
        str(Path(__file__).resolve().parents[1]/'fixtures/directed-pointer/main.swift'),'-o',str(helper)],check=True)
    profile=out/('profile-'+uuid.uuid4().hex)
    log=(out/'chrome.log').open('w')
    marker='--user-data-dir='+str(profile)
    require(not any(marker in command for _,command in process_table().values()),'profile already owned')
    subprocess.run(['open','-n','-a',str(args.chrome.parents[2]),'--args',marker,'--no-first-run','--no-default-browser-check',
        '--window-position=320,100','--window-size=1100,850','--new-window',f'http://127.0.0.1:{server.server_port}/'],stdout=log,stderr=log,check=True,timeout=10)
    def owned_pid():
        matches=[pid for pid,(_,command) in process_table().items() if command.startswith(str(args.chrome)) and marker in command]
        require(len(matches)<=1,'ambiguous owned browser');return matches[0] if matches else None
    pid=None
    client=None
    report={'scope':'owned Chrome raw-event feasibility; no product or arbitrary Chromium claim','cases':[]}
    try:
        pid=wait_until(owned_pid);report['pid']=pid
        wait_until(lambda:any(e['kind']=='ready' for e in events));require(owned_pid()==pid,'owned Chrome forwarded startup')
        def inspect():
            rows=json.loads(subprocess.check_output([str(helper),'inspect',str(pid)],text=True))
            return next((r for r in rows if r.get('kCGWindowName','').startswith(title)),None)
        row=wait_until(inspect);b=row['kCGWindowBounds'];frame=next(e for e in events if e['kind']=='ready')
        local_x=(frame['outerWidth']-frame['innerWidth'])/2
        local_y=frame['outerHeight']-frame['innerHeight']
        if args.dtw:
            client=AppClient(args.dtw.resolve(),out/'mcp')
            report['scope']='owned Chrome normal apps/exec, DOM effects and independent cursor audit'
        for operation,a,z in [('move',(200,150),(200,150)),('click',(200,150),(200,150)),
                              ('drag',(105,345),(500,345))]:
            view=None
            if client:
                view=client.capture('com.google.Chrome',operation)['view']
                require(view['windows'][0]['title'].startswith(title),'owned window is not the top app surface')
                require(inspect()['kCGWindowNumber']==row['kCGWindowNumber'],'owned window changed')
            offset=len(events);started=time.time()*1000
            points=[b['X']+local_x+a[0],b['Y']+local_y+a[1],b['X']+local_x+z[0],b['Y']+local_y+z[1]]
            require(owned_pid()==pid,'owned browser incarnation changed')
            if client:
                image=view['image'];ox,oy,width,height=image['bounds']
                def pixel(x,y):return {'x':(x-ox)*image['width']/width,'y':(y-oy)*image['height']/height}
                first,last=pixel(*points[:2]),pixel(*points[2:])
                argument={'path':[first,last]} if operation=='drag' else first
                client.execute('com.google.Chrome','await app.'+operation+'('+json.dumps(argument)+');')
            else:
                subprocess.run([str(helper),operation,str(pid),str(row['kCGWindowNumber']),str(b['X']),str(b['Y']),
                                *map(str,points),'1',args.transport],check=True,timeout=8)
            time.sleep(.25);observed=list(events[offset:]);ended=time.time()*1000
            effect=any(e['kind']=='mousemove' and e.get('hover') for e in observed) if operation=='move' else (
                any(e['kind']=='click' and e.get('target')=='pad' for e in observed) if operation=='click' else
                any(e['kind']=='selection' and len(e.get('selected',''))>10 for e in observed))
            report['cases'].append({'operation':operation,'transport':'apps/exec' if client else args.transport,'start_unix_ms':started,'end_unix_ms':ended,
                                    'effect_proven':effect,'events':observed})
            write_json(out/('report.json' if client else 'directed-actions.json'),report)
    finally:
        if client:client.close()
        if pid and owned_pid()==pid:os.kill(pid,signal.SIGTERM)
        log.close();server.shutdown();server.server_close();write_json(out/('report.json' if client else 'directed-actions.json'),report)
    print(json.dumps({'effects_proven':sum(c['effect_proven'] for c in report['cases']),'report':str(out)}))

    if client:require(all(c['effect_proven'] for c in report['cases']),report)


if __name__=='__main__':main()
