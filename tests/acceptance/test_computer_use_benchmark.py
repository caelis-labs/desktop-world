import hashlib
import json
from pathlib import Path
import socket
import tempfile
import time
import unittest
from unittest import mock

from computer_use_benchmark import BenchmarkReport, CoreCases, fixture_module, image_point, load_targets, review_capture, utf16_fingerprint


class BaselineReportTests(unittest.TestCase):
    def setUp(self):
        self.report = BenchmarkReport(load_targets(), {'binary_sha256': 'test-only'})
        self.report.data['clean_run'] = True

    def mark_core(self):
        for target in self.report.targets['scenarios']:
            if target['profile'] == 'core':
                self.assertTrue(hasattr(CoreCases, target['id']), target['id'])
                self.report.record(target['id'], 'passed', {'oracle': 'unit-test only'})

    def test_missing_targets_never_pass_the_complete_baseline(self):
        self.mark_core()
        self.report.data['cleanup'] = {'passed': True}
        result = self.report.snapshot()
        self.assertTrue(result['core_passed'])
        self.assertFalse(result['target_baseline_passed'])
        self.assertGreater(result['counts']['not_run'], 0)
        self.assertEqual(result['counts']['failed'], 0)

    def test_unsupported_and_blocked_core_are_not_success(self):
        for status in ('unsupported', 'blocked', 'failed'):
            report = BenchmarkReport(load_targets(), {})
            for target in report.targets['scenarios']:
                if target['profile'] == 'core':
                    report.record(target['id'], status if target['id'] == 'N01' else 'passed', {})
            report.data['cleanup'] = {'passed': True}
            self.assertFalse(report.snapshot()['core_passed'], status)

    def test_cleanup_and_setup_failures_cannot_be_hidden_by_business_success(self):
        self.mark_core()
        self.assertFalse(self.report.snapshot()['core_passed'])
        self.report.data['cleanup'] = {'passed': False}
        self.assertFalse(self.report.snapshot()['core_passed'])
        self.report.data['cleanup'] = {'passed': True}
        self.report.data['setup_error'] = {'error': 'fixture failed'}
        self.assertFalse(self.report.snapshot()['core_passed'])

    def test_recovered_tool_error_keeps_strict_core_failed(self):
        self.mark_core()
        self.report.data['cleanup'] = {'passed': True}
        self.report.data['clean_run'] = False
        self.report.data['tool_errors'] = [{'is_error': True}]
        result = self.report.snapshot()
        self.assertTrue(result['core_automation_passed'])
        self.assertFalse(result['core_passed'])

    def test_system_panel_unknown_opening_is_read_back_without_replay(self):
        fixture = mock.Mock()
        client = mock.Mock()
        opening = {'failure': {'code': 'ax_action_failed'}, 'effects': {'dispatch': {'unknown': 1}}}
        client.execute.side_effect = [(opening, {'isError': True}),
            ({'effects': {'dispatch': {'dispatched': 1}}}, {'isError': False})]
        case = CoreCases(fixture, client, Path('/unused'))
        case.native = mock.Mock(side_effect=[{'panel': {'visible': True}},
            {'panel': {'result': 'cancel', 'file_saves': 0, 'opened_text': ''}}])
        case.capture = mock.Mock()
        case.run = mock.Mock(return_value={'result': {'elements': [{'id': 'w1.e1', 'name': 'Cancel'}]}})
        result = case.N10()
        self.assertEqual(client.execute.call_count, 2)
        self.assertEqual(client.execute.call_args_list[0].args[1], "await app.click('Open file…');")
        self.assertEqual(client.execute.call_args_list[1].args[1], 'await app.click({target:"w1.e1"});')
        self.assertIn('app.look', case.run.call_args.args[0])
        self.assertTrue(result['opening_receipt']['tool_error'])
        self.assertEqual(result['opening_receipt']['dispatch'], {'unknown': 1})

    def test_original_attempt_cannot_be_overwritten(self):
        self.report.record('N01', 'failed', {'error': 'original'})
        with self.assertRaises(ValueError):
            self.report.record('N01', 'passed', {})
        self.assertEqual(self.report.snapshot()['cases'][0]['detail']['error'], 'original')

    def test_pending_visual_review_blocks_core_success_and_preserves_review_history(self):
        for target in self.report.targets['scenarios']:
            if target['profile'] == 'core' and target['id'] != 'C01':
                self.report.record(target['id'], 'passed', {})
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            image = output / 'chat.png'
            image.write_bytes(b'owned captured PNG bytes; decoder checked by AppClient')
            digest = hashlib.sha256(image.read_bytes()).hexdigest()
            self.report.record('C01', 'pending_review', {'capture': 'chat.png', 'capture_sha256': digest})
            self.report.data['cleanup'] = {'passed': True}
            result = self.report.snapshot()
            self.assertTrue(result['core_automation_passed'])
            self.assertFalse(result['core_passed'])
            path = output / 'report.json'
            path.write_text(json.dumps(result))
            rejected = review_capture(path, 'C01', 'unit reviewer', 'blank image', outcome='failed')
            self.assertFalse(rejected['core_passed'])
            self.assertEqual(next(c for c in rejected['cases'] if c['id'] == 'C01')['status'], 'failed')
            accepted = review_capture(path, 'C01', 'unit reviewer', 'corrected inspection notes', outcome='passed')
            self.assertTrue(accepted['core_passed'])
            self.assertFalse(accepted['target_baseline_passed'])
            self.assertEqual([r['outcome'] for r in accepted['visual_reviews']], ['failed', 'passed'])
            image.write_bytes(b'replaced')
            with self.assertRaisesRegex(AssertionError, 'Capture bytes changed'):
                review_capture(path, 'C01', 'unit reviewer', 'different image')

    def test_fingerprint_covers_middle_and_utf16_surrogates(self):
        first = 'prefix 中文🙂 ' + 'a' * 8000 + ' end'
        changed = 'prefix 中文🙂 ' + 'b' + 'a' * 7999 + ' end'
        self.assertEqual(len(first), len(changed))
        self.assertNotEqual(utf16_fingerprint(first), utf16_fingerprint(changed))
        self.assertEqual(utf16_fingerprint('🙂'), utf16_fingerprint('\ud83d\ude42'))

    def test_duplicate_manifest_ids_are_rejected(self):
        targets = load_targets()
        targets['scenarios'].append(targets['scenarios'][0])
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'targets.json'
            path.write_text(json.dumps(targets))
            with self.assertRaises(ValueError):
                load_targets(path)

    def test_pixel_oracle_uses_actual_capture_mapping(self):
        view = {'image': {'bounds': [100, 200, 400, 300], 'width': 800, 'height': 600}}
        self.assertEqual(image_point(view, [200, 275]), {'x': 200, 'y': 150})
        with self.assertRaises(AssertionError):
            image_point(view, [99, 200])


