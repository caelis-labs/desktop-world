#!/usr/bin/env python3
"""Cross-platform package CLI/real MCP lease acceptance; no desktop interaction."""
import argparse, hashlib, json, os, queue, shutil, subprocess, threading, time, uuid
from pathlib import Path
from standard_agent_probe import Mcp, stop_owned

class PackageClient(Mcp):
    """This probe initializes MCP only; it never creates native child workers.

    Reap the exact owned process without the GUI probe's Unix process scanner.
    Native worker lifetime is checked independently below on both platforms.
    """
    def close(self):
        try:
            if not self.child.stdin.closed:self.child.stdin.close()
            cleanup=stop_owned(self.child)
            self.reader.join(timeout=2)
            assert not self.reader.is_alive(),"MCP output reader did not stop"
            assert cleanup['returncode']==0 and not cleanup['forced'],cleanup
            return cleanup
        finally:
            self.child.stdout.close();self.stderr.close();self.transcript.close()
            # Release the completed process object/OS handle as well as its pipes.
            self.child=None

def native_worker_command(executable):
    command=[executable,'--dtw-native-worker']
    if os.name=='nt':
        import ctypes
        from ctypes import wintypes as w
        kernel=ctypes.WinDLL('kernel32',use_last_error=True)
        kernel.GetCurrentProcess.restype=w.HANDLE
        kernel.GetProcessTimes.argtypes=[w.HANDLE,*([ctypes.POINTER(w.FILETIME)]*4)]
        kernel.GetProcessTimes.restype=w.BOOL
        created,exited,cpu_kernel,cpu_user=(w.FILETIME() for _ in range(4))
        if not kernel.GetProcessTimes(kernel.GetCurrentProcess(),*(ctypes.byref(v) for v in [created,exited,cpu_kernel,cpu_user])):
            raise ctypes.WinError(ctypes.get_last_error())
        birth=(created.dwHighDateTime<<32)|created.dwLowDateTime
        command.extend([str(os.getpid()),str(birth)])
    return command

