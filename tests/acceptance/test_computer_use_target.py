import copy
import json
from pathlib import Path
import tempfile
import unittest

from computer_use_benchmark import BenchmarkReport, fixture_module, load_targets, summarize
from computer_use_target import expected_errors_verified, target_status


class TargetEvidenceTests(unittest.TestCase):
    def test_unavailable_and_blocked_never_become_success(self):
        self.assertEqual(target_status({'unavailable': ['move']}), 'unsupported')
        self.assertEqual(target_status({'blocked': 'requires another platform'}), 'blocked')
        self.assertEqual(target_status({'gui_review': 'required'}), 'pending_review')

    def test_expected_error_requires_exact_identity_and_clean_refusal(self):
        row = {'execution_id': 'owned', 'error_code': 'selector_ambiguous',
               'effects': {'dispatch': {'not_dispatched': 1}}}
        self.assertTrue(expected_errors_verified([row], ['owned']))
        for identifiers in ([], ['other'], ['owned', 'owned']):
            self.assertFalse(expected_errors_verified([row], identifiers))
        for effects in ({}, {'dispatch': {'unknown': 1}}, {'dispatch': {'not_dispatched': 1, 'dispatched': 1}},
                        {'dispatch': {'not_dispatched': 1}, 'focus_preparation': {'dispatch': {'not_dispatched': 1}}},
                        {'dispatch': {'not_dispatched': 1}, 'interrupted': 1}):
            changed = copy.deepcopy(row)
            changed['effects'] = effects
            self.assertFalse(expected_errors_verified([changed], ['owned']))

    def target_report(self, status):
        report = BenchmarkReport(load_targets(), {})
        report.data.update(selected_scenarios=['W04'], cleanup={'passed': True}, clean_run=True,
                           cursor_policy={'passed': True})
        detail = {'capture_sha256': 'original'} if status == 'pending_review' else {}
        report.record('W04', status, detail)
        return report.snapshot()

    def test_one_target_pass_cannot_pass_full_baseline(self):
        result = self.target_report('passed')
        self.assertTrue(result['targeted_acceptance_passed'])
        self.assertFalse(result['acceptance_passed'])
        self.assertFalse(result['target_baseline_passed'])
        self.assertFalse(summarize({**result, 'cursor_policy': {'passed': False}})['targeted_acceptance_passed'])

    def test_unfinished_or_unsupported_target_cannot_pass(self):
        for status in ('blocked', 'unsupported', 'failed', 'pending_review'):
            with self.subTest(status=status):
                self.assertFalse(self.target_report(status)['targeted_acceptance_passed'])

    def test_expected_negative_contract_preserves_raw_clean_run_failure(self):
        result = self.target_report('passed')
        result.update(clean_run=False, expected_errors_verified=True)
        graded = summarize(result)
        self.assertTrue(graded['targeted_acceptance_passed'])
        self.assertFalse(graded['clean_run'])
        self.assertFalse(graded['target_baseline_passed'])
        self.assertFalse(summarize({**result, 'expected_errors_verified': False})['targeted_acceptance_passed'])


class DetachedExitTests(unittest.TestCase):
    def test_unknown_status_requires_exact_quit_journal_and_observed_exit(self):
        class Observer:
            pid = 123
            returncode = None
            def poll(self):
                return 'exit_observed'
            def wait(self, timeout):
                return None
            def cleanup_evidence(self):
                return {'pid': self.pid, 'birth': 456, 'returncode': None,
                        'exit_observed': True, 'exit_reason': 'absent', 'expected_detached_exit': True}

        for journal_birth, quit_kind, expected in ((456, 'quit', True), (457, 'quit', False), (456, 'ready', False)):
            with self.subTest(journal_birth=journal_birth, kind=quit_kind), tempfile.TemporaryDirectory() as temporary:
                output = Path(temporary)
                (output / 'native-events.jsonl').write_text(json.dumps({'pid': 123, 'process_birth': journal_birth, 'kind': quit_kind}) + '\n')
                fixture = fixture_module().ComputerUseFixture(output, 'detached-exit-unit')
                fixture.child = Observer()
                result = fixture.close()
                self.assertEqual(result['passed'], expected, result)
                self.assertIsNone(result['native']['returncode'])


if __name__ == '__main__':
    unittest.main()
