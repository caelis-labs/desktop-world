import copy
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import MagicMock, patch

from computer_use_focus import (evaluate_action, evaluate_focus, exec_intervals,
                                finalize_proof, run_background_contract)


def sample(at, pid=7, kind='sample', **fields):
    return {'unix_ms': at, 'kind': kind, 'frontmost_pid': pid, **fields}


def wire(direction, at, message):
    return {'direction': direction, 'at_unix_ms': at, 'message': message}


class FocusEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.samples = [sample(at) for at in range(0, 42, 2)]

    def grade(self, rows=None):
        return evaluate_focus(self.samples if rows is None else rows, 5, 35, 7)

    def test_stable_exact_pid_and_complete_bracketing_pass(self):
        result = self.grade()
        self.assertTrue(result['passed'], result)
        self.assertEqual(result['max_sample_gap_ms'], 2)
        self.assertEqual(result['bracket_ms'], [4, 36])

    def test_gap_limit_includes_both_bracketing_samples(self):
        accepted = evaluate_focus([sample(0), sample(50)], 1, 49, 7)
        self.assertTrue(accepted['passed'])
        stalled = evaluate_focus([sample(0), sample(51)], 1, 49, 7)
        self.assertFalse(stalled['passed'])
        self.assertEqual(stalled['max_sample_gap_ms'], 51)

    def test_missing_empty_or_only_in_call_samples_cannot_prove_focus(self):
        for rows in ([], [sample(6), sample(34)], self.samples[3:], self.samples[:-3], None):
            with self.subTest(rows=rows):
                self.assertFalse(evaluate_focus(rows, 5, 35, 7)['passed'])

    def test_foreign_sample_fails_even_when_final_pid_restored(self):
        rows = copy.deepcopy(self.samples)
        rows[10]['frontmost_pid'] = 99
        result = self.grade(rows)
        self.assertFalse(result['passed'])
        self.assertEqual(result['failures'][0]['reason'], 'foreign_foreground')

    def test_transient_activation_between_stable_samples_fails(self):
        rows = self.samples[:9] + [sample(17, 99, 'activation'), sample(17.5, 7, 'activation')] + self.samples[9:]
        result = self.grade(rows)
        self.assertFalse(result['passed'])
        self.assertEqual(result['max_sample_gap_ms'], 2)
        self.assertEqual(result['failures'][0]['row']['frontmost_pid'], 99)

    def test_unknown_actor_is_preserved_and_never_assumed_witness(self):
        for pid in (None, 0, -1, True, '7'):
            rows = copy.deepcopy(self.samples)
            rows[10]['frontmost_pid'] = pid
            result = self.grade(rows)
            self.assertFalse(result['passed'])
            self.assertEqual(result['failures'][0]['reason'], 'unknown_actor')
            self.assertEqual(result['failures'][0]['row']['frontmost_pid'], pid)
        rows = self.samples[:9] + [sample(17, None, 'activation')] + self.samples[9:]
        self.assertFalse(self.grade(rows)['passed'])

    def test_explicit_observer_error_and_activation_read_disagreement_fail(self):
        for fields in ({'error': 'frontmost_application_unavailable'}, {'observed_frontmost_pid': 99}):
            rows = self.samples[:9] + [sample(17, 7, 'activation', **fields)] + self.samples[9:]
            self.assertFalse(self.grade(rows)['passed'])

    def test_malformed_nonfinite_and_reordered_evidence_fail(self):
        for replacement in ({}, None, sample(float('nan')), sample(20, kind='unknown')):
            rows = copy.deepcopy(self.samples)
            rows[10] = replacement
            self.assertFalse(self.grade(rows)['passed'])
        rows = copy.deepcopy(self.samples)
        rows[10], rows[11] = rows[11], rows[10]
        self.assertFalse(self.grade(rows)['passed'])
        for args in ((35, 5, 7), (5, 35, 0), (5, 35, True), (float('inf'), 35, 7)):
            self.assertFalse(evaluate_focus(self.samples, *args)['passed'])

    def test_cleanup_activation_outside_brackets_is_not_graded(self):
        self.samples.extend([sample(60, 99, 'activation'), sample(62, 99)])
        self.assertTrue(self.grade()['passed'])


