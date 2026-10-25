#!/usr/bin/env python3
"""Opt-in macOS real-browser authorization acceptance, owned fixture only.

Usage: DTW_CUA_WORKER=... python3 browser_authorization_probe.py DTW NEW_DIR
The test MCP host deliberately returns each explicit decision below. This tests
protocol + real CUA/Chrome effects, not a particular product's confirmation UI.
"""
import hashlib
import http.server
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time
from accept_chrome import exact_chrome_pid
from probe import call, exec_call, structured_result

root = Path(sys.argv[2]).resolve()
root.mkdir(parents=True, exist_ok=False)
downloads = root/'downloads'; downloads.mkdir()
payload = b'DTW explicit host approval fixture\n'
html = b'<!doctype html><title>DTW authorization fixture</title><h1>Owned authorization fixture</h1><a href="/download" download>Download fixture</a>'
requests = []
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        requests.append(self.path)
        body = payload if self.path == '/download' else html
        self.send_response(200)
        self.send_header('Content-Type', 'application/octet-stream' if self.path == '/download' else 'text/html')
        if self.path == '/download': self.send_header('Content-Disposition', 'attachment; filename="owned.txt"')
        self.end_headers(); self.wfile.write(body)
    def log_message(self, *args): pass
server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
url = f'http://127.0.0.1:{server.server_port}'
number = 0
processes = []
report = {}
pid = None

def host(label, flags=(), elicitation=True):
    p = subprocess.Popen([sys.argv[1], *flags], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=(root/f'{label}-stderr.log').open('w'), text=True)
    processes.append(p)
    call(p, {'jsonrpc':'2.0','id':1,'method':'initialize','params':{
        'protocolVersion':'2025-06-18', 'capabilities':{'elicitation':{}} if elicitation else {},
        'clientInfo':{'name':'owned-authorization-test-host','version':'1'}}})
    return p

def stop(p):
    if not p.stdin.closed: p.stdin.close()
    try: p.wait(timeout=8)
    except subprocess.TimeoutExpired: p.kill(); p.wait()

def run(p, op, arguments=None, **extra):
    global number
    number += 1
    args = {'operation':op,'wait_ms':1000, **extra}
    if arguments is not None: args['arguments'] = arguments
    folder = root/f'{number:03d}-{op}'; folder.mkdir()
    (folder/'request.json').write_text(json.dumps(args, indent=2))
    receipt = exec_call(p, number*100, args)
    deadline = time.monotonic()+100
    while receipt.get('status') == 'running' and time.monotonic() < deadline:
        time.sleep(.1)
        receipt = exec_call(p, number*100+1, {'operation':'result','id':receipt['id']})
    native = structured_result(p, number*100+2, None, receipt['id']) if 'id' in receipt else receipt
    (folder/'receipt.json').write_text(json.dumps(receipt, indent=2))
    (folder/'native.json').write_text(json.dumps(native, indent=2))
    return receipt, native

def ok(p, op, arguments=None, **extra):
    receipt, native = run(p, op, arguments, **extra)
    assert not receipt.get('is_error'), (op, receipt, native)
    return native

