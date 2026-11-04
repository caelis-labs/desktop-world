#!/usr/bin/env python3
"""Unified acceptance entry: current MCP business tasks plus strict cursor audits."""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import sys

from app_client import artifact_identity
from computer_use_benchmark import BenchmarkReport, fixture_module, load_targets, summarize
from cursor_audit import MAX_SAMPLE_GAP_MS, POLICY_VERSION, independence_violation

HERE = Path(__file__).resolve().parent


def merge_evidence(benchmark, cursor, *, capture_prefix, audit_path):
    result = copy.deepcopy(benchmark)
    for case in result['cases']:
        detail = case.get('detail')
        if isinstance(detail, dict) and detail.get('capture'):
            detail['capture'] = str(Path(capture_prefix) / detail['capture'])
    calls = cursor.get('calls', [])
    violations = list(cursor['independence_violations']) if isinstance(cursor.get('independence_violations'), list) else []
    if (cursor.get('policy_version') != POLICY_VERSION or cursor.get('max_sample_gap_ms_policy') != MAX_SAMPLE_GAP_MS
            or not isinstance(cursor.get('independence_violations'), list)):
        violations.append({'failure': 'strict cursor policy report is missing or uses another policy version'})
    for row in calls:
        try:
            issue = independence_violation(row)
        except (KeyError, TypeError, ValueError):
            issue = 'cursor call evidence is malformed'
        if issue and not any(v.get('request_id') == row.get('request_id') and v.get('failure') == issue for v in violations):
            violations.append({'request_id': row.get('request_id'), 'failure': issue})
    result['cursor_policy'] = {
        'version': cursor.get('policy_version'), 'audit_report': audit_path,
        'max_sample_gap_ms_policy': cursor.get('max_sample_gap_ms_policy'),
        'passed': bool(calls) and not violations,
        'exec_intervals': len(calls),
        'moved_intervals': sum(row.get('cursor', {}).get('status') == 'moved' for row in calls),
        'max_sample_gap_ms': max((row.get('cursor', {}).get('max_sample_gap_ms', 0) for row in calls), default=None),
        'violations': violations,
        'limits': cursor.get('limits', [])}
    pointer_case = next(row for row in result['cases'] if row['id'] == 'P02')
    pointer_case.update(status='passed' if result['cursor_policy']['passed'] else 'failed', detail=result['cursor_policy'])
    return summarize(result)


def compile_observer(source, executable, output, *, as_library=False):
    with (output / (executable.name + '-build.log')).open('w') as log:
        subprocess.run(['xcrun', 'swiftc', *(['-parse-as-library'] if as_library else []),
            '-module-cache-path', str(output / 'swift-cache'), str(source), '-o', str(executable)],
            stdout=log, stderr=subprocess.STDOUT, check=True, timeout=120)
    return executable


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', default='acceptance')
    parser.add_argument('--source-revision', default='unrecorded')
    parser.add_argument('--observer', type=Path, help='Already compiled read-only system cursor observer')
    parser.add_argument('--overlay-observer', type=Path, help='Already compiled exact-owned virtual cursor observer')
    parser.add_argument('--focus-observer', type=Path, help='Already compiled continuous foreground observer')
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('Native and cursor acceptance currently require macOS')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    output.chmod(0o700)
    module = fixture_module()
    report = BenchmarkReport(load_targets(), artifact_identity(args.dtw)).snapshot()
    try:
        observer = args.observer or compile_observer(HERE.parent / 'fixtures/cursor-observer/main.swift', output / 'cursor-observer', output)
        overlay_observer = args.overlay_observer or compile_observer(HERE.parent / 'fixtures/cursor-overlay-observer.swift', output / 'overlay-observer', output, as_library=True)
        focus_observer = args.focus_observer or compile_observer(HERE.parent / 'fixtures/focus-observer/main.swift', output / 'focus-observer', output)
        probe_output = output / 'cursor/probe'
        command = [sys.executable, str(HERE / 'cursor_audit.py'), '--assert-independent',
            '--observer', str(observer.resolve()), '--output', str(output / 'cursor'), '--',
            sys.executable, str(HERE / 'computer_use_benchmark.py'), '--dtw', str(args.dtw.resolve()),
            '--output', str(probe_output), '--case', args.case, '--source-revision', args.source_revision,
            '--overlay-observer', str(overlay_observer.resolve()), '--focus-observer', str(focus_observer.resolve())]
        import os
        environment = dict(os.environ, DTW_INPUT_TRACE='1')
        audit_result = subprocess.run(command, env=environment, check=False)
        benchmark = module.read_json(probe_output / 'report.json')
        cursor = module.read_json(output / 'cursor/report.json')
        if benchmark is None or cursor is None:
            raise RuntimeError('Acceptance child did not publish both original benchmark and cursor reports')
        report = merge_evidence(benchmark, cursor, capture_prefix='cursor/probe', audit_path='cursor/report.json')
        report['audit_returncode'] = audit_result.returncode
        report['original_benchmark_report'] = 'cursor/probe/report.json'
    except BaseException as error:
        report['acceptance_error'] = str(error)
        report['acceptance_passed'] = False
        raise
    finally:
        module.write_json(output / 'report.json', report)
    print(json.dumps({key: report.get(key) for key in
        ('counts', 'core_automation_passed', 'core_passed', 'clean_run', 'acceptance_passed', 'target_baseline_passed')}))
    raise SystemExit(0 if report.get('acceptance_passed') else 1)


if __name__ == '__main__':
    main()
