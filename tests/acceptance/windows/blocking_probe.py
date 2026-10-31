#!/usr/bin/env python3
"""Opt-in blocked UIA read/worker protocol controls; DTW is the only app driver."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import traceback

from cursor_probe import Mcp, ProcessHandle, read, require, wait, write


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dtw',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    args=p.parse_args()
    require(os.name=='nt' and sys.flags.utf8_mode,'Use Windows: python -X utf8')
    binary=args.dtw.resolve(strict=True); output=args.output.resolve()
    require(not output.exists(),'Use a new output directory');output.mkdir(parents=True)
    summary={'passed':False,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'cases':[],'cleanup':[]}
    fixtures=[];clients=[];handles=[]
    try:
        compiled=subprocess.run(['powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',str(Path(__file__).with_name('compile-cursor.ps1')),
                        '-OutputDirectory',str(output/'build')],capture_output=True)
        (output/'compile.stdout').write_bytes(compiled.stdout);(output/'compile.stderr').write_bytes(compiled.stderr)
        require(compiled.returncode==0,'Fixture compile failed; see compile.stderr')
        for mode in ['cancel','timeout']:
            directory=output/mode;directory.mkdir()
            exe=output/'build/Fixture.exe'
            fixture=subprocess.Popen([str(exe),str(directory),'A']);fixtures.append((fixture,directory))
            before=wait(lambda:read(directory/'state.json'))
            client=Mcp(binary,output/('mcp-'+mode));clients.append(client)
            catalog=client.tool('apps',{})
            found=[a['app'] for a in catalog['apps'] if a['app'].casefold()==str(exe).casefold()]
            require(len(found)==1,'Owned blocking fixture not in catalog')
            app=found[0]
            def snapshot(name):
                path=output/(name+'.json')
                subprocess.run([str(output/'build/CursorObserver.exe'),'--snapshot',str(path),before['hwnd']],check=True,
                               creationflags=subprocess.CREATE_NO_WINDOW)
                return read(path)
            a=snapshot(mode+'-before')
            # Inventory is Win32-only; only the next semantic find enters UIA.
            value,_=client.execute(app,mode+'-select','const w=await app.windows();return w.windows[0];')
            previous={x['pid'] for x in a['processes'] if x['parent']==client.proc.pid}
            children=[x['pid'] for x in snapshot(mode+'-selected')['processes'] if x['parent']==client.proc.pid and x['pid'] not in previous]
            require(len(children)==1,'Native provider is not uniquely owned')
            handle=ProcessHandle(children[0],binary);handles.append(handle)
            (directory/'block-uia').write_text('block',encoding='utf-8')
            timeout=5000 if mode=='cancel' else 1200
            started=time.monotonic()
            request=client.begin('tools/call',{'name':'exec','arguments':{'app':app,
                'source':f'return await app.find({{window_ref:{json.dumps(value["window_ref"])},selector:{{text:"intentionally absent blocked probe"}},timeout_ms:{timeout}}});'}})
            wait(lambda:(directory/'uia-entered').exists(),timeout=8)
            if mode=='cancel':
                client.send({'jsonrpc':'2.0','method':'notifications/cancelled','params':{'requestId':request,'reason':'owned read-only UIA barrier'}})
            # MCP cancellation may suppress the cancelled request's response.
            # Fetch the original execution ledger through result, never replay.
            if mode=='timeout':
                response=client.receive(request,timeout=38);write(output/(mode+'-response.json'),response)
            def settled():
                record=client.tool('result',{'app':app})
                return record if record.get('status')=='completed' else None
            full=wait(settled,8);write(output/(mode+'-ledger.json'),full)
            require(full['error'] is not None and full['script']['shutdown_error'] is None,'Read failure/cleanup separation lost')
            failure=full['script']['failure']
            require((failure['kind']=='cancelled' if mode=='cancel' else failure['code']=='timed_out'),failure)
            require(not full['script']['state_committed'],'Blocked read committed state')
            exit_code=handle.wait();require(exit_code==0,'Read-only worker shutdown misreported input cleanup failure')
            handle.close();handles.remove(handle)
            (directory/'release-uia').write_text('release',encoding='utf-8')
            after=wait(lambda:read(directory/'state.json') if (read(directory/'state.json') or {}).get('sequence',0)>before['sequence'] else None)
            require(all(after[k]==before[k] for k in ['text','saved','saves','increments','left_up','right_up','wheel_delta']),
                    'Read-only blocked query changed app effects')
            require(not after['keys_down_global'],'Held input after a read-only query')
            summary['cases'].append({'case':'blocked-uia-read-'+mode,'native_exit':exit_code,'effects_unchanged':True,
                                     'elapsed_seconds':round(time.monotonic()-started,3),'failure':failure})
            summary['cleanup'].append(client.close());clients.remove(client)
            (directory/'stop').write_text('stop',encoding='utf-8');require(fixture.wait(timeout=5)==0,'Fixture cleanup failed');fixtures.remove((fixture,directory))

        # Parent EOF is normal. Malformed protocol is still an error. Neither
        # test submits a native application operation or injects input.
        parent=ProcessHandle(os.getpid(),Path(sys.executable));handles.append(parent)
        for mode,payload,expected in [('empty-eof',b'',0),('malformed',b'{broken-json}\n',1)]:
            proc=subprocess.run([str(binary),'--dtw-native-worker',str(os.getpid()),str(parent.birth)],
                                input=payload,capture_output=True,timeout=8)
            (output/(mode+'.stderr')).write_bytes(proc.stderr)
            require(proc.returncode==expected,f'{mode} exit {proc.returncode}')
            summary['cases'].append({'case':'native-protocol-'+mode,'exit_code':proc.returncode})
        parent.close();handles.remove(parent)
        summary['passed']=True
    except BaseException:
        summary['failure']=traceback.format_exc()
    finally:
        for handle in handles:handle.close()
        for fixture,directory in fixtures:(directory/'release-uia').write_text('release',encoding='utf-8')
        for client in clients:
            try:summary['cleanup'].append(client.close())
            except BaseException:summary['passed']=False;summary['cleanup'].append({'error':traceback.format_exc()})
        for fixture,directory in fixtures:
            (directory/'stop').write_text('stop',encoding='utf-8')
            try:summary['cleanup'].append({'fixture_exit':fixture.wait(timeout=5)})
            except subprocess.TimeoutExpired:fixture.kill();summary['passed']=False;summary['cleanup'].append({'fixture_forced':True})
        write(output/'summary.json',summary);print(json.dumps(summary))
    return 0 if summary['passed'] else 1


if __name__=='__main__':raise SystemExit(main())
