"""Business oracles and failure accounting; no native apps or Agents launched."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch
import zipfile
from xml.sax.saxutils import escape

import real_app_tasks as tasks
from test_browser_workbench_probe import completed, call, NAME, business_state


class TaskEvidenceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def write_docx(self, path, paragraphs):
        with zipfile.ZipFile(path, 'w') as archive:
            content = '<w:document xmlns:w="'+tasks.W[1:-1]+'"><w:body>'
            content += ''.join('<w:p><w:r><w:t>'+escape(p)+'</w:t></w:r></w:p>' for p in paragraphs)
            archive.writestr('word/document.xml', content+'</w:body></w:document>')

    def test_wps_requires_new_document_correct_paragraphs_and_unchanged_seed(self):
        spec = tasks.task_spec('wps', self.root, 'case-unit')
        with self.assertRaisesRegex(AssertionError, 'not saved'):
            tasks.independent_readback('wps', spec, self.root, 'case-unit')
        self.write_docx(spec['output'], spec['paragraphs'])
        self.assertTrue(tasks.independent_readback('wps', spec, self.root, 'case-unit')['seed_unchanged'])
        self.write_docx(spec['output'], [*spec['paragraphs'][:-1], '需要修改：旧内容'])
        with self.assertRaisesRegex(AssertionError, 'paragraphs differ'):
            tasks.independent_readback('wps', spec, self.root, 'case-unit')
        self.write_docx(spec['output'], spec['paragraphs'])
        Path(spec['seed']).write_bytes(b'changed')
        with self.assertRaisesRegex(AssertionError, 'Starting document changed'):
            tasks.independent_readback('wps', spec, self.root, 'case-unit')

    def test_obsidian_vault_alone_does_not_prove_notes_or_links(self):
        spec = tasks.task_spec('obsidian', self.root, 'case-unit')
        vault = Path(spec['vault']); (vault/'.obsidian').mkdir(parents=True)
        with self.assertRaisesRegex(AssertionError, 'Missing note'):
            tasks.independent_readback('obsidian', spec, self.root, 'case-unit')
        for relative, content in spec['notes'].items():
            path = vault/relative; path.parent.mkdir(exist_ok=True); path.write_text(content)
        self.assertEqual(len(tasks.independent_readback('obsidian', spec, self.root, 'case-unit')['notes']), 2)
        path = vault/'Projects/Summary.md'
        path.write_text(path.read_text().replace('[[Tasks]]', 'Tasks'))
        with self.assertRaisesRegex(AssertionError, 'Saved note differs'):
            tasks.independent_readback('obsidian', spec, self.root, 'case-unit')

    def test_starting_docx_preserves_literal_xml_characters(self):
        path = self.root/'seed.docx'; tasks.seed_docx(path, 'A & B < marker > 🐦')
        self.assertEqual(tasks.docx_paragraphs(path), ['A & B < marker > 🐦'])

    def test_chrome_requires_the_actual_attachment_bytes_and_one_upload(self):
        spec = tasks.task_spec('chrome', self.root, 'case-unit')
        web = self.root/'web'; web.mkdir()
        state = business_state('case-unit'); state['uploads'] = []
        (web/'state.json').write_text(json.dumps(state))
        with self.assertRaisesRegex(AssertionError, 'exactly one'):
            tasks.independent_readback('chrome', spec, self.root, 'case-unit')
        upload = {'name': Path(spec['attachment']).name, 'sha256': spec['attachment_sha256']}
        state['uploads'] = [upload]
        (web/'state.json').write_text(json.dumps(state))
        self.assertTrue(tasks.independent_readback('chrome', spec, self.root, 'case-unit')['records_and_save_journal_match'])
        wrong = copy.deepcopy(state); wrong['uploads'][0]['sha256'] = 'other bytes'
        repeated = copy.deepcopy(state); repeated['uploads'].append(upload)
        for invalid in (wrong, repeated):
            (web/'state.json').write_text(json.dumps(invalid))
            with self.assertRaises(AssertionError):
                tasks.independent_readback('chrome', spec, self.root, 'case-unit')

    def test_recovery_does_not_erase_failed_attempt_or_claim_replay(self):
        failure = completed(); failure.update(status='failed', error={'code':'stale_observation'})
        failure['script']['summary']['failed'] = 1
        success = completed(); success['execution_id'] = 'execution-after-refresh'
        measured = tasks.recovery_metrics([call(failure), call(success)], NAME)
        self.assertEqual((measured['attempted_exec'], measured['failed_exec'], measured['successful_exec']), (2,1,1))
        self.assertEqual(measured['followup_exec_after_failure'], 1)
        self.assertEqual(measured['executions'][0]['execution_id'], 'execution-test')
        self.assertIn('not proof of replay', measured['retry_measurement'])

    def test_unparseable_execution_is_a_failed_attempt(self):
        measured = tasks.recovery_metrics([call('not JSON'), call(completed())], NAME)
        self.assertEqual(measured['failed_exec'], 1)
        self.assertEqual(measured['followup_exec_after_failure'], 1)
        self.assertIn('normalization_error', measured['executions'][0])

    def test_caught_native_failure_is_retained_in_attempt_counts(self):
        data = completed(); data['script']['summary']['failed'] = 1
        measured = tasks.recovery_metrics([call(data)], NAME)
        self.assertEqual(measured['failed_exec'], 1)
        self.assertEqual(measured['executions'][0]['failed_steps'], 1)

    def test_other_server_is_not_counted_as_this_task(self):
        other = call(completed()); other['name'] = 'mcp__unrelated__exec'
        self.assertEqual(tasks.recovery_metrics([other], NAME)['attempted_exec'], 0)

    def test_file_results_never_automatically_pass_the_gui_route(self):
        evidence = {'passed': True, 'business_passed': True, 'clean_run': False}
        tasks.leave_gui_review_pending(evidence)
        self.assertTrue(evidence['automatic_checks_passed'])
        self.assertFalse(evidence['passed'])
        self.assertFalse(evidence['clean_run'])
        self.assertEqual(evidence['acceptance_status'], 'pending_gui_review')

    def test_acp_protocol_failure_keeps_the_original_terminal_reason(self):
        path = self.root/'agent.jsonl'
        original = {'code': -32603, 'message': 'Reached maximum number of turns (35)'}
        path.write_text(json.dumps({'jsonrpc':'2.0','id':2,'error':original})+'\n')
        calls, terminal = tasks.normalize_acp(path)
        self.assertEqual(calls, [])
        self.assertEqual(terminal, {'error':original, 'response_id':2})
        with self.assertRaises(AssertionError):
            tasks.check_terminal('claude', terminal, 1)

    def test_document_cleanup_does_not_close_the_remaining_personal_tab(self):
        client = Mock(context_epochs={})
        client.execute.side_effect = [
            ([{'window_ref':'shared-window', 'title':'Owned-case.docx'}], None),
            ({}, None),
            ([{'window_ref':'shared-window', 'title':'Personal document'}], None),
        ]
        with patch.object(tasks, 'checked_windows', side_effect=lambda record, app: record), \
             patch.object(tasks, 'stable_execution'):
            proof = tasks.close_visible_owned_document(client, tasks.APPS['wps'][0], '/tmp/Owned-case.docx')
        actions = [c for c in client.execute.call_args_list if 'keypress' in c.args[2]]
        self.assertEqual(len(actions), 1)
        self.assertEqual(proof['status'], 'close_dispatched_document_no_longer_visible')

    def test_document_cleanup_leaves_unrelated_visible_documents_alone(self):
        client = Mock(context_epochs={})
        client.execute.return_value = ([{'window_ref':'personal', 'title':'Personal document'}], None)
        with patch.object(tasks, 'checked_windows', side_effect=lambda record, app: record):
            proof = tasks.close_visible_owned_document(client, tasks.APPS['wps'][0], '/tmp/Owned-case.docx')
        self.assertEqual(client.execute.call_count, 1)
        self.assertIsNone(proof['dispatch'])

    def test_app_allowlist_does_not_change_existing_browser_metric_contract(self):
        data = completed(); data['app'] = tasks.APPS['wps'][0]
        invocation = call(data); invocation['arguments']['app'] = data['app']
        self.assertTrue(tasks.metrics([invocation], NAME)['issues'])
        self.assertFalse(tasks.metrics([invocation], NAME, allowed_apps=[data['app']])['issues'])


if __name__ == '__main__':
    unittest.main()
