#!/usr/bin/env python3
"""Two live Runtimes of the same binary: SCK must work after either client captures.
Usage: DTW_CUA_WORKER=... python3 multi_runtime_scene_probe.py DTW PID OUTPUT_DIR
Read-only GUI test. Persistent clients alternate, then submit concurrent reads.
"""
import concurrent.futures
import json
import subprocess
import sys
import time
from pathlib import Path
from probe import call, exec_call, result, structured_result

binary, target, output = sys.argv[1:]
root = Path(output)
root.mkdir(parents=True, exist_ok=False)
processes = []
records = []

def observe(index, number):
    proc = processes[index]
    began = time.monotonic()
    ident = number * 10
    done = exec_call(proc, ident, {'operation': 'app', 'pid': int(target)})
    if done.get('status') == 'running':
        done = result(proc, ident + 1, None, done['id'])
    record = {'runtime': index, 'elapsed_seconds': round(time.monotonic()-began, 3), 'receipt': done}
    (root / f'{number}-{index}-receipt.json').write_text(json.dumps(record, indent=2))
    assert done.get('status') == 'completed' and not done.get('is_error'), record
    view = structured_result(proc, ident + 2, None, done['id'])
    assert view['pid'] == int(target) and view['frames'], view
    assert any(frame['backend'] == 'screencapturekit' for frame in view['frames']), view
    (root / f'{number}-{index}-observation.json').write_text(json.dumps(view, indent=2))
    return record

try:
    for index in range(2):
        proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=(root/f'{index}-stderr.log').open('w'), text=True)
        processes.append(proc)
        call(proc, {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
            'protocolVersion': '2025-06-18', 'capabilities': {},
            'clientInfo': {'name': 'multi-runtime-scene', 'version': '1'}}})
    for number in range(1, 7):
        records.append(observe((number-1) % 2, number))
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        futures = [pool.submit(observe, index, 7+index) for index in range(2)]
        records.extend(future.result() for future in futures)
    summary = {'status': 'passed', 'live_runtimes': 2, 'alternating_reads': 6,
               'concurrent_reads': 2, 'input_sent': False,
               'max_seconds': max(record['elapsed_seconds'] for record in records)}
    (root/'summary.json').write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary))
finally:
    for proc in processes:
        proc.stdin.close()
        try:
            proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
