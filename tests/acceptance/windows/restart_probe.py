#!/usr/bin/env python3
"""Same-executable sibling/restart routing on owned Windows fixtures via MCP."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import traceback

from cursor_probe import Mcp, read, require, wait, write


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dtw',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    args=p.parse_args();require(os.name=='nt' and sys.flags.utf8_mode,'Use Windows: python -X utf8')
    binary=args.dtw.resolve(strict=True);out=args.output.resolve();require(not out.exists(),'Use new output');out.mkdir(parents=True)
    summary={'passed':False,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'cases':[],'cleanup':[]}
    owned=[];client=None
    def start(label,version):
        d=out/(label+version);d.mkdir();proc=subprocess.Popen([str(out/'build/Fixture.exe'),str(d),label]);owned.append((proc,d))
        wait(lambda:(s if (s:=read(d/'state.json')) and s['pid']==proc.pid and s['ready'] else None))
        return proc,d
    def stop(pair):
        proc,d=pair;(d/'stop').write_text('stop',encoding='utf-8');require(proc.wait(timeout=5)==0,'Fixture did not stop cleanly');owned.remove(pair)
    def reject(app,ref,label):
        compact=client.tool('exec',{'app':app,'source':f'return await app.press({{window_ref:{json.dumps(ref)},selector:{{text:"Increment"}}}});'})
        full=client.tool('result',{'app':app,'execution_id':compact['execution_id']});write(out/(label+'-ledger.json'),full)
        require(full['status']=='completed' and full['error'] is not None,'Old window unexpectedly accepted')
        steps=full.get('script',{}).get('steps',[])
        require(all(not s.get('receipt') or s['receipt']['outcome']['dispatch']=='not_dispatched' for s in steps),'Old reference reached input')
        codes=[full.get('script',{}).get('failure',{}).get('code')]+[s.get('error',{}).get('code') for s in steps if s.get('error')]
        require(any(c in ['stale_window_ref','invalid_window_ref','window_not_found','target_changed','window_closed'] for c in codes),codes)
        return full
    try:
        build=subprocess.run(['powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',str(Path(__file__).with_name('compile-cursor.ps1')),
                              '-OutputDirectory',str(out/'build')],capture_output=True)
        (out/'compile.stdout').write_bytes(build.stdout);(out/'compile.stderr').write_bytes(build.stderr);require(build.returncode==0,'Fixture compile failed')
        a=start('A','1');b=start('B','1');client=Mcp(binary,out/'mcp')
        catalog=client.tool('apps',{});apps=[x for x in catalog['apps'] if x['app'].casefold()==str(out/'build/Fixture.exe').casefold()]
        require(len(apps)==1 and apps[0]['instances']==2,'Same-executable instances did not aggregate');app=apps[0]['app']
        refs,_=client.execute(app,'initial','state.proof="sibling-state";const w=await app.windows();return {proof:state.proof,windows:w.windows};')
        windows={w['title']:w['window_ref'] for w in refs['windows']};ar=windows['DTW Windows Smoke A'];br=windows['DTW Windows Smoke B']
        stop(b)
        survivor,ledger=client.execute(app,'surviving-sibling',f'return {{proof:state.proof,view:await app.observe({{window_ref:{json.dumps(ar)},text:"Increment"}})}};')
        require(survivor['proof']=='sibling-state' and not ledger['state_reset'] and not ledger['context_reset'],'Sibling exit reset surviving state/context')
        reject(app,br,'closed-sibling')
        summary['cases'].append('sibling-exit-preserves-survivor-reference-and-state')
        b=start('B','2');reject(app,br,'restarted-same-title')
        require(read(a[1]/'state.json')['increments']==0 and read(b[1]/'state.json')['increments']==0,'Old reference changed a live instance')
        current,ledger=client.execute(app,'fresh-restarted-sibling','return {proof:state.proof,windows:(await app.windows()).windows};')
        require(current['proof']=='sibling-state' and not ledger['state_reset'] and not ledger['context_reset'],'Sibling join reset state/context')
        fresh=next(w['window_ref'] for w in current['windows'] if w['title']=='DTW Windows Smoke B');require(fresh!=br,'Restart reused old reference')
        client.execute(app,'fresh-ref-press',f'return await app.press({{window_ref:{json.dumps(fresh)},selector:{{text:"Increment"}}}});')
        wait(lambda:(s if (s:=read(b[1]/'state.json')) and s['increments']==1 else None))
        require(read(a[1]/'state.json')['increments']==0,'Fresh restarted reference changed sibling')
        summary['cases'].append('same-title-restart-old-reference-rejected-fresh-reference-exactly-once')
        stop(a);stop(b);a=start('A','3')
        reset,ledger=client.execute(app,'all-instances-replaced','return {empty:state.proof===undefined,windows:(await app.windows()).windows};')
        require(reset['empty'] and ledger['state_reset'],'Replacing every instance retained old logical state')
        reject(app,ar,'all-replaced-old-ref');require(read(a[1]/'state.json')['increments']==0,'Old all-replaced reference changed new app')
        summary['cases'].append('all-instances-replaced-resets-state-and-rejects-old-reference')
        summary['passed']=True
    except BaseException:summary['failure']=traceback.format_exc()
    finally:
        if client:
            try:summary['cleanup'].append(client.close())
            except BaseException:summary['passed']=False;summary['cleanup'].append({'mcp_error':traceback.format_exc()})
        for pair in owned[:]:
            try:stop(pair);summary['cleanup'].append({'fixture_exit':0})
            except BaseException:pair[0].kill();summary['passed']=False;summary['cleanup'].append({'fixture_forced':True})
        write(out/'summary.json',summary);print(json.dumps(summary))
    return 0 if summary['passed'] else 1


if __name__=='__main__':raise SystemExit(main())
