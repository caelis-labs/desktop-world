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
import os
from pathlib import Path
import re
import select
import subprocess
import sys

POLICY_VERSION = 'bounded-pointer-borrow-v3'
MAX_SAMPLE_GAP_MS = 50
POINTER_OPERATIONS = {'click', 'move', 'drag', 'scroll'}


def trace(samples):
    valid = [row for row in samples if all(isinstance(row.get(k), (int, float))
             and math.isfinite(row[k]) for k in ('x', 'y', 'unix_ms'))]
    timestamps = [row['unix_ms'] for row in samples
                  if isinstance(row.get('unix_ms'), (int, float)) and math.isfinite(row['unix_ms'])]
    max_gap = max((b - a for a, b in zip(timestamps, timestamps[1:])), default=0)
    if (len(valid) < 2 or len(valid) != len(samples)
            or any(b['unix_ms'] < a['unix_ms'] for a, b in zip(valid, valid[1:]))):
        return {'status': 'oracle_unavailable', 'samples': len(samples),
                'max_sample_gap_ms': max_gap}
    first = valid[0]
    distance = lambda row: math.hypot(row['x'] - first['x'], row['y'] - first['y'])
    transitions = sum((a['x'], a['y']) != (b['x'], b['y']) for a, b in zip(valid, valid[1:]))
    maximum = max(distance(row) for row in valid)
    return {'status': 'moved' if maximum > 0 else 'stable', 'samples': len(samples),
            'sample_start_unix_ms': first['unix_ms'], 'sample_end_unix_ms': valid[-1]['unix_ms'],
            'transition_intervals': [{'start_unix_ms': a['unix_ms'], 'end_unix_ms': b['unix_ms']}
                                     for a, b in zip(valid, valid[1:])
                                     if (a['x'], a['y']) != (b['x'], b['y'])],
            'before': [first['x'], first['y']], 'after': [valid[-1]['x'], valid[-1]['y']],
            'max_displacement_points': round(maximum, 3), 'transitions': transitions,
            'max_sample_gap_ms': max_gap}


def eof_recovery(root, path, request_id, start, boundaries, transcripts):
    """Use sidecars only to locate independently verified recovery wire evidence."""
    def timestamp(value):
        return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)

    def same_id(left, right):
        return type(left) is type(right) and left == right

    def owned_path(value):
        if not isinstance(value, str) or not Path(value).is_absolute():
            return None
        resolved = Path(value).resolve()
        return resolved if resolved.is_relative_to(root.resolve()) else None

    for boundary in boundaries:
        try:
            if (boundary['mode'] != 'eof' or owned_path(boundary['transcript']) != path.resolve()
                    or not same_id(boundary['request_id'], request_id)
                    or boundary['async_response']['outcome'] != 'no-response'):
                continue
            cleanup = boundary['cleanup_read']
            recovery_path = owned_path(cleanup['transcript'])
            if recovery_path is None or recovery_path == path.resolve():
                continue
            frames = transcripts.get(recovery_path, [])
            requests = [f for f in frames if f['direction'] == 'request'
                        and same_id(f['message'].get('id'), cleanup['request_id'])]
            responses = [f for f in frames if f['direction'] == 'response'
                         and same_id(f['message'].get('id'), cleanup['request_id'])]
            if len(requests) != 1 or len(responses) != 1:
                continue
            request, response = requests[0], responses[0]
            message = request['message']
            if message.get('method') != 'tools/call' or message.get('params', {}).get('name') != 'exec':
                continue
            source = message['params']['arguments']['source']
            # Every app reference must be a direct read call, including in mixed scripts.
            reads = re.sub(r'\bapp\.(look|capture)\s*\(', '(', source)
            if reads == source or re.search(r'\bapp\b', reads):
                continue
            envelope = response['message'].get('result', {})
            result = envelope.get('structuredContent', {})
            execution = result.get('execution_id')
            if (response['message'].get('error') or envelope.get('isError')
                    or result.get('error') or result.get('failure')
                    or result.get('status') != 'completed' or cleanup['status'] != 'completed'
                    or not isinstance(execution, str) or not execution
                    or cleanup['execution_id'] != execution):
                continue
            bounds = (start, cleanup['start_unix_ms'], request['at_unix_ms'],
                      response['at_unix_ms'], boundary['async_response']['observed_until_unix_ms'],
                      cleanup['end_unix_ms'])
            if not all(timestamp(t) for t in bounds) or any(a > b for a, b in zip(bounds, bounds[1:])):
                continue
            return response
        except (KeyError, TypeError, ValueError, OSError):
            # A partial or malformed sidecar never proves recovery.
            continue
    return None


