#!/usr/bin/env python3
"""Live macOS focus recovery and AX readiness against a task-owned fixture only."""
import base64
import json
from pathlib import Path
import subprocess
import sys
import time
from probe import call, exec_call, result, structured_result
binary, fixture_path, output = sys.argv[1:]
fixture, out = Path(fixture_path), Path(output)
out.mkdir(parents=True, exist_ok=False)
deadline=time.monotonic()+10
while not (fixture/'state.json').exists() and time.monotonic()<deadline: time.sleep(.05)
def state(): return json.loads((fixture/'state.json').read_text())
pid=state()['pid']
proc=subprocess.Popen([binary],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(out/'stderr.log').open('w'),text=True)
ident=10

def run(args, wait=True):
    global ident
    ident+=10
    r=exec_call(proc,ident,args)
    if wait and r.get('status')=='running': r=result(proc,ident+1,None,r['id'])
    (out/f'{ident}-receipt.json').write_text(json.dumps({'request':args,'receipt':r},indent=2))
    return r

def command(text):
    (fixture/'command').write_text(text)
    deadline=time.monotonic()+3
    while state().get('command')!=text and time.monotonic()<deadline: time.sleep(.02)
    assert state().get('command')==text, state()

def native(r):
    v=structured_result(proc,ident+2,None,r['id'])
    (out/f'{ident}-native.json').write_text(json.dumps(v,indent=2))
    return v

try:
    call(proc,{'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'recovery-live','version':'1'}}})
    windows=run({'operation':'windows','arguments':{'pid':pid}})['data']['items']
    base=next(w['window_id'] for w in windows if w['title']=='DTW base')
    count=state()['clicks']
    command('hold_not_ready')
    screenshot=run({'operation':'app','pid':pid,'window_id':base})
    reply=call(proc,{'jsonrpc':'2.0','id':ident+3,'method':'tools/call','params':{'name':'exec','arguments':{'operation':'result','id':screenshot['id'],'part':'image'}}})
    png=next(c for c in reply['result']['content'] if c['type']=='image')
    (out/'visible-before-ax-ready.png').write_bytes(base64.b64decode(png['data']))
    args={'pid':pid,'window_id':base,'ready_when':'Ready target'}
    once=run({'operation':'observe','arguments':{**args,'ready_timeout_ms':0}})
    assert once['data']['dtw_readiness']['status']=='not_observed',once
    assert once['data']['dtw_readiness']['attempts']==1,once
    assert not state()['ready_ax']
    command('delay_ready')
    loaded=run({'operation':'observe','arguments':{**args,'ready_timeout_ms':4000}})
    readiness=loaded['data']['dtw_readiness']
    assert readiness['status']=='matched' and readiness['attempts']>1,loaded
    assert state()['ready_ax']
    full=native(loaded)
    assert 'Ready target' in full['tree_markdown'] and full['snapshot_id']==loaded['data']['snapshot_id']
    missing=run({'operation':'observe','arguments':{**args,'ready_when':'Never present','ready_timeout_ms':700}})
    assert missing['data']['dtw_readiness']['status']=='not_observed',missing
    assert missing['data']['dtw_readiness']['elapsed_ms']<5000,missing
    # A window title must not satisfy the content condition.
    title=run({'operation':'observe','arguments':{**args,'ready_when':'DTW base','ready_timeout_ms':0}})
    assert title['data']['dtw_readiness']['status']=='not_observed',title
    command('hide_base')
    focused=run({'operation':'focus','pid':pid,'window_id':base})
    recovery=focused['data']['dtw_recovery']
    assert recovery['verification']=='unverified',focused
    assert recovery['next_call']=={'operation':'windows','arguments':{'pid':pid}},focused
    assert 'dtw_recovery' in native(focused)
    candidates=run(recovery['next_call'])
    assert candidates['status']=='completed',candidates
    command('show_base')
    # No alternate target on a missing exact window, and no polling after error.
    gone=run({'operation':'observe','arguments':{**args,'window_id':4294967295,'ready_timeout_ms':4000}})
    assert gone['is_error'] and gone['data']['dtw_readiness']['status']=='blocked',gone
    assert gone['data']['dtw_readiness']['attempts']==1,gone
    pending=run({'operation':'observe','arguments':{**args,'ready_when':'Never present','ready_timeout_ms':5000},'wait_ms':0},wait=False)
    assert pending['status']=='running',pending
    time.sleep(.3)
    cancelled=run({'operation':'cancel','id':pending['id']},wait=False)
    final=run({'operation':'result','id':pending['id']})
    assert final['status'] in ['unknown','failed'],final
    assert state()['clicks']==count,state()
    report={'pid':pid,'base_window':base,'visible_before_ax':'fixture screenshot retained',
            'single_read':'not_observed, one attempt','delayed_ax':readiness,
            'missing_content':'bounded not_observed','window_title':'not a content match',
            'hidden_focus':'native unverified evidence and executable window enumeration',
            'missing_exact_window':'blocked on first native error','cancellation':final['status'],
            'input_count_change':state()['clicks']-count}
    (out/'summary.json').write_text(json.dumps(report,indent=2))
    print(json.dumps(report))
finally:
    command('show_base')
    command('reset_ready')
    command('restore')
    proc.stdin.close()
    proc.wait(timeout=10)
    assert proc.returncode==0,proc.returncode