def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def write(path,value): path.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--dtw',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--next-dtw',type=Path);args=parser.parse_args()
    binary=args.dtw.resolve();manager=(args.next_dtw or args.dtw).resolve();out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    store=out/'managed store 中文';version=subprocess.check_output([str(binary),'--version'],encoding='utf-8').strip().split()[-1]
    manager_version=subprocess.check_output([str(manager),'--version'],encoding='utf-8').strip().split()[-1]
    def cli(*arguments,ok=True):
        result=subprocess.run([str(manager),'package',*map(str,arguments),'--store',str(store)],capture_output=True,encoding='utf-8',timeout=20)
        assert (result.returncode==0)==ok,(arguments,result.stdout,result.stderr)
        if not ok:return result.stderr
        value=json.loads(result.stdout);assert value['manager_version']==manager_version and value['store_schema']==1,value
        return value
    def source(name,executable_source=binary,*,schema=1):
        package_version=subprocess.check_output([str(executable_source),'--version'],encoding='utf-8').strip().split()[-1]
        skill_name='desktop-world' if schema==1 else 'dtw'
        root=out/('source '+name);(root/'bin').mkdir(parents=True);(root/'skills'/skill_name).mkdir(parents=True);(root/'cordis').mkdir()
        executable='dtw.exe' if os.name=='nt' else 'dtw';shutil.copy2(executable_source,root/'bin'/executable)
        for guide in ['SKILL.md','API.md','PATTERNS.md']:(root/'skills'/skill_name/guide).write_text(name+' '+guide,encoding='utf-8')
        for manifest in ['plugin.json','.claude-plugin/plugin.json','.antigravity-plugin/plugin.json','package.json']:
            path=root/manifest;path.parent.mkdir(parents=True,exist_ok=True);write(path,{'version':package_version})
        (root/'cordis/index.mjs').write_text('export{}',encoding='utf-8')
        write(root/'BUILD.json',{'package_store_schema':schema,'version':package_version,'target':'x86_64-pc-windows-msvc' if os.name=='nt' else 'aarch64-apple-darwin','binary':{'path':'bin/'+executable,'sha256':sha(root/'bin'/executable)},'skill_sha256':sha(root/'skills'/skill_name/'SKILL.md')})
        (root/'SHA256SUMS').write_text(''.join(sha(p)+'  '+p.relative_to(root).as_posix()+'\n' for p in sorted(root.rglob('*')) if p.is_file()),encoding='utf-8')
        return cli('import',root)
    a=source('A');b=source('B',manager,schema=2);c=source('C');assert cli('import',out/'source A')['package_id']==a['package_id']
    cli('bind','agy',a['package_id']);cli('activate','agy');cli('bind','dsh-desktop',a['package_id']);cli('activate','dsh-desktop')
    cli('bind','dsh-desktop',b['package_id']);staged=cli('status')['bindings'];dsh=next(r for r in staged if r['host']=='dsh-desktop');assert dsh['current']==a['package_id'] and dsh['pending']==b['package_id']
    fresh=out/'fresh-version';fresh.mkdir();verified=PackageClient(Path(b['executable']),fresh)
    try:
        initialized=verified.rpc('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'updated-runtime-verifier','version':'1'}});verified.notify('notifications/initialized')
        assert initialized['serverInfo']['version']==b['version'] and initialized['serverInfo']['name']=='dtw',initialized
        assert [t['name'] for t in verified.rpc('tools/list',{})['tools']]==['apps','exec']
    finally:verified.close()
    cli('activate','dsh-desktop');live=out/'live';live.mkdir();m=PackageClient(Path(c['executable']),live)
    try:
        old_runtime=m.rpc('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'managed-package-lease','version':'1'}});m.notify('notifications/initialized')
        assert old_runtime['serverInfo']['version']==a['version'],old_runtime
        assert [t['name'] for t in m.rpc('tools/list',{})['tools']]==['apps','exec']
        for package in (store/'packages').iterdir():
            path=package/'PACKAGE.json';record=json.loads(path.read_text(encoding='utf-8'));record['imported_at']=int(time.time())-8*86400;write(path,record)
        rows=cli('gc','--apply')['packages'];assert next(r for r in rows if r['package_id']==c['package_id'])['retained_by']=='running',rows
        assert all(next(r for r in rows if r['package_id']==p['package_id'])['retained_by']=='host_pin' for p in [a,b])
    finally:m.close()
    # A worker protects itself after the MCP supervisor is already gone. Release
    # of a nonexistent binding performs no AX/SCK/input/desktop operation.
    with (out/'worker-stderr.log').open('w',encoding='utf-8') as error:
        worker=subprocess.Popen(native_worker_command(c['executable']),stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=error,text=True,encoding='utf-8')
        try:
            request_id=str(uuid.uuid4());worker.stdin.write(json.dumps({'request_id':request_id,'operation':'release','binding':'never-created-'+str(uuid.uuid4())})+'\n');worker.stdin.flush()
            messages=queue.Queue();output_reader=threading.Thread(target=lambda:messages.put(worker.stdout.readline()),daemon=True);output_reader.start()
            line=messages.get(timeout=5);output_reader.join(timeout=2)
            if not line:
                worker.wait(timeout=5);error.flush();raise AssertionError({'exit':worker.returncode,'stderr':Path(error.name).read_text(encoding='utf-8',errors='replace')[-4096:]})
            assert json.loads(line)=={'request_id':request_id,'result':{'released':False}}
            rows=cli('gc','--apply')['packages'];assert next(r for r in rows if r['package_id']==c['package_id'])['retained_by']=='running',rows
        finally:
            worker.stdin.close()
            try:worker.wait(timeout=5)
            except subprocess.TimeoutExpired:worker.kill();worker.wait(timeout=5);raise
            worker.stdout.close()
        assert worker.returncode==0,worker.returncode
        del worker
    rows=cli('gc','--apply')['packages'];assert next(r for r in rows if r['package_id']==c['package_id'])['action']=='removed',rows
    assert Path(a['package_path']).exists() and Path(b['package_path']).exists() and not Path(c['package_path']).exists()
    (out/'source A/skills/desktop-world/API.md').write_text('corrupt',encoding='utf-8');assert 'checksum' in cli('import',out/'source A',ok=False)
    cli('unbind','agy');cli('unbind','dsh-desktop');collected=cli('gc','--apply');assert not cli('status')['packages']
    report={'passed':True,'binary_sha256':sha(binary),'manager_sha256':sha(manager),'baseline_version':a['version'],'target_version':b['version'],'scope':'synthetic complete packages + real MCP process/locks; no host configuration or GUI','cases':['Unicode/space paths','complete-package deduplication','two host pins','schema1 baseline and schema2 upgrade','manager/runtime version update','fresh actual runtime version verification','pending failure protection','rollback protection','live MCP shared lease','independent native worker lease','collection after process exit','corruption rejection','owned cleanup'],'collection':collected}
    write(out/'report.json',report);print(json.dumps(report,ensure_ascii=True))
if __name__=='__main__':main()
