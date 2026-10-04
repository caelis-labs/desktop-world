#!/usr/bin/env python3
"""Opt-in same-desktop POC. All target operations go through persistent dtw.
Artifacts contain controlled fixture text only. No OS permissions are prompted.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import platform
import selectors
import shutil
import subprocess
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--mode', choices=['public_pid', 'skylight', 'no_raise', 'cooperative'], default='public_pid')
parser.add_argument('--rounds', type=int, default=3)
parser.add_argument('--case', choices=['appkit', 'webkit'], default='appkit')
args = parser.parse_args()
if platform.system() != 'Darwin' or not 1 <= args.rounds <= 10:
    parser.error('requires an interactive macOS desktop; rounds 1..10')
env = dict(os.environ, GOWORK='off')
run_id = time.strftime('%Y%m%dT%H%M%SZ', time.gmtime()) + '-' + uuid.uuid4().hex[:6]
out = ROOT / 'artifacts' / ('background-poc-' + run_id)
out.mkdir(parents=True)
def run(cmd):
    return subprocess.run(cmd, cwd=ROOT, env=env, check=True, capture_output=True, text=True).stdout
run(['go', 'build', '-tags', 'dtw_background_poc', '-o', 'bin/dtw-background-poc', './cmd/dtw'])
run(['go', 'build', '-o', 'bin/dtw', './cmd/dtw'])
run(['./script/build_and_run.sh', '--build-only'])
active = []
clients = []
def events(path):
    return [json.loads(s) for s in path.read_text().splitlines()] if path.exists() else []
def launch(label, background):
    bundle = ROOT / 'bin' / ('DTWPOC' + label + '.app')
    if bundle.exists(): shutil.rmtree(bundle)
    shutil.copytree(ROOT / 'bin' / 'DWNativeFixture.app', bundle)
    path = bundle / 'Contents' / 'Info.plist'
    info = plistlib.loads(path.read_bytes())
    info['CFBundleIdentifier'] = 'dev.caelis.desktop-world.poc.' + label.lower()
    info['CFBundleName'] = 'DTWPOC' + label
    path.write_bytes(plistlib.dumps(info))
    run(['codesign', '--force', '--sign', '-', str(bundle)])
    title = 'DTW POC ' + label + ' ' + run_id
    log = out / (label.lower() + '.jsonl')
    run(['/usr/bin/open', '-n'] + (['-g'] if background else []) + [str(bundle), '--args', '--title', title, '--log', str(log), '--background', '1' if background else '0', '--poc-observe-seat', '0' if background else '1', '--poc-log-input', '1', '--semantic-case', 'input-poc' if background and args.case == 'webkit' else ''])
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        rows = events(log)
        if rows and any(r['event'] == 'ready' for r in rows):
            active.append(rows[0]['pid'])
            return title, log
        time.sleep(.05)
    raise RuntimeError('fixture startup timeout')

class Client:
    def __init__(self, label, title, mode=None, policy='shared_input'):
        self.label, self.seq, self.calls, self.bytes = label, 0, 0, 0
        self.trace = (out / (label + '-wire.jsonl')).open('w')
        self.stderr = (out / (label + '-native.jsonl')).open('w')
        binary = 'dtw-background-poc' if mode and mode != 'cooperative' else 'dtw'
        executable = os.environ.get('DTW_ACCEPT_HELPER') if binary == 'dtw' else None
        command = [executable or str(ROOT / 'bin' / binary), 'serve', '--write-app-window', title, '--input-policy', policy]
        if mode == 'cooperative': command += ['--input-mode', mode]
        elif mode: command += ['--experimental-background-input', mode]
        self.proc = subprocess.Popen(command, cwd=ROOT, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr, text=True)
        clients.append(self)
        self.hello = self.recv()
    def recv(self):
        sel = selectors.DefaultSelector()
        sel.register(self.proc.stdout, selectors.EVENT_READ)
        try:
            if not sel.select(15): raise RuntimeError('dtw timeout')
            line = self.proc.stdout.readline()
        finally: sel.close()
        if not line: raise RuntimeError('dtw exited: ' + str(self.proc.poll()))
        self.trace.write(line); self.trace.flush()
        self.bytes += len(line.encode())
        return json.loads(line)
    def call(self, op, body, *, identity=None, allow_error=False):
        self.seq += 1; self.calls += 1
        request = {'id': identity or self.label + '-' + str(self.seq), 'op': op, 'args': body}
        self.trace.write(json.dumps({'request': request}, ensure_ascii=False) + '\n'); self.trace.flush()
        self.proc.stdin.write(json.dumps(request, ensure_ascii=False) + '\n'); self.proc.stdin.flush()
        reply = self.recv()
        if reply.get('error') and not allow_error: raise RuntimeError(json.dumps(reply, ensure_ascii=False))
        return reply
    def observe(self, scope, projection, fields, match=None):
        body = {'scope': scope, 'projection': projection, 'fields': fields, 'budget': {'max_results': 32 if projection == 'summary' else 4, 'max_output_bytes': 8192 if projection == 'summary' else 4096, 'max_visited_nodes': 512 if projection == 'summary' else 128, 'max_depth': 12, 'read_deadline_ms': 3000}}
        if match: body['match'] = match
        objects = []
        for _ in range(8):
            result = self.call('observe', body)['result']
            objects += result.get('objects', [])
            continuation = result.get('coverage', {}).get('continuation')
            if not continuation: return objects
            body['continuation'] = continuation
        raise RuntimeError('bounded discovery incomplete')
    def window(self, title):
        matches = [o for o in self.observe({'desktop': True}, 'summary', ['name', 'role', 'app']) if o.get('kind') == 'window' and known(o.get('name')) == title]
        assert len(matches) == 1, matches
        return matches[0]
    def find(self, window, name):
        for _ in range(8):
            objects = self.observe({'refs': [window['ref']]}, 'outline', ['name', 'role'], {'within': window['ref'], 'name_equals': name})
            if len(objects) == 1: return objects[0]
            assert not objects, (name, objects)
            time.sleep(.1)  # WebKit publishes AX children asynchronously.
        raise RuntimeError('control unavailable: ' + name)
    def act(self, steps, **options):
        return self.call('act', {'steps': [dict(s, id='step-' + str(i)) for i, s in enumerate(steps)]}, **options)
    def close(self):
        self.proc.stdin.close()
        try: self.proc.wait(timeout=3)
        except subprocess.TimeoutExpired: self.proc.terminate(); self.proc.wait(timeout=3)
        self.trace.close(); self.stderr.close()
def known(fact):
    return fact.get('known') if fact else None
def step(op, target, **arm):
    return dict(id=op, op=op, target={'ref': target['ref']}, **arm)
def click(target): return step('pointer.click', target, click={'button': 'left', 'count': 1})
summary = {'run': run_id, 'platform': platform.platform(), 'mode': args.mode, 'case': args.case, 'base_commit': run(['git', 'rev-parse', 'HEAD']).strip(), 'rounds': [], 'status': 'failed'}
human_thread = None
bg_log = fg_log = None
try:
    bg_title, bg_log = launch('Background', True)
    fg_title, fg_log = launch('Human', False)
    bg = Client('background', bg_title, args.mode)
    fg = Client('human', fg_title)
    bw, fw = bg.window(bg_title), fg.window(fg_title)
    editor = bg.find(bw, 'POC Web text' if args.case == 'webkit' else '内容')
    submit = bg.find(bw, 'POC Web submit' if args.case == 'webkit' else '提交')
    human = fg.find(fw, '内容')
    fg.act([step('focus', fw), step('focus', human)])
    # Prove the target point is covered by the user's window. The normal helper
    # must reject exactly the same target instead of treating it as hittable.
    normal = Client('normal-control', bg_title)
    normal_window = normal.window(bg_title)
    normal_editor = normal.find(normal_window, 'POC Web text' if args.case == 'webkit' else '内容')
    rejection = normal.act([click(normal_editor)], allow_error=True)
    assert rejection.get('error', {}).get('code') == 'target_not_hittable', rejection
    assert rejection['result']['steps'][0]['delivery'] == 'none', rejection
    summary['occlusion_control'] = rejection
    # Host policy cannot be relaxed by the experimental transport.
    strict = Client('strict-control', bg_title, args.mode, 'no_shared_input')
    strict_window = strict.window(bg_title)
    strict_editor = strict.find(strict_window, 'POC Web text' if args.case == 'webkit' else '内容')
    refused = strict.act([step('set_value', strict_editor, set_value={'text': 'MUST-NOT-APPEAR'}), click(strict_editor)], allow_error=True)
    assert refused.get('error', {}).get('code') == 'requires_shared_input', refused
    assert all(s['delivery'] == 'none' for s in refused['result']['steps']), refused
    summary['strict_policy_control'] = refused
    before = fg.call('observe', {'scope': {'refs': [fw['ref']]}, 'projection': 'detail', 'fields': ['role'], 'budget': {'max_results': 1, 'max_output_bytes': 4096}})['result']['seat']
    summary['before_seat'] = before
    human_text = 'User-writing-a-document-while-Agent-saves-order-notes-' + ('abcde' * args.rounds)
    human_errors = []
    human_started = threading.Event()
    def human_input():
        for index, char in enumerate(human_text):
            try:
                reply = fg.act([step('keyboard.type_text', human, type_text={'text': char})], identity='human-key-' + str(index), allow_error=True)
                if reply.get('error'):
                    if args.mode == 'cooperative' and reply.get('result', {}).get('steps', [{}])[0].get('delivery') == 'none' and reply['error']['code'] in ('needs_user_focus', 'user_interrupted'):
                        # A declared short borrow: only a proven no-delivery input
                        # may be retried as a new user action after restoration.
                        for attempt in range(100):
                            time.sleep(.02)
                            reply = fg.act([step('keyboard.type_text', human, type_text={'text': char})], identity='human-key-' + str(index) + '-wait-' + str(attempt), allow_error=True)
                            if not reply.get('error'): break
                            assert reply['result']['steps'][0]['delivery'] == 'none', reply
                    if reply.get('error'): human_errors.append(reply)
            except Exception as error: human_errors.append(str(error)); break
            human_started.set()
            time.sleep(.10)
    human_thread = threading.Thread(target=human_input)
    human_thread.start(); assert human_started.wait(5)
    task_started = time.time()
    print('ready; testing ' + args.mode, flush=True)
    # First dispatch is independently verified before any text or submission.
    receipt = bg.act([click(editor)], identity='background-first-click')
    time.sleep(.2)
    detail = bg.observe({'refs': [editor['ref']]}, 'detail', ['states', 'bounds'])
    summary['first_click'] = receipt
    summary['first_click_detail'] = detail
    print('target focused after click: ' + str(known(detail[0]['states']['focused'])), flush=True)
    for i in range(args.rounds):
        text = 'POC-' + str(i) + '-中文-🙂'
        # Semantic reset is disclosed separately; text delivery itself is native.
        if args.mode != 'cooperative': bg.act([step('set_value', editor, set_value={'text': ''})])
        text_step = step('keyboard.type_text', editor, type_text={'text': text}, completion='verify', after=[{'target': {'ref': editor['ref']}, 'property': 'value', 'equals_string': text}])
        submit_step = click(submit)
        submit_step['before'] = [{'target': {'ref': editor['ref']}, 'property': 'value', 'equals_string': text}]
        action = [click(editor)] + ([step('keyboard.press', editor, press={'key':'A', 'modifiers':['primary']}), step('keyboard.press', editor, press={'key':'Backspace'})] if args.mode == 'cooperative' else []) + [text_step, submit_step]
        identity = 'background-task-' + str(i)
        r = bg.act(action, identity=identity, allow_error=True)
        summary['rounds'].append({'text': text, 'reply': r})
        print('round ' + str(i) + ': ' + r['result'].get('outcome', 'no receipt'), flush=True)
        if r.get('error'): break
        time.sleep(.2)
        assert any(e['event'] == ('web_submit' if args.case == 'webkit' else 'submit') and e['value'] == text for e in events(bg_log)), 'business submit missing'
        # Reconcile the original request after dispatch; it must not resubmit.
        again = bg.act(action, identity=identity)
        assert again['result']['run_id'] == r['result']['run_id'], 'receipt changed on reconciliation'
        # Simulate reasoning/network delay while the user keeps writing.
        time.sleep(.6)
    task_finished = time.time()
    human_thread.join(20)
    assert not human_thread.is_alive(), 'human simulation did not finish'
    summary['task_interval'] = [task_started, task_finished]
    summary['human_errors'] = human_errors
    summary['expected_human_text'] = human_text
    summary['background_events'] = events(bg_log)
    summary['human_events'] = events(fg_log)
    changes = [e['value'] for e in summary['human_events'] if e['event'] == 'text_changed']
    summary['actual_human_text'] = changes[-1] if changes else ''
    samples = [dict(json.loads(e['value']), time=e['time']) for e in summary['human_events'] if e['event'] == 'seat_sample' and task_started <= e['time'] <= task_finished]
    summary['seat_samples'] = len(samples)
    summary['foreground_or_key_loss_samples'] = sum(s['front_pid'] != active[1] or not s['active'] or not s['key'] for s in samples)
    summary['pointer_positions'] = sorted(set((s['x'], s['y']) for s in samples))
    summary['human_keys_during_task'] = sum(e['event'] == 'key_down' and task_started <= e['time'] <= task_finished for e in summary['human_events'])
    submits = [e for e in summary['background_events'] if e['event'] == ('web_submit' if args.case == 'webkit' else 'submit')]
    assert len(submits) == args.rounds, 'submission was duplicated or missing'
    assert summary['actual_human_text'] == human_text and not human_errors, 'foreground user input interrupted'
    assert samples and (args.mode == 'cooperative' or summary['foreground_or_key_loss_samples'] == 0), 'foreground/key focus changed'
    summary['status'] = 'passed' if len(summary['rounds']) == args.rounds and all(not r['reply'].get('error') for r in summary['rounds']) else 'failed'
except Exception as error:
    summary['error'] = str(error)
    print('POC result: ' + str(error), flush=True)
finally:
    if human_thread and human_thread.is_alive(): human_thread.join(20)
    for client in clients: client.close()
    if bg_log: summary['background_events'] = events(bg_log)
    if fg_log: summary['human_events'] = events(fg_log)
    for pid in active:
        try: os.kill(pid, 15)
        except ProcessLookupError: pass
    summary['helper_sha256'] = hashlib.sha256(Path(os.environ.get('DTW_ACCEPT_HELPER',str(ROOT / 'bin' / 'dtw')) if args.mode == 'cooperative' else str(ROOT/'bin/dtw-background-poc')).read_bytes()).hexdigest()
    summary['native_dispatch'] = [json.loads(line) for line in (out/'background-native.jsonl').read_text().splitlines() if line.startswith('{')] if (out/'background-native.jsonl').exists() else []
    names = run(['git', 'ls-files', '-c', '-o', '--exclude-standard', '-z']).split('\0')
    manifest = {name: hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in sorted(set(names)) if name and (ROOT/name).is_file() and (ROOT/name).suffix in ['.go','.m','.h','.swift','.py','.mod','.sum']}
    (out/'source-sha256.json').write_text(json.dumps(manifest, indent=2)+'\n')
    summary['source_manifest'] = 'source-sha256.json'
    summary['cost'] = {c.label: {'calls': c.calls, 'wire_bytes': c.bytes} for c in clients}
    (out / 'summary.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')
    print(str(out / 'summary.json'), flush=True)
if summary['status'] != 'passed': raise SystemExit(1)
