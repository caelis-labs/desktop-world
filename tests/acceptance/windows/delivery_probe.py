#!/usr/bin/env python3
"""Cancel at a real high-surrogate handler while another MCP owner queues focus."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import traceback

from cursor_probe import Mcp,read,require,wait,write


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dtw',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--iterations',type=int,default=5)
    p.add_argument('--handler-delay',action='store_true',help='Optional owned GUI handler delay; default uses normal app processing')
    args=p.parse_args();require(os.name=='nt' and sys.flags.utf8_mode,'Use Windows: python -X utf8')
    require(1<=args.iterations<=20,'Use 1..20 independent cancellation transactions')
    binary=args.dtw.resolve(strict=True);out=args.output.resolve();require(not out.exists(),'Use new output');out.mkdir(parents=True)
    summary={'passed':False,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'iterations':[],'cleanup':[]}
    clients=[];fixtures=[]
    try:
        compile=subprocess.run(['powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',str(Path(__file__).with_name('compile-cursor.ps1')),
            '-OutputDirectory',str(out/'build')],capture_output=True)
        (out/'compile.stdout').write_bytes(compile.stdout);(out/'compile.stderr').write_bytes(compile.stderr);require(compile.returncode==0,'Fixture compilation failed')
        for label in ['A','B']:
            d=out/label;d.mkdir();proc=subprocess.Popen([str(out/'build/Fixture.exe'),str(d),label]);fixtures.append((proc,d))
            wait(lambda:(s if (s:=read(d/'state.json')) and s['ready'] else None))
        a,b=(x[1] for x in fixtures)
        (b/'track-surrogate').write_text('read-only journal marker',encoding='utf-8')
        if args.handler_delay:(b/'delay-surrogate').write_text('controlled 40ms app handler delay',encoding='utf-8')
        first=Mcp(binary,out/'first');second=Mcp(binary,out/'second');clients=[first,second]
        catalogs=[c.tool('apps',{}) for c in clients]
        app=next(x['app'] for x in catalogs[0]['apps'] if x['app'].casefold()==str(out/'build/Fixture.exe').casefold())
        rows,_=second.execute(app,'warm-window','return (await app.windows()).windows;')
        warm=next(w['window_ref'] for w in rows if w['title']=='DTW Windows Smoke A')
        second.execute(app,'warm-focus',f'return await app.focus({{window_ref:{json.dumps(warm)}}});')
        payload='中文😀|'*120
        for iteration in range(args.iterations):
            prefix=f'{iteration+1:02d}'
            def refs(c):
                rows,_=c.execute(app,prefix+'-windows','return (await app.windows()).windows;')
                return {w['title']:w['window_ref'] for w in rows}
            fw,sw=refs(first),refs(second)
            br=fw['DTW Windows Smoke B'];ar=sw['DTW Windows Smoke A']
            before=wait(lambda:read(b/'state.json'))
            editor=before['editor_bounds'];bounds=before['bounds']
            x=editor['x']+editor['width']//2-bounds['x'];y=editor['y']+editor['height']//2-bounds['y']
            # Invoke completion does not prove the fixture's editor.Focus() has
            # survived queued activation/button focus. Place real pointer input
            # once, then wait only for fresh independent readiness observations.
            first.execute(app,prefix+'-prepare',f'await app.focus({{window_ref:{json.dumps(br)}}});await app.press({{window_ref:{json.dumps(br)},selector:{{text:"Increment"}}}});const c=await app.capture({{window_ref:{json.dumps(br)}}});await app.click({{window_ref:{json.dumps(br)},observation_ref:c.observation_ref,x:{x},y:{y}}});return await app.keypress({{window_ref:{json.dumps(br)},keys:["CTRL","A"]}});')
            stable=None;sequence=before['sequence']
            def editor_ready():
                nonlocal stable,sequence
                s=read(b/'state.json')
                if not s or s['sequence']<=sequence:return None
                sequence=s['sequence']
                if not s['foreground'] or not s['editor_focused'] or s['keys_down_global']:
                    stable=None;return None
                if stable is None:stable=time.monotonic()
                return s if time.monotonic()-stable>=.1 else None
            write(out/(prefix+'-editor-ready.json'),wait(editor_ready,3))
            sa=wait(lambda:read(a/'state.json'));sb=wait(lambda:read(b/'state.json'))
            journal=b/'events.jsonl';offset=journal.stat().st_size
            request=first.begin('tools/call',{'name':'exec','arguments':{'app':app,'source':f'await app.typeText({{window_ref:{json.dumps(br)},text:{json.dumps(payload)}}});throw Error("cancellation missed actual input");'}})
            def half():
                with journal.open('rb') as stream:stream.seek(offset);lines=stream.read().splitlines()
                for line in lines:
                    try:e=json.loads(line)
                    except ValueError:continue
                    if e['kind']=='surrogate_half':return e
                return None
            entered=wait(half)
            running=first.tool('result',{'app':app});require(running['status']=='running','Missed the original input; do not replay')
            write(out/(prefix+'-running.json'),{'record':running,'half_event':entered})
            x=sa['pad_point']['x']-sa['bounds']['x'];y=sa['pad_point']['y']-sa['bounds']['y']
            queued=second.begin('tools/call',{'name':'exec','arguments':{'app':app,'source':f'await app.focus({{window_ref:{json.dumps(ar)}}});const c=await app.capture({{window_ref:{json.dumps(ar)}}});return await app.click({{window_ref:{json.dumps(ar)},observation_ref:c.observation_ref,x:{x},y:{y}}});'}})
            def parked():
                record=second.tool('result',{'app':app})
                return record if record['status']=='running' and any(s['operation']=='focus' for s in record.get('script',{}).get('steps',[])) else None
            wait(parked)
            time.sleep((iteration%8)*.001)
            first.send({'jsonrpc':'2.0','method':'notifications/cancelled','params':{'requestId':request,'reason':'owned high-surrogate handoff control'}})
            def settled():
                r=first.tool('result',{'app':app,'execution_id':running['execution_id']})
                return r if r['status']=='completed' else None
            cancelled=wait(settled,8);write(out/(prefix+'-cancelled.json'),cancelled)
            require(cancelled['script']['failure']['kind']=='cancelled' and cancelled['script']['shutdown_error'] is None,'Cancellation/cleanup evidence changed')
            typed=[s for s in cancelled['script']['steps'] if s['operation']=='typeText']
            require(len(typed)==1 and typed[0]['receipt']['outcome']['dispatch']=='unknown','Original input uncertainty was lost')
            response=second.receive(queued);write(out/(prefix+'-queued-response.json'),response)
            q=second.tool('result',{'app':app});write(out/(prefix+'-queued-ledger.json'),q)
            require(q['error'] is None,'Queued independent owner did not complete')
            aa=wait(lambda:(s if (s:=read(a/'state.json')) and s['left_up']==sa['left_up']+1 else None))
            time.sleep(.15);bb=wait(lambda:read(b/'state.json'))
            write(out/(prefix+'-effects.json'),{'next_owner':aa,'original_owner':bb,
                'scalar_prefix':payload.startswith(bb['text']),
                'utf16_prefix':payload.encode('utf-16-le').startswith(bb['text'].encode('utf-16-le','surrogatepass'))})
            require(aa['text']=='' and not aa['keys_down_global'],'Unicode tail leaked into the next owner')
            require(0<len(bb['text'])<len(payload) and payload.startswith(bb['text']),'Interrupted text is not a scalar prefix')
            require(not bb['keys_down_global'] and bb['saves']==sb['saves'],'Input/save cleanup changed')
            summary['iterations'].append({'iteration':iteration+1,'prefix_codepoints':len(bb['text']),'no_replacement_or_split_surrogate':True,
                'next_owner_text_empty':True,'next_owner_clicks':1,'held_keys':[],'original_dispatch':'unknown','shutdown_error':None})
        summary['passed']=True
    except BaseException:
        summary['failure']=traceback.format_exc()
        if clients and 'app' in locals():
            try:write(out/'failure-original-ledger.json',clients[0].tool('result',{'app':app}))
            except BaseException:pass
    finally:
        for c in clients:
            try:summary['cleanup'].append(c.close())
            except BaseException:summary['passed']=False;summary['cleanup'].append({'mcp_error':traceback.format_exc()})
        for proc,d in fixtures:
            (d/'stop').write_text('stop',encoding='utf-8')
            try:
                code=proc.wait(timeout=5);summary['cleanup'].append({'fixture_exit':code});summary['passed'] &= code==0
            except subprocess.TimeoutExpired:proc.kill();summary['passed']=False;summary['cleanup'].append({'fixture_forced':True})
        write(out/'summary.json',summary);print(json.dumps(summary))
    return 0 if summary['passed'] else 1


if __name__=='__main__':raise SystemExit(main())
