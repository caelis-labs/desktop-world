#!/usr/bin/env python3
"""Opt-in: switch two already-observed windows in one persistent helper session.
No document edits. Old AppKit-cache implementation fails this regression.
"""
import argparse
import json
import subprocess
import time
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--helper', required=True)
p.add_argument('--first', required=True, help='exact observed window title')
p.add_argument('--second', required=True, help='exact observed window title')
p.add_argument('--log', type=Path, required=True)
a = p.parse_args()
s = subprocess.Popen([a.helper, 'serve', '--write-app', '访达', '--write-app', '文本编辑'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
seq = 0
log = a.log.open('x')
def call(op, args):
    global seq
    seq += 1
    req = dict(id=f'focus-{seq}', op=op, args=args)
    start = time.monotonic()
    s.stdin.write(json.dumps(req, ensure_ascii=False)+'\n'); s.stdin.flush()
    reply = json.loads(s.stdout.readline())
    log.write(json.dumps(dict(request=req, response=reply, elapsed_ms=1000*(time.monotonic()-start)), ensure_ascii=False)+'\n');log.flush()
    assert not reply.get('error'), reply
    return reply['result']
try:
    hello = json.loads(s.stdout.readline())
    assert hello.get('type') == 'hello', hello
    ob = call('observe', dict(scope=dict(desktop=True), projection='summary', fields=['name','app'], budget=dict(max_results=512,max_output_bytes=200000)))
    windows = []
    for title in [a.first, a.second]:
        matches = [o for o in ob['objects'] if o['kind']=='window' and o.get('name',{}).get('known')==title]
        assert len(matches)==1, (title, [(o.get('name'),o['kind']) for o in ob['objects']])
        windows.append(matches[0])
    for window in windows + windows:
        receipt = call('act', dict(steps=[dict(id='focus',op='focus',target=dict(ref=window['ref']))]))
        assert receipt['outcome']=='completed', receipt
        after = call('observe', dict(scope=dict(refs=[window['ref']]), projection='detail', fields=['name','app']))
        assert after['seat']['foreground_application']['known'] == window['app'], after['seat']
        assert after['seat']['foreground_window']['known'] == window['ref'], after['seat']
    print(json.dumps(dict(passed=True,switches=4,calls=seq,epoch=ob['epoch'] if 'epoch' in ob else None)))
finally:
    s.stdin.close();s.wait(timeout=10);log.close()
