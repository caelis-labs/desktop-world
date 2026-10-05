#!/usr/bin/env python3
"""Opt-in rc.2 native SDK + dynamic owner acceptance on an interactive desktop.
Only creates/terminates owned fixture processes. Native effects all use dtw.
"""
import argparse
import asyncio
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import time
import uuid
ROOT=Path(__file__).resolve().parent.parent
sys.path.insert(0,str(ROOT/'clients/python'))
from desktop_world import HostSession, DesktopError, known
parser=argparse.ArgumentParser()
parser.add_argument('--helper',type=Path,required=True)
args=parser.parse_args()
windows=platform.system()=='Windows'
if platform.system() not in ['Darwin','Windows']:parser.error('interactive macOS/Windows required')
helper=str(args.helper.resolve())
out=ROOT/'artifacts'/('rc2-native-'+datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')+'-'+uuid.uuid4().hex[:6])
out.mkdir(parents=True)
ENV={**os.environ,'GOWORK':'off','PYTHONUTF8':'1'}
NO_CONSOLE=getattr(subprocess,'CREATE_NO_WINDOW',0)
active=[]
def run(command,**kwargs):return subprocess.run(command,cwd=ROOT,env=ENV,check=True,creationflags=NO_CONSOLE,**kwargs)
if windows:
 fixture=ROOT/'bin/DWNativeFixture.exe';run(['go','build','-o',str(fixture),'./tests/native-fixtures/windows'])
else:
 run(['./script/build_and_run.sh','--build-only']);fixture=ROOT/'bin/DWNativeFixture.app'
 human=out/'DTWRC2Human.app';shutil.copytree(fixture,human)
 info_path=human/'Contents/Info.plist';info=plistlib.loads(info_path.read_bytes());info['CFBundleIdentifier']='dev.caelis.rc2.human.'+uuid.uuid4().hex;info['CFBundleName']='DTWRC2Human';info_path.write_bytes(plistlib.dumps(info));run(['codesign','--force','--sign','-',str(human)])
def events(log):return [json.loads(line) for line in log.read_text(encoding='utf-8').splitlines()] if log.exists() else []
def launch(title,label,front=False):
 log=out/(label+'.jsonl')
 if windows:
  child=subprocess.Popen([str(fixture),'-title',title,'-log',str(log)],cwd=ROOT,creationflags=NO_CONSOLE);active.append(child)
 else:run(['/usr/bin/open','-n',str(human if front else fixture),'--args','--title',title,'--log',str(log),'--background','0' if front else '1','--poc-observe-seat','1'])
 deadline=time.monotonic()+10
 while time.monotonic()<deadline:
  rows=events(log)
  if any(r.get('event')=='ready' for r in rows):
   if not windows:active.append(rows[0]['pid'])
   return log
  time.sleep(.05)
 raise RuntimeError('native fixture did not become ready')
def one(ob,name):
 cov=ob['coverage']
 if not cov['complete'] or cov.get('dirty') or cov.get('truncated') or cov.get('unavailable_sources'):raise RuntimeError('incomplete native coverage')
 matches=[o for o in ob['objects'] if o.get('name',{}).get('status')=='known' and known(o['name'])==name]
 if len(matches)!=1:raise RuntimeError(f'expected unique {name}: {len(matches)}')
 return matches[0]
summary={'commit':run(['git','rev-parse','HEAD'],capture_output=True,text=True).stdout.strip(),'helper':json.loads(run([helper,'version'],capture_output=True,text=True).stdout),'platform':platform.platform(),'cases':{}}
async def accept():
 title='DTW rc2 A '+out.name;other='DTW rc2 B '+out.name
 owner=str(out/'owner.json');audit=str(out/'audit.jsonl')
 async with await HostSession.start(helper,owner_file=owner,audit=audit,write_app_windows=[title],write_apps=['DTW deliberately absent '+out.name]+([] if windows else ['DTWRC2Human'])) as host:
  grants=await host.grants();assert all(g['state']=='pending' for g in grants['grants'])
  human_title='DTW rc2 human '+out.name
  if not windows:humanlog=launch(human_title,'human',front=True)
  log=launch(title,'app-a');otherlog=launch(other,'app-b')
  if windows:humanlog=launch(human_title,'human',front=True)
  dw=host.desktop;inv=await dw.observe({'budget':{'max_results':256,'max_output_bytes':65536}})
  window=one(inv,title);second=one(inv,other);original_seat=inv.get('seat',{})
  deadline=time.monotonic()+5
  while True:
   refreshed=await host.grants()
   if any(g['state']=='active' for g in refreshed['grants']):break
   if time.monotonic()>deadline:
    summary['initial_grants']=refreshed
    raise AssertionError(refreshed)
   await asyncio.sleep(.05)
  if not windows:assert any(g.get('name')=='DTWRC2Human' and g['state']=='active' for g in refreshed['grants']),refreshed
  field=one(await dw.find(window['ref'],{'role':'text_field','name_equals':'内容'}),'内容')
  submit=one(await dw.find(window['ref'],{'role':'button','name_equals':'提交'}),'提交')
  secondfield=one(await dw.find(second['ref'],{'role':'text_field','name_equals':'内容'}),'内容')
  try:await dw.set(secondfield['ref'],'unauthorized',request_id='before-grant');raise AssertionError('APP B was authorized')
  except DesktopError:pass
  auth=json.loads(run([helper,'auth','add','--session',owner,'--app-ref',second['app'],'--id','native-add-b'],capture_output=True,text=True).stdout);assert not auth.get('error')
  if windows:
   # Startup logs do not guarantee foreground acknowledgement. Prepare only
   # the owned human field through dtw, then sample the restoration oracle.
   human_window=one(inv,human_title)
   async with await HostSession.start(helper,input_mode='shared',write_app_windows=[human_title]) as prep:
    prep_inv=await prep.desktop.observe({'budget':{'max_results':256,'max_output_bytes':65536}})
    prep_window=one(prep_inv,human_title)
    prep_field=one(await prep.desktop.find(prep_window['ref'],{'role':'text_field','name_equals':'内容'}),'内容')
    setup=await prep.desktop.act(prep.desktop.plan().focus(prep_field['ref']),request_id='human-foreground-setup')
    summary['cases']['foreground_preparation']={'passed':True,'receipt':setup}
   original_seat=(await dw.observe({'scope':{'refs':[human_window['ref']]},'projection':'detail','fields':['name'],'budget':{'max_results':1}}))['seat']
   assert known(original_seat['foreground_window'])==human_window['ref']
  token='rc.2 Python 中文 🙂';p=dw.plan().focus(field['ref']);input_ref=p.bind_focus('input',window['ref']);p.press(input_ref,'A',['primary']).type(input_ref,token).invoke(submit['ref'])
  receipt=await dw.act(p,request_id='native-python-original');assert receipt.get('input',{}).get('restoration')=='restored',receipt.get('input')
  after=await dw.observe({'scope':{'refs':[window['ref']]},'projection':'detail','fields':['name'],'budget':{'max_results':1}})
  for seat_field in ['foreground_application','foreground_window','focused_object']:
   assert known(after['seat'][seat_field])==known(original_seat[seat_field]),(seat_field,after['seat'].get(seat_field),original_seat.get(seat_field))
  assert (await dw.act(p,request_id='native-python-original'))['run_id']==receipt['run_id']
  assert known((await dw.read(field['ref']))['text'])==token
  assert [r['value'] for r in events(log) if r['event']=='submit']==[token]
  await host.revoke(window['app'])
  try:await dw.set(field['ref'],'revoked',request_id='after-revoke');raise AssertionError('APP A still authorized')
  except DesktopError:pass
  await dw.set(secondfield['ref'],'APP B independent')
  assert known((await dw.read(secondfield['ref']))['text'])=='APP B independent'
  assert known((await dw.read(field['ref']))['text'])==token
  assert (await dw.reconcile('native-python-original'))['result']['run_id']==receipt['run_id']
  summary['cases']['python_dynamic_owner']={'passed':True,'run_id':receipt['run_id'],'input':receipt.get('input'),'audit_path':host.hello.get('audit_path')}
  # Exact native package helper, with SDK runtime loaded from the checkout.
  for language,command in [('typescript',['node','clients/typescript/test/native.mjs']),('rust',['cargo','run','--locked','--quiet','--manifest-path','clients/rust/Cargo.toml','--example','native'])]:
   sdk_token='rc.2 '+language+' 中文 🙂';env={**ENV,'DTW_NATIVE_HELPER':helper,'DTW_NATIVE_TITLE':title,'DTW_NATIVE_TOKEN':sdk_token}
   result=subprocess.run(command,cwd=ROOT,env=env,check=True,capture_output=True,text=True,timeout=90,creationflags=NO_CONSOLE)
   (out/(language+'-output.txt')).write_text(result.stdout+result.stderr,encoding='utf-8')
   summary['cases'][language]=json.loads(result.stdout.strip().splitlines()[-1])
   assert [r['value'] for r in events(log) if r['event']=='submit'].count(sdk_token)==1
  # Metadata does not leak input text; rotate does not overwrite the original.
  actual=Path(host.hello['audit_path']);assert token not in actual.read_text(encoding='utf-8')
  summary['cases']['audit_privacy']={'passed':True}
  if windows:
   app_name=known(next(o for o in inv['objects'] if o['ref']==window['app'])['name'])
   async with await HostSession.start(helper,write_apps=[app_name]) as ambiguous:
    ambiguous_grants=await ambiguous.grants()
    assert any(g['state']=='ambiguous' for g in ambiguous_grants['grants']),ambiguous_grants
    own_inv=await ambiguous.desktop.observe({'budget':{'max_results':256,'max_output_bytes':65536}})
    own_window=one(own_inv,title)
    own_field=one(await ambiguous.desktop.find(own_window['ref'],{'role':'text_field','name_equals':'内容'}),'内容')
    try:await ambiguous.desktop.set(own_field['ref'],'ambiguous forbidden');raise AssertionError('ambiguous authorized')
    except DesktopError:pass
   summary['cases']['ambiguous_instances']={'passed':True}
   # The focus is explicitly in A; binding it inside B must stop before typing.
   await host.grant(window['app'])
   wrong=dw.plan().focus(field['ref']);wrong_input=wrong.bind_focus('wrong',second['ref']);wrong.type(wrong_input,'out of scope forbidden')
   try:await dw.act(wrong,request_id='focus-outside-scope');raise AssertionError('foreign focus accepted')
   except DesktopError as error:
    assert error.receipt and error.receipt['steps'][-1]['delivery']=='none',error.reply
    assert error.receipt.get('input',{}).get('restoration')=='restored',error.reply
    summary['cases']['focus_scope']={'passed':True,'receipt':error.receipt}
   assert known((await dw.read(secondfield['ref']))['text'])=='APP B independent'
   # Bind two independent aliases to A, then revoke one during a native wait.
   await host.declare_window(title)
   status=await host.grants();aliases=[g for g in status['grants'] if g.get('application')==window['app'] and g['state']=='active']
   assert len(aliases)>=2,status
   blocked=dw.plan().set(field['ref'],'waiting for revoke').add({'op':'wait','timeout_ms':2000,'after':[{'target':{'ref':field['ref']},'property':'value','equals_string':'never becomes ready'}]}).invoke(submit['ref'])
   task=asyncio.create_task(dw.act(blocked,request_id='revoke-inflight-original'))
   deadline=time.monotonic()+1
   while known((await dw.read(field['ref']))['text'])!='waiting for revoke':
    assert not task.done() and time.monotonic()<deadline
    await asyncio.sleep(.01)
   assert not task.done()
   revoked_at=time.monotonic()
   await host.revoke_grant(aliases[0]['id'])
   try:await asyncio.wait_for(task,3);raise AssertionError('revoked wait completed')
   except DesktopError as error:
    assert time.monotonic()-revoked_at<1,error.reply
    assert error.receipt and error.receipt['steps'][-1]['delivery']=='none' and error.receipt['seat_health']=='ready',error.reply
    # Predicate cancellation uses verification_timeout as well. Its 2s limit
    # cannot explain termination within 1s of revocation; all effects are known.
    assert error.receipt['fault']['code'] in ['cancelled','turn_expired','permission_denied','verification_timeout'],error.reply
    assert all(s.get('verification')!='unknown' for s in error.receipt['steps']),error.reply
    assert known((await dw.read(field['ref']))['text'])=='waiting for revoke'
    assert all(s['delivery'] not in ['unknown','partial'] for s in error.receipt['steps']),error.reply
    original=await dw.reconcile('revoke-inflight-original');assert original['result']['run_id']==error.receipt['run_id']
    summary['cases']['revoke_alias_inflight']={'passed':True,'receipt':error.receipt,'native_oracle_resolved':True,'cancellation_ms':int((time.monotonic()-revoked_at)*1000)}
   status=await host.grants();assert all(g['state']=='revoked' for g in status['grants'] if g.get('application')==window['app']),status
   await dw.set(secondfield['ref'],'B survives alias revoke')
   await asyncio.sleep(.25)
   assert len([r for r in events(log) if r['event']=='submit'])==3
  # Restart the bound APP B, retaining the original session and prior receipts.
  pid=events(otherlog)[0]['pid']
  if windows:
   child=next(c for c in active if c.pid==pid);child.terminate();child.wait(timeout=5)
  else:os.kill(pid,15);await asyncio.sleep(.2)
  deadline=time.monotonic()+3
  while True:
   expired=await host.grants()
   if any(g.get('application')==second['app'] and g['state']=='expired' for g in expired['grants']):break
   if time.monotonic()>deadline:raise AssertionError(expired)
   await asyncio.sleep(.1)
  restartedlog=launch(other,'app-b-restarted')
  new_inventory=await dw.observe({'budget':{'max_results':256,'max_output_bytes':65536}});new_window=one(new_inventory,other)
  assert new_window['app']!=second['app']
  new_field=one(await dw.find(new_window['ref'],{'role':'text_field','name_equals':'内容'}),'内容')
  try:await dw.set(new_field['ref'],'inherited');raise AssertionError('restart inherited permission')
  except DesktopError:pass
  await host.grant(new_window['app']);await dw.set(new_field['ref'],'explicit reapproval')
  assert known((await dw.read(new_field['ref']))['text'])=='explicit reapproval'
  summary['cases']['instance_restart']={'passed':True}
  await host.end_turn()
  assert (await dw.get(receipt['run_id']))['run_id']==receipt['run_id']
  await host.begin_turn('fresh-native-turn')
  assert not (await host.grants())['grants']
  fresh_inv=await dw.observe({'budget':{'max_results':256,'max_output_bytes':65536}});fresh_window=one(fresh_inv,other)
  fresh_field=one(await dw.find(fresh_window['ref'],{'role':'text_field','name_equals':'内容'}),'内容')
  try:await dw.set(fresh_field['ref'],'turn forbidden');raise AssertionError('new turn retained write grant')
  except DesktopError:pass
  await host.grant(fresh_window['app']);await dw.set(fresh_field['ref'],'fresh turn explicitly approved')
  assert known((await dw.read(fresh_field['ref']))['text'])=='fresh turn explicitly approved'
  summary['cases']['new_turn_and_original_run']={'passed':True}

 # Closing SDK stdin cancels a known native wait and prevents its queued submit.
 if windows:
  eof=await HostSession.start(helper,write_app_windows=[title])
  try:
   eof_win=one(await eof.desktop.observe({'budget':{'max_results':256,'max_output_bytes':65536}}),title)
   eof_field=one(await eof.desktop.find(eof_win['ref'],{'role':'text_field','name_equals':'内容'}),'内容')
   eof_submit=one(await eof.desktop.find(eof_win['ref'],{'role':'button','name_equals':'提交'}),'提交')
   before_submits=len([r for r in events(log) if r['event']=='submit'])
   pending=eof.desktop.plan().set(eof_field['ref'],'waiting for EOF').add({'op':'wait','timeout_ms':2000,'after':[{'target':{'ref':eof_field['ref']},'property':'value','equals_string':'never becomes ready'}]}).invoke(eof_submit['ref'])
   waiting=asyncio.create_task(eof.desktop.act(pending,request_id='stdin-eof-original'))
   deadline=time.monotonic()+1
   while known((await eof.desktop.read(eof_field['ref']))['text'])!='waiting for EOF':
    assert not waiting.done() and time.monotonic()<deadline
    await asyncio.sleep(.01)
   closed_at=time.monotonic();await eof.close()
   assert time.monotonic()-closed_at<1
   try:await waiting;raise AssertionError('EOF task completed')
   except DesktopError as error:
    assert error.code=='session_unknown' or error.receipt,error.reply
    original_reply=error.reply
   await asyncio.sleep(.3)
   assert len([r for r in events(log) if r['event']=='submit'])==before_submits
   assert [r['value'] for r in events(log) if r['event']=='set_value'][-1]=='waiting for EOF'
   # EOF can discard the in-flight transport receipt. Resolve this particular
   # finite semantic plan against the APP oracle, without replaying any effect.
   async with await HostSession.start(helper,input_policy='no_shared_input') as inspector:
    inspected_win=one(await inspector.desktop.observe({'budget':{'max_results':256,'max_output_bytes':65536}}),title)
    inspected_field=one(await inspector.desktop.find(inspected_win['ref'],{'role':'text_field','name_equals':'内容'}),'内容')
    assert known((await inspector.desktop.read(inspected_field['ref']))['text'])=='waiting for EOF'
   summary['cases']['stdin_eof']={'passed':True,'original_reply':original_reply,'exit_code':eof._transport.process.returncode,'native_oracle_resolved':True,'late_submit_count':0}
  finally:
   if eof._transport.process.returncode is None:await eof.close()

 async with await HostSession.start(helper,audit=audit,input_mode='cooperative') as second_host:
  assert second_host.hello['audit_path']!=audit
  summary['cases']['audit_rotation']={'passed':True}
 original_audit=Path(audit).read_bytes()
 try:
  unexpected=await HostSession.start(helper,audit=audit,audit_mode='create');await unexpected.close();raise AssertionError('create overwrote audit')
 except DesktopError:pass
 assert Path(audit).read_bytes()==original_audit
 async with await HostSession.start(helper,audit=audit,audit_mode='append'):
  try:
   unexpected=await HostSession.start(helper,audit=audit,audit_mode='append');await unexpected.close();raise AssertionError('two append writers admitted')
  except DesktopError:pass
 incomplete=out/'incomplete-audit.jsonl';incomplete.write_bytes(b'{"event":"incomplete"')
 try:
  unexpected=await HostSession.start(helper,audit=str(incomplete),audit_mode='append');await unexpected.close();raise AssertionError('incomplete append admitted')
 except DesktopError:pass
 assert incomplete.read_bytes()==b'{"event":"incomplete"'
 summary['cases']['audit_create_append_guards']={'passed':True}
try:
 asyncio.run(accept())
 summary['passed']=True
except BaseException as error:
 summary['passed']=False
 summary['error']={'type':type(error).__name__,'message':str(error)}
 if isinstance(error,DesktopError):summary['original_reply']=error.reply
 raise
finally:
 for child in active:
  if windows:child.terminate();child.wait(timeout=5)
  else:
   try:os.kill(child,15)
   except ProcessLookupError:pass
 (out/'summary.json').write_text(json.dumps(summary,indent=2,ensure_ascii=False)+'\n',encoding='utf-8')
 print(out)
