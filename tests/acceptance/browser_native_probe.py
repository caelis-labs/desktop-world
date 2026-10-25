#!/usr/bin/env python3
"""Opt-in real Chromium acceptance. Own localhost page/profile/files only.
Usage: DTW_CUA_WORKER=... python3 browser_native_probe.py DTW NEW_EVIDENCE_DIR
Never attaches a personal profile. All browser actions go through DTW's MCP.
"""
import base64
import http.server
import json
from pathlib import Path
import subprocess
import sys
import threading
import time
from probe import call, exec_call, structured_result

root = Path(sys.argv[2]).resolve()
root.mkdir(parents=True, exist_ok=False)
html = (Path(__file__).parents[1]/'fixtures/browser/index.html').read_bytes()
events = []
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        download = self.path == '/download'
        payload = b'DTW owned fixture download\n' if download else html
        self.send_response(200)
        self.send_header('Content-Type', 'application/octet-stream' if download else 'text/html; charset=utf-8')
        if download: self.send_header('Content-Disposition', 'attachment; filename="fixture.txt"')
        self.end_headers(); self.wfile.write(payload)
    def do_POST(self):
        value = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        events.append(value)
        with (root/'events.jsonl').open('a') as f: f.write(json.dumps(value, ensure_ascii=False)+'\n')
        self.send_response(204); self.end_headers()
    def log_message(self, *args): pass