def summarize(root, samples):
    times = [row['unix_ms'] for row in samples]
    rows = []
    boundaries = []
    for sidecar in root.rglob('boundary-interruptions.jsonl'):
        for line in sidecar.read_text().splitlines():
            try:
                boundary = json.loads(line)
                if isinstance(boundary, dict):
                    boundaries.append(boundary)
            except ValueError:
                continue
    transcripts = {path.resolve(): [json.loads(line) for line in path.read_text().splitlines()]
                   for path in sorted(root.rglob('mcp.jsonl'))}
    for path in sorted(root.rglob('mcp.jsonl')):
        frames = transcripts[path.resolve()]
        pending = {}
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
                         'dispatch': result.get('effects', {}).get('dispatch', {}),
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
            completion = 'no_reply_after_cancel'
            if recovery is None:
                recovery = eof_recovery(root, path, request_id, frame['at_unix_ms'], boundaries, transcripts)
                completion = 'no_reply_after_eof'
            if recovery:
                lo = max(0, bisect.bisect_left(times, frame['at_unix_ms']) - 1)
                hi = min(len(samples), bisect.bisect_right(times, recovery['at_unix_ms']) + 1)
                row.update(completion=completion,
                           start_unix_ms=frame['at_unix_ms'],end_unix_ms=recovery['at_unix_ms'],
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


def borrow_intervals(records):
    """Pair the complete transport log before assigning intervals to MCP calls."""
    groups, errors, intervals = {}, [], []
    for record in records:
        key = (record.get('pid'), record.get('trace_id'))
        if not all(isinstance(value, int) and not isinstance(value, bool) and value > 0 for value in key):
            errors.append('invalid borrow interval identity')
            continue
        groups.setdefault(key, []).append(record)
    for records in groups.values():
        if len(records) != 2 or [r.get('phase') for r in records] != ['begin', 'end']:
            errors.append('incomplete or duplicate borrow interval')
            continue
        begin, end = records
        fields = ('pid', 'trace_id', 'operation', 'window', 'route', 'reason')
        if (any(k not in begin or k not in end or begin[k] != end[k] for k in fields)
                or begin.get('window') is None):
            errors.append('borrow interval metadata mismatch')
            continue
        start, stop = begin.get('at_unix_ms'), end.get('at_unix_ms')
        if (not all(isinstance(t, (int, float)) and math.isfinite(t) for t in (start, stop))
                or start > stop):
            errors.append('invalid borrow interval bounds')
            continue
        intervals.append({**{k: begin.get(k) for k in fields},
                          'start_unix_ms': start, 'end_unix_ms': stop})
    return intervals, errors


def attach_routes(root, rows):
    traces, interval_records, errors = {}, {}, {}
    for path in root.rglob('dtw-stderr.log'):
        transport = str(path.parent.relative_to(root))
        for line in path.read_text().splitlines():
            for prefix, store in [('dtw_input ', traces), ('dtw_input_interval ', interval_records)]:
                if line.startswith(prefix):
                    try:
                        record = json.loads(line.removeprefix(prefix))
                        if not isinstance(record, dict):
                            raise ValueError('trace must be an object')
                        store.setdefault(transport, []).append(record)
                    except ValueError:
                        errors.setdefault(transport, []).append('malformed input trace')
    transports = set(traces) | set(interval_records) | set(errors)
    paired = {transport: borrow_intervals(interval_records.get(transport, [])) for transport in transports}
    for row in rows:
        transport = str(Path(row['transcript']).parent)
        intervals, issues = paired.get(transport, ([], []))
        # Unpairable evidence cannot safely be localized to one action.
        row['interval_errors'] = issues + errors.get(transport, [])
        row['borrow_intervals'] = []
        row['routes'] = []
        if 'start_unix_ms' in row and 'end_unix_ms' in row:
            start, end = row['start_unix_ms'], row['end_unix_ms']
            row['routes'] = [route for route in traces.get(transport, [])
                             if isinstance(route.get('at_unix_ms'), (int, float))
                             and start <= route['at_unix_ms'] <= end]
            row['borrow_intervals'] = [interval for interval in intervals
                                       if interval['start_unix_ms'] <= end
                                       and interval['end_unix_ms'] >= start]


def allowed_borrow(interval):
    operation, reason = interval.get('operation'), interval.get('reason')
    return interval.get('route') == 'borrowed' and (
        (reason == 'native_hover_tracking' and operation == 'move') or
        (reason == 'addressed_event_api_unavailable' and operation in POINTER_OPERATIONS))


def independence_violation(row):
    cursor, routes = row['cursor'], row.get('routes', [])
    if cursor['status'] not in ('stable', 'moved'):
        return 'system cursor oracle unavailable or request incomplete'
    gap = cursor.get('max_sample_gap_ms')
    if (not isinstance(gap, (int, float)) or isinstance(gap, bool)
            or not math.isfinite(gap) or gap < 0 or gap > MAX_SAMPLE_GAP_MS):
        return 'system cursor sample gap exceeds policy or is unavailable'
    start, end = row.get('start_unix_ms'), row.get('end_unix_ms')
    if (start is None or end is None or start > end
            or cursor.get('sample_start_unix_ms', float('inf')) > start
            or cursor.get('sample_end_unix_ms', float('-inf')) < end):
        return 'system cursor samples do not bracket the complete request and cleanup'
    if row.get('interval_errors'):
        return 'invalid or incomplete borrow interval evidence'
    intervals = row.get('borrow_intervals', [])
    for interval in intervals:
        if (not allowed_borrow(interval)
                or not start <= interval['start_unix_ms'] <= interval['end_unix_ms'] <= end):
            return 'physical borrowing lacks bounded classified capability evidence'
    for route in routes:
        if route.get('route') == 'borrowed':
            if not allowed_borrow(route):
                return 'physical borrowing lacks classified capability evidence'
            if not any(all(route.get(k) == interval.get(k) for k in ('pid', 'window', 'operation', 'reason'))
                       and interval['start_unix_ms'] <= route.get('at_unix_ms', -1) <= interval['end_unix_ms']
                       for interval in intervals):
                return 'event-only borrowed route is insufficient; bounded interval required'
    pointer_methods = POINTER_OPERATIONS.intersection(row.get('methods', []))
    if pointer_methods and any(row.get('dispatch', {}).get(state, 0) for state in ('dispatched', 'unknown')):
        # The aggregate receipt cannot identify individual dispatches in a script;
        # require metadata for every pointer operation shape it contains.
        for operation in pointer_methods:
            evidence = [route for route in routes
                        if (route.get('operation') == operation
                            and route.get('route') in ('native', 'addressed', 'borrowed'))
                        or (operation == 'click' and route.get('operation') == 'press'
                            and route.get('route') == 'native' and route.get('reason') == 'ax_action')
                        or (route.get('route') == 'refused'
                            and route.get('reason') == 'execution_scope_preflight'
                            and route.get('dispatch') == 'not_dispatched'
                            and route.get('operation') in ((operation, 'press') if operation == 'click' else (operation,)))]
            if len(evidence) < row.get('methods', []).count(operation):
                return 'native input route evidence missing'
    if cursor['status'] == 'moved':
        transitions = cursor.get('transition_intervals', [])
        if not transitions:
            return 'cursor movement lacks sample transition evidence'
        for transition in transitions:
            if not any(interval['start_unix_ms'] <= transition['start_unix_ms']
                       <= transition['end_unix_ms'] <= interval['end_unix_ms'] for interval in intervals):
                return 'sampled system cursor movement outside an allowed borrow interval (actor undetermined)'
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--observer', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--assert-independent', action='store_true',
                        help='Fail on cursor movement in an addressed/native route or unclassified borrowing (requires DTW_INPUT_TRACE=1)')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('a probe command after -- is required')
    if args.assert_independent and os.environ.get('DTW_INPUT_TRACE') != '1':
        parser.error('--assert-independent requires DTW_INPUT_TRACE=1')
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
    attach_routes(out, rows)
    violations = [{'request_id': row['request_id'], 'transcript': row['transcript'], 'failure': issue}
                  for row in rows if (issue := independence_violation(row))]
    if args.assert_independent and not rows:
        violations.append({'failure':'no MCP requests were observed inside the audit directory'})
    report = {'policy_version': POLICY_VERSION, 'max_sample_gap_ms_policy': MAX_SAMPLE_GAP_MS,
              'coordinate_space': 'global_screen_points', 'requested_sample_interval_ms': 2,
              'limits': ['Actual sample gaps are recorded per call; shorter excursions can be missed.',
                         'Position changes alone do not identify the actor. Match them to delivery source and fixture effects.'],
              'probe_returncode': code, 'calls': rows, 'independence_violations': violations,
              'directed_experiments': directed_experiments(out, samples), 'scope':
              'sampled actual system cursor, including transient movement; not an input route or virtual overlay'}
    (out / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'probe_returncode': code, 'observed_calls': len(rows),
                      'moved_calls': sum(row['cursor']['status'] == 'moved' for row in rows),
                      'report': str(out / 'report.json')}))
    return code or (1 if args.assert_independent and violations else 0)


if __name__ == '__main__':
    sys.exit(main())
