#!/usr/bin/env python3
"""Run one fixed benchmark target in a fresh owned fixture; never replay an attempt."""
import argparse
import hashlib
import json
from pathlib import Path
import time
import traceback

from app_client import AppClient, artifact_identity
from computer_use_benchmark import BenchmarkReport, CoreCases, fixture_module, load_targets


def target_status(detail):
    if detail.get('blocked'):
        return 'blocked'
    if detail.get('unavailable'):
        return 'unsupported'
    if detail.get('gui_review') == 'required':
        return 'pending_review'
    return 'passed'


def expected_errors_verified(errors, identifiers):
    if len(set(identifiers)) != len(identifiers):
        return False
    actual = {row.get('execution_id') for row in errors}
    if actual != set(identifiers) or len(actual) != len(errors):
        return False
    return all(row.get('execution_id') and row.get('error_code')
        and row.get('effects', {}).get('dispatch') == {'not_dispatched': 1}
        and not row.get('effects', {}).get('focus_preparation')
        and not row.get('effects', {}).get('interrupted') for row in errors)


def case_class():
    from computer_use_native_cases import NativeExtendedCases
    from computer_use_web_cases import WebExtendedCases
    from computer_use_browser_cases import BrowserExtendedCases
    from computer_use_boundary_cases import BoundaryCases

    class TargetCases(NativeExtendedCases, WebExtendedCases, BrowserExtendedCases, BoundaryCases, CoreCases):
        pass
    return TargetCases


def run_target(args):
    module = fixture_module()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    output.chmod(0o700)
    identity = artifact_identity(args.dtw)
    identity.update(source_revision=args.source_revision,
        runner_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        target_manifest_sha256=hashlib.sha256((Path(__file__).parents[1] / 'fixtures/computer-use-app/scenarios.json').read_bytes()).hexdigest())
    identity['helper_files_sha256'] = {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(Path(__file__).parent.glob('computer_use_*.py'))}
    identity['helper_files_sha256']['app_client.py'] = hashlib.sha256(
        (Path(__file__).parent / 'app_client.py').read_bytes()).hexdigest()
    report = BenchmarkReport(load_targets(), identity)
    report.data['selected_scenarios'] = [args.scenario]
    report.data['scope'] = 'one fresh target attempt; other targets remain unexecuted'
    report.data['fixture_activation'] = {'requested': args.fixture_foreground, 'background_failure': None}
    if args.fixture_foreground:
        if args.background_failure is None:
            raise ValueError('Foreground diagnostics require the preserved background failure report')
        previous = json.loads(args.background_failure.read_text())
        prior_case = next(row for row in previous['cases'] if row['id'] == args.scenario)
        if prior_case['status'] not in ('failed', 'blocked'):
            raise ValueError('Foreground diagnostics require a failed background attempt of the same target')
        report.data['fixture_activation']['background_failure'] = {
            'path': str(args.background_failure.resolve()),
            'sha256': hashlib.sha256(args.background_failure.read_bytes()).hexdigest()}
    fixture = module.ComputerUseFixture(output / 'fixture', args.case)
    cases = client = None

    def publish():
        snapshot = report.snapshot()
        module.write_json(output / 'report.json', snapshot)
        return snapshot

    publish()
    started = time.monotonic()
    try:
        fixture.prepare(activate=args.fixture_foreground, role='accessory' if args.scenario == 'S02' else 'target',
                        compiled_native=args.fixture_binary)
        fixture.start()
        report.data['fixture_source'] = module.read_json(fixture.output / 'fixture-source.json')
        report.data['fixture_binary_sha256'] = hashlib.sha256(fixture.executable.read_bytes()).hexdigest()
        client = AppClient(args.dtw.resolve(), output / 'mcp')
        cases = case_class()(fixture, client, output,
            overlay_observer=args.overlay_observer, focus_observer=args.focus_observer)
        cases.vision_answer_timeout = args.vision_answer_timeout
        if args.scenario in ('C02', 'C03'):
            cases.run("await app.click('Canvas chat');")
        if args.scenario in ('W02', 'W03'):
            cases.web_ready()
        detail = getattr(cases, args.scenario)()
        report.record(args.scenario, target_status(detail), detail)
    except Exception as error:
        report.record(args.scenario, 'failed', {'error': str(error), 'traceback': traceback.format_exc(),
            'classification': 'inspect original evidence; desktop interference and product failure are not interchangeable'})
    finally:
        # A lifecycle target can replace the client after a deliberate EOF. Keep
        # the replacement and the original attempt's close evidence independent.
        active_client = cases.client if cases is not None else client
        fixture_cleanup = transport = None
        try:
            fixture_cleanup = fixture.close()
        except Exception as error:
            fixture_cleanup = {'passed': False, 'error': str(error)}
        try:
            if active_client is not None:
                transport = active_client.close()
        except Exception as error:
            transport = {'error': str(error)}
        transport_passed = (transport is not None and transport.get('returncode') == 0
            and not transport.get('forced') and transport.get('native_workers_reaped') is True)
        report.data['cleanup'] = {'fixture': fixture_cleanup, 'mcp': transport,
            'passed': fixture_cleanup['passed'] and transport_passed}
        clients = [item for item in (client, active_client) if item is not None]
        if client is active_client:
            clients = clients[:1]
        report.data['tool_errors'] = [row for item in clients for row in item.metrics if row['is_error']]
        report.data['clean_run'] = report.data['cleanup']['passed'] and not report.data['tool_errors']
        selected = next(row for row in report.data['cases'] if row['id'] == args.scenario)
        expected = selected.get('detail', {}).get('expected_tool_errors', [])
        report.data['expected_tool_errors'] = expected
        report.data['expected_errors_verified'] = (selected['status'] == 'passed'
            and expected_errors_verified(report.data['tool_errors'], expected))
        next(row for row in report.data['cases'] if row['id'] == args.scenario)['seconds'] = time.monotonic() - started
        result = publish()
    print(json.dumps({key: result.get(key) for key in ('selected_scenarios', 'counts', 'targeted_automation_passed', 'targeted_passed', 'clean_run')}))
    return 0 if result['targeted_passed'] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', default='target')
    parser.add_argument('--source-revision', default='unrecorded')
    parser.add_argument('--scenario', choices=[row['id'] for row in load_targets()['scenarios'] if row['id'] != 'P02'], required=True)
    parser.add_argument('--fixture-binary', type=Path)
    parser.add_argument('--vision-answer-timeout', type=int, default=180)
    parser.add_argument('--fixture-foreground', action='store_true', help='Explicit diagnostic condition after a documented failed background attempt')
    parser.add_argument('--background-failure', type=Path)
    parser.add_argument('--overlay-observer', type=Path)
    parser.add_argument('--focus-observer', type=Path)
    args = parser.parse_args()
    raise SystemExit(run_target(args))


if __name__ == '__main__':
    main()
