import copy
import json
from pathlib import Path
import queue
import tempfile
import unittest
from unittest.mock import Mock, patch

from computer_use_scope import (TEXT, _activate, _partial_drag, async_outcome,
                                evaluate_scope_focus, scope_source)


def sample(at, pid, kind='sample', **fields):
    return {'unix_ms': at, 'frontmost_pid': pid, 'kind': kind, **fields}


def evidence(returned=False):
    rows = [sample(at, 7 if at < 20 or 80 <= at < 90 or (at >= 90 and not returned) else 8)
            for at in range(0, 102, 2)]
    rows.extend([sample(19, 8, 'activation', observed_frontmost_pid=8),
                 sample(79, 7, 'activation', observed_frontmost_pid=7)])
    if returned:
        rows.append(sample(89, 8, 'activation', observed_frontmost_pid=8))
    return sorted(rows, key=lambda r: r['unix_ms'])


class ScopeFocusTests(unittest.TestCase):
    def grade(self, rows=None, **kwargs):
        return evaluate_scope_focus(evidence() if rows is None else rows, 5, 95, 8, 7, **kwargs)

    def test_one_borrow_and_one_restore_need_events_and_samples(self):
        self.assertTrue(self.grade()['passed'])
        self.assertTrue(self.grade([r for r in evidence() if r['kind'] == 'sample'])['passed'])
        self.assertFalse(self.grade([r for r in evidence() if r['kind'] == 'activation'])['passed'])

    def test_per_action_restore_and_reborrow_is_rejected(self):
        rows = evidence()
        rows.extend([sample(40.1, 7, 'activation'), sample(40.2, 8, 'activation')])
        self.assertFalse(self.grade(sorted(rows, key=lambda r: r['unix_ms']))['passed'])
        rows = evidence()
        rows[20]['frontmost_pid'] = 7
        self.assertFalse(self.grade(rows)['passed'])

    def test_sampling_gap_is_diagnostic_without_failing_observed_behavior(self):
        rows = [r for r in evidence() if not (20 < r['unix_ms'] < 72 and r['kind'] == 'sample')]
        self.assertTrue(self.grade(rows)['passed'])
        self.assertGreater(self.grade(rows)['max_sample_gap_ms'], 50)
        self.assertTrue(self.grade(rows)['behavior_passed'])
        self.assertTrue(self.grade(rows)['sampling_quality']['passed'])
        self.assertEqual(self.grade(rows)['sampling_quality']['status'], 'usable')

    def test_unknown_foreign_and_observer_disagreement_fail(self):
        for fields in ({'frontmost_pid': None}, {'frontmost_pid': 99}, {'error': 'read failed'},
                       {'observed_frontmost_pid': 7}):
            rows = copy.deepcopy(evidence())
            rows[15].update(fields)
            self.assertFalse(self.grade(rows)['passed'], fields)

    def test_bracketing_and_order_mandatory(self):
        self.assertFalse(self.grade(evidence()[4:])['passed'])
        self.assertFalse(self.grade(evidence()[:-4])['passed'])
        rows = evidence()
        rows[10], rows[11] = rows[11], rows[10]
        self.assertFalse(self.grade(rows)['passed'])

    def test_external_choice_can_return_but_extra_restore_is_forbidden(self):
        rows = evidence(returned=True)
        self.assertTrue(self.grade(rows, case='external', returned=True)['passed'])
        self.assertFalse(self.grade(rows, case='external')['passed'])
        rows += [sample(96, 7, 'activation')]
        rows.sort(key=lambda r: r['unix_ms'])
        # A return over the external choice is inside the post-response bracket.
        self.assertFalse(evaluate_scope_focus(rows, 5, 97, 8, 7, case='external', returned=True)['passed'])

    def test_endpoints_without_target_samples_do_not_prove_borrow(self):
        rows = [r for r in evidence() if r['kind'] == 'activation' or r['frontmost_pid'] == 7]
        self.assertTrue(self.grade(rows)['passed'])
        self.assertTrue(self.grade(rows)['borrow_observed'])


class ScopeScriptTests(unittest.TestCase):
    def test_single_script_keeps_physical_typing_and_later_operations(self):
        source = scope_source({'x': 10, 'y': 20}, {'x': 30, 'y': 40})
        self.assertEqual(source.count("text:'Increment counter'"), 4)
        self.assertLess(source.index('app.move'), source.index('app.typeText'))
        suffix = source[source.index('app.typeText'):]
        for method in ('app.click', 'app.keypress', 'app.look', 'app.capture'):
            self.assertIn(method, suffix)
        self.assertIn(json.dumps(TEXT), source)

    def test_keyboard_trigger_uses_initial_editor_without_pointer_or_hover(self):
        source = scope_source({'x': 10, 'y': 20}, {'x': 30, 'y': 40}, trigger='keyboard')
        self.assertNotIn('app.move', source)
        self.assertNotIn('"x": 30', source)
        self.assertNotIn("app.keypress('META+A')", source)
        self.assertLess(source.index('app.typeText'), source.index("app.keypress('ESC')"))
        self.assertEqual(source.count("text:'Increment counter'"), 4)

    def test_external_return_keeps_exec_alive_before_suffix_and_reports_boundary(self):
        source = scope_source({}, {}, case='external', path=[], external_return=True)
        self.assertLess(source.index('while (Date.now() < until)'), source.index('await app.capture();'))
        self.assertLess(source.index('await app.capture();'), source.index('const suffix_unix_ms'))
        self.assertIn('return {denied,suffix_unix_ms}', source)
        self.assertEqual(source.count('catch (e)'), 5)
        for name in ('semantic', 'point', 'keyboard', 'text'):
            self.assertIn(json.dumps(name), source)

    def test_interruption_has_suffix_that_must_be_blocked(self):
        source = scope_source({}, {}, case='external', path=[{'x': i, 'y': 2} for i in range(64)])
        self.assertLess(source.index('app.typeText'), source.index('app.drag'))
        self.assertIn('app.click', source[source.index('app.drag'):])


