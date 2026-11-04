import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from computer_use_benchmark import CoreCases
from computer_use_browser_cases import BrowserExtendedCases, wait_for_image_answer


class Cases(BrowserExtendedCases, CoreCases):
    pass


class BrowserCaseTests(unittest.TestCase):
    def make_case(self, output, *, correct=True, unknown=False, field_actions=None, readback='ABCD2345'):
        fixture = mock.Mock()
        client = mock.Mock(output=output)
        case = Cases(fixture, client, output)
        case.web_ready = mock.Mock()
        case.state = mock.Mock(return_value={'web': {'loaded': True, 'url': 'http://127.0.0.1:123/vision'}})
        submitted = []

        def execute(app, source, **kwargs):
            if 'app.look' in source:
                self.assertEqual(submitted, [], 'Readback must precede verification')
                fixture.web_state.assert_not_called()
                return {'status': 'completed', 'result': {'elements': [
                    {'role': 'text_field', 'name': 'Image code', 'id': 'w1.e59', 'value': readback}]}}, {}
            if 'Verify image' in source:
                submitted.append(source)
                if unknown:
                    return ({'failure': {'code': 'ax_action_failed'},
                             'effects': {'dispatch': {'unknown': 1}}}, {'isError': True})
            return {'status': 'completed', 'effects': {'dispatch': {'dispatched': 1}}}, {}

        client.execute.side_effect = execute

        def capture(app, name):
            (output / (name + '.png')).write_bytes(b'\x89PNG\r\n\x1a\nunit-test-capture')
            return {'view': {'image': {'width': 640, 'height': 160, 'bounds': [0, 0, 640, 160]},
                             'elements': [{'role': 'text_field', 'name': 'Image code', 'id': 'w1.e59',
                                           'actions': field_actions or []}]}}

        client.capture.side_effect = capture

        def oracle():
            self.assertEqual(len(submitted), 1, 'Private oracle read before exactly one submission')
            return {'vision': {'expected_token': 'ABCD2345' if correct else 'XXXXXXXX',
                               'attempts': [{'submitted': 'ABCD2345', 'passed': correct}]}}

        fixture.web_state.side_effect = oracle
        return case, submitted

    def answer(self, request, *, timeout):
        self.assertTrue(Path(request['capture']).is_file())
        self.assertEqual(request['capture_sha256'], hashlib.sha256(Path(request['capture']).read_bytes()).hexdigest())
        answer = {'capture_sha256': request['capture_sha256'], 'answer': 'ABCD2345',
                  'reviewer': 'unit-test fake reviewer', 'notes': 'not actual model evidence',
                  'input_point': {'x': 200, 'y': 120}}
        Path(request['answer_file']).write_text(json.dumps(answer))
        return wait_for_image_answer(request, timeout=timeout)

    def test_chrome_cases_block_without_browser_contact(self):
        case = Cases(mock.Mock(), mock.Mock(), Path('/unused'))
        for identifier in ('B01', 'B04'):
            result = getattr(case, identifier)()
            self.assertIn('blocked', result)
            self.assertIn('binding', result['blocked'])
        case.client.assert_not_called()
        self.assertEqual(case.client.mock_calls, [])
        self.assertEqual(case.fixture.mock_calls, [])

    def test_external_handoff_precedes_single_submit_and_private_oracle(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            case, submitted = self.make_case(root)

            def answer(request, **kwargs):
                case.fixture.web_state.assert_not_called()
                self.assertEqual(submitted, [])
                self.assertEqual(json.loads((root / 'B05-answer-request.json').read_text()), request)
                return self.answer(request, **kwargs)

            with mock.patch('computer_use_browser_cases.wait_for_image_answer', side_effect=answer):
                result = case.B05()
            self.assertEqual(len(submitted), 1)
            self.assertEqual(result['attempts'], [{'submitted': 'ABCD2345', 'passed': True}])
            case.client.capture.assert_called_once_with(case.app, 'B05-challenge')
            self.assertNotIn('expected_token', (root / 'B05-answer-request.json').read_text())
            self.assertTrue((root / 'B05-result.json').is_file())
            self.assertNotIn('blocked', result)

    def test_pressable_capture_field_uses_id_before_typing(self):
        with tempfile.TemporaryDirectory() as temporary:
            case, submitted = self.make_case(Path(temporary), field_actions=['press', 'setValue'])
            with mock.patch('computer_use_browser_cases.wait_for_image_answer', side_effect=self.answer):
                result = case.B05()
            sources = [call.args[1] for call in case.client.execute.call_args_list]
            typing = next(source for source in sources if 'typeText' in source)
            self.assertTrue(typing.startswith('await app.click({"target": "w1.e59"});'))
            self.assertEqual(result['input_route']['kind'], 'semantic_press')
            self.assertTrue(result['readback']['matched'])
            self.assertEqual(len(submitted), 1)

    def test_field_without_press_uses_model_point(self):
        with tempfile.TemporaryDirectory() as temporary:
            case, _ = self.make_case(Path(temporary), field_actions=['setValue'])
            with mock.patch('computer_use_browser_cases.wait_for_image_answer', side_effect=self.answer):
                result = case.B05()
            typing = next(call.args[1] for call in case.client.execute.call_args_list if 'typeText' in call.args[1])
            self.assertTrue(typing.startswith('await app.click({"x": 200, "y": 120});'))
            self.assertEqual(result['input_route']['kind'], 'model_point')

    def test_readback_mismatch_preserves_failure_without_submit_or_oracle(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            case, submitted = self.make_case(root, field_actions=['press', 'setValue'], readback='')
            with mock.patch('computer_use_browser_cases.wait_for_image_answer', side_effect=self.answer):
                with self.assertRaises(AssertionError):
                    case.B05()
            self.assertEqual(submitted, [])
            case.fixture.web_state.assert_not_called()
            result = json.loads((root / 'B05-result.json').read_text())
            self.assertFalse(result['readback']['matched'])
            self.assertFalse(result['submitted'])
            self.assertEqual(result['input_route']['target'], 'w1.e59')

    def test_unknown_field_input_never_falls_back_or_submits(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            case, submitted = self.make_case(root, field_actions=['press'])
            original = case.client.execute.side_effect

            def execute(app, source, **kwargs):
                if 'typeText' in source:
                    raise AssertionError({'effects': {'dispatch': {'unknown': 1}}})
                return original(app, source, **kwargs)

            case.client.execute.side_effect = execute
            with mock.patch('computer_use_browser_cases.wait_for_image_answer', side_effect=self.answer):
                with self.assertRaises(AssertionError):
                    case.B05()
            self.assertEqual(sum('typeText' in call.args[1] for call in case.client.execute.call_args_list), 1)
            self.assertEqual(submitted, [])
            case.fixture.web_state.assert_not_called()
            self.assertEqual(json.loads((root / 'B05-result.json').read_text())['input_route']['kind'],
                             'semantic_press')

    def test_missing_answer_blocks_without_input_or_oracle(self):
        with tempfile.TemporaryDirectory() as temporary:
            case, submitted = self.make_case(Path(temporary))
            case.vision_answer_timeout = 0.001
            result = case.B05()
            self.assertIn('blocked', result)
            self.assertFalse(result['submitted'])
            self.assertEqual(submitted, [])
            case.fixture.web_state.assert_not_called()
            sources = [call.args[1] for call in case.client.execute.call_args_list]
            self.assertFalse(any('typeText' in source for source in sources))

    def test_wrong_image_answer_is_retained_as_failed_once(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            case, submitted = self.make_case(root, correct=False)
            with mock.patch('computer_use_browser_cases.wait_for_image_answer', side_effect=self.answer):
                with self.assertRaises(AssertionError):
                    case.B05()
            self.assertEqual(len(submitted), 1)
            self.assertFalse(json.loads((root / 'B05-result.json').read_text())['attempts'][0]['passed'])
            self.assertTrue((root / 'B05-answer.json').is_file())

    def test_unknown_submission_is_observed_but_never_replayed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            case, submitted = self.make_case(root, unknown=True)
            with mock.patch('computer_use_browser_cases.wait_for_image_answer', side_effect=self.answer):
                with self.assertRaises(AssertionError):
                    case.B05()
            self.assertEqual(len(submitted), 1)
            result = json.loads((root / 'B05-result.json').read_text())
            self.assertTrue(result['tool_error'])
            self.assertEqual(result['submission_receipt']['effects']['dispatch']['unknown'], 1)
            case.fixture.web_state.assert_called_once()

    def test_undispatched_submit_cannot_read_private_oracle(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            case, _ = self.make_case(root)
            original = case.client.execute.side_effect

            def execute(app, source, **kwargs):
                if 'Verify image' in source:
                    return ({'error': {'code': 'foreground_required'},
                             'effects': {'dispatch': {'not_dispatched': 1}}}, {'isError': True})
                return original(app, source, **kwargs)

            case.client.execute.side_effect = execute
            with mock.patch('computer_use_browser_cases.wait_for_image_answer', side_effect=self.answer):
                with self.assertRaises(AssertionError):
                    case.B05()
            case.fixture.web_state.assert_not_called()
            self.assertEqual(sum('Verify image' in call.args[1]
                                 for call in case.client.execute.call_args_list), 1)

    def test_existing_handoff_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            case, _ = self.make_case(root)
            path = root / 'B05-answer.json'
            path.write_text('original attempt')
            with self.assertRaises(AssertionError):
                case.B05()
            self.assertEqual(path.read_text(), 'original attempt')
            self.assertEqual(case.client.mock_calls, [])

    def test_invalid_or_mismatched_answer_cannot_reach_input(self):
        for change in ({'capture_sha256': 'different'}, {'answer': 'bad'}, {'reviewer': ''},
                       {'input_point': {'x': -1, 'y': 0}}, {'input_point': {'x': 640, 'y': 0}},
                       {'input_point': {'x': float('nan'), 'y': 0}}, {'input_point': None}):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                request = {'answer_file': str(root / 'answer.json'), 'capture_sha256': 'capture',
                           'view': {'image': {'width': 640, 'height': 160}}}
                answer = {'capture_sha256': 'capture', 'answer': 'ABCD2345', 'reviewer': 'test',
                          'input_point': {'x': 200, 'y': 120}, **change}
                Path(request['answer_file']).write_text(json.dumps(answer))
                with self.assertRaises(AssertionError):
                    wait_for_image_answer(request, timeout=0.1)

    def test_invalid_timeout_rejects_before_gui(self):
        for timeout in (0, -1, 601, float('nan'), float('inf'), True):
            with self.subTest(timeout=timeout), tempfile.TemporaryDirectory() as temporary:
                case, _ = self.make_case(Path(temporary))
                case.vision_answer_timeout = timeout
                with self.assertRaises(AssertionError):
                    case.B05()
                self.assertEqual(case.client.mock_calls, [])

    def test_changed_capture_rejects_before_typing(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            case, submitted = self.make_case(root)

            def answer(request, **kwargs):
                result = self.answer(request, **kwargs)
                Path(request['capture']).write_bytes(b'changed')
                return result

            with mock.patch('computer_use_browser_cases.wait_for_image_answer', side_effect=answer):
                with self.assertRaises(AssertionError):
                    case.B05()
            self.assertEqual(submitted, [])
            case.fixture.web_state.assert_not_called()
            self.assertFalse(any('typeText' in call.args[1] for call in case.client.execute.call_args_list))


if __name__ == '__main__':
    unittest.main()
