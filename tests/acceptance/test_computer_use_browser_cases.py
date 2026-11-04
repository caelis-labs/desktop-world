import copy
import hashlib
import signal
import plistlib
import shutil
import subprocess
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from computer_use_benchmark import CoreCases
from computer_use_browser_cases import BrowserExtendedCases, wait_for_image_answer
from chrome_isolation import ChromeIsolationError, IsolatedChrome, loopback_url


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

    def test_chrome_isolation_failure_blocks_without_public_input(self):
        with tempfile.TemporaryDirectory() as temporary:
            case = Cases(mock.Mock(web_url='http://127.0.0.1:123/', workbench_url='http://127.0.0.1:123/'),
                         mock.Mock(), Path(temporary))
            for identifier in ('B01', 'B04'):
                with mock.patch('computer_use_browser_cases.IsolatedChrome', side_effect=ChromeIsolationError('actual denial')):
                    result = getattr(case, identifier)()
                self.assertEqual(result['blocked'], 'actual denial')
                self.assertEqual(result['classification'], 'technical_isolation_failure')
            self.assertEqual(case.client.mock_calls, [])
            case.fixture.web_state.assert_not_called()

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


class ChromeIsolationTests(unittest.TestCase):
    def instance(self, root):
        source = root / 'Installed.app'
        source.mkdir()
        chrome = IsolatedChrome(root / 'output', 'http://127.0.0.1:123/', source=source)
        chrome.root = root / 'owned'
        chrome.root.mkdir()
        chrome.bundle = chrome.root / 'Chrome.app'
        chrome.bundle.mkdir()
        chrome.executable = chrome.bundle / 'Contents/MacOS/Google Chrome'
        chrome.child = mock.Mock(pid=71)
        chrome.child.poll.return_value = None
        chrome.birth = 1234
        chrome.evidence['argv'] = [str(chrome.executable), '--user-data-dir=' + str(chrome.root / 'profile')]
        chrome.observe = mock.Mock(return_value={'pid': 71, 'birth': 1234, 'stable': True,
            'argv': chrome.evidence['argv'],
            'executable': str(chrome.executable), 'bundle_path': str(chrome.bundle), 'instances': [71]})
        return chrome

    def test_only_loopback_fixture_urls_are_accepted(self):
        for url in ('https://127.0.0.1:1/', 'http://localhost:123/', 'http://example.org/',
                    'file:///tmp/a', 'http://user@127.0.0.1:123/', 'http://127.0.0.1/'):
            with self.subTest(url=url), self.assertRaises(AssertionError):
                loopback_url(url)
        self.assertEqual(loopback_url('http://127.0.0.1:123/'), 'http://127.0.0.1:123/')

    def test_nested_bundle_output_is_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(AssertionError):
                IsolatedChrome(Path(temporary) / 'Outer.app/output', 'http://127.0.0.1:123/')

    def test_independent_identity_refuses_handoff_duplicate_and_reused_pid(self):
        for change in ({'instances': [71, 72]}, {'instances': [72]}, {'birth': 999},
                       {'stable': False}, {'argv': ['original-profile']}, {'bundle_path': '/Applications/Google Chrome.app'},
                       {'executable': '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'}):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as temporary:
                chrome = self.instance(Path(temporary))
                chrome.observe.return_value.update(change)
                with self.assertRaises(AssertionError):
                    chrome.verify()

    def test_bundle_alias_resolves_to_owned_copy_and_preserves_raw_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            chrome = self.instance(Path(temporary).resolve())
            alias = Path(temporary) / 'bundle-alias.app'
            alias.symlink_to(chrome.bundle, target_is_directory=True)
            chrome.observe.return_value['bundle_path'] = str(alias)
            self.assertEqual(chrome.verify()['bundle_path'], str(alias))
            self.assertEqual(chrome.evidence['latest_observation']['bundle_path'], str(alias))

    def test_handoff_exit_is_refused_even_if_metadata_looks_owned(self):
        with tempfile.TemporaryDirectory() as temporary:
            chrome = self.instance(Path(temporary))
            chrome.child.poll.return_value = 0
            with self.assertRaises(AssertionError):
                chrome.verify()
            chrome.observe.assert_not_called()

    def test_copy_fallback_only_for_unsupported_clone_and_never_launches_original(self):
        for clone_error, expected_full in (('illegal option -- c', True),
                                           ('clonefile: Operation not supported', True),
                                           ('Operation not permitted', False)):
            with self.subTest(clone_error=clone_error), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = root / 'Installed.app'
                (source / 'Contents/MacOS').mkdir(parents=True)
                (source / 'Contents/MacOS/Google Chrome').write_text('fake test executable')
                (source / 'Contents/Info.plist').write_bytes(plistlib.dumps({'CFBundleExecutable': 'Google Chrome'}))
                chrome = IsolatedChrome(root / 'output', 'http://127.0.0.1:123/', source=source)
                calls = []
                def run(argv, **kwargs):
                    calls.append(argv)
                    if argv[0] == 'xcrun':
                        return subprocess.CompletedProcess(argv, 0, '', '')
                    if argv[1] == '-cR':
                        return subprocess.CompletedProcess(argv, 1, '', clone_error)
                    shutil.copytree(source, chrome.bundle)
                    return subprocess.CompletedProcess(argv, 0, '', '')
                with mock.patch('chrome_isolation.subprocess.run', side_effect=run), \
                     mock.patch('chrome_isolation.subprocess.Popen', side_effect=OSError('mock launch stop')) as launch:
                    with self.assertRaises(ChromeIsolationError):
                        chrome.start()
                chrome.close()
                copies = [c for c in calls if c[0] == '/bin/cp']
                self.assertEqual(len(copies), 2 if expected_full else 1)
                self.assertEqual(chrome.evidence['classification'], 'technical_isolation_failure')
                if expected_full:
                    argv = launch.call_args.args[0]
                    self.assertEqual(argv[0], str(chrome.bundle / 'Contents/MacOS/Google Chrome'))
                    self.assertIn('--user-data-dir=' + str(chrome.profile), argv)
                    self.assertEqual(argv[-1], 'http://127.0.0.1:123/')
                    self.assertFalse((chrome.bundle / 'Contents/MacOS/Google Chrome').samefile(source / 'Contents/MacOS/Google Chrome'))
                else:
                    launch.assert_not_called()

    def test_cleanup_signals_only_owned_pid_and_retains_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            chrome = self.instance(Path(temporary))
            chrome.child.poll.side_effect = [None, 0, 0]
            with mock.patch('chrome_isolation.os.kill') as kill:
                result = chrome.close()
            kill.assert_called_once_with(71, signal.SIGTERM)
            self.assertFalse(result['forced'])
            self.assertEqual(result['errors'], [])
            self.assertTrue(chrome.root.exists())
            self.assertEqual(chrome.close(), result)

    def test_cleanup_refuses_pid_reuse_and_preserves_error(self):
        with tempfile.TemporaryDirectory() as temporary:
            chrome = self.instance(Path(temporary))
            chrome.observe.return_value['birth'] = 999
            with mock.patch('chrome_isolation.os.kill') as kill:
                result = chrome.close()
            kill.assert_not_called()
            self.assertTrue(result['errors'])
            self.assertTrue(json.loads((chrome.root / 'isolation.json').read_text())['cleanup']['errors'])

    def test_forced_cleanup_remains_forced(self):
        with tempfile.TemporaryDirectory() as temporary:
            chrome = self.instance(Path(temporary))
            chrome.child.wait.side_effect = [subprocess.TimeoutExpired('chrome', 5), 0]
            chrome.child.poll.side_effect = [None, 0, 0]
            with mock.patch('chrome_isolation.os.kill') as kill:
                result = chrome.close()
            self.assertEqual(kill.call_args_list, [mock.call(71, signal.SIGTERM), mock.call(71, signal.SIGKILL)])
            self.assertTrue(result['forced'])