class AsyncEvidenceTests(unittest.TestCase):
    def client(self, frame=None, error=None):
        client = Mock()
        client.metrics = []
        client.transport.receive.side_effect = error
        client.transport.receive.return_value = frame
        return client

    def test_no_reply_retains_unknown_and_error_metric(self):
        client = self.client(error=queue.Empty())
        row = async_outcome(client, 12, 'await app.drag();', timeout=.001)
        self.assertEqual(row['outcome'], 'no-response')
        self.assertTrue(row['metric']['is_error'])
        self.assertIsNone(row['metric']['provider_execution_id'])
        self.assertEqual(row['metric']['effects']['dispatch']['unknown'], 1)
        self.assertEqual(client.metrics, [row['metric']])

    def test_original_failure_frame_is_preserved(self):
        frame = {'id': 12, 'result': {'isError': True, 'structuredContent': {
            'status': 'completed', 'execution_id': 'owned-exec', 'error': {'code': 'focus_changed'},
            'effects': {'dispatch': {'unknown': 1}, 'interrupted': 1}}}}
        row = async_outcome(self.client(frame), 12, 'x')
        self.assertIs(row['frame'], frame)
        self.assertEqual(row['metric']['error_code'], 'focus_changed')
        self.assertTrue(row['metric']['is_error'])

    def test_caught_uncertainty_never_looks_clean_even_with_iserror_false(self):
        for effects in ({'dispatch': {'unknown': 1}}, {'interrupted': 1},
                        {'focus_preparation': {'dispatch': {'unknown': 1}}},
                        {'focus_preparation': {'interrupted': 1}}):
            frame = {'id': 12, 'result': {'isError': False, 'structuredContent': {
                'status': 'completed', 'effects': effects}}}
            row = async_outcome(self.client(frame), 12, 'x')
            self.assertTrue(row['metric']['is_error'], effects)
            self.assertIs(row['frame'], frame)

    def test_jsonrpc_error_never_looks_clean(self):
        frame = {'id': 12, 'error': {'code': -32000, 'message': 'cancelled'}}
        row = async_outcome(self.client(frame), 12, 'x')
        self.assertTrue(row['metric']['is_error'])
        self.assertIs(row['frame'], frame)

    def test_clean_reply_is_distinct_from_expected_interruption(self):
        frame = {'id': 12, 'result': {'structuredContent': {
            'status': 'completed', 'execution_id': 'e', 'effects': {}}}}
        self.assertFalse(async_outcome(self.client(frame), 12, 'x')['metric']['is_error'])


class PartialDragTests(unittest.TestCase):
    def rows(self, drags=3, up=True):
        return [{'phase': 'click'}] + [{'phase': 'drag', 'screen_x': i, 'screen_y': 2}
                                     for i in range(drags)] + ([{'phase': 'up', 'screen_x': 3, 'screen_y': 2}] if up else [])

    def test_real_partial_drag_with_release(self):
        self.assertEqual(_partial_drag(self.rows(), [63, 2], 63)['counts'], {'down': 1, 'drag': 3, 'up': 1})

    def test_complete_unpaired_and_terminal_paths_fail(self):
        for rows in (self.rows(drags=63), self.rows(up=False), self.rows(drags=0),
                     self.rows() + [{'phase': 'drag', 'screen_x': 63, 'screen_y': 2}]):
            with self.assertRaises(AssertionError):
                _partial_drag(rows, [63, 2], 63)

    def test_actor_invocation_uses_exact_owned_identity_and_retains_stdout(self):
        fixture = Mock()
        fixture.state.return_value = {'pid': 123, 'process_birth': 456}
        fixture.bundle = Path('/owned/Fixture.app')
        result = Mock(returncode=0, stdout='{"pid":123,"process_birth":456}\n', stderr='')
        with tempfile.TemporaryDirectory() as directory, patch('computer_use_scope.subprocess.run', return_value=result) as run:
            _activate(Path('/actor'), fixture, Path(directory), 'injection')
            self.assertEqual(run.call_args.args[0], ['/actor', '123', '456', '/owned/Fixture.app'])
            self.assertEqual(json.loads((Path(directory) / 'injection.json').read_text())['stdout'], result.stdout)


if __name__ == '__main__':
    unittest.main()
