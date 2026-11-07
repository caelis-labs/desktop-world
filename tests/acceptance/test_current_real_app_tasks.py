"""Current task wiring, transcript failures and independent oracles, without GUI."""
import copy
import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

import current_real_app_tasks as runner
import current_task_metrics as metrics
import real_app_tasks as historical
from test_browser_workbench_probe import business_state, completed as legacy_completed

NAME = 'task-current'
APP = historical.APPS['chrome'][0]


def completed():
    return {'app': APP, 'execution_id': 'execution-current', 'status': 'completed',
            'view': {'kind': 'unchanged'}, 'result': {'tail_marker': '末尾 🐦'}}


def call(data, method='exec', arguments=None):
    return {'name': 'mcp__' + NAME + '__' + method, 'status': 'completed',
            'arguments': arguments if arguments is not None else {'app': APP, 'source': 'return await app.look();'},
            'output': {'structuredContent': data, 'content': [{'type': 'text', 'text': json.dumps(data)}]},
            'duration_seconds': 0.2}


class CurrentMetricsTests(unittest.TestCase):
    def measure(self, calls):
        return metrics.metrics(calls, NAME, allowed_apps=[APP])

    def test_current_catalog_compact_view_result_effects_and_new_methods(self):
        catalog = {'apps': [{'app': APP, 'name': 'Chrome', 'running': True}],
                   'next': None, 'provider': {}, 'coverage': {}}
        data = completed()
        data['effects'] = {'dispatch': {'dispatched': 1}, 'evidence': {'unconfirmed': 1}, 'interrupted': 0,
                           'focus_preparation': {'attempted': 1, 'dispatch': {'dispatched': 1}}}
        invocation = call(data, arguments={'app': APP, 'source': 'return await app.readText({target:"w1.e2",offset:240});'})
        result = self.measure([call(catalog, 'apps', {}), invocation])
        self.assertEqual(result['issues'], [])
        self.assertEqual((result['mcp_calls'], result['exec_calls']), (2, 1))
        self.assertEqual(result['mcp_call_seconds'], 0.4)
        self.assertIsNone(result['script_host_calls'])
        self.assertIsNone(result['duplicate_business_writes'])
        self.assertEqual(result['executions'][0]['effects'], data['effects'])
        self.assertEqual(result['executions'][0]['execution_id'], data['execution_id'])

    def test_legacy_payload_and_methods_never_pass_as_current(self):
        cases = [call(legacy_completed()), call(completed(), 'api'), call(completed(), 'result'),
                 call({'version': 1, 'apps': []}, 'apps', {}),
                 call(completed(), arguments={'app': APP, 'source': 'return 1;', 'timeout': 1})]
        for invocation in cases:
            with self.subTest(invocation=invocation):
                self.assertTrue(self.measure([invocation])['issues'])

    def test_failed_envelope_status_caught_failure_and_unknown_dispatch_are_not_success(self):
        bad_envelope = call(completed()); bad_envelope['output']['isError'] = True
        bad_tool = call(completed()); bad_tool['status'] = 'failed'
        bad_json = call(completed()); bad_json['output'] = 'truncated'
        failure = completed(); failure['failure'] = {'operation': 'click', 'error': {'code': 'stale'}}
        unknown = completed(); unknown['effects'] = {'dispatch': {'unknown': 1}}
        unsettled = completed(); unsettled['status'] = 'running'
        wrong_app = completed(); wrong_app['app'] = 'other'
        for invocation in [bad_envelope, bad_tool, bad_json, call(failure), call(unknown), call(unsettled), call(wrong_app)]:
            with self.subTest(invocation=invocation):
                result = self.measure([invocation])
                self.assertTrue(result['issues'])
                self.assertTrue(result['executions'][0]['failed'])
                recovery = metrics.recovery_metrics([invocation, call(completed())], NAME)
                self.assertEqual((recovery['failed_exec'], recovery['successful_exec'], recovery['followup_exec_after_failure']), (1, 1, 1))
                self.assertIn('not proof of replay', recovery['retry_measurement'])

    def test_focus_preparation_uncertainty_is_unclean_and_remains_separate(self):
        for preparation in ({'dispatch': {'unknown': 1}},
                            {'dispatch': {'dispatched': 1}, 'interrupted': 1}):
            with self.subTest(preparation=preparation):
                data = completed()
                data['effects'] = {'dispatch': {'dispatched': 1}, 'focus_preparation': preparation}
                result = self.measure([call(data)])
                self.assertTrue(result['issues'])
                self.assertTrue(result['executions'][0]['failed'])
                self.assertEqual(result['executions'][0]['effects']['dispatch'], {'dispatched': 1})
                self.assertEqual(result['executions'][0]['effects']['focus_preparation'], preparation)
                self.assertIsNone(result['duplicate_business_writes'])

    def test_wrapper_server_filter_and_missing_execution(self):
        invocation = call(completed())
        invocation.update(name='call_mcp_tool', arguments={'ServerName': NAME, 'ToolName': 'exec',
                          'Arguments': invocation['arguments']})
        self.assertEqual(self.measure([invocation])['issues'], [])
        invocation['arguments']['ServerName'] = 'other'
        result = self.measure([invocation])
        self.assertEqual(result['mcp_calls'], 0)
        self.assertTrue(result['issues'])

    def test_unavailable_image_and_missing_duration_remain_unknown(self):
        invocation = call(completed()); invocation['image_unavailable'] = 'not admitted'
        del invocation['duration_seconds']
        result = self.measure([invocation])
        self.assertIsNone(result['image_bytes'])
        self.assertIsNone(result['mcp_call_seconds'])
        self.assertEqual(result['issues'], [])


class CurrentRunnerTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_inventory_uses_full_look_and_task_result_not_legacy_windows(self):
        snapshot = {'task_windows': [{'title': 'Owned', 'title_truncated': False}],
                    'coverage': {'complete_within_scope': True, 'windows_truncated': False}}
        client = Mock()
        client.execute.return_value = ({'view': {'kind': 'unchanged'}, 'result': snapshot}, {})
        self.assertEqual(runner.inventory(client, APP)['windows'], snapshot['task_windows'])
        self.assertEqual(client.execute.call_args.args[0], APP)
        self.assertIn('app.look({full:true})', client.execute.call_args.args[1])
        for key in ('complete_within_scope', 'windows_truncated'):
            invalid = copy.deepcopy(snapshot); invalid['coverage'][key] = not invalid['coverage'][key]
            client.execute.return_value = ({'result': invalid}, {})
            with self.assertRaisesRegex(AssertionError, 'Incomplete'):
                runner.inventory(client, APP)

    def test_agent_uses_only_two_tools_and_preserves_legacy_issues_for_final_gate(self):
        evidence = {'metrics': {'issues': []}}
        args = (Path('.'), self.root, Path('/fake/dtw'), 'dsh', None, NAME, 'prompt', evidence)
        with patch.object(historical, 'run_agent') as launch:
            runner.run_agent(*args)
        self.assertEqual(launch.call_args.kwargs['tools'], ('apps', 'exec'))
        self.assertIs(launch.call_args.kwargs['measure'], metrics.metrics)
        evidence['metrics'] = metrics.metrics([call(legacy_completed())], NAME, allowed_apps=[APP])
        with patch.object(historical, 'run_agent'):
            runner.run_agent(*args)
        self.assertTrue(evidence['metrics']['issues'])

    def test_current_skill_api_read_is_allowed_but_task_file_read_is_rejected(self):
        api = self.root / '.agents/skills' / NAME / 'API.md'
        runner.check_routes([{'name': 'read', 'arguments': {'file_path': str(api)}}], NAME, self.root)
        with self.assertRaises(AssertionError):
            runner.check_routes([{'name': 'read', 'arguments': {'file_path': str(self.root / 'task.json')}}], NAME, self.root)

    def test_current_wps_retains_owned_files_without_dispatching_legacy_cleanup(self):
        output = self.root / 'wps'
        client = Mock()
        client.close.return_value = {'returncode': 0, 'forced': []}
        evidence = {}

        def attempt(*args, **kwargs):
            evidence.update(args[7])
            args[7]['metrics'] = {'issues': []}

        argv = ['current_real_app_tasks.py', '--dtw', '/fake/dtw', '--output', str(output), '--task', 'wps']
        with contextlib.ExitStack() as stack:
            stack.enter_context(patch.object(sys, 'argv', argv))
            stack.enter_context(patch.object(historical, 'prepare_runtime', return_value=(Path('/fake/dtw'), {})))
            stack.enter_context(patch.object(runner, 'initialize', return_value=client))
            stack.enter_context(patch.object(historical, 'launch_task', return_value='Owned task'))
            stack.enter_context(patch.object(historical, 'wait_until', return_value={'title': 'Owned task'}))
            stack.enter_context(patch.object(runner, 'run_agent', side_effect=attempt))
            stack.enter_context(patch.object(historical, 'independent_readback', return_value={'matches': True}))
            cleanup = stack.enter_context(patch.object(historical, 'close_visible_owned_document'))
            stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            self.assertEqual(runner.main(), 0)
        cleanup.assert_not_called()
        client.execute.assert_not_called()
        client.close.assert_called_once()
        evidence = json.loads((output / 'evidence.json').read_text())
        self.assertFalse(evidence['passed'])
        self.assertEqual(evidence['acceptance_status'], 'pending_gui_review')
        self.assertEqual(len(evidence['retained_documents']['paths']), 2)
        self.assertTrue(Path(evidence['retained_documents']['paths'][0]).is_file())

    def invoke(self, *, duplicate=False, recovered=False):
        output = self.root / ('duplicate' if duplicate else 'normal')
        client = Mock()
        client.close.return_value = {'returncode': 0, 'forced': []}
        client.execute.return_value = ({'result': {'task_windows': [
            {'title': 'Owned task', 'title_truncated': False}], 'coverage': {
                'complete_within_scope': True, 'windows_truncated': False}}}, {})

        def attempt(repo, root, binary, harness, adapter, name, prompt, evidence, **kwargs):
            self.assertEqual(harness, 'dsh')
            spec = json.loads((root / 'task.json').read_text())
            state = business_state(evidence['case'])
            state['uploads'] = [{'name': Path(spec['attachment']).name, 'sha256': spec['attachment_sha256']}]
            if duplicate:
                state['saves'].append(copy.deepcopy(state['saves'][0]))
            (root / 'web').mkdir()
            (root / 'web/state.json').write_text(json.dumps(state))
            invocation = call(completed()); invocation['name'] = 'mcp__' + name + '__exec'
            calls = [invocation]
            if recovered:
                failure = copy.deepcopy(invocation)
                failed_data = completed()
                failed_data.update(execution_id='execution-failed', error={'code': 'stale_target'})
                failure['output'] = call(failed_data)['output']
                calls.insert(0, failure)
            evidence['metrics'] = kwargs['measure'](calls, name, allowed_apps=[APP])
            evidence['recovery'] = kwargs['measure_recovery'](calls, name)

        argv = ['current_real_app_tasks.py', '--dtw', '/fake/dtw', '--output', str(output), '--task', 'chrome']
        with contextlib.ExitStack() as stack:
            stack.enter_context(patch.object(sys, 'argv', argv))
            stack.enter_context(patch.object(historical, 'prepare_runtime', return_value=(Path('/fake/dtw'), {})))
            stack.enter_context(patch.object(runner, 'initialize', return_value=client))
            stack.enter_context(patch.object(historical, 'launch_task', return_value='Owned task'))
            stack.enter_context(patch.object(historical, 'run_agent', side_effect=attempt))
            stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            code = runner.main()
        client.close.assert_called_once()
        return code, json.loads((output / 'evidence.json').read_text())

    def test_current_cli_reuses_oracle_and_keeps_gui_review_pending(self):
        code, evidence = self.invoke()
        self.assertEqual(code, 0)
        self.assertEqual(evidence['protocol'], 'apps/exec')
        self.assertTrue(evidence['business_passed'])
        self.assertTrue(evidence['automatic_checks_passed'])
        self.assertFalse(evidence['passed'])
        self.assertEqual(evidence['acceptance_status'], 'pending_gui_review')
        self.assertTrue(evidence['readback_after_cleanup']['records_and_save_journal_match'])

    def test_recovered_task_runs_independent_oracle_but_retains_unclean_automatic_result(self):
        code, evidence = self.invoke(recovered=True)
        self.assertEqual(code, 1)
        self.assertTrue(evidence['business_passed'])
        self.assertTrue(evidence['readback']['records_and_save_journal_match'])
        self.assertTrue(evidence['readback_after_cleanup']['records_and_save_journal_match'])
        self.assertFalse(evidence['clean_run'])
        self.assertFalse(evidence['automatic_checks_passed'])
        self.assertFalse(evidence['passed'])
        self.assertEqual(evidence['acceptance_status'], 'automatic_checks_failed')
        self.assertEqual(len(evidence['metrics']['issues']), 1)
        self.assertEqual([row['failed'] for row in evidence['metrics']['executions']], [True, False])
        self.assertEqual(evidence['recovery']['followup_exec_after_failure'], 1)
        self.assertEqual((evidence['recovery']['failed_exec'], evidence['recovery']['successful_exec']), (1, 1))

    def test_duplicate_write_journal_fails_current_cli_with_original_oracle(self):
        code, evidence = self.invoke(duplicate=True)
        self.assertEqual(code, 1)
        self.assertFalse(evidence['automatic_checks_passed'])
        self.assertFalse(evidence['business_passed'])
        self.assertIn('two independent saves', evidence['failure']['message'])


if __name__ == '__main__':
    unittest.main()
