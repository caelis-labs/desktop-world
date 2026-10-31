#!/usr/bin/env python3
"""Owned Unicode delivery admission, selection and uncertain-handoff controls."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import traceback

from cursor_probe import Mcp, ProcessHandle, read, require, wait, write


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dtw',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    args=p.parse_args();require(os.name=='nt' and sys.flags.utf8_mode,'Use Windows: python -X utf8')
    binary=args.dtw.resolve(strict=True);out=args.output.resolve();require(not out.exists(),'Use a new output');out.mkdir(parents=True)
    summary={'passed':False,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'cases':[],'cleanup':[]}
    fixtures=[];clients=[];handles=[];expected_exits={}
    def fixture(name,password=False):
        d=out/name;d.mkdir()
        if password:(d/'password-editor').write_text('owned negative control',encoding='utf-8')
        label=name if name in ('A','B') else 'B'
        proc=subprocess.Popen([str(out/'build/Fixture.exe'),str(d),label]);fixtures.append((proc,d))
        wait(lambda:read(d/'state.json'));return d
    def client(name):
        c=Mcp(binary,out/name);clients.append(c);return c
    def select(c,label):
        app=next(x['app'] for x in c.tool('apps',{})['apps'] if x['app'].casefold()==str(out/'build/Fixture.exe').casefold())
        rows,_=c.execute(app,label+'-select','return (await app.windows()).windows;')
        title=label if label in ('A','B') else 'B'
        return app,next(w['window_ref'] for w in rows if w['title']=='DTW Windows Smoke '+title)
    def stop(proc,d):
        (d/'release-surrogate').write_text('release',encoding='utf-8');(d/'stop').write_text('stop',encoding='utf-8')
        code=proc.wait(timeout=7);require(code==0,'Fixture cleanup failed');fixtures.remove((proc,d))
        summary['cleanup'].append({'fixture_exit':code})
    try:
        built=subprocess.run(['powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',str(Path(__file__).with_name('compile-cursor.ps1')),
            '-OutputDirectory',str(out/'build')],capture_output=True)
        (out/'compile.stderr').write_bytes(built.stderr);require(built.returncode==0,'Fixture compilation failed')
        d=fixture('selection');c=client('selection-mcp');app,w=select(c,'selection');q=json.dumps(w)
        source=f'''await app.focus({{window_ref:{q}}});
await app.setValue({{window_ref:{q},selector:{{text:"Message"}},value:"A😀BC🐦D"}});
await app.press({{window_ref:{q},selector:{{text:"Increment"}}}});
await app.keypress({{window_ref:{q},keys:["HOME"]}});
await app.keypress({{window_ref:{q},keys:["RIGHT"]}});
await app.keypress({{window_ref:{q},keys:["RIGHT"]}});
await app.keypress({{window_ref:{q},keys:["SHIFT","RIGHT"]}});
await app.keypress({{window_ref:{q},keys:["SHIFT","RIGHT"]}});
return await app.typeText({{window_ref:{q},text:"中🐦\\nX"}});'''
        c.execute(app,'selected-text-and-linebreaks',source)
        state=wait(lambda:(s if (s:=read(d/'state.json')) and s['text'].replace('\r\n','\n')=='A😀中🐦\nX🐦D' else None))
        require(not state['keys_down_global'],'Held keys after selected Unicode input')
        summary['cases'].append('selected-text-surrounding-surrogates-and-linebreaks')
        c.execute(app,'focus-button',f'return await app.keypress({{window_ref:{q},keys:["TAB"]}});')
        wait(lambda:(s if (s:=read(d/'state.json')) and not s['editor_focused'] else None))
        before=read(d/'state.json')
        _,ledger=c.execute(app,'unsupported-focused-control',f'return await app.typeText({{window_ref:{q},text:"🐦"}});',error='unsupported_capability')
        require(ledger['script']['steps'][0]['receipt']['outcome']['dispatch']=='not_dispatched','Unsupported control received text')
        after=read(d/'state.json');require(after['text']==before['text'] and after['saves']==before['saves'],'Unsupported input changed effects')
        summary['cases'].append('unsupported-focused-control-denied-before-text');summary['cleanup'].append(c.close());clients.remove(c)
        stop(*fixtures[0])
        d=fixture('password',password=True);c=client('password-mcp');app,w=select(c,'password')
        c.execute(app,'password-focus',f'return await app.focus({{window_ref:{json.dumps(w)}}});')
        _,ledger=c.execute(app,'password-input',f'return await app.typeText({{window_ref:{json.dumps(w)},text:"🐦"}});',error='unsupported_capability')
        require(ledger['script']['steps'][0]['receipt']['outcome']['dispatch']=='not_dispatched' and read(d/'state.json')['text']=='','Password input was not denied')
        summary['cases'].append('password-denied-before-text');summary['cleanup'].append(c.close());clients.remove(c);stop(*fixtures[0])

        a=fixture('A');b=fixture('B');first=client('blocked-first');second=client('blocked-second')
        app,br=select(first,'B');_,ar=select(second,'A')
        second.execute(app,'warm-second-focus',f'return await app.focus({{window_ref:{json.dumps(ar)}}});')
        first.execute(app,'prepare-blocked',f'await app.focus({{window_ref:{json.dumps(br)}}});return await app.press({{window_ref:{json.dumps(br)},selector:{{text:"Increment"}}}});')
        process_ids=json.loads(subprocess.check_output(['powershell.exe','-NoProfile','-Command',f'@(Get-CimInstance Win32_Process -Filter "ParentProcessId = {first.proc.pid}" | Where-Object {{$_.CommandLine -match "--dtw-native-worker"}} | Select-Object -ExpandProperty ProcessId) | ConvertTo-Json -Compress'],text=True))
        if isinstance(process_ids,int):process_ids=[process_ids]
        # Same executable, two independently routed app instances. Retain every
        # owned worker; do not guess which PID routes the B window.
        require(1<=len(process_ids)<=2,'Unexpected owned native child set')
        blocked_handles=[ProcessHandle(pid,binary) for pid in process_ids];handles.extend(blocked_handles)
        for name in ['track-surrogate','block-surrogate']:(b/name).write_text('owned processing barrier',encoding='utf-8')
        request=first.begin('tools/call',{'name':'exec','arguments':{'app':app,'source':f'return await app.typeText({{window_ref:{json.dumps(br)},text:"中🐦|tail"}});'}})
        wait(lambda:(b/'surrogate-blocked').exists())
        queued=second.begin('tools/call',{'name':'exec','arguments':{'app':app,'source':f'return await app.focus({{window_ref:{json.dumps(ar)}}});'}})
        def parked():
            v=second.tool('result',{'app':app});return v if v['status']=='running' else None
        wait(parked)
        first.send({'jsonrpc':'2.0','method':'notifications/cancelled','params':{'requestId':request,'reason':'owned blocked character delivery'}})
        def settled():
            v=first.tool('result',{'app':app});return v if v['status']=='completed' else None
        original=wait(settled,8);write(out/'blocked-original-ledger.json',original)
        second.receive(queued);later=second.tool('result',{'app':app});write(out/'blocked-queued-ledger.json',later)
        require(original['script']['steps'][0]['receipt']['outcome']['dispatch']=='unknown','Original uncertainty changed')
        require(original['script']['shutdown_error'] is not None,'Unconfirmed delivery was reported as clean')
        require(later['error'] is not None and later['script']['steps'][0]['receipt']['outcome']['dispatch']=='not_dispatched','Queued owner changed foreground after unconfirmed delivery')
        require(later['script']['shutdown_error'] is not None,'Abandoned lease uncertainty was lost')
        expected_exits[first]=2;expected_exits[second]=2
        exits=wait(lambda:([h.wait() for h in blocked_handles if not h.alive()] or None))
        require(exits.count(74)==1 and all(code in (0,74) for code in exits),'Unconfirmed delivery worker exit was not retained')
        (b/'release-surrogate').write_text('release',encoding='utf-8')
        aa=read(a/'state.json');bb=wait(lambda:(s if (s:=read(b/'state.json')) and s['text']=='中🐦' and not s['keys_down_global'] else None))
        require(aa['text']=='' and aa['saves']==0 and aa['left_up']==0,'Blocked handoff leaked effects')
        summary['cases'].append({'case':'blocked-character-cancel-refuses-queued-handoff','observed_native_exits':exits,'next_owner_effects_unchanged':True,'original_dispatch':'unknown'})
        summary['passed']=True
    except BaseException:summary['failure']=traceback.format_exc()
    finally:
        for proc,d in fixtures:(d/'release-surrogate').write_text('release',encoding='utf-8')
        for handle in handles:handle.close()
        for c in clients:
            try:summary['cleanup'].append(c.close(expected_exit=expected_exits.get(c,0)))
            except BaseException:summary['passed']=False;summary['cleanup'].append({'mcp_error':traceback.format_exc()})
        for proc,d in list(fixtures):
            try:stop(proc,d)
            except BaseException:proc.kill();summary['passed']=False;summary['cleanup'].append({'fixture_forced':True})
        write(out/'summary.json',summary);print(json.dumps(summary))
    return 0 if summary['passed'] else 1


if __name__=='__main__':raise SystemExit(main())
