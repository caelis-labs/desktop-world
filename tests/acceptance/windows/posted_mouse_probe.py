#!/usr/bin/env python3
"""Real HWND delivery, physical cursor/foreground and DPI equivalence controls."""
import argparse, ctypes, hashlib, json, os, subprocess, sys, time, traceback
from ctypes import wintypes as w
from pathlib import Path
from cursor_probe import Mcp, read, require, wait, write


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dtw',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();binary=a.dtw.resolve(strict=True);out=a.output.resolve()
    require(os.name=='nt' and sys.flags.utf8_mode and not out.exists(),'Use Windows, utf8 and fresh output')
    out.mkdir(parents=True);summary={'passed':False,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'cases':[],'cleanup':[]}
    user=ctypes.WinDLL('user32',use_last_error=True);user.GetForegroundWindow.restype=w.HWND
    user.GetPhysicalCursorPos.argtypes=[ctypes.POINTER(w.POINT)]
    def desktop():
        pt=w.POINT();require(user.GetPhysicalCursorPos(ctypes.byref(pt)),'cursor read failed')
        return {'cursor':[pt.x,pt.y],'foreground':user.GetForegroundWindow()}
    fixtures=[];clients=[]
    from PIL import Image, ImageChops
    try:
        build=subprocess.run(['powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',str(Path(__file__).with_name('compile-cursor.ps1')),'-OutputDirectory',str(out/'build')],capture_output=True)
        (out/'compile.stdout').write_bytes(build.stdout);(out/'compile.stderr').write_bytes(build.stderr);require(build.returncode==0,'compile failed')
        for awareness in ['aware','unaware']:
            for label in ['A','B']:
                d=out/(awareness+label);d.mkdir();proc=subprocess.Popen([str(out/'build/Fixture.exe'),str(d),label,*(['unaware'] if awareness=='unaware' else [])]);fixtures.append((proc,d))
                wait(lambda:(s if (s:=read(d/'state.json')) and s['ready'] else None))
            da,db=[x[1] for x in fixtures[-2:]]
            c=Mcp(binary,out/(awareness+'-mcp'));clients.append(c)
            app=next(x['app'] for x in c.tool('apps',{})['apps'] if x['app'].casefold()==str(out/'build/Fixture.exe').casefold())
            rows,_=c.execute(app,'refs','return (await app.windows()).windows;')
            refs={x['title']:x['window_ref'] for x in rows};ar=refs['DTW Windows Smoke A'];br=refs['DTW Windows Smoke B']
            c.execute(app,'foreground-A',f'return await app.focus({{window_ref:{json.dumps(ar)}}});')
            wait(lambda:(s if (s:=read(da/'state.json')) and s['foreground'] else None));time.sleep(.15)
            b=read(db/'state.json');origin=desktop();require(origin['foreground']==int(read(da/'state.json')['hwnd']),'A not foreground')
            for name,operation,expected in [('single-left','click',1),('double-left','double',2),('wheel','scroll',-240),('edge-left','edge',1)]:
                before=read(db/'state.json');capture,_=c.execute(app,name+'-capture',f'return await app.capture({{window_ref:{json.dumps(br)}}});')
                # Physical geometry comes from a separate fixture publication.
                image=Image.open(c.images[-1]).convert('RGB');mask=Image.new('L',image.size,255)
                for channel,value in zip(image.split(),(173,216,230)):
                    mask=ImageChops.multiply(mask,channel.point([255 if i==value else 0 for i in range(256)]))
                bounds=mask.getbbox();require(bounds,'No rendered pointer pad')
                x=(bounds[0]+bounds[2])//2;y=(bounds[1]+bounds[3])//2
                if operation=='edge':
                    # Near the far edge detects double scaling hidden by center hits.
                    x=bounds[2]-8;y=bounds[3]-8
                arg=f'window_ref:{json.dumps(br)},observation_ref:{json.dumps(capture["observation_ref"])},x:{x},y:{y}'
                if operation=='scroll':source=f'return await app.scroll({{{arg},delta_y:2}});'
                else:source=f'return await app.click({{{arg},count:{2 if operation=="double" else 1}}});'
                receipt,ledger=c.execute(app,name,source)
                step=next(s for s in ledger['script']['steps'] if s['operation'] in ['click','scroll']);delivery=step['receipt']['outcome']['delivery'];require(delivery['route']=='background',delivery)
                key='wheel_delta' if operation=='scroll' else 'left_up'
                effect=wait(lambda:(s if (s:=read(db/'state.json')) and s[key]==before[key]+expected else None))
                time.sleep(.1);after=desktop();require(after==origin,{'before':origin,'after':after})
                require(not effect['keys_down_global'],'held input')
                summary['cases'].append({'name':awareness+'-'+name,'effect':expected,'desktop_unchanged':True,'delivery':delivery})
            if awareness=='aware':
                before=read(db/'state.json')
                request=c.begin('tools/call',{'name':'exec','arguments':{'app':app,'source':f'const c=await app.capture({{window_ref:{json.dumps(br)}}});return await app.scroll({{window_ref:{json.dumps(br)},observation_ref:c.observation_ref,x:{x},y:{y},delta_y:100}});'}})
                wait(lambda:(s if (s:=read(db/'state.json')) and s['wheel_delta']<before['wheel_delta'] else None))
                running=c.tool('result',{'app':app});require(running['status']=='running','Missed cancellation barrier')
                c.send({'jsonrpc':'2.0','method':'notifications/cancelled','params':{'requestId':request,'reason':'owned posted wheel cancellation'}})
                def settled():
                    record=c.tool('result',{'app':app,'execution_id':running['execution_id']})
                    return record if record['status']=='completed' else None
                cancelled=wait(settled,8);write(out/'cancelled-ledger.json',cancelled)
                require(cancelled['script']['failure']['kind']=='cancelled' and cancelled['script']['shutdown_error'] is None,'Cancellation cleanup changed')
                receipt=next(s for s in cancelled['script']['steps'] if s['operation']=='scroll')['receipt'];require(receipt['outcome']['dispatch']=='unknown',receipt)
                time.sleep(.15);partial=read(db/'state.json');require(0<before['wheel_delta']-partial['wheel_delta']<12000,'Cancellation did not interrupt wheel sequence')
                require(desktop()==origin and not partial['keys_down_global'],'Cancel changed shared input')
                rows,_=c.execute(app,'after-cancel','return (await app.windows()).windows;')
                refs={row['title']:row['window_ref'] for row in rows};ar=refs['DTW Windows Smoke A'];br=refs['DTW Windows Smoke B']
                time.sleep(.15);require(read(db/'state.json')['wheel_delta']==partial['wheel_delta'],'Wheel tail after cleanup')
                summary['cases'].append({'name':'posted-wheel-cancel-no-tail','partial_delta':partial['wheel_delta']-before['wheel_delta'],'dispatch':'unknown','shutdown_error':None})
            # Fail before dispatch under background-only for unsupported right click.
            strict=Mcp(binary,out/(awareness+'-strict'),['--input-policy','background-only']);clients.append(strict);strict.tool('apps',{})
            result,ledger=strict.execute(app,'right-denied',f'const w=(await app.windows()).windows.find(w=>w.title==="DTW Windows Smoke B");const c=await app.capture({{window_ref:w.window_ref}});return await app.click({{window_ref:w.window_ref,observation_ref:c.observation_ref,x:{x},y:{y},button:"right"}});',error='foreground_required')
            click=next(s for s in ledger['script']['steps'] if s['operation']=='click');require(click['receipt']['outcome']['dispatch']=='not_dispatched',click)
            require(desktop()==origin,'denied route changed desktop');summary['cases'].append({'name':awareness+'-unsupported-right-rejected','dispatch':'not_dispatched'})
            before=read(db/'state.json')
            _,ledger=c.execute(app,'automatic-foreground-continuation',f'const c=await app.capture({{window_ref:{json.dumps(br)}}});await app.click({{window_ref:{json.dumps(br)},observation_ref:c.observation_ref,x:{x},y:{y},button:"right"}});await app.keypress({{window_ref:{json.dumps(br)},keys:["ESC"]}});return await app.typeText({{window_ref:{json.dumps(ar)},text:"automatic foreground continuation"}});')
            click=next(s for s in ledger['script']['steps'] if s['operation']=='click');require(click['receipt']['outcome']['delivery']['route']=='foreground',click)
            wait(lambda:(s if (s:=read(db/'state.json')) and s['right_up']==before['right_up']+1 else None))
            wait(lambda:(s if (s:=read(da/'state.json')) and s['text']=='automatic foreground continuation' and s['foreground'] else None))
            summary['cases'].append({'name':awareness+'-automatic-foreground-continuation','right_clicks':1,'typed_in_correct_app':True})
            for client in clients[-2:]:summary['cleanup'].append(client.close())
            clients=[]
            for proc,d in fixtures[-2:]:
                (d/'stop').write_text('stop');code=proc.wait(5);require(code==0,'fixture failed');summary['cleanup'].append({'fixture_exit':code})
            fixtures=[]
        summary['passed']=True
    except BaseException:summary['failure']=traceback.format_exc()
    finally:
        for c in clients:
            try:summary['cleanup'].append(c.close())
            except BaseException:summary['passed']=False;summary['cleanup'].append({'error':traceback.format_exc()})
        for proc,d in fixtures:
            (d/'stop').write_text('stop')
            try:summary['cleanup'].append({'fixture_exit':proc.wait(5)})
            except subprocess.TimeoutExpired:proc.kill();summary['passed']=False
        write(out/'summary.json',summary);print(json.dumps(summary))
    return 0 if summary['passed'] else 1

if __name__=='__main__':raise SystemExit(main())
