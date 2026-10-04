#!/usr/bin/env python3
"""Independent full-operation real-desktop POC; no target DOM/AX action shims."""
import argparse,hashlib,json,os,plistlib,shutil,subprocess,time,uuid,threading
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from pathlib import Path
import support as h
ROOT=h.ROOT
parser=argparse.ArgumentParser();parser.add_argument('--provider',choices=['appkit','chrome','electron'],default='appkit');args=parser.parse_args()
server=None
browser_process=None
web_events=[]
out=ROOT/'artifacts'/('background-full-'+time.strftime('%Y%m%dT%H%M%SZ',time.gmtime())+'-'+uuid.uuid4().hex[:6]);out.mkdir(parents=True)
h.out=out;h.env=dict(os.environ,GOWORK='off')
active=[]
def run(cmd): return subprocess.run(cmd,cwd=ROOT,env=h.env,check=True,capture_output=True,text=True).stdout
def events(path): return [json.loads(x) for x in path.read_text().splitlines()] if path.exists() else []
def launch(label,human=False):
 bundle=out/(label+'.app');shutil.copytree(ROOT/'bin/DTWFullFixture.app',bundle)
 p=bundle/'Contents/Info.plist';info=plistlib.loads(p.read_bytes());info['CFBundleIdentifier']='dev.caelis.dtw.full.'+label.lower();p.write_bytes(plistlib.dumps(info));run(['codesign','--force','--sign','-',str(bundle)])
 title='DTW Full '+label+' '+out.name;log=out/(label+'.jsonl')
 run(['/usr/bin/open','-n']+([] if human else ['-g'])+[str(bundle),'--args','--title',title,'--log',str(log),'--background','0' if human else '1','--human','1' if human else '0'])
 for _ in range(200):
  rows=events(log)
  if rows: active.append(rows[0]['pid']);return title,log
  time.sleep(.05)
 raise RuntimeError('startup timeout')
