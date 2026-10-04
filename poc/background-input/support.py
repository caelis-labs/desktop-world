"""Bounded dtw client shared by independent native acceptance scenarios."""
import json, os, selectors, subprocess, time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
clients=[]
out=None
env=None
class Client:
    def __init__(self, label, title, mode=None, policy='shared_input'):
        self.label, self.seq, self.calls, self.bytes = label, 0, 0, 0
        self.trace = (out / (label + '-wire.jsonl')).open('w')
        self.stderr = (out / (label + '-native.jsonl')).open('w')
        binary = 'dtw-background-poc' if mode and mode != 'cooperative' else 'dtw'
        executable = os.environ.get('DTW_ACCEPT_HELPER') if binary == 'dtw' else None
        command = [executable or str(ROOT / 'bin' / binary), 'serve', '--input-policy', policy]
        if title: command += ['--write-app-window', title]
        if label == 'agent': command += ['--assets-dir', str(out/'captures')]
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
