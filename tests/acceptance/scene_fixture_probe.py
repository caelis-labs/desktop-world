#!/usr/bin/env python3
"""Real GUI contract against tests/fixtures/scene. Writes only fixture commands.
Usage: DTW_CUA_WORKER=... python3 scene_fixture_probe.py DTW FIXTURE_DIR EVIDENCE_DIR [SCENE_SESSION_ROOT]
The optional scene_session.py runtime reuses an existing process; this is explicitly
not a cold-start test. Both modes retain original receipts and PNGs.
"""
import base64,json,shutil,socket,subprocess,sys,time
from pathlib import Path
from probe import call,exec_call,result,structured_result
binary,fixture_path,output=sys.argv[1:4]
shared=Path(sys.argv[4]) if len(sys.argv)>4 else None
last_folder=None
fixture=Path(fixture_path);out=Path(output);out.mkdir(parents=True,exist_ok=False)
pid=json.loads((fixture/'state.json').read_text())['pid']
proc=None if shared else subprocess.Popen([binary],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(out/'stderr.log').open('w'),text=True)
ident=1

def run(args):
 global ident,last_folder
 ident+=10
 if shared:
  with socket.socket(socket.AF_UNIX) as client:
   client.connect(str(shared/'socket'));client.sendall((json.dumps(args)+'\n').encode())
   response=json.loads(client.makefile().readline())
  assert 'error' not in response,response
  last_folder=Path(response['folder']);v=response['receipt']
 else:
  v=exec_call(proc,ident,args)
  if v.get('status')=='running':v=result(proc,ident+1,None,v['id'])
 (out/f'{ident}-receipt.json').write_text(json.dumps({'request':args,'result':v},indent=2))
 return v

def observe(limit=2):
 for attempt in range(3):
  r=run({'operation':'app','pid':pid,'max_windows':limit})
  if r.get('status')=='completed':break
  time.sleep(.3)
 assert not r.get('is_error'),r
 v=json.loads((last_folder/'native.json').read_text()) if shared else structured_result(proc,ident+2,None,r['id'])
 (out/f'{ident}-observation.json').write_text(json.dumps(v,indent=2))
 for i in range(len(v['frames'])):
  if shared:
   shutil.copyfile(last_folder/f'image-{i}.png',out/f'{ident}-image-{i}.png');continue
  reply=call(proc,{'jsonrpc':'2.0','id':ident+3+i,'method':'tools/call','params':{'name':'exec','arguments':{'operation':'result','id':r['id'],'part':'image','image_index':i}}})
  image=next(c for c in reply['result']['content'] if c['type']=='image')
  (out/f'{ident}-image-{i}.png').write_bytes(base64.b64decode(image['data']))
 return v

def click(v,index=0):
 f=v['frames'][index]
 return run({'operation':'click','observation_id':v['observation_id'],'image_index':index,'x':f['width']*.5,'y':f['height']*.55})

def command(name):
 (fixture/'command').write_text(name);time.sleep(.4)

def count():return json.loads((fixture/'state.json').read_text())['clicks']
try:
 if proc:call(proc,{'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'scene-fixture','version':'1'}}})
 v=observe();assert len(v['windows'])==3 and len(v['frames'])==2 and v['omitted_count']==1,v
 expanded=observe(8);assert len(expanded['frames'])==3,expanded
 before=count();r=click(expanded);assert not r.get('is_error'),r
 time.sleep(.3);assert count()==before+1,(r,count(),before)
 reused=click(expanded);assert reused.get('code')=='stale_observation',reused
 v=observe();command('move');stale=click(v);assert stale.get('code')=='stale_observation',stale
 assert count()==before+1
 command('restore');v=observe();command('close');closed=click(v);assert closed.get('code')=='stale_observation',closed
 assert count()==before+1
 command('restore');v=observe();run({'script':'return 1;'});invalidated=click(v);assert invalidated.get('code')=='stale_observation',invalidated
 # CUA's ordinary-window route remains usable through the same point contract.
 v=observe(8);ordinary_backend=v['frames'][2]['backend'];base=count();ordinary=click(v,2);assert not ordinary.get('is_error'),ordinary
 time.sleep(.3);assert count()==base+1,(ordinary,count(),base)
 # A separately launched process must not be attributed to the target or clicked through.
 v=observe();base=count();cover=out/'cover'
 subprocess.run(['open','-n',str(fixture/'DTWSceneFixture.app'),'--args',str(cover)],check=True)
 deadline=time.monotonic()+8
 while not (cover/'state.json').exists() and time.monotonic()<deadline:time.sleep(.1)
 assert (cover/'state.json').exists(),'Cover fixture failed to launch'
 time.sleep(.3)
 try:
  blocked=click(v);assert blocked.get('code') in ('target_occluded','stale_observation'),blocked
  assert count()==base
  assert json.loads((cover/'state.json').read_text())['clicks']==0
 finally:(cover/'command').write_text('quit')
 time.sleep(.3)
 # Expiry is checked without sending input; no synthetic clock override.
 v=observe();time.sleep(30.2);expired=click(v);assert expired.get('code')=='stale_observation',expired
 print(json.dumps({'runtime':'existing' if shared else 'fresh','pid':pid,'inventory':3,'default_images':2,'expanded_images':3,'pixel_effect':'independent counter +1','ordinary_point_effect':'independent counter +1','ordinary_backend':ordinary_backend,'reuse':'refused','moved':'refused','closed':'refused','intervening_script':'refused','opaque_other_process':'refused','expired':'refused'}))
finally:
 if proc:
  proc.stdin.close()
  try:proc.wait(timeout=8)
  except subprocess.TimeoutExpired:proc.kill();proc.wait()
  assert proc.returncode==0,proc.returncode