class CurrentWireAndBusinessTests(unittest.TestCase):
    def setUp(self):
        self.request = {'id': 3, 'method': 'tools/call',
                        'params': {'name': 'exec', 'arguments': {'app': '/owned.app', 'source': 'await app.look();'}}}
        self.rows = [wire('request', 10, self.request),
                     wire('response', 20, {'id': 3, 'result': {'isError': True}})]
        self.rejected = {'status': 'completed', 'error': {'code': 'foreground_required'},
                         'effects': {'dispatch': {'not_dispatched': 1}}}

    def test_original_failed_exec_still_has_a_graded_interval(self):
        intervals, failures = exec_intervals(self.rows)
        self.assertEqual(failures, [])
        self.assertEqual(len(intervals), 1)
        self.assertEqual((intervals[0]['start_ms'], intervals[0]['end_ms']), (10, 20))
        self.assertEqual(intervals[0]['request_id'], 3)

    def test_missing_response_duplicate_id_and_legacy_tool_fail(self):
        for rows in (self.rows[:1], self.rows + self.rows[:1], self.rows + self.rows[1:]):
            self.assertTrue(exec_intervals(rows)[1])
        legacy = copy.deepcopy(self.rows)
        legacy[0]['message']['params']['name'] = 'result'
        self.assertTrue(exec_intervals(legacy)[1])

    def test_policy_rejection_is_expected_only_for_the_rejection_case(self):
        self.assertTrue(evaluate_action(self.rejected, {'isError': True},
                                       expected_rejection='foreground_required')['passed'])
        self.assertFalse(evaluate_action(self.rejected)['passed'])
        self.assertFalse(evaluate_action({'status': 'completed'},
                                        expected_rejection='foreground_required')['passed'])
        for dispatch in ({}, {'unknown': 1}, {'not_dispatched': 1, 'dispatched': 1}):
            changed = copy.deepcopy(self.rejected)
            changed['effects']['dispatch'] = dispatch
            self.assertFalse(evaluate_action(changed, expected_rejection='foreground_required')['passed'])

    def test_uncertain_or_interrupted_effects_cannot_pass_background_actions(self):
        for effects in ({'dispatch': {'unknown': 1}}, {'interrupted': 1},
                        {'focus_preparation': {'dispatch': {'unknown': 1}}},
                        {'focus_preparation': {'interrupted': 1}},
                        {'focus_preparation': {'dispatch': {'dispatched': 1}}}):
            with self.subTest(effects=effects):
                self.assertFalse(evaluate_action({'status': 'completed', 'effects': effects})['passed'])
        for preparation in ({'dispatch': {'not_dispatched': 1}}, {'dispatch': {'unknown': 1}},
                            {'dispatch': {'dispatched': 1}}, {'interrupted': 1}):
            changed = copy.deepcopy(self.rejected)
            changed['effects']['focus_preparation'] = preparation
            self.assertFalse(evaluate_action(changed, expected_rejection='foreground_required')['passed'])

    def test_expected_rejection_never_masks_business_or_evidence_failure(self):
        proof = {'cases': [evaluate_action(self.rejected, expected_rejection='foreground_required')],
                 'focus': {'passed': True, 'intervals': [{'passed': True}]},
                 'witness_no_input': True, 'cleanup': {'passed': True}}
        self.assertTrue(finalize_proof(copy.deepcopy(proof))['passed'])
        proof['cases'].append(evaluate_action(self.rejected))
        self.assertFalse(finalize_proof(copy.deepcopy(proof))['passed'])
        proof['cases'].pop()
        for path in ('focus', 'cleanup'):
            changed = copy.deepcopy(proof)
            changed[path]['passed'] = False
            self.assertFalse(finalize_proof(changed)['passed'])
        changed = copy.deepcopy(proof)
        changed['focus']['intervals'] = []
        self.assertFalse(finalize_proof(changed)['passed'])
        changed = copy.deepcopy(proof)
        changed['failure'] = {'message': 'original failure'}
        self.assertFalse(finalize_proof(changed)['passed'])


class ResourceFailureTests(unittest.TestCase):
    def test_failed_mode_precondition_writes_report_without_starting_anything(self):
        target = MagicMock()
        target.state.return_value = {'mode': 'web'}
        with tempfile.TemporaryDirectory() as temporary, patch('computer_use_focus.subprocess.Popen') as spawn:
            proof = run_background_contract(target, '/dtw', '/observer', Path(temporary) / 'run')
            self.assertFalse(proof['passed'])
            self.assertIn('native mode', proof['failure']['message'])
            self.assertTrue((Path(temporary) / 'run/report.json').is_file())
            spawn.assert_not_called()

    def test_cleanup_errors_preserve_original_failure_and_reap_resources_independently(self):
        target = MagicMock()
        target.state.return_value = {'mode': 'native', 'pid': 22, 'native': {'counter': 0}}
        witness = MagicMock()
        witness.child.pid = 7
        witness.state.return_value = {'environment': {'frontmost_pid': 7},
                                     'witness': {'text': 'witness sentinel', 'key_events': 0, 'flags_events': 0}}
        witness.close.side_effect = RuntimeError('witness close failed')
        client = MagicMock()
        client.execute.side_effect = RuntimeError('original execution failed')
        client.close.return_value = {'returncode': 0, 'forced': [], 'native_workers_reaped': True}
        observer = MagicMock()
        observer.communicate.side_effect = subprocess.TimeoutExpired('owned observer', 3)
        observer.poll.return_value = 0
        observer.stdin, observer.stdout = io.BytesIO(), io.BytesIO()
        with tempfile.TemporaryDirectory() as temporary, \
                patch('computer_use_focus._fixture_class', return_value=MagicMock(return_value=witness)), \
                patch('computer_use_focus.AppClient', return_value=client), \
                patch('computer_use_focus.subprocess.Popen', return_value=observer), \
                patch('computer_use_focus._ready'), patch('computer_use_focus.time.sleep'):
            proof = run_background_contract(target, '/dtw', '/observer', Path(temporary) / 'run')
            self.assertFalse(proof['passed'])
            self.assertEqual(proof['failure']['message'], 'original execution failed')
            self.assertFalse(proof['cases'][0]['passed'])
            witness.close.assert_called_once()
            client.close.assert_called_once()
            self.assertFalse(proof['cleanup']['passed'])
            self.assertTrue(any('observer' in error for error in proof['cleanup']['errors']))
            self.assertTrue(any('witness close failed' in error for error in proof['cleanup']['errors']))
            saved = json.loads((Path(temporary) / 'run/report.json').read_text())
            self.assertEqual(saved['failure'], proof['failure'])


if __name__ == '__main__':
    unittest.main()
