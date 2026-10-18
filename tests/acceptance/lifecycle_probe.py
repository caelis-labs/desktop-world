#!/usr/bin/env python3
"""Stop an in-flight script, keep its unknown receipt, refuse further dispatch."""
import json, subprocess, sys, time, tempfile
from probe import call, exec_call
from runtime_probe import start
p = start(sys.argv[1])
try:
    v = exec_call(p, 2, {'script': 'while (true) {}', 'wait_ms': 0})
    assert v['status'] == 'running', v
    time.sleep(.3)
    before = time.monotonic()
    stopped = exec_call(p, 3, {'operation': 'cancel', 'id': v['id']})
    for n in range(200):
        state = exec_call(p, n + 4, {'operation': 'status', 'id': v['id']})
        if state.get('status') != 'running': break
        time.sleep(.025)
    assert state['status'] == 'unknown', state
    assert time.monotonic() - before < 5, state
    refused = exec_call(p, 300, {'script': 'return 1;'})
    assert 'runtime_stopped' in refused.get('error', ''), refused
    print(json.dumps({'cancel':'bounded', 'receipt':state, 'further_dispatch':'refused'}))
finally:
    p.stdin.close()
    p.wait(timeout=8)
    assert p.returncode == 0, p.returncode

# On native POSIX hosts inspect only descendants of this test's process.
if sys.platform == 'darwin':
    import os, signal
    def children(pid):
        result = subprocess.run(['pgrep', '-P', str(pid)], capture_output=True, text=True)
        return [int(n) for n in result.stdout.split()]
    def alive(pid):
        result = subprocess.run(['ps', '-o', 'stat=', '-p', str(pid)], capture_output=True, text=True)
        return bool(result.stdout.strip()) and not result.stdout.strip().startswith('Z')
    for termination in ('eof', 'sigterm', 'sigkill'):
        p = start(sys.argv[1])
        v = exec_call(p, 2, {'script': 'while (true) {}', 'wait_ms': 0})
        time.sleep(.2)
        supervisors = children(p.pid)
        workers = [pid for supervisor in supervisors for pid in children(supervisor)]
        assert len(supervisors) == 1 and len(workers) == 1, (supervisors, workers)
        if termination == 'eof': p.stdin.close()
        else: p.send_signal(signal.SIGTERM if termination == 'sigterm' else signal.SIGKILL)
        p.wait(timeout=5)
        deadline = time.monotonic()+5
        while any(alive(pid) for pid in supervisors+workers) and time.monotonic()<deadline: time.sleep(.025)
        assert not any(alive(pid) for pid in supervisors+workers), (termination,supervisors,workers)
    print(json.dumps({'eof':'reaped','sigterm':'reaped','sigkill_parent':'descendants_stopped'}))

# Standard MCP cancellation is scoped to an outstanding request ID.
p = start(sys.argv[1])
try:
    p.stdin.write(json.dumps({'jsonrpc':'2.0','id':400,'method':'tools/call','params':{'name':'exec','arguments':{'script':'while(true){}','wait_ms':5000}}})+'\n')
    p.stdin.flush()
    time.sleep(.2)
    p.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/cancelled','params':{'requestId':400}})+'\n')
    p.stdin.flush()
    time.sleep(.2)
    refused = exec_call(p,401,{'script':'return 1;'})
    assert 'runtime_stopped' in refused.get('error',''), refused
    print('{"mcp_request_cancellation":"stopped"}')
finally:
    p.stdin.close();p.wait(timeout=5)
