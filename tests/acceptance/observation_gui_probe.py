#!/usr/bin/env python3
"""Live macOS #49 acceptance; input is restricted to the task-owned scene fixture.
Usage: DTW_CUA_WORKER=... python3 observation_gui_probe.py DTW FIXTURE_DIR NEW_EVIDENCE_DIR
"""
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
deadline = time.monotonic() + 10
while not (fixture/'state.json').exists() and time.monotonic() < deadline:
    time.sleep(.05)
pid = json.loads((fixture/'state.json').read_text())['pid']
proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=(out/'stderr.log').open('w'), text=True)
ident = 10


def run(args, success=True):
    global ident
    ident += 10
    receipt = exec_call(proc, ident, args)
    if receipt.get('status') == 'running':
        receipt = result(proc, ident+1, None, receipt['id'])
    (out/f'{ident}-receipt.json').write_text(json.dumps({'request':args,'receipt':receipt}, indent=2))
    if success:
        assert receipt.get('status') == 'completed' and not receipt['is_error'], receipt
    return receipt


def raw(receipt):
    value = structured_result(proc, ident+2, None, receipt['id'])
    (out/f'{ident}-native.json').write_text(json.dumps(value, indent=2))
    return value


def image(receipt, name):
    response = call(proc, {'jsonrpc':'2.0','id':ident+3,'method':'tools/call','params':{
        'name':'exec','arguments':{'operation':'result','id':receipt['id'],'part':'image','image_index':0}}})
    png = next(c for c in response['result']['content'] if c['type']=='image')
    (out/name).write_bytes(base64.b64decode(png['data']))


def command(value):
    (fixture/'command').write_text(value)
    time.sleep(.5)


try:
    call(proc, {'jsonrpc':'2.0','id':1,'method':'initialize','params':{
        'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'observation-live','version':'1'}}})
    apps = run({'operation':'apps','arguments':{'limit':2}})
    assert apps['data']['returned'] <= 2
    assert all(a['running'] for a in apps['data']['items'])
    inventory = raw(apps)
    assert inventory['page']['total'] == sum(a.get('running') is True for a in inventory['native']['apps'])
    if 'next_call' in apps['data']:
        page2 = run(apps['data']['next_call'])
        assert page2['data']['offset'] == apps['data']['returned']
    all_apps = run({'operation':'apps','arguments':{'running':False,'limit':100}})
    assert len(all_apps['text'].encode()) < 12288, all_apps
    filtered = run({'operation':'apps','arguments':{'query':'DTWScene','running':True}})
    assert any(a['pid']==pid for a in filtered['data']['items']), filtered
    windows = run({'operation':'windows','arguments':{'pid':pid}})['data']['items']
    assert len(windows) == 3, windows
    base = next(w for w in windows if w['title']=='DTW base')
    top = next(w for w in windows if w['title']=='DTW top')
    assert base['layer_hint']=='ordinary_layer' and top['layer_hint']=='elevated_layer', windows
    wid = base['window_id']
    default = run({'operation':'app','pid':pid,'max_windows':1})
    default_view = raw(default)
    assert default_view['frames'][0]['window_id'] == top['window_id'], default_view
    assert default['data']['selection'] == 'front_to_back'
    exact = run({'operation':'app','pid':pid,'window_id':wid,'max_windows':8})
    view = raw(exact)
    assert view['selection']=='exact_window_id' and view['requested_window_id']==wid
    assert len(view['frames'])==1 and view['frames'][0]['window_id']==wid
    assert len(view['windows'])==3 and view['omitted_count']==2
    image(exact, 'exact-base-before.png')
    frame = view['frames'][0]
    before = json.loads((fixture/'state.json').read_text())['clicks']
    clicked = run({'operation':'click','observation_id':view['observation_id'],'image_index':0,
                   'x':frame['width']*.5,'y':frame['height']*.55})
    time.sleep(.3)
    state = json.loads((fixture/'state.json').read_text())
    assert state['clicks']==before+1 and state['events'][-1]['window']==wid, state
    after = run({'operation':'app','pid':pid,'window_id':wid})
    image(after, 'exact-base-after.png')
    observation = run({'operation':'observe','arguments':{'pid':pid,'window_id':wid}})
    native = raw(observation)
    summary = observation['data']
    assert summary['window_id']==wid and 'tree_markdown' in summary, summary
    assert 'text' not in summary and 'elements' not in summary
    assert native.get('elements') and 'Count click' in summary['tree_markdown'], native
    for key in ['snapshot_id','degraded_reason','truncated','truncation_reason']:
        if key in native: assert summary[key]==native[key], (key,summary,native)
    command('duplicate_titles')
    duplicate = run({'operation':'windows','arguments':{'pid':pid,'title':'duplicate'}})
    assert duplicate['data']['total']==2 and len({w['window_id'] for w in duplicate['data']['items']})==2
    command('close')
    hidden = run({'operation':'app','pid':pid,'window_id':top['window_id']}, success=False)
    assert hidden['is_error'] and hidden['code']=='window_not_attributed', hidden
    missing = run({'operation':'app','pid':pid,'window_id':4294967295}, success=False)
    assert missing['is_error'] and missing['code']=='window_not_attributed', missing
    # A caller's arbitrary fields must not be projected as an observation.
    custom = run({'script':'return {text:"keep-text",tree_markdown:"keep-tree",elements:["keep-elements"]};'})
    assert custom['value']=={'text':'keep-text','tree_markdown':'keep-tree','elements':['keep-elements']},custom
    command('restore')
    exact = run({'operation':'app','pid':pid,'window_id':wid})
    exact_view = raw(exact)
    before = json.loads((fixture/'state.json').read_text())['clicks']
    command('move')
    stale = run({'operation':'click','observation_id':exact_view['observation_id'],'image_index':0,
                 'x':exact_view['frames'][0]['width']*.5,'y':exact_view['frames'][0]['height']*.55}, success=False)
    assert stale['code']=='stale_observation', stale
    assert json.loads((fixture/'state.json').read_text())['clicks']==before
    report = {'pid':pid,'target_window':wid,'apps_pagination':'passed','installed_apps_summary_bytes':len(all_apps['text'].encode()),
              'window_candidates':3,'duplicate_title_candidates':2,'default_capture':'top floating window',
              'exact_capture':'base window only','click_effect':'fixture counter +1 and event window ID matches target',
              'hidden_and_missing':'refused without fallback','observe':'single tree, native elements retained',
              'arbitrary_script_values':'unchanged','exact_capture_other_window_moved':'stale, no input'}
    (out/'summary.json').write_text(json.dumps(report,indent=2))
    print(json.dumps(report))
finally:
    command('restore')
    proc.stdin.close()
    proc.wait(timeout=10)
    assert proc.returncode==0,proc.returncode
