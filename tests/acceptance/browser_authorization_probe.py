#!/usr/bin/env python3
"""Opt-in macOS real-browser authorization acceptance, owned fixture only.

Usage: DTW_CUA_WORKER=... python3 browser_authorization_probe.py DTW NEW_DIR
The model-facing path uses only inline receipts, with no scout MCP Runtime
or hidden structured-result fetches. --caelis also runs the real local Harness.
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
from probe import call, exec_call

root = Path(sys.argv[2]).resolve()
root.mkdir(parents=True, exist_ok=False)
downloads = root/'downloads'; downloads.mkdir()
payload = b'DTW Harness-owned approval fixture\n'
html = b'<!doctype html><title>DTW authorization fixture</title><h1>Owned authorization fixture</h1><label>Message<input id="message"></label><button onclick="document.querySelector(\'output\').textContent=document.querySelector(\'input\').value;fetch(\'/applied?text=\'+encodeURIComponent(document.querySelector(\'input\').value))">Apply</button><output>Not applied</output><a href="/download" download>Download fixture</a>'
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

def host(label, flags=()):
    p = subprocess.Popen([sys.argv[1], *flags], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=(root/f'{label}-stderr.log').open('w'), text=True, encoding='utf-8')
    processes.append(p)
    call(p, {'jsonrpc':'2.0','id':1,'method':'initialize','params':{
        'protocolVersion':'2025-06-18', 'capabilities':{},
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
    native = receipt.get('data', receipt)
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
    # Host configuration identifies its own fixture window via macOS. These
    # IDs are never supplied to the Agent prompt or discovered by a scout DTW.
    swift = root/'owned-window.swift'
    swift.write_text('import CoreGraphics\nimport Foundation\nlet p = Int(CommandLine.arguments[1])!\nlet windows = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as! [[String:Any]]\nlet ids = windows.filter { ($0[kCGWindowOwnerPID as String] as? Int) == p && ($0[kCGWindowLayer as String] as? Int) == 0 }.compactMap { $0[kCGWindowNumber as String] as? Int }\nprint(String(data:try! JSONSerialization.data(withJSONObject:ids),encoding:.utf8)!)\n')
    windows = []
    for _ in range(30):
        windows = json.loads(subprocess.check_output(['swift',str(swift),str(pid)],text=True))
        if windows: break
        time.sleep(.2)
    assert len(windows) == 1, windows
    window = windows[0]
    manifest = root/'approved.yaml'
    native_tools = ['start_session','end_session','browser_prepare','get_browser_state','browser_navigate','browser_download','browser_type','browser_click','health_report']
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
    wrong, _ = run(p, 'browser_prepare', {'pid':pid,'window_id':window+1,'strategy':{'kind':'existing_profile'}})
    assert wrong.get('is_error'), wrong
    discovery = ok(p, 'describe')
    assert 'run' not in discovery['operations'] and len(discovery['targets']) == 1, discovery
    next_call = discovery['targets'][0]['next_call']
    receipt, bound = run(p, next_call['operation'], next_call['arguments'])
    if receipt.get('is_error'):
        next_call = bound.get('next_call', bound.get('detail',{}).get('next_call'))
        assert next_call and next_call['operation'] == 'browser_prepare', bound
        assert 'session' not in next_call['arguments'], next_call
        ok(p, next_call['operation'], next_call['arguments'])
        bound = ok(p, 'get_browser_state', discovery['targets'][0]['next_call']['arguments'])
    assert bound['binding_quality'] == 'exact' and len(bound['tabs']) == 1, bound
    binding = {'target_id':bound['target_id'],'tab_id':bound['tabs'][0]['tab_id']}
    denied, _ = run(p, 'browser_navigate', {**binding,'url':'https://example.invalid'})
    assert denied.get('is_error'), denied
    report['bounded_checks'] = 'unapproved startup / wrong window / outside origin / SCK bypass refused; manifest replacement ignored'
    snap = ok(p, 'get_browser_state', {**binding,'snapshot_format':'semantic_v2'})
    download_ref = next(r['ref'] for r in snap['refs'] if r.get('name') == 'Download fixture')
    before = requests.count('/download')
    native = ok(p, 'browser_download', {**binding,'ref':download_ref,'destination_root':str(downloads)+'/' + '.'})
    files = list(downloads.iterdir())
    assert len(files) == 1 and files[0].read_bytes() == payload, files
    assert requests.count('/download') == before+1, requests
    report['download'] = {'native':native, 'sha256':hashlib.sha256(files[0].read_bytes()).hexdigest(), 'bytes':len(payload)}
    report['workflow'] = 'describe -> configured target -> native recovery -> binding/tabs -> snapshot/refs -> download, all inline; no elicitation'
    outside = root/'outside-manifest'; outside.mkdir()
    snap = ok(p, 'get_browser_state', {**binding,'snapshot_format':'semantic_v2'})
    download_ref = next(r['ref'] for r in snap['refs'] if r.get('name') == 'Download fixture')
    before = requests.count('/download')
    denied, _ = run(p, 'browser_download', {**binding,'ref':download_ref,'destination_root':str(outside)})
    assert denied.get('is_error') and not list(outside.iterdir()) and requests.count('/download') == before, denied
    report['approved_outside_manifest'] = 'native refusal; no HTTP download or file'
    ok(p, 'get_browser_state', {**binding,'snapshot_format':'semantic_v2'})
    denied, _ = run(p, 'browser_download', {**binding,'ref':download_ref,'destination_root':str(downloads)})
    assert denied.get('is_error') and requests.count('/download') == before and len(list(downloads.iterdir())) == 1, denied
    report['approved_stale_ref'] = 'native refusal; no replay or second download'
    stop(p)
    assert exact_chrome_pid(profile) == pid, 'DTW must not terminate a pre-existing browser'
    report['existing_browser_survived_eof'] = True
    if '--caelis' in sys.argv[3:]:
        from caelis_harness_probe import run_caelis
        manifest.write_text(json.dumps(content, indent=2))
        report['caelis'] = run_caelis(root, Path(sys.argv[1]).resolve().parent.parent, manifest, downloads)
        assert any(path.startswith('/applied?text=DTW-Harness-verified') for path in requests), requests
        assert requests.count('/download') == 2, requests
        assert len(list(downloads.iterdir())) == 2 and all(f.read_bytes() == payload for f in downloads.iterdir())
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
