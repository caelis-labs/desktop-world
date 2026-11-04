"""Run one owned GUI probe with independent native foreground/cursor/key samples."""
import argparse
import json
from pathlib import Path
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--observer', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
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
            report['passed'] = (report.get('probe_exit') == 0 and report.get('observer_exit') == 0
                                and sampling.get('samples', 0) > 1
                                and sampling.get('max_gap_ms', float('inf')) <= 50
                                and report['cursor_readable']
                                and not report.get('forced_observer_cleanup'))
            (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
