"""Run one owned GUI probe with independent native foreground/cursor/key samples."""
import argparse
import json
import math
from pathlib import Path
import subprocess
import time


def sample_gaps(rows):
    gaps = []
    for a, b in zip(rows, rows[1:]):
        if 'timestamp' in a and 'timestamp' in b:
            frequency = a.get('frequency', 0)
            if frequency <= 0 or b.get('frequency') != frequency:
                return None
            gap = (b['timestamp'] - a['timestamp']) * 1000 / frequency
        else:
            gap = b['unix_ms'] - a['unix_ms']
        if not math.isfinite(gap) or gap < 0:
            return None
        gaps.append(gap)
    return gaps


def check_intervals(rows, intervals):
    results = []
    for interval in intervals:
        if (not isinstance(interval.get('start_ms'), (float, int))
                or not isinstance(interval.get('end_ms'), (float, int))
                or not math.isfinite(interval['start_ms']) or not math.isfinite(interval['end_ms'])
                or interval['end_ms'] < interval['start_ms']):
            results.append({'name': interval.get('name'), 'passed': False, 'error': 'missing_or_invalid_interval'})
            continue
        before = [index for index, row in enumerate(rows) if row['unix_ms'] < interval['start_ms']]
        after = [index for index, row in enumerate(rows) if row['unix_ms'] > interval['end_ms']]
        result = {'name': interval['name'], 'passed': False, 'sampling_status': 'unknown',
                  'sample_gaps_are_diagnostic': True}
        if before and after:
            samples = rows[before[-1]:after[0] + 1]
            gaps = sample_gaps(samples)
            if not gaps:
                result['error'] = 'invalid_sample_clock'
                results.append(result)
                continue
            result.update(samples=len(samples), max_gap_ms=max(gaps, default=0),
                          sampling_status='observed',
                          foreground_asserted='foreground_pid' in interval,
                          cursor_asserted='cursor' in interval)
            if result['foreground_asserted']:
                result['foreground_preserved'] = all(row['foreground_pid'] == interval['foreground_pid'] for row in samples)
            if result['cursor_asserted']:
                result['cursor_preserved'] = all(row['cursor_readable'] and row['cursor'] == interval['cursor'] for row in samples)
            result['passed'] = result.get('foreground_preserved', True) and result.get('cursor_preserved', True)
        else:
            result['error'] = 'missing_bracketing_samples'
        results.append(result)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--observer', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--intervals', type=Path, help='Probe-owned background interval assertions against native samples')
    parser.add_argument('--metrics-root', type=Path, help='Check sample boundaries for recorded MCP calls; gaps are diagnostic')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('probe command required after --')
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    trace = out / 'desktop.jsonl'
    report = {'passed': False, 'functional_status': 'not_run', 'command': command,
              'sample_gaps_are_diagnostic': True}
    with (out / 'observer.stderr').open('wb') as error:
        observer = subprocess.Popen([str(args.observer.resolve()), '--stream', str(trace), '0'],
                                    stderr=error, creationflags=subprocess.CREATE_NO_WINDOW)
        try:
            until = time.monotonic() + 5
            while not (trace.exists() and trace.stat().st_size):
                if observer.poll() is not None or time.monotonic() >= until:
                    raise RuntimeError('native observer did not become ready')
                time.sleep(.01)
            with (out / 'probe.log').open('wb') as log:
                report['probe_exit'] = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT).returncode
                report['functional_status'] = 'passed' if report['probe_exit'] == 0 else 'failed'
            time.sleep(.03)  # Retain samples after the complete child interval.
        finally:
            Path(str(trace) + '.stop').write_text('owned observer normal stop', encoding='utf-8')
            try:
                report['observer_exit'] = observer.wait(timeout=5)
            except subprocess.TimeoutExpired:
                observer.terminate()
                observer.wait(timeout=5)
                report['forced_observer_cleanup'] = True
            summary = Path(str(trace) + '.summary.json')
            if summary.exists():
                report['sampling'] = json.loads(summary.read_text(encoding='utf-8'))
            sampling = report.get('sampling', {})
            rows = [json.loads(line) for line in trace.read_text(encoding='utf-8').splitlines()] if trace.exists() else []
            report['cursor_readable'] = bool(rows) and all(row.get('cursor_readable') is True for row in rows)
            if args.intervals:
                intervals = json.loads(args.intervals.read_text(encoding='utf-8')) if args.intervals.exists() else []
                report['intervals'] = check_intervals(rows, intervals)
            sampling_ok = bool(sample_gaps(rows))
            if args.metrics_root:
                calls = []
                for path in args.metrics_root.rglob('protocol-metrics.json'):
                    for index, metric in enumerate(json.loads(path.read_text(encoding='utf-8'))):
                        calls.append({'name': str(path.relative_to(args.metrics_root)) + ':' + str(index),
                                      'start_ms': metric.get('started_unix_ms'), 'end_ms': metric.get('ended_unix_ms')})
                extra = args.metrics_root / 'rpc-intervals.json'
                if extra.exists():
                    calls.extend(json.loads(extra.read_text(encoding='utf-8')))
                report['call_sampling'] = check_intervals(rows, calls)
                report['sampling_scope'] = 'clock validity and bracketing of recorded MCP calls and interrupted intervals; no maximum-gap gate'
                sampling_ok = sampling_ok and bool(calls) and all(row['passed'] for row in report['call_sampling'])
            report['sampling_status'] = 'observed' if sampling_ok and report['cursor_readable'] else 'unknown'
            report['passed'] = (report.get('probe_exit') == 0 and report.get('observer_exit') == 0
                                and sampling.get('samples', 0) > 1
                                and sampling_ok
                                and report['cursor_readable']
                                and (not args.intervals or bool(report['intervals']) and all(row['passed'] for row in report['intervals']))
                                and not report.get('forced_observer_cleanup'))
            (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
