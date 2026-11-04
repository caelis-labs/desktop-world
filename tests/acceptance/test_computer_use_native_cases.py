"""Pure boundary tests; never start an application or deliver GUI input."""
from pathlib import Path
import signal
import tempfile
import unittest
from unittest import mock

from computer_use_native_cases import NativeExtendedCases, ProcessObserver, _process_identity


class NativeExtendedCaseTests(unittest.TestCase):
    def harness(self):
        case = NativeExtendedCases()
        for name in ('run', 'capture', 'anchor', 'type_at', 'native', 'chat', 'state', 'events'):
            setattr(case, name, mock.Mock())
        case.fixture = mock.Mock()
        case.client = mock.Mock()
        case.app = Path('/owned/ComputerUse.app')
        return case

    def test_uncertain_panel_action_is_retained_without_replay(self):
        case = self.harness()
        data = {'failure': {'code': 'ax_action_failed'}, 'effects': {'dispatch': {'unknown': 1}}}
        case.client.execute.return_value = data, {'isError': True}
        receipt = case._native_attempt("await app.click('Save file…');")
        self.assertTrue(receipt['tool_error'])
        self.assertEqual(receipt['failure'], data['failure'])
        case.client.execute.assert_called_once()

    def test_known_rejection_is_not_misread_as_uncertain_delivery(self):
        case = self.harness()
        case.client.execute.return_value = ({'effects': {'dispatch': {'not_dispatched': 1}}}, {'isError': True})
        with self.assertRaises(AssertionError):
            case._native_attempt("await app.click('Save file…');")
        case.client.execute.assert_called_once()

    def test_background_hover_unsupported_does_not_double_click_or_activate(self):
        case = self.harness()
        case.state.return_value = {'mode': 'native', 'environment': {'frontmost_pid': 17}}
        case.native.return_value = {'gestures': {}}
        case.anchor.return_value = {'x': 20, 'y': 30}
        data = {'failure': {'code': 'foreground_required'}, 'effects': {'dispatch': {'not_dispatched': 1}}}
        case.client.execute.return_value = data, {'isError': True}
        detail = case.N13()
        self.assertIn('unavailable', detail)
        case.client.execute.assert_called_once_with(case.app, 'await app.move({"x": 20, "y": 30});', allow_error=True)
        self.assertEqual(case.run.call_args_list, [mock.call("await app.click('Native controls');")])

    def test_hover_unknown_delivery_never_becomes_unsupported(self):
        case = self.harness()
        case.state.return_value = {'mode': 'native', 'environment': {'frontmost_pid': 17}}
        case.native.return_value = {'gestures': {}}
        case.anchor.return_value = {'x': 20, 'y': 30}
        case.client.execute.return_value = ({'failure': {'code': 'foreground_required'},
                                            'effects': {'dispatch': {'unknown': 1}}}, {'isError': True})
        with self.assertRaises(AssertionError):
            case.N13()
        case.client.execute.assert_called_once()

    def test_n12_never_overwrites_an_existing_seed(self):
        case = self.harness()
        case.state.return_value = {'mode': 'native'}
        with tempfile.TemporaryDirectory() as directory:
            case.fixture.output = Path(directory)
            path = Path(directory) / 'owned-note.txt'
            path.write_bytes(b'user-owned existing content')
            with self.assertRaises(AssertionError):
                case.N12()
            self.assertEqual(path.read_bytes(), b'user-owned existing content')
        case.client.execute.assert_not_called()

    def test_pixel_menu_refuses_wrong_message_before_selecting_action(self):
        case = self.harness()
        case.chat.return_value = {'menu_open': True, 'selected_id': 'message-133'}
        case.anchor.return_value = {'x': 10, 'y': 20}
        with self.assertRaises(AssertionError):
            case._chat_menu('message-132', 'pin', 'older')
        self.assertEqual(case.run.call_count, 1)
        case.anchor.assert_called_once_with('chat-message-132', case.capture.return_value)

    def test_c04_rejects_neighbour_edit_even_with_expected_target_and_count(self):
        case = self.harness()
        case.state.return_value = {'mode': 'chat'}
        before = {'edited_messages': {}, 'edits': 0, 'replies': [], 'pins': 0}
        after = {**before, 'edits': 1, 'edited_messages': {'message-137': 'Release check 137 — edited 中文 café 🙂',
                                                       'message-136': 'unexpected'}}
        case.chat.side_effect = [before, {'editing_id': 'message-137'}, {'editing_id': 'message-137'}, after]
        case.events.return_value = []
        case.anchor.return_value = {'x': 1, 'y': 2}
        case._chat_menu = mock.Mock()
        with self.assertRaises(AssertionError):
            case.C04()

    def test_n14_moves_but_never_submits_an_unbound_old_point(self):
        case = self.harness()
        original = {'title': 'DTW Computer Use — test', 'bounds': [20, 20, 100, 100]}
        moved = {**original, 'bounds': [55, 20, 100, 100]}
        case.state.side_effect = [{'mode': 'native'}, {'windows': [original], 'native': {'counter': 0}}, {'windows': [moved]}]
        case.native.return_value = {'counter': 0}
        case.capture.return_value = {'image': {'width': 150, 'height': 100, 'bounds': [20, 20, 150, 100]}}
        detail = case.N14()
        self.assertIn('unavailable', detail)
        self.assertFalse(detail['stale_point_dispatched'])
        case.client.execute.assert_not_called()
        self.assertIn('app.drag', case.run.call_args[0][0])

    def test_restarted_process_absence_does_not_invent_an_exit_status(self):
        with mock.patch('computer_use_native_cases._process_identity', side_effect=[{'birth': 7, 'zombie': False}, None]):
            child = ProcessObserver(123, 7, mock.Mock(), Path('/owned/ComputerUse.app'))
            self.assertEqual(child.poll(), 'exit_observed')
            self.assertIsNone(child.wait(timeout=0))
            self.assertIsNone(child.returncode)
            self.assertTrue(child.cleanup_evidence()['exit_observed'])

    def test_restarted_pid_reuse_never_signals_the_replacement(self):
        client = mock.Mock()
        with mock.patch('computer_use_native_cases._process_identity', side_effect=[{'birth': 7, 'zombie': False}, {'birth': 8, 'zombie': False}]), \
                mock.patch('computer_use_native_cases.os.kill') as send_signal:
            child = ProcessObserver(123, 7, client, '/owned/ComputerUse.app')
            child.terminate()
            child.kill()
            self.assertEqual(child.cleanup_evidence()['exit_reason'], 'birth_changed')
        send_signal.assert_not_called()
        client.execute.assert_not_called()

    def test_restarted_forced_cleanup_distinguishes_signals_and_checks_each_birth(self):
        client = mock.Mock()
        with mock.patch('computer_use_native_cases._process_identity', return_value={'birth': 7, 'zombie': False}) as identity, \
                mock.patch('computer_use_native_cases.os.kill') as send_signal:
            child = ProcessObserver(123, 7, client, '/owned/ComputerUse.app')
            child.terminate()
            child.kill()
        self.assertEqual(identity.call_args_list, [mock.call(123)] * 3)
        self.assertEqual(send_signal.call_args_list, [mock.call(123, signal.SIGTERM), mock.call(123, signal.SIGKILL)])
        self.assertIsNone(child.returncode)
        client.execute.assert_not_called()

    def test_restarted_pid_reuse_between_term_and_kill_blocks_second_signal(self):
        with mock.patch('computer_use_native_cases._process_identity', side_effect=[
                {'birth': 7, 'zombie': False}, {'birth': 7, 'zombie': False}, {'birth': 8, 'zombie': False}]), \
                mock.patch('computer_use_native_cases.os.kill') as send_signal:
            child = ProcessObserver(123, 7, mock.Mock(), '/owned/ComputerUse.app')
            child.terminate()
            child.kill()
        send_signal.assert_called_once_with(123, signal.SIGTERM)
        self.assertTrue(child.exit_observed)
        self.assertEqual(child.exit_reason, 'birth_changed')

    def test_restarted_absent_process_never_receives_signal(self):
        with mock.patch('computer_use_native_cases._process_identity', side_effect=[{'birth': 7, 'zombie': False}, None]), \
                mock.patch('computer_use_native_cases.os.kill') as send_signal:
            child = ProcessObserver(123, 7, mock.Mock(), '/owned/ComputerUse.app')
            child.kill()
        send_signal.assert_not_called()
        self.assertEqual(child.exit_reason, 'absent')


if __name__ == '__main__':
    unittest.main()