class FixtureLifecycleTests(unittest.TestCase):
    def test_portable_services_are_owned_and_reaped(self):
        module = fixture_module()
        with tempfile.TemporaryDirectory() as temporary:
            fixture = module.ComputerUseFixture(Path(temporary) / 'case', 'unit-case')
            try:
                fixture.prepare(build=False)
                ports = [server.server_port for server, _ in fixture.servers]
                self.assertEqual(len(ports), 2)
                self.assertEqual(fixture.web_state()['case'], 'unit-case')
                self.assertTrue((fixture.output / 'workbench/state.json').is_file())
                cleanup = fixture.close()
                self.assertTrue(cleanup['passed'], cleanup)
                self.assertIs(fixture.close(), cleanup)
                for port in ports:
                    with socket.socket() as connection:
                        connection.settimeout(1)
                        self.assertNotEqual(connection.connect_ex(('127.0.0.1', port)), 0)
            finally:
                fixture.close()

    def test_partial_headers_cannot_hold_either_owned_service_open(self):
        module = fixture_module()
        with tempfile.TemporaryDirectory() as temporary:
            fixture = module.ComputerUseFixture(Path(temporary) / 'case', 'unit-case')
            connections = []
            try:
                fixture.prepare(build=False)
                for server, _ in fixture.servers:
                    connection = socket.create_connection(('127.0.0.1', server.server_port), timeout=1)
                    connections.append(connection)
                    connection.sendall(b'GET / HTTP/1.1\r\nHost:')
                    deadline = time.monotonic() + 1
                    while time.monotonic() < deadline and not getattr(server, '_threads', []):
                        time.sleep(0.01)
                    self.assertTrue(getattr(server, '_threads', []), 'request must be admitted before cleanup')
                started = time.monotonic()
                cleanup = fixture.close()
                self.assertTrue(cleanup['passed'], cleanup)
                self.assertLess(time.monotonic() - started, 3)
            finally:
                for connection in connections:
                    connection.close()
                fixture.close()

    def test_build_failure_stops_prepared_services(self):
        module = fixture_module()
        with tempfile.TemporaryDirectory() as temporary:
            fixture = module.ComputerUseFixture(Path(temporary) / 'case', 'unit-case')
            with mock.patch.object(fixture, 'build', side_effect=RuntimeError('compiler failed')):
                with self.assertRaisesRegex(RuntimeError, 'compiler failed'):
                    fixture.prepare()
            self.assertIsNotNone(fixture.cleanup)
            self.assertTrue(fixture.cleanup['passed'], fixture.cleanup)
            self.assertTrue(all(not thread.is_alive() for _, thread in fixture.servers))

    def test_forced_native_cleanup_is_not_success(self):
        import subprocess
        module = fixture_module()
        with tempfile.TemporaryDirectory() as temporary:
            fixture = module.ComputerUseFixture(temporary, 'unit-case')
            child = mock.Mock(pid=123, returncode=-15)
            child.poll.return_value = None
            child.wait.side_effect = [subprocess.TimeoutExpired('owned fixture', 5), -15]
            fixture.child = child
            cleanup = fixture.close()
            self.assertFalse(cleanup['passed'])
            self.assertEqual(cleanup['native']['forced'], ['terminate'])
            child.terminate.assert_called_once_with()


if __name__ == '__main__':
    unittest.main()