class ChromeBusinessTests(unittest.TestCase):
    def test_field_route_chosen_before_input_and_never_replayed(self):
        for pressable, unknown in ((True, False), (False, False), (False, True)):
            with self.subTest(pressable=pressable, unknown=unknown), tempfile.TemporaryDirectory() as temporary:
                case = Cases(mock.Mock(), mock.Mock(), Path(temporary))
                case._chrome_pointer = mock.Mock(return_value={'x': 30, 'y': 40})
                sources = []
                def execute(source):
                    sources.append(source)
                    if 'typeText' in source:
                        if unknown:
                            raise AssertionError('unknown input')
                        return {}, {}
                    return {'result': {'elements': [{'id': 'w1.e1', 'name': 'Web note', 'role': 'text_field',
                        'actions': ['press'] if pressable else ['setValue'], 'value': '中文🙂'}]}}, {}
                result = {'case': 'B01', 'receipts': []}
                if unknown:
                    with self.assertRaises(AssertionError):
                        case._chrome_type(mock.Mock(), execute, 'Web note', '中文🙂', result)
                    self.assertEqual(len(sources), 2)
                else:
                    case._chrome_type(mock.Mock(), execute, 'Web note', '中文🙂', result)
                typing = [s for s in sources if 'typeText' in s]
                self.assertEqual(len(typing), 1)
                self.assertIn('"target": "w1.e1"' if pressable else '"x": 30', typing[0])
                if pressable:
                    case._chrome_pointer.assert_not_called()
                else:
                    case._chrome_pointer.assert_called_once()

    def test_missing_field_semantics_uses_reviewed_pixels_once_and_defers_effect_proof(self):
        with tempfile.TemporaryDirectory() as temporary:
            case = Cases(mock.Mock(), mock.Mock(), Path(temporary))
            case._chrome_pointer = mock.Mock(return_value={'x': 30, 'y': 40})
            execute = mock.Mock(return_value=({'result': {'elements': []}}, {}))
            result = {'case': 'B01', 'receipts': []}
            case._chrome_type(mock.Mock(), execute, 'Web note', '中文🙂', result)
            typing = [call.args[0] for call in execute.call_args_list if 'typeText' in call.args[0]]
            self.assertEqual(len(typing), 1)
            self.assertIn('"x": 30', typing[0])
            case._chrome_pointer.assert_called_once()
            self.assertEqual(result['field_readbacks'][0]['verification'],
                             'deferred to exact independent business oracle')

    def test_b01_exact_unicode_and_checkpoint_once(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fixture = mock.Mock(web_url='http://127.0.0.1:123/')
            state = {'note': '', 'saves': [], 'checkpoint_hits': [], 'catalogue_loaded': False,
                     'records': [], 'catalogue_reviews': {}}
            fixture.web_state.side_effect = lambda: copy.deepcopy(state)
            case = Cases(fixture, mock.Mock(), root)
            case._chrome_type = mock.Mock()
            sources = []
            def execute(source, **kwargs):
                sources.append(source)
                if 'Commit web note' in source:
                    state['note'] = '基准 Web café العربية 🙂 — once'
                    state['saves'].append(state['note'])
                if 'Load noisy catalogue' in source:
                    state['catalogue_loaded'] = True
                if 'app.look' in source:
                    return {'result': {'elements': [{'name': 'Audit checkpoint 137', 'id': 'w1.e137'}]}}, {}
                if 'target:' in source:
                    state['checkpoint_hits'].append({'id': 137, 'activation': 1})
                return {}, {}
            case._chrome_case = lambda identifier, url, business: business(mock.Mock(), execute, {})
            result = case.B01()
            case._chrome_type.assert_called_once()
            self.assertEqual(case._chrome_type.call_args.args[1:4],
                             (execute, 'Web note', '基准 Web café العربية 🙂 — once'))
            self.assertEqual(result['checkpoint_hits_delta'], 1)
            self.assertEqual(sum('Commit web note' in source for source in sources), 1)
            self.assertEqual(state['records'], [])

    def test_b04_modes_original_oracle_single_attempts_and_trusted_events(self):
        for unknown in (False, True):
            with self.subTest(unknown=unknown), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                (root / 'workbench').mkdir()
                path = root / 'workbench/state.json'
                oracle = {'events': [], 'attempts': [], 'submissions': [], 'icon_activations': 0}
                def publish():
                    path.write_text(json.dumps({'reliability': oracle}))
                publish()
                fixture = mock.Mock(output=root, workbench_url='http://127.0.0.1:123/')
                case = Cases(fixture, mock.Mock(), root)
                case._chrome_type = mock.Mock()
                case._chrome_pointer = mock.Mock(return_value={'x': 20, 'y': 30})
                submissions = []
                def execute(source, **kwargs):
                    if 'Create test record' in source or '"x"' in source:
                        mode = 'normal' if not submissions else 'trusted-pointer'
                        label = 'B04-' + mode
                        submissions.append(source)
                        attempt = {'run': label, 'title': label + ' 中文 café', 'mode': mode, 'accepted': True,
                                   'duplicate': False, 'nonce': mode, 'gesture': {'down': True, 'up': True, 'click': True}}
                        oracle['attempts'].append(attempt)
                        oracle['submissions'].append(attempt)
                        for kind in ('pointerdown', 'pointerup', 'click'):
                            oracle['events'].append({'kind': kind, 'trusted': True, 'target': 'create', 'run': label})
                        oracle['events'].append({'kind': 'submit', 'trusted': True, 'target': 'form', 'run': label})
                        publish()
                        return {'status': 'failed' if unknown else 'completed', 'effects': {'dispatch': {
                            'unknown' if unknown else 'dispatched': 1}}}, {'isError': unknown}
                    return {'status': 'completed'}, {}
                case._chrome_case = lambda identifier, url, business: business(mock.Mock(), execute, {'modes': []})
                if unknown:
                    with self.assertRaises(AssertionError):
                        case.B04()
                    self.assertEqual(len(submissions), 1)
                    case._chrome_pointer.assert_not_called()
                else:
                    result = case.B04()
                    self.assertEqual(len(submissions), 2)
                    self.assertEqual([r['mode'] for r in result['modes']], ['normal', 'trusted-pointer'])
                    self.assertTrue(all(r['attempts_delta'] == r['submissions_delta'] == 1 for r in result['modes']))
                    self.assertIn('"x": 20', submissions[1])

    def test_pointer_answer_bound_to_png_and_current_geometry(self):
        for changed in (False, True):
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                fixture = mock.Mock()
                client = mock.Mock(output=root)
                case = Cases(fixture, client, root)
                chrome = mock.Mock(bundle=root / 'Chrome.app')
                view = {'image': {'bounds': [0, 0, 640, 480], 'width': 640, 'height': 480},
                        'windows': [{'id': 'w1', 'bounds': [0, 0, 640, 480]}]}
                def capture(app, name):
                    (root / (name + '.png')).write_bytes(b'png-test')
                    current = copy.deepcopy(view)
                    if changed and name.endswith('preflight'):
                        current['image']['bounds'][0] = 10
                    return {'view': current}
                client.capture.side_effect = capture
                case.chrome_pointer_point = lambda request: {
                    'capture_sha256': request['capture_sha256'], 'reviewer': 'unit-test fake',
                    'input_point': {'x': 100, 'y': 100}}
                if changed:
                    with self.assertRaises(AssertionError):
                        case._chrome_pointer(chrome, {})
                else:
                    self.assertEqual(case._chrome_pointer(chrome, {}), {'x': 100, 'y': 100})
                client.execute.assert_not_called()

    def test_pointer_default_file_flow_and_bounded_timeout(self):
        for answer_kind in ('file', 'timeout', 'wrong_hash'):
            with self.subTest(answer_kind=answer_kind), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                client = mock.Mock(output=root)
                case = Cases(mock.Mock(), client, root)
                case.chrome_pointer_timeout = 0.001 if answer_kind == 'timeout' else 180
                view = {'image': {'bounds': [0, 0, 640, 480], 'width': 640, 'height': 480}, 'windows': []}
                def capture(app, name):
                    (root / (name + '.png')).write_bytes(b'fake-test-png')
                    return {'view': view}
                client.capture.side_effect = capture
                def publish_answer(seconds):
                    if answer_kind == 'timeout':
                        return
                    request = json.loads((root / 'B04-pointer-request.json').read_text())
                    Path(request['answer_file']).write_text(json.dumps({
                        'capture_sha256': request['capture_sha256'] if answer_kind == 'file' else 'wrong',
                        'reviewer': 'fake unit reviewer', 'input_point': {'x': 100, 'y': 100}}))
                with mock.patch('computer_use_browser_cases.time.sleep', side_effect=publish_answer):
                    if answer_kind == 'wrong_hash':
                        with self.assertRaises(AssertionError):
                            case._chrome_pointer(mock.Mock(), {})
                    else:
                        point = case._chrome_pointer(mock.Mock(), {})
                        self.assertEqual(point, {'x': 100, 'y': 100} if answer_kind == 'file' else None)
                client.execute.assert_not_called()

    def test_public_target_and_isolation_check_before_every_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            chrome = mock.Mock(bundle=root / 'owned/Chrome.app', evidence={})
            chrome.close.return_value = {'errors': [], 'forced': False}
            case = Cases(mock.Mock(), mock.Mock(), root)
            case.client.execute.return_value = ({'status': 'completed', 'result': {
                'running': True, 'windows': [{'id': 'w1'}], 'coverage': {'instance_count': 1}}}, {})
            def business(instance, execute, result):
                execute('await app.look();')
                return {'business': True}
            with mock.patch('computer_use_browser_cases.IsolatedChrome', return_value=chrome):
                result = case._chrome_case('B01', 'http://127.0.0.1:123/', business)
            self.assertTrue(result['business'])
            self.assertEqual(chrome.verify.call_count, 2)
            self.assertTrue(all(call.args[0] == chrome.bundle for call in case.client.execute.call_args_list))
            chrome.close.assert_called_once()


if __name__ == '__main__':
    unittest.main()