def step(op,o,**kw): return dict(op=op,target={'ref':o['ref']},**kw)
def pointer(op,o,u=.5,v=.5,**kw): return dict(op=op,target={'anchor':{'target':o['ref'],'u':u,'v':v}},**kw)
summary={'provider':args.provider,'status':'failed','features':[],'base_commit':run(['git','rev-parse','HEAD']).strip()}
try:
 if not os.environ.get('DTW_ACCEPT_HELPER'):run(['go','build','-o','bin/dtw','./cmd/dtw'])
 run(['./poc/background-input/build-full.sh'])
 if args.provider=='appkit': bt,bl=launch('Agent')
 else:
  bt='DTW Browser '+out.name;bl=out/'browser.jsonl'
  html=(ROOT/'poc/background-input/Fixture.html').read_text().replace('__TITLE__',bt).encode()
  class Handler(BaseHTTPRequestHandler):
   def log_message(self,*args): pass
   def do_GET(self):
    self.send_response(200);self.send_header('Content-Type','text/html; charset=utf-8');self.end_headers();self.wfile.write(html)
   def do_POST(self):
    row=json.loads(self.rfile.read(int(self.headers['Content-Length'])));row['time']=time.time();web_events.append(row)
    with bl.open('a') as f: f.write(json.dumps(row,ensure_ascii=False)+'\n')
    self.send_response(204);self.end_headers()
  server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
  if args.provider=='chrome':
   command=['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome','--user-data-dir='+str(out/'chrome-profile'),'--no-first-run','--no-default-browser-check','--force-renderer-accessibility','--window-position=180,200','--window-size=600,650','http://127.0.0.1:'+str(server.server_port)]
  else:
   application=out/'electron-app';application.mkdir()
   (application/'package.json').write_text(json.dumps({'name':'dtw-native-electron-fixture','version':'1.0.0','main':'main.cjs'}))
   (application/'main.cjs').write_text("const {app,BrowserWindow}=require('electron');app.setPath('userData',"+json.dumps(str(out/'electron-profile'))+");app.commandLine.appendSwitch('force-renderer-accessibility');app.whenReady().then(()=>{app.setAccessibilitySupportEnabled(true);const w=new BrowserWindow({x:180,y:200,width:600,height:650,webPreferences:{contextIsolation:true,nodeIntegration:false}});w.loadURL("+json.dumps('http://127.0.0.1:'+str(server.server_port))+");});app.on('window-all-closed',()=>app.quit());")
   command=[str(ROOT/'bin/dtw-electron-poc/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'),str(application)]
  browser_process=subprocess.Popen(command,stdout=(out/'browser.stdout').open('w'),stderr=(out/'browser.stderr').open('w'))
  active.append(browser_process.pid);time.sleep(2)
  discovery=h.Client('discovery',None)
  windows=discovery.observe({'desktop':True},'summary',['name','role','app']);matches=[o for o in windows if bt in (h.known(o.get('name')) or '')];assert len(matches)==1,matches
  bt=h.known(matches[0]['name'])
 ht,hl=launch('Human',True)
 bg=h.Client('agent',bt,'cooperative');human=h.Client('human',ht)
 bw,hw=bg.window(bt),human.window(ht);field=bg.find(bw,'POC text');multi=bg.find(bw,'POC multiline');canvas=bg.find(bw,'POC Canvas');submit=bg.find(bw,'POC submit');dialog=bg.find(bw,'POC dialog');hf=human.find(hw,'POC text')
 human.act([step('focus',hw),step('focus',hf)])
 def check(name,steps,event=None,value=None,error=None):
  start=time.time();before=len(events(bl));request='feature-'+name
  reply=bg.act(steps,identity=request,allow_error=True);row={'name':name,'receipt':reply,'seconds':time.time()-start};summary['features'].append(row)
  if error: assert reply.get('error',{}).get('code')==error,reply
  else:
   if reply.get('error'):
    row['diagnostic']=bg.observe({'refs':[bw['ref']]},'outline',['name','role','capabilities'],{'within':bw['ref'],'name_equals':'POC menu commit'})
    raise AssertionError((reply,row['diagnostic']))
   assert reply['result']['input']['restoration'] in ('restored','not_borrowed'),reply
   if event:
    for _ in range(20):
     found=[e for e in events(bl)[before:] if e['event']==event and (value is None or e['value']==value)]
     if found: break
     time.sleep(.02)
    assert found,(name,event,value,events(bl)[before:]);row['business_evidence']=found
    if name in ('vertical-scroll','horizontal-scroll'):
     dx,dy=map(float,found[-1]['value'].split(','));assert (dx==0 and dy!=0) if name=='vertical-scroll' else (dx!=0 and dy==0),found
    if name=='drag' and args.provider!='appkit':
     dx,dy=map(float,found[-1]['value'].split(','));assert abs(dx)>100 and abs(dy)<5,found
  again=bg.act(steps,identity=request,allow_error=True);assert again['result']['run_id']==reply['result']['run_id']
  # Simulate the user's independent work between transactions.
  human.act([step('keyboard.type_text',hf,type_text={'text':name+';'})])
  print(name+': passed',flush=True)
 check('move',[pointer('pointer.move',canvas,.3,.5)],'move')
 check('single',[pointer('pointer.click',canvas,.3,.5,click={'button':'left','count':1})],'click','1')
 check('double',[pointer('pointer.click',canvas,.3,.5,click={'button':'left','count':2})],'double')
 check('middle',[pointer('pointer.click',canvas,.3,.5,click={'button':'middle','count':1})],'middle',('2' if args.provider=='appkit' else '1'))
 check('drag',[pointer('pointer.drag',canvas,.2,.5,drag={'to':{'anchor':{'target':canvas['ref'],'u':.7,'v':.5}},'duration_ms':250})],'drop',('215,0' if args.provider=='appkit' else None))
 check('vertical-scroll',[pointer('pointer.scroll',canvas,scroll={'dx':0,'dy':3,'unit':'wheel_step'})],'scroll')
 check('horizontal-scroll',[pointer('pointer.scroll',canvas,scroll={'dx':3,'dy':0,'unit':'wheel_step'})],'scroll')
 text='Full-中文-🙂'
 check('unicode-submit',[step('pointer.click',field,click={'button':'left','count':1}),step('keyboard.type_text',field,type_text={'text':text},completion='verify',after=[{'target':{'ref':field['ref']},'property':'value','equals_string':text}]),step('pointer.click',submit,click={'button':'left','count':1})],'submit',text)
 check('shortcut-replace',[step('pointer.click',field,click={'button':'left','count':1}),step('keyboard.press',field,press={'key':'A','modifiers':['primary']}),step('keyboard.type_text',field,type_text={'text':'replaced'}),step('keyboard.press',field,press={'key':'Left','modifiers':['shift']}),step('keyboard.press',field,press={'key':'Backspace'})],'text','replace')
 check('multiline',[step('pointer.click',multi,click={'button':'left','count':1}),step('keyboard.type_text',multi,type_text={'text':'Line1\n中文🙂\nLine3'},completion='verify',after=[{'target':{'ref':multi['ref']},'property':'value','equals_string':'Line1\n中文🙂\nLine3'}])],'multiline','Line1\n中文🙂\nLine3')
 check('window-key',[step('keyboard.press',bw,press={'key':'Tab'})],('multiline' if args.provider=='appkit' else 'focus'),('Line1\n中文🙂\nLine3\t' if args.provider=='appkit' else 'POC Canvas'))
 # Bind the exact new transient control inside the same short transaction.
 app=bw['app'] if args.provider=='appkit' else bw['ref']
 menu=[pointer('pointer.click',canvas,click={'button':'right','count':1}),{'op':'bind','bind':{'name':'menu','require_unique':True,'locator':{'within':app,'name_equals':'POC menu commit','role':'menu_item','max_depth':12}}},{'op':'invoke','target':{'bound':'menu'}}]
 check('context-menu',menu,'menu_commit')
 sheet=[step('pointer.click',dialog,click={'button':'left','count':1}),{'op':'bind','bind':{'name':'dialogtext','require_unique':True,'locator':{'within':bw['ref'],'name_equals':'POC dialog text','role':'text_field','max_depth':12}}},{'op':'pointer.click','target':{'bound':'dialogtext'},'click':{'button':'left','count':1}},{'op':'keyboard.type_text','target':{'bound':'dialogtext'},'type_text':{'text':'CONFIRMED-中文'}},{'op':'bind','bind':{'name':'confirm','require_unique':True,'locator':{'within':bw['ref'],'name_equals':'POC confirm','role':'button','max_depth':12}}},{'op':'invoke','target':{'bound':'confirm'}}]
 check('dialog',sheet,'dialog_text','CONFIRMED-中文')
 canvas=bg.find(bw,'POC Canvas');field=bg.find(bw,'POC text')
 check('long-drag-rejected',[pointer('pointer.drag',canvas,drag={'to':{'ref':canvas['ref']},'duration_ms':501})],error='input_burst_limit')
 check('long-text-rejected',[step('keyboard.type_text',field,type_text={'text':'x'*257})],error='input_burst_limit')
 if args.provider=='appkit':
  # Interrupt a dispatched drag by the step deadline. Inspect its original run
  # until native cleanup completes; never issue a replacement drag.
  cancel_steps=[pointer('pointer.click',canvas,click={'button':'left','count':1}),pointer('pointer.drag',canvas,.2,.5,drag={'to':{'anchor':{'target':canvas['ref'],'u':.7,'v':.5}},'duration_ms':500},timeout_ms=100)]
  cancelled=bg.act(cancel_steps,identity='cancel-drag',allow_error=True);assert cancelled.get('error'),cancelled
  run_id=cancelled['result']['run_id']
  for _ in range(100):
   cancelled=bg.call('get',{'run_id':run_id},allow_error=True)
   if cancelled['result'].get('input',{}).get('restoration')=='restored' and cancelled['result']['seat_health']=='ready':break
   time.sleep(.02)
  assert cancelled['result']['input']['restoration']=='restored' and cancelled['result']['seat_health']=='ready',cancelled
  assert bg.act(cancel_steps,identity='cancel-drag',allow_error=True)['result']['run_id']==run_id
  summary['cancel_drag']=cancelled
  human.act([step('keyboard.type_text',hf,type_text={'text':'cancel;'})]);summary['features'].append({'name':'cancel'})
  wait_steps=[pointer('pointer.click',canvas,click={'button':'left','count':1}),{'op':'wait','timeout_ms':2500,'after':[{'target':{'ref':field['ref']},'property':'value','equals_string':'NEVER'}]}]
  began=time.monotonic();expired=bg.act(wait_steps,identity='expire-lease',allow_error=True)
  assert expired.get('error',{}).get('code')=='input_lease_expired',expired
  assert expired['result']['input']['restoration']=='restored' and time.monotonic()-began<1.8,expired
  summary['lease_expiry']=expired;assert bg.observe({'refs':[field['ref']]},'detail',['role']);human.act([step('keyboard.type_text',hf,type_text={'text':'expiry;'})]);summary['features'].append({'name':'expiry'})
  # The simulated user chooses a THIRD application during verification. The
  # helper must respect that choice rather than return to the original human app.
  tt,tl=launch('Third',True);third=h.Client('third',tt);tw=third.window(tt);tf=third.find(tw,'POC text')
  human.act([step('focus',hw),step('focus',hf)])
  result=[]
  def interrupted(): result.append(bg.act(wait_steps,identity='user-switch',allow_error=True))
  worker=threading.Thread(target=interrupted);worker.start();time.sleep(.3)
  run(['/usr/bin/open','-a',str(out/'Third.app')]);worker.join(5);assert not worker.is_alive()
  interrupted=result[0];assert interrupted.get('error') and interrupted['result']['input']['restoration']=='user_superseded',interrupted
  third.act([step('keyboard.type_text',tf,type_text={'text':'THIRD-KEPT'})]);time.sleep(.1)
  assert any(e['event']=='text' and e['value']=='THIRD-KEPT' for e in events(tl))
  summary['user_switch']=interrupted;human.act([step('focus',hw),step('focus',hf)])
 capture_windows=bg.observe({'refs':[bw['app']]},'capture_windows',['name','role','app'])
 capture_targets=[o for o in capture_windows if out.name in (h.known(o.get('name')) or '') and ('DTW Browser' in (h.known(o.get('name')) or '') or 'DTW Full Agent' in (h.known(o.get('name')) or ''))]
 assert len(capture_targets)==1,capture_targets
 image=bg.call('capture',{'kind':'window_content','target':capture_targets[0]['ref'],'max_pixel_width':640,'max_pixel_height':640})
 assert image['result']['files'],image
 summary['capture']=image
 expected=''.join(x['name']+';' for x in summary['features'])
 for _ in range(30):
  values=[e['value'] for e in events(hl) if e['event']=='text'];actual=values[-1] if values else ''
  if actual==expected: break
  time.sleep(.02)
 assert expected==actual,(expected,actual)
 summary['human_text']=actual;summary['browser_events']=web_events;summary['agent_events']=events(bl);summary['status']='passed'
