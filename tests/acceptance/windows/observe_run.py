"""Run one owned GUI probe with independent native foreground/cursor/key samples."""
import argparse
import json
from pathlib import Path
import subprocess
import time


def check_intervals(rows, intervals):
    results = []
    for interval in intervals:
        if (not isinstance(interval.get('start_ms'), (float, int))
                or not isinstance(interval.get('end_ms'), (float, int))
                or interval['end_ms'] < interval['start_ms']):
            results.append({'name': interval.get('name'), 'passed': False, 'error': 'missing_or_invalid_interval'})
            continue
        before = [index for index, row in enumerate(rows) if row['unix_ms'] < interval['start_ms']]
        after = [index for index, row in enumerate(rows) if row['unix_ms'] > interval['end_ms']]
        result = {'name': interval['name'], 'passed': False}
        if before and after:
            samples = rows[before[-1]:after[0] + 1]
            gaps = [((b['timestamp'] - a['timestamp']) * 1000 / a['frequency']
                     if 'timestamp' in a and 'timestamp' in b else b['unix_ms'] - a['unix_ms'])
                    for a, b in zip(samples, samples[1:])]
            result.update(samples=len(samples), max_gap_ms=max(gaps, default=0),
                          foreground_preserved=all(row['foreground_pid'] == interval['foreground_pid'] for row in samples) if 'foreground_pid' in interval else True,
                          cursor_preserved=all(row['cursor_readable'] and row['cursor'] == interval['cursor'] for row in samples) if 'cursor' in interval else True)
            result['passed'] = (bool(gaps) and min(gaps) >= 0 and max(gaps) <= 50
                                and result['foreground_preserved'] and result['cursor_preserved'])
        results.append(result)
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--observer', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--intervals', type=Path, help='Probe-owned background interval assertions against native samples')
    parser.add_argument('--metrics-root', type=Path, help='Require <=50ms bracketing for every recorded MCP call, retaining whole-run gaps separately')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('probe command required after --')
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    trace = out / 'desktop.jsonl'
    report = {'passed': False, 'command': command, 'max_allowed_gap_ms': 50}
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
            sampling_ok = sampling.get('max_gap_ms', float('inf')) <= 50
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
                report['strict_sampling_scope'] = 'every recorded MCP call plus interrupted execution interval; whole-run max retained'
                sampling_ok = bool(calls) and all(row['passed'] for row in report['call_sampling'])
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
