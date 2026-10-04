#!/usr/bin/env python3
"""Save an owned document using real TextEdit or Electron (VS Code) via dtw."""
import argparse,hashlib,json,os,plistlib,subprocess,time,uuid
from pathlib import Path
import support as h
ROOT=h.ROOT
parser=argparse.ArgumentParser();parser.add_argument('--provider',choices=['textedit','vscode'],required=True);args=parser.parse_args()
out=ROOT/'artifacts'/('background-editor-'+args.provider+'-'+uuid.uuid4().hex[:8]);out.mkdir(parents=True);h.out=out;h.env=dict(os.environ,GOWORK='off')
owned=out/('dtw-'+uuid.uuid4().hex[:8]+'.txt');owned.write_text('original\n');app_process=None;active=[]
summary={'provider':args.provider,'status':'failed'}
def run(c): return subprocess.run(c,cwd=ROOT,env=h.env,capture_output=True,text=True,check=True).stdout
try:
 if not os.environ.get('DTW_ACCEPT_HELPER'):run(['go','build','-o','bin/dtw','./cmd/dtw'])
 if args.provider=='vscode':
  settings=out/'code-profile/User/settings.json';settings.parent.mkdir(parents=True)
  settings.write_text(json.dumps({'editor.accessibilitySupport':'on','window.restoreWindows':'none','workbench.startupEditor':'none','update.mode':'none','telemetry.telemetryLevel':'off'}))
  app_process=subprocess.Popen(['/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code','--wait','--new-window','--user-data-dir',str(out/'code-profile'),'--extensions-dir',str(out/'extensions'),'--disable-extensions','--skip-welcome','--skip-release-notes',str(owned)],stdout=(out/'app.stdout').open('w'),stderr=(out/'app.stderr').open('w'));active.append(app_process.pid)
 else: run(['/usr/bin/open','-n','-a','TextEdit',str(owned)])
 time.sleep(3)
 discovery=h.Client('discovery',None)
 for _ in range(30):
  objects=discovery.observe({'desktop':True},'summary',['name','role','app']);matches=[o for o in objects if o['kind']=='window' and owned.name in (h.known(o.get('name')) or '')]
  if len(matches)==1: break
  time.sleep(.5)
 if len(matches)!=1:
  apps={o['ref']:h.known(o.get('name')) for o in objects if o['kind']=='application'}
  warnings=[o for o in objects if o['kind']=='window' and h.known(o.get('name'))=='警告' ]
  if warnings:
   diagnostic=discovery.observe({'refs':[warnings[0]['ref']]},'outline',['name','role'])
   summary['launch_warning']=diagnostic;print([(o['role'],h.known(o.get('name'))) for o in diagnostic],flush=True)
  raise AssertionError(matches)
 title=h.known(matches[0]['name']);client=h.Client('editor',title,'cooperative');w=client.window(title)
 # A separate controlled foreground app represents the user's work.
 bundle=ROOT/'bin/DTWFullFixture.app';humanlog=out/'human.jsonl';human_title='DTW Editor Human '+out.name
 run(['/usr/bin/open','-n',str(bundle),'--args','--title',human_title,'--log',str(humanlog),'--human','1'])
 for _ in range(100):
  if humanlog.exists(): active.append(json.loads(humanlog.read_text().splitlines()[0])['pid']);break
  time.sleep(.05)
 human=h.Client('human',human_title);hw=human.window(human_title);hf=human.find(hw,'POC text');human.act([h.step('focus',hw),h.step('focus',hf)])
 text='DTW real '+args.provider+' 中文🙂\nSaved via scoped keyboard.\n'
 steps=[h.step('keyboard.press',w,press={'key':'A','modifiers':['primary']}),h.step('keyboard.type_text',w,type_text={'text':text}),h.step('keyboard.press',w,press={'key':'S','modifiers':['primary']})]
 reply=client.act(steps,identity='real-editor-save',allow_error=True);summary['receipt']=reply;assert not reply.get('error'),reply
 for _ in range(100):
  actual=owned.read_text()
  if actual==text: break
  time.sleep(.05)
 assert actual==text,(actual,text)
 again=client.act(steps,identity='real-editor-save');assert again['result']['run_id']==reply['result']['run_id']
 human.act([h.step('keyboard.type_text',hf,type_text={'text':'USER-RESUMED'})]);time.sleep(.1)
 assert any(json.loads(x).get('value')=='USER-RESUMED' for x in humanlog.read_text().splitlines())
 summary.update(status='passed',expected=text,actual=actual,sha256=hashlib.sha256(owned.read_bytes()).hexdigest(),title=title)
 if args.provider=='textedit':
  # Close only our exact owned document after saving; never quit user TextEdit.
  client.act([h.step('keyboard.press',w,press={'key':'W','modifiers':['primary']})])
except Exception as e: summary['error']=str(e);print(str(e),flush=True)
finally:
 if args.provider=='vscode':
  rows=run(['/bin/ps','-axo','pid=,command=']).splitlines()
  active.extend(int(row.strip().split(None,1)[0]) for row in rows if str(out/'code-profile') in row)
 for c in h.clients:c.close()
 for pid in active:
  try:os.kill(pid,15)
  except ProcessLookupError:pass
 summary['helper_sha256']=hashlib.sha256(Path(os.environ.get('DTW_ACCEPT_HELPER',str(ROOT/'bin/dtw'))).read_bytes()).hexdigest()
 summary['cost']={c.label:{'calls':c.calls,'wire_bytes':c.bytes} for c in h.clients};(out/'summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n');print(out/'summary.json',flush=True)
if summary['status']!='passed':raise SystemExit(1)