except Exception as e: summary['error']=str(e);print('failed: '+str(e),flush=True)
finally:
 if server: server.shutdown();server.server_close()
 for c in h.clients: c.close()
 for pid in active:
  try: os.kill(pid,15)
  except ProcessLookupError: pass
 summary['cost']={c.label:{'calls':c.calls,'wire_bytes':c.bytes} for c in h.clients}
 summary['helper_sha256']=hashlib.sha256(Path(os.environ.get('DTW_ACCEPT_HELPER',str(ROOT/'bin/dtw'))).read_bytes()).hexdigest()
 summary['platform']=run(['sw_vers']).strip()
 if args.provider=='electron': summary['provider_version']=json.loads((ROOT/'bin/dtw-electron-poc/node_modules/electron/package.json').read_text())['version']
 elif args.provider=='chrome': summary['provider_version']=plistlib.loads(Path('/Applications/Google Chrome.app/Contents/Info.plist').read_bytes())['CFBundleShortVersionString']
 names=run(['git','ls-files','-c','-o','--exclude-standard','-z']).split('\0')
 manifest={name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in sorted(set(names)) if name and (ROOT/name).is_file() and (ROOT/name).suffix in ['.go','.m','.h','.swift','.py','.html','.sh','.mod','.sum']}
 (out/'source-sha256.json').write_text(json.dumps(manifest,indent=2)+'\n');summary['source_manifest']='source-sha256.json'
 (out/'summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n');print(out/'summary.json',flush=True)
if summary['status']!='passed': raise SystemExit(1)
