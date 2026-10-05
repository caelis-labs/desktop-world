#!/usr/bin/env python3
"""Windows: install archived SDKs outside the checkout and run real dtw tasks."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

ROOT=Path(__file__).resolve().parent.parent
parser=argparse.ArgumentParser()
parser.add_argument('--package',type=Path,required=True)
args=parser.parse_args()
if os.name!='nt':parser.error('requires the Windows interactive acceptance host')
package=args.package.resolve();helper=package/'bin/dtw.exe'
out=ROOT/'artifacts'/('rc2-package-native-'+datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')+'-'+uuid.uuid4().hex[:6])
out.mkdir(parents=True)
consumer=Path(tempfile.mkdtemp(prefix='dtw-rc2-consumer-'))
env={**os.environ,'GOWORK':'off','PYTHONUTF8':'1'}
flags=subprocess.CREATE_NO_WINDOW
summary={'package':str(package),'consumer':str(consumer),'helper_sha256':hashlib.sha256(helper.read_bytes()).hexdigest(),'cases':{}}
children=[]
def run(command,**kwargs):
 result=subprocess.run([str(x) for x in command],cwd=consumer,env=kwargs.pop('env',env),creationflags=flags,capture_output=True,text=True,encoding='utf-8',timeout=180,**kwargs)
 with (out/'commands.jsonl').open('a',encoding='utf-8') as log:log.write(json.dumps({'command':[str(x) for x in command],'exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr},ensure_ascii=False)+'\n')
 if result.returncode:raise RuntimeError(f'command failed: {command}; inspect original commands.jsonl')
 return result
def rows(path):return [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines()] if path.exists() else []
try:
 summary['helper']=json.loads(run([helper,'version']).stdout)
 # Real npm archive install, rather than a symlink to a developer checkout.
 ts=consumer/'typescript';ts.mkdir()
 packed=json.loads(run(['npm.cmd','pack',str(package/'clients/typescript'),'--pack-destination',str(ts),'--json']).stdout)[0]
 (ts/'package.json').write_text('{"private":true,"type":"module"}',encoding='utf-8')
 run(['npm.cmd','install','--prefix',str(ts),'--ignore-scripts','--no-audit','--no-fund',str(ts/packed['filename'])])
 source=(package/'source/clients/typescript/test/native.mjs').read_text(encoding='utf-8').replace("'../dist/index.js'","'@caelis-labs/desktop-world'")
 (ts/'native.mjs').write_text(source,encoding='utf-8')
 py=consumer/'python';run([sys.executable,'-m','venv',str(py)])
 interpreter=py/'Scripts/python.exe';wheel_dir=consumer/'wheels';wheel_dir.mkdir()
 run([interpreter,'-m','pip','wheel','--no-deps','--wheel-dir',str(wheel_dir),str(package/'clients/python')])
 wheel=next(wheel_dir.glob('*.whl'));run([interpreter,'-m','pip','install','--no-deps',str(wheel)])
 (consumer/'native.py').write_text('''import asyncio,json,os,desktop_world
from desktop_world import HostSession,known
def one(ob,name):
 assert ob['coverage']['complete'] and not ob['coverage'].get('dirty') and not ob['coverage'].get('truncated')
 matches=[o for o in ob['objects'] if o.get('name',{}).get('status')=='known' and known(o['name'])==name]
 assert len(matches)==1
 return matches[0]
async def main():
 async with await HostSession.start(os.environ['DTW_NATIVE_HELPER'],write_app_windows=[os.environ['DTW_NATIVE_TITLE']]) as host:
  dw=host.desktop;win=one(await dw.observe({'budget':{'max_results':256,'max_output_bytes':65536}}),os.environ['DTW_NATIVE_TITLE'])
  field=one(await dw.find(win['ref'],{'role':'text_field','name_equals':'内容'}),'内容');button=one(await dw.find(win['ref'],{'role':'button','name_equals':'提交'}),'提交')
  p=dw.plan().focus(field['ref']);focused=p.bind_focus('input',win['ref']);p.press(focused,'A',['primary']).type(focused,os.environ['DTW_NATIVE_TOKEN']).invoke(button['ref'])
  r=await dw.act(p,request_id='package-python-original')
  assert (await dw.act(p,request_id='package-python-original'))['run_id']==r['run_id']
  assert known((await dw.read(field['ref']))['text'])==os.environ['DTW_NATIVE_TOKEN']
  print(json.dumps({'language':'python','verified':True,'module':desktop_world.__file__,'run_id':r['run_id'],'input':r['input']}))
asyncio.run(main())
''',encoding='utf-8')
 rust=consumer/'rust';(rust/'src').mkdir(parents=True)
 (rust/'Cargo.toml').write_text('[package]\nname="dtw_rc2_independent_consumer"\nversion="0.0.0"\nedition="2021"\n[dependencies]\ncaelis-desktop-world={path='+json.dumps(str(package/'clients/rust'))+'}\ntokio={version="1",features=["rt","macros"]}\nserde_json="1"\n',encoding='utf-8')
 shutil.copyfile(package/'source/clients/rust/examples/native.rs',rust/'src/main.rs')
 run(['cargo','build','--quiet','--manifest-path',str(rust/'Cargo.toml')])
 fixture=ROOT/'bin/DWNativeFixture.exe'
 subprocess.run(['go','build','-o',str(fixture),'./tests/native-fixtures/windows'],cwd=ROOT,env=env,creationflags=flags,check=True)
 # Remove Node, Cargo, Go and developer PYTHONPATH from Python/Rust runtime.
 native_env={k:v for k,v in env.items() if k not in ['PYTHONPATH','NODE_PATH']}
 native_env['PATH']=str(Path(os.environ['SystemRoot'])/'System32')+';'+os.environ['SystemRoot']
 for language,command in [('typescript',['node',ts/'native.mjs']),('python',[interpreter,'-I',consumer/'native.py']),('rust',[rust/'target/debug/dtw_rc2_independent_consumer.exe'])]:
  title='DTW isolated '+language+' '+out.name;log=out/(language+'-events.jsonl')
  child=subprocess.Popen([str(fixture),'-title',title,'-log',str(log)],cwd=consumer,creationflags=flags);children.append(child)
  deadline=time.monotonic()+10
  while not any(r['event']=='ready' for r in rows(log)):
   if time.monotonic()>deadline:raise RuntimeError('fixture startup failed')
   time.sleep(.05)
  token='rc.2 package '+language+' 中文 🙂'
  runtime_env={**(env if language=='typescript' else native_env),'DTW_NATIVE_HELPER':str(helper),'DTW_NATIVE_TITLE':title,'DTW_NATIVE_TOKEN':token}
  result=run(command,env=runtime_env)
  receipt=json.loads(result.stdout.strip().splitlines()[-1]);assert receipt['verified'] and receipt['input']['restoration']=='restored',receipt
  assert [r['value'] for r in rows(log) if r['event']=='submit']==[token]
  if language=='python':assert Path(receipt['module']).is_relative_to(py)
  receipt.update({'passed':True,'independent_submit_count':1,'node_required':language=='typescript'})
  summary['cases'][language]=receipt
  child.terminate();child.wait(timeout=5)
 summary['passed']=True
except BaseException as error:
 summary.update({'passed':False,'error':{'type':type(error).__name__,'message':str(error)}})
 raise
finally:
 for child in children:
  if child.poll() is None:child.terminate();child.wait(timeout=5)
 (out/'summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
 print(out)
