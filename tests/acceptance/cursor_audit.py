#!/usr/bin/env python3
"""Observe the actual system cursor during an owned GUI probe, without posting input.

The probe still sends each original request exactly once. This independent process
samples CGEvent.location at 2 ms; endpoint equality alone never counts as stable.
Example: cursor_audit.py --observer /tmp/observer --output /tmp/new-audit -- \
  python3 tests/acceptance/app_scene_probe.py --dtw /tmp/frozen/dtw --output /tmp/new-probe
"""
import argparse
import bisect
import hashlib
import json
import math
from pathlib import Path
import re
import select
import subprocess
import sys


def trace(samples):
    valid = [row for row in samples if 'x' in row and 'y' in row]
    if len(valid) < 2 or len(valid) != len(samples):
        return {'status': 'oracle_unavailable', 'samples': len(samples)}
    first = valid[0]
    distance = lambda row: math.hypot(row['x'] - first['x'], row['y'] - first['y'])
    transitions = sum((a['x'], a['y']) != (b['x'], b['y']) for a, b in zip(valid, valid[1:]))
    maximum = max(distance(row) for row in valid)
    return {'status': 'moved' if maximum > 0 else 'stable', 'samples': len(samples),
            'before': [first['x'], first['y']], 'after': [valid[-1]['x'], valid[-1]['y']],
            'max_displacement_points': round(maximum, 3), 'transitions': transitions,
            'max_sample_gap_ms': round(max((b['unix_ms'] - a['unix_ms'] for a, b in zip(valid, valid[1:])), default=0), 3)}


def summarize(root, samples):
    times = [row['unix_ms'] for row in samples]
    rows = []
    for path in sorted(root.rglob('mcp.jsonl')):
        pending = {}
        frames = [json.loads(line) for line in path.read_text().splitlines()]
        reads = {f['message'].get('id') for f in frames if f['direction'] == 'request'
                 and re.search(r'\bapp\.(look|capture)\s*\(',
                               f['message'].get('params', {}).get('arguments', {}).get('source', ''))}
        cancelled = {}
        for frame in frames:
            message = frame['message']
            request_id = message.get('id')
            if frame['direction'] == 'request':
                if message.get('method') == 'notifications/cancelled':
                    cancelled[message['params']['requestId']] = frame['at_unix_ms']
                if message.get('method') == 'tools/call' and message.get('params', {}).get('name') == 'exec':
                    pending[request_id] = frame
                continue
            start = pending.pop(request_id, None)
            if start is None:
                continue
            source = start['message']['params']['arguments'].get('source', '')
            # Method names only: do not publish script arguments or typed text.
            methods = re.findall(r'\bapp\.(\w+)\s*\(', source)
            lo = max(0, bisect.bisect_left(times, start['at_unix_ms']) - 1)
            hi = min(len(samples), bisect.bisect_right(times, frame['at_unix_ms']) + 1)
            result = message.get('result', {}).get('structuredContent', {})
            rows.append({'transcript': str(path.relative_to(root)), 'request_id': request_id,
                         'methods': methods, 'source_sha256': hashlib.sha256(source.encode()).hexdigest(),
                         'start_unix_ms': start['at_unix_ms'], 'end_unix_ms': frame['at_unix_ms'],
                         'tool_error': message.get('result', {}).get('isError', False),
                         'failure': result.get('failure', {}).get('code'),
                         'cursor': trace(samples[lo:hi])})
        for request_id, frame in pending.items():
            # Cancelled requests may intentionally have no wire reply. Preserve
            # that fact; the next successful read bounds the cleanup oracle,
            # and is never presented as the missing action's dispatch receipt.
            recovery = next((f for f in frames if request_id in cancelled
                             and f['direction'] == 'response' and f['message'].get('id') in reads
                             and f['at_unix_ms'] >= cancelled[request_id]
                             and f['message'].get('result', {}).get('structuredContent', {}).get('status') == 'completed'), None)
            source = frame['message']['params']['arguments'].get('source', '')
            row = {'transcript': str(path.relative_to(root)), 'request_id': request_id,
                   'methods': re.findall(r'\bapp\.(\w+)\s*\(', source),
                   'cursor': {'status': 'incomplete_request'}}
            if recovery:
                lo = max(0, bisect.bisect_left(times, frame['at_unix_ms']) - 1)
                hi = min(len(samples), bisect.bisect_right(times, recovery['at_unix_ms']) + 1)
                row.update(completion='no_reply_after_cancel',
                           oracle_ends_at='first successful recovery read; not an action receipt',
                           cursor=trace(samples[lo:hi]))
            rows.append(row)
    return rows


def directed_experiments(root, samples):
    times = [row['unix_ms'] for row in samples]
    rows = []
    for path in sorted(root.rglob('directed-actions.json')):
        for case in json.loads(path.read_text())['cases']:
            lo = max(0, bisect.bisect_left(times, case['start_unix_ms']) - 1)
            hi = min(len(samples), bisect.bisect_right(times, case['end_unix_ms']) + 1)
            rows.append({**case, 'cursor': trace(samples[lo:hi])})
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--observer', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('a probe command after -- is required')
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    out.chmod(0o700)
    raw = out / 'cursor.jsonl'
    observer = subprocess.Popen([str(args.observer.resolve()), str(raw)], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, text=True)
    code = 1
    try:
        if not select.select([observer.stdout], [], [], 5)[0] or observer.stdout.readline().strip() != 'ready':
            raise RuntimeError('independent cursor observer did not start')
        code = subprocess.run(command, check=False).returncode
    finally:
        observer.stdin.close()
        observer.wait(timeout=5)
    if observer.returncode:
        raise RuntimeError('cursor observer failed')
    samples = [json.loads(line) for line in raw.read_text().splitlines()]
    # Probe output must be inside the audit directory; exact transcripts are
    # used rather than guessing completion or replaying a failed operation.
    rows = summarize(out, samples)
    report = {'coordinate_space': 'global_screen_points', 'requested_sample_interval_ms': 2,
              'limits': ['Actual sample gaps are recorded per call; shorter excursions can be missed.',
                         'Position changes alone do not identify the actor. Match them to delivery source and fixture effects.'],
              'probe_returncode': code, 'calls': rows,
              'directed_experiments': directed_experiments(out, samples), 'scope':
              'sampled actual system cursor, including transient movement; not an input route or virtual overlay'}
    (out / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'probe_returncode': code, 'observed_calls': len(rows),
                      'moved_calls': sum(row['cursor']['status'] == 'moved' for row in rows),
                      'report': str(out / 'report.json')}))
    return code


if __name__ == '__main__':
    sys.exit(main())