try:
    # Existing means Chrome was launched independently before this DTW runtime.
    # Only a disposable profile is used. --remote-debugging-port=0 gives CUA an
    # independently owned existing endpoint; no personal profile is addressed.
    profile = root/'existing-profile'
    subprocess.run(['open','-na','/Applications/Google Chrome.app','--args',
        f'--user-data-dir={profile}','--no-first-run','--no-default-browser-check',
        '--disable-background-mode',
        *([] if '--without-endpoint' in sys.argv[3:] else ['--remote-debugging-port=0']),url], check=True)
    for _ in range(100):
        try: pid = exact_chrome_pid(profile); break
        except (RuntimeError, subprocess.CalledProcessError): time.sleep(.1)
    assert pid
    scout = host('scout', elicitation=False)
    for _ in range(30):
        windows = ok(scout, 'run', script=f'return await cua.listWindows({{pid:{pid}}});')['value']
        if windows: break
        time.sleep(.2)
    assert len(windows) == 1, windows
    window = windows[0]['window_id']
    anchor = {'pid':pid,'window_id':window,'strategy':{'kind':'existing_profile'},'delivery_mode':'foreground'}
    refused, detail = run(scout, 'browser_prepare', anchor)
    assert refused.get('is_error') and 'consent' in json.dumps(detail).lower(), (refused,detail)
    report['standard_existing_profile'] = 'refused without native grant'
    denied, _ = run(scout, 'browser_download', {'target_id':'x','tab_id':'x','ref':'x','destination_root':str(downloads)})
    assert denied.get('is_error') and not list(downloads.iterdir()), denied
    report['no_elicitation'] = 'refused without download'
    stop(scout)

    manifest = root/'approved.yaml'
    native_tools = ['start_session','end_session','browser_prepare','get_browser_state','browser_navigate','browser_download','health_report']
    content = {'version':2,'mode':'bounded','expires_after':'20m','idle_timeout':'10m',
        'allow':{'tools':native_tools},'resources':{
            'browser':{'existing_profiles':[{'pid':pid,'window_id':window}], 'origins':[url]},
            'files':{'write':[{'dir':str(downloads),'recursive':False}]}}}
    manifest.write_text(json.dumps(content, indent=2))
    unapproved = subprocess.run([sys.argv[1], '--capability-manifest', str(manifest)],
        input='', capture_output=True, text=True, timeout=10)
    assert unapproved.returncode == 2 and '--approve-capability-manifest' in unapproved.stderr
    p = host('bounded', ['--capability-manifest', str(manifest), '--approve-capability-manifest'])
    # Original file changes cannot widen the already approved runtime snapshot.
    manifest.write_text('invalid: changed after launch\n')
    denied, _ = run(p, 'app', pid=pid)
    assert denied.get('is_error') and 'bounded_operation_unavailable' in denied['error'], denied
    wrong, _ = run(p, 'browser_prepare', {**anchor,'window_id':window+1})
    assert wrong.get('is_error'), wrong
    prepared = ok(p, 'browser_prepare', anchor)
    report['existing_profile'] = {'pid':pid,'window_id':window,'prepared':prepared}
    bound = ok(p, 'get_browser_state', {'pid':pid,'window_id':window})
    assert bound['binding_quality'] == 'exact' and len(bound['tabs']) == 1, bound
    binding = {'target_id':bound['target_id'],'tab_id':bound['tabs'][0]['tab_id']}
    denied, _ = run(p, 'browser_navigate', {**binding,'url':'https://example.invalid'})
    assert denied.get('is_error'), denied
    report['bounded_checks'] = 'unapproved startup / wrong window / outside origin / SCK bypass refused; original manifest replacement ignored'

    decisions = []
    expected_destination = downloads
    def confirm(request):
        prompt = request['params']
        assert json.dumps(str(expected_destination)) in prompt['message'], prompt
        assert prompt['requestedSchema']['properties']['approve']['default'] is False
        # While waiting, another action cannot mutate the confirmed binding.
        busy = exec_call(p, 999998, {'operation':'health_report','arguments':{'include':['binary_version']}})
        assert busy.get('is_error') and 'runtime_busy' in busy['error'], busy
        decision = decisions.pop(0)
        with (root/'confirmations.jsonl').open('a') as f:
            f.write(json.dumps({'request':request,'response':decision})+'\n')
        # An unrelated response must not satisfy this pending confirmation.
        p.stdin.write(json.dumps({'jsonrpc':'2.0','id':'unrelated','result':{'action':'accept','content':{'approve':True}}})+'\n'); p.stdin.flush()
        return decision
    p._dtw_elicitation = confirm
    for answer in [
        {'result':{'action':'decline'}}, {'result':{'action':'cancel'}},
        {'result':{'action':'accept','content':{'approve':False}}},
        {'error':{'code':-32601,'message':'no confirmation handler'}},
        {'result':{'action':'accept','content':{'approve':True}}}]:
        snap = ok(p, 'get_browser_state', {**binding,'snapshot_format':'semantic_v2'})
        refs = [r['ref'] for r in snap['refs'] if r.get('name') == 'Download fixture']
        assert len(refs) == 1, snap
        decisions.append(answer)
        before = requests.count('/download')
        receipt, native = run(p, 'browser_download', {**binding,'ref':refs[0],'destination_root':str(downloads)})
        accepted = answer.get('result',{}).get('content',{}).get('approve') is True
        if accepted:
            assert not receipt.get('is_error'), (receipt,native)
            files = list(downloads.iterdir())
            assert len(files) == 1 and files[0].read_bytes() == payload, files
            assert requests.count('/download') == before+1, requests
            report['download'] = {'native':native, 'sha256':hashlib.sha256(files[0].read_bytes()).hexdigest(), 'bytes':len(payload)}
        else:
            assert receipt.get('is_error') and not list(downloads.iterdir()), receipt
            assert requests.count('/download') == before, requests
    report['confirmation'] = 'decline / cancel / false / host error / unrelated id refused; explicit one-shot accept downloaded exact bytes'
    outside = root/'outside-manifest'; outside.mkdir()
    expected_destination = outside
    snap = ok(p, 'get_browser_state', {**binding,'snapshot_format':'semantic_v2'})
    download_ref = next(r['ref'] for r in snap['refs'] if r.get('name') == 'Download fixture')
    decisions.append({'result':{'action':'accept','content':{'approve':True}}})
    before = requests.count('/download')
    denied, _ = run(p, 'browser_download', {**binding,'ref':download_ref,'destination_root':str(outside)})
    assert denied.get('is_error') and not list(outside.iterdir()) and requests.count('/download') == before, denied
    report['approved_outside_manifest'] = 'native refusal; no HTTP download or file'
    expected_destination = downloads
    ok(p, 'get_browser_state', {**binding,'snapshot_format':'semantic_v2'})
    decisions.append({'result':{'action':'accept','content':{'approve':True}}})
    denied, _ = run(p, 'browser_download', {**binding,'ref':download_ref,'destination_root':str(downloads)})
    assert denied.get('is_error') and requests.count('/download') == before and len(list(downloads.iterdir())) == 1, denied
    report['approved_stale_ref'] = 'native refusal; no replay or second download'
    stop(p)
    assert exact_chrome_pid(profile) == pid, 'DTW must not terminate a pre-existing browser'
    report['existing_browser_survived_eof'] = True
    report['completed'] = True
finally:
    for p in processes: stop(p)
    if pid:
        try:
            if exact_chrome_pid(root/'existing-profile') == pid: os.kill(pid, signal.SIGTERM)
        except (RuntimeError, subprocess.CalledProcessError, ProcessLookupError): pass
    server.shutdown()
    (root/'report.json').write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
