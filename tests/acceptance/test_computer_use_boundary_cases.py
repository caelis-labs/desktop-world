"""Focused non-GUI checks for boundary grading and transport ownership."""
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from computer_use_boundary_cases import BoundaryCases, gesture_counts, owned_workers


class Harness(BoundaryCases):
    def __init__(self, output):
        self.output = Path(output)
        self.app = '/owned/ComputerUse.app'
        self.fixture = SimpleNamespace(case='unit', child=SimpleNamespace(poll=lambda: None))
        self.focus_observer = None
        self.rows = [{'kind': 'gesture', 'phase': 'click'},
                     {'kind': 'gesture', 'phase': 'drag', 'screen_x': 101, 'screen_y': 100}]
        self._drag_terminal = [163, 100]
        self._drag_full_samples = 63
        self.old = SimpleNamespace(metrics=[{'is_error': True, 'tool': 'exec'}],
            transport=SimpleNamespace(child=SimpleNamespace(pid=40, args=['/owned/dtw'], poll=lambda: 0)),
            close=Mock(return_value={'returncode': 0, 'forced': [], 'native_workers_reaped': True}))
        self.client = self.old
        self._begin_held_drag = Mock(return_value=(7, 0, {'down': 1, 'drag': 1, 'up': 0}))
        self.old.cancel = Mock(side_effect=lambda request: self.rows.append({'kind': 'gesture', 'phase': 'up', 'screen_x': 101, 'screen_y': 100}))
        self.run = Mock(return_value={'status': 'completed', 'result': {'elements': []}})
        self._cleanup_read = Mock(side_effect=lambda *args: (self.run('await app.look();'),
                                  {'async_response': {'outcome': 'no-response'}}))

    def events(self, kind):
        assert kind == 'gesture'
        return self.rows

    def state(self):
        return {'pressed_mouse_buttons': 0}


class BoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.cases = Harness(self.temporary.name)

    def test_journal_phase_not_kind_counts_actual_paired_events(self):
        rows = [{'kind': 'gesture', 'phase': phase} for phase in
                ('hover', 'click', 'drag', 'drag', 'up', 'double_click', 'up', 'right_click')]
        self.assertEqual(gesture_counts(rows), {'down': 2, 'drag': 2, 'up': 2})
        self.assertEqual(gesture_counts([{'kind': 'click'}]), {'down': 0, 'drag': 0, 'up': 0})

    def test_cancel_uses_original_mcp_id_then_fresh_read_without_input_replay(self):
        result = self.cases.R01()
        self.cases.old.cancel.assert_called_once_with(7)
        self.assertEqual(self.cases._begin_held_drag.call_count, 1)
        self.assertEqual(self.cases.run.call_count, 1)
        self.assertIn('app.look()', self.cases.run.call_args.args[0])
        self.assertEqual(result['events'], {'down': 1, 'drag': 1, 'up': 1})
        self.assertFalse(result['input_replayed'])

    def test_completed_drag_cannot_be_used_as_held_interruption_evidence(self):
        del self.cases._begin_held_drag
        self.cases._boundary_native = Mock()
        self.cases.capture = Mock(return_value={'image': {'bounds': [0, 0, 800, 600], 'width': 800, 'height': 600}})
        self.cases.anchor = Mock(return_value={'x': 100, 'y': 100})
        self.cases.state = Mock(return_value={'anchors': {'gesture-desk': [100, 100]}})
        self.cases.client.begin_execute = Mock(return_value=8)
        self.cases.events = Mock(side_effect=[[], [{'phase': 'click'}, {'phase': 'drag'}, {'phase': 'up'}]])
        with self.assertRaisesRegex(AssertionError, 'Drag completed before interruption'):
            self.cases._begin_held_drag('finished-before-cancel')
        self.cases.client.begin_execute.assert_called_once()
        self.cases.client.cancel.assert_not_called()

    def test_natural_completion_is_rejected_even_with_one_paired_release(self):
        self.cases.old.cancel.side_effect = lambda request: self.cases.rows.append(
            {'phase': 'up', 'screen_x': 163, 'screen_y': 100})
        with self.assertRaisesRegex(AssertionError, 'Terminal sentinel reached'):
            self.cases.R01()

    def test_full_sample_count_cannot_prove_interruption(self):
        self.cases.rows += [{'phase': 'drag', 'screen_x': 102, 'screen_y': 100}] * 62
        with self.assertRaisesRegex(AssertionError, 'No partial-path interruption'):
            self.cases.R01()

    def test_cleanup_boundary_keeps_missing_response_distinct_from_success(self):
        del self.cases._cleanup_read
        transport = SimpleNamespace(output=self.cases.output, serial=8)
        self.cases.client.transport = transport
        transcript = self.cases.output / 'mcp.jsonl'
        transcript.write_text('')
        recovery, boundary = self.cases._cleanup_read(transport, 7, 'eof')
        self.assertEqual(boundary['async_response']['outcome'], 'no-response')
        self.assertEqual(boundary['cleanup_read']['request_id'], 9)
        self.assertEqual(boundary['cleanup_read']['status'], 'completed')
        metric = self.cases.client.metrics[-1]
        self.assertTrue(metric['is_error'])
        self.assertEqual(metric['error_code'], 'async_outcome_unknown')
        self.assertEqual(metric['effects']['dispatch']['unknown'], 1)
        self.assertIn(':7', metric['execution_id'])
        self.cases._cleanup_read(transport, 7, 'eof')
        self.assertEqual(sum(row.get('async_request_id') == 7 for row in self.cases.client.metrics), 1)
        self.assertTrue((self.cases.output / 'boundary-interruptions.jsonl').is_file())
        frame = {'direction': 'response', 'message': {'id': 7, 'result': {'isError': True}}}
        transcript.write_text(json.dumps(frame) + '\n')
        _, boundary = self.cases._cleanup_read(transport, 7, 'cancel')
        self.assertEqual(boundary['async_response']['frame'], frame)
        self.cases.client.metrics.clear()
        frame['message']['result'] = {'structuredContent': {'execution_id': 'original-async', 'status': 'completed',
            'effects': {'dispatch': {'unknown': 1}, 'interrupted': 1}, 'failure': {'code': 'cancelled'}}}
        transcript.write_text(json.dumps(frame) + '\n')
        self.cases._cleanup_read(transport, 7, 'cancel')
        self.assertEqual(len(self.cases.client.metrics), 1)
        metric = self.cases.client.metrics[0]
        self.assertTrue(metric['is_error'])
        self.assertEqual(metric['execution_id'], 'original-async')
        self.assertEqual(metric['error_code'], 'cancelled')
        self.assertEqual(metric['effects']['dispatch']['unknown'], 1)

    def test_duplicate_release_fails_instead_of_passing_recovery(self):
        self.cases.rows.append({'phase': 'up'})
        with self.assertRaises(AssertionError):
            self.cases.R01()

    def test_eof_requires_active_worker_reaps_then_replaces_client(self):
        table = {51: (40, '/owned/dtw --dtw-native-worker'), 52: (40, '/owned/dtw --dtw-scene-worker')}
        def close():
            self.cases.rows.append({'phase': 'up', 'screen_x': 101, 'screen_y': 100})
            return {'returncode': 0, 'forced': [], 'native_workers_reaped': True}
        self.cases.old.close.side_effect = close
        replacement = SimpleNamespace(metrics=[])
        with patch('computer_use_boundary_cases.process_table', side_effect=[table, {}]), \
                patch('computer_use_boundary_cases.AppClient', return_value=replacement) as constructor:
            result = self.cases.R02()
        self.assertIs(self.cases.client, replacement)
        self.assertEqual(replacement.metrics, self.cases.old.metrics)
        self.cases.old.close.assert_called_once_with()
        constructor.assert_called_once_with(Path('/owned/dtw'), self.cases.output / 'mcp-after-eof')
        self.assertEqual(set(result['workers']), {51, 52})
        self.assertEqual(self.cases._begin_held_drag.call_count, 1)

    def test_forced_eof_cleanup_cannot_pass_or_start_recovery(self):
        self.cases.old.close.return_value['forced'] = ['terminate']
        with patch('computer_use_boundary_cases.process_table', return_value={51: (40, '--dtw-native-worker')}), \
                patch('computer_use_boundary_cases.AppClient') as constructor:
            with self.assertRaises(AssertionError):
                self.cases.R02()
        constructor.assert_not_called()
        self.cases.run.assert_not_called()

    def test_workers_are_exact_parent_and_native_role_only(self):
        self.assertEqual(owned_workers({1: (40, '--dtw-native-worker'), 2: (99, '--dtw-native-worker'),
            3: (40, 'unrelated'), 4: (40, '--dtw-scene-worker')}, 40),
            {1: '--dtw-native-worker', 4: '--dtw-scene-worker'})

    def test_unestablished_conditions_remain_blocked(self):
        for name in ('R04', 'S02', 'S03'):
            with self.subTest(name=name):
                self.assertTrue(getattr(self.cases, name)()['blocked'])
        self.cases.run.assert_not_called()

    def test_accessory_actual_no_window_stays_unsupported_and_retains_error(self):
        state = {'pid': 99, 'mode': 'accessory', 'windows': [],
                 'accessory': {'status_item': True, 'window_count': 0, 'counter': 0}}
        self.cases.state = Mock(return_value=state)
        self.cases.client = SimpleNamespace(execute=Mock(return_value=(
            {'execution_id': 'accessory-read', 'failure': {'code': 'no_window'}}, {'isError': True})))
        result = self.cases.S02()
        self.assertTrue(result['unavailable'])
        self.assertEqual(result['expected_tool_errors'], ['accessory-read'])
        self.cases.client.execute.assert_called_once_with(self.cases.app, 'await app.look();', allow_error=True)

    def test_sibling_uses_old_canvas_and_rejects_without_focus_or_business_change(self):
        native = {'counter': 0, 'note': 'initial', 'saved_note': '', 'saves': 0, 'audit_count': 0,
                  'slider': 25, 'reviewed': False, 'selection': 'Draft', 'gestures': {}}
        target = {'pid': 40, 'windows': [{'bounds': [20, 30, 800, 600]}],
                  'anchors': {'gesture-desk': [300, 250]}, 'native': native}
        class Fixture:
            def __init__(self, output, case):
                self.output, self.case = output, case
                self.bundle = Path('/owned/ComputerUse.app')
                self.executable = self.bundle / 'Contents/MacOS/ComputerUse'
            def prepare(self, **kwargs):
                self.output.mkdir()
                (self.output / 'config.json').write_text('{}')
            def state(self):
                return {**target, 'pid': 50, 'ready': True}
            def close(self):
                if hasattr(self, 'log'):
                    self.log.close()
                return {'passed': True}
        self.cases.fixture = Fixture(self.cases.output, 'unit')
        self.cases._boundary_native = Mock()
        self.cases.capture = Mock(return_value={'image': {}})
        self.cases.anchor = Mock(return_value={'x': 300, 'y': 250})
        self.cases.state = Mock(return_value=target)
        self.cases.native = Mock(return_value=native)
        self.cases.client = SimpleNamespace(execute=Mock(return_value=(
            {'execution_id': 'sibling-reject', 'failure': {'code': 'stale_scene'},
             'effects': {'dispatch': {'not_dispatched': 1}}}, {'isError': True})))
        with patch('computer_use_boundary_cases.subprocess.Popen') as launch:
            result = self.cases.R03()
        self.assertTrue(result['sibling_rejected'])
        self.assertTrue(result['unavailable'])  # foreign subtarget still unmet
        self.assertEqual(result['expected_tool_errors'], ['sibling-reject'])
        self.assertEqual(self.cases.capture.call_count, 1)
        self.cases.run.assert_not_called()
        self.assertEqual(launch.call_args.args[0], [str(self.cases.fixture.executable), '--fixture-root',
            str(self.cases.output / 'boundary-sibling')])
        self.assertTrue(result['cleanup']['passed'])
        self.cases.output = self.cases.output / 'look-required'
        self.cases.output.mkdir()
        self.cases.client.execute.return_value = (
            {'execution_id': 'public-reject', 'error': {'code': 'look_required'}}, {'isError': True})
        with patch('computer_use_boundary_cases.subprocess.Popen'):
            unsupported = self.cases.R03()
        self.assertIn('lacks dispatch', unsupported['unavailable'])
        self.assertFalse(unsupported['sibling_rejection_verified'])
        self.assertTrue(unsupported['both_business_states_unchanged'])
        self.assertNotIn('expected_tool_errors', unsupported)

    def test_background_uses_default_auto_and_requires_proof(self):
        self.cases.focus_observer = Path('/owned/observer')
        self.cases._boundary_native = Mock()
        with patch('computer_use_focus.run_background_contract', return_value={'passed': True}) as contract:
            self.cases.R04()
        self.assertEqual(contract.call_args.kwargs['input_policy'], 'auto')
        with patch('computer_use_focus.run_background_contract', return_value={'passed': False}):
            with self.assertRaises(AssertionError):
                self.cases.R04()

    def test_dock_current_no_window_is_unsupported_without_arbitrary_fallback(self):
        self.cases.client = SimpleNamespace(tool=Mock(return_value=({'apps': [{'app': 'com.apple.dock'}]}, {})),
            execute=Mock(return_value=({'execution_id': 'dock-read', 'failure': {'code': 'no_window'}}, {'isError': True})))
        with patch('computer_use_boundary_cases.sys.platform', 'darwin'):
            result = self.cases.S01()
        self.assertIn('no_window', result['unavailable'])
        self.cases.client.execute.assert_called_once_with('com.apple.dock', 'await app.look();', allow_error=True)

    def test_dock_unexpected_error_fails(self):
        self.cases.client = SimpleNamespace(tool=Mock(return_value=({'apps': [{'app': 'com.apple.dock'}]}, {})),
            execute=Mock(return_value=({'failure': {'code': 'backend_failed'}}, {'isError': True})))
        with patch('computer_use_boundary_cases.sys.platform', 'darwin'), self.assertRaises(AssertionError):
            self.cases.S01()

    def test_actual_display_inventory_is_retained_but_cannot_prove_conditions(self):
        actual = {'SPDisplaysDataType': [{'spdisplays_ndrvs': [{'name': 'actual display'}]}]}
        with patch('computer_use_boundary_cases.sys.platform', 'darwin'), \
                patch('computer_use_boundary_cases.subprocess.run', return_value=SimpleNamespace(
                    returncode=0, stdout=json.dumps(actual), stderr='')) as command:
            result = self.cases.S04()
        self.assertTrue(result['blocked'])
        self.assertEqual(result['actual_display_inventory']['data'], actual)
        self.assertTrue((self.cases.output / result['artifact']).is_file())
        self.assertEqual(command.call_args.args[0], ['system_profiler', 'SPDisplaysDataType', '-json'])


if __name__ == '__main__':
    unittest.main()