server = http.server.ThreadingHTTPServer(('127.0.0.1',0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
proc = subprocess.Popen([sys.argv[1]], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=(root/'stderr.log').open('w'), text=True)
number = 0
binding = {}
report = {}
def run(op, arguments=None, **top):
    global number
    number += 1; ident = number*100
    args = {'operation':op,'wait_ms':1000,**top}
    if arguments is not None: args['arguments'] = arguments
    folder = root/f'{number:03d}-{op}'; folder.mkdir()
    (folder/'request.json').write_text(json.dumps(args,ensure_ascii=False,indent=2))
    value = exec_call(proc, ident, args)
    deadline = time.monotonic()+120
    while value.get('status') == 'running' and time.monotonic()<deadline:
        time.sleep(.1)
        value = exec_call(proc, ident+1, {'operation':'result','id':value['id']})
    (folder/'receipt.json').write_text(json.dumps(value,ensure_ascii=False,indent=2))
    assert value.get('status') != 'running', value
    native = structured_result(proc, ident+2, None, value['id']) if 'id' in value else value
    (folder/'native.json').write_text(json.dumps(native,ensure_ascii=False,indent=2))
    if op == 'get_browser_state' and arguments.get('include_screenshot') and not value.get('is_error'):
        image = call(proc, {'jsonrpc':'2.0','id':ident+3,'method':'tools/call','params':{
            'name':'exec','arguments':{'operation':'result','id':value['id'],'part':'image'}}})
        pixels = [c for c in image['result']['content'] if c['type']=='image']
        assert pixels, image
        (folder/'image.png').write_bytes(base64.b64decode(pixels[0]['data']))
    return value, native

def ok(op, arguments=None, **top):
    value, native = run(op, arguments, **top)
    assert not value.get('is_error'), (op, value, native)
    return native

def snapshot(image=False):
    return ok('get_browser_state', {**binding,'snapshot_format':'semantic_v2','include_screenshot':image})

def ref(snap, name):
    matches = [r for r in snap['refs'] if r.get('name')==name]
    assert len(matches)==1, (name, matches)
    return matches[0]['ref']

def event(kind, start, **expected):
    deadline = time.monotonic()+5
    while time.monotonic()<deadline:
        matches = [e for e in events[start:] if e['kind']==kind and all(e.get(k)==v for k,v in expected.items())]
        if matches: return matches
        time.sleep(.05)
    raise AssertionError((kind, expected, events[start:]))

try:
    call(proc, {'jsonrpc':'2.0','id':1,'method':'initialize','params':{
        'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'browser-native-fixture','version':'1'}}})
    prepared = ok('browser_prepare', {'allow_launch':True,'profile':{'mode':'isolated_new'}})
    pid = prepared['prepared_pid']; report['pid']=pid
    for _ in range(30):
        windows = ok('run', script=f'return await cua.listWindows({{pid:{pid}}});')['value']
        if windows: break
        time.sleep(.2)
    assert len(windows)==1, windows
    bound = ok('get_browser_state', {'pid':pid,'window_id':windows[0]['window_id']})
    assert bound['binding_quality']=='exact' and len(bound['tabs'])==1, bound
    binding = {'target_id':bound['target_id'],'tab_id':bound['tabs'][0]['tab_id']}
    ok('browser_navigate', {**binding,'url':f'http://127.0.0.1:{server.server_port}'})
    snap = snapshot(True); old_ref = ref(snap,'Apply')
    snap = snapshot()
    stale, _ = run('browser_click', {**binding,'ref':old_ref,'input_route':'dom_event'})
    assert stale.get('is_error'), stale
    assert not any(e['kind']=='apply' for e in events), events
    report['stale_ref'] = stale.get('code')
    start = len(events)
    ok('browser_type', {**binding,'ref':ref(snap,'Message'),'text':'DTW-中文-0380','replace':True})
    event('input', start, value='DTW-中文-0380')
    snap = snapshot(); start = len(events)
    ok('browser_click', {**binding,'ref':ref(snap,'Apply'),'input_route':'dom_event'})
    assert len(event('apply', start, value='DTW-中文-0380'))==1
    report['type_and_dom_click'] = 'exact independent input/apply events'
    snap = snapshot(); start = len(events)
    ok('browser_select', {**binding,'ref':ref(snap,'Color'),'value':'green'})
    event('select', start, value='green'); report['select']='green'
    for action,name,kind in [('hover','Hover target','hover'),('right_click','Right target','right-click'),
                             ('double_click','Double target','double-click'),('drag','Drag source','drop')]:
        snap = snapshot(); start = len(events)
        args = {**binding,'action':action,'ref':ref(snap,name),'input_route':'trusted','delivery_mode':'foreground'}
        if action=='drag': args['destination_ref']=ref(snap,'Drop target')
        receipt,native = run('browser_pointer', args)
        if receipt.get('is_error'):
            report[action] = {'code':receipt.get('code'),'text':receipt['text']}
        else:
            try: event(kind,start); report[action]='independent page event'
            except AssertionError: report[action]='posted but no page event (upstream effect gap)'
    # Register dialog events before the action that can open one. CUA 0.38's
    # first Page.enable can time out if a synchronous dialog is already open.
    ok('browser_dialog', {**binding,'action':'inspect'})
    snap = snapshot(); start = len(events)
    opened, _ = run('browser_click', {**binding,'ref':ref(snap,'Open prompt'),
                                    'input_route':'trusted','delivery_mode':'foreground'})
    report['dialog_trigger'] = {'status':opened.get('status'),'code':opened.get('code'),
                               'text':opened.get('text')}
    dialog = ok('browser_dialog', {**binding,'action':'inspect'})
    (root/'dialog-inspection.json').write_text(json.dumps(dialog,indent=2))
    # Inspect response is the authority for the live generation, not the click receipt.
    dialog_id = dialog.get('dialog_id') or dialog.get('dialog',{}).get('dialog_id')
    assert dialog_id, (opened,dialog)
    ok('browser_dialog', {**binding,'action':'accept','dialog_id':dialog_id,
                         'prompt_text':'DTW prompt answer','delivery_mode':'foreground'})
    event('prompt',start,value='DTW prompt answer'); report['dialog']='exact prompt accepted'
    upload = root/'upload.txt'; upload.write_text('DTW owned upload fixture\n')
    downloads=root/'downloads'; downloads.mkdir()
    for op,name,extra,kind in [('browser_set_input_files','Test file',{'files':[str(upload)]},'file'),
                             ('browser_download','Download fixture',{'destination_root':str(downloads)},None)]:
        snap=snapshot();start=len(events)
        receipt,native=run(op,{**binding,'ref':ref(snap,name),**extra})
        if receipt.get('is_error'):
            report[op]={'code':receipt.get('code'),'text':receipt['text']}
            if op=='browser_download': assert not list(downloads.iterdir()), 'Refused download wrote a file'
        elif kind: event(kind,start,names=['upload.txt']);report[op]='independent file change event'
        else:
            files=list(downloads.rglob('*'))
            assert any(p.is_file() and p.read_bytes()==b'DTW owned fixture download\n' for p in files), files
            report[op]='verified download bytes'
    snap=snapshot();start=len(events)
    receipt,_=run('browser_pointer',{**binding,'action':'scroll','ref':ref(snap,'Apply'),
        'delta_y':300,'input_route':'trusted','delivery_mode':'foreground'})
    if receipt.get('is_error'): report['scroll']={'code':receipt.get('code')}
    else: event('scroll',start);report['scroll']='independent page scroll event'
    ok('health_report', {'include':['binary_version','tcc_accessibility','tcc_screen_recording']})
    report['health']='native structured checks preserved'
    report['completed']=True
finally:
    (root/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2))
    proc.stdin.close()
    try: proc.wait(timeout=8)
    except subprocess.TimeoutExpired: proc.kill();proc.wait()
    server.shutdown()
    print(json.dumps(report,ensure_ascii=False,indent=2))
    assert proc.returncode==0, proc.returncode
