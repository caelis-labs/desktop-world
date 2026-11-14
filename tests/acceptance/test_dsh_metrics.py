import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

from dsh_metrics import summarize
from dsh_client import DshClient, native_cleanup_unconfirmed


class PublicJournalFacts(unittest.TestCase):
    def test_cleanup_failure_cancels_but_expected_user_interrupt_does_not(self):
        def update(value):
            return {'content': [{'content': {'type': 'text', 'text': 'Error: ' + json.dumps(value)}}]}
        self.assertTrue(native_cleanup_unconfirmed(update({'error': {'code': 'shutdown_failed'}})))
        self.assertTrue(native_cleanup_unconfirmed(update({'shutdown_error': {'code': 'backend_failed'}})))
        self.assertFalse(native_cleanup_unconfirmed(update({'error': {'code': 'execution_interrupted'}})))

    def test_cleanup_cancellation_matches_only_the_selected_package_server(self):
        for server in ('dtw', 'desktop-world'):
            with self.subTest(server=server), tempfile.TemporaryDirectory() as directory:
                def event(update):
                    return (json.dumps({'params': {'sessionId':'owned', 'update':update}})+'\n').encode()
                updates = []
                for index, name in enumerate(('mcp__other__exec', f'mcp__{server}__exec')):
                    updates.extend([
                        event({'sessionUpdate':'tool_call', 'toolCallId':str(index), 'title':name}),
                        event({'sessionUpdate':'tool_call_update', 'toolCallId':str(index), 'content':[
                            {'content':{'type':'text','text':'Error: {"error":{"code":"shutdown_failed"}}'}}]})])
                process = Mock(stdout=updates, stdin=io.BytesIO())
                with patch('dsh_client.subprocess.Popen', return_value=process), \
                     patch('dsh_client.threading.Thread'):
                    client = DshClient('unused-dsh', 'unused-patch', Path(directory)/'dsh',
                                       Path(directory), server_name=server)
                try:
                    client.owned_sessions.add('owned')
                    client._read()
                    sent = [json.loads(line) for line in process.stdin.getvalue().splitlines()]
                    self.assertEqual(sent, [{'jsonrpc':'2.0', 'method':'session/cancel',
                                            'params':{'sessionId':'owned'}}])
                    journal = client.log.name
                    records = [json.loads(line) for line in Path(journal).read_text().splitlines()]
                    cancelled = [row['message'] for row in records if row['direction']=='critical_cleanup_cancel']
                    self.assertEqual(cancelled, [{'sessionId':'owned','toolCallId':'1'}])
                finally:
                    client.log.close()
                    client.stderr.close()

    def test_unknown_or_failed_tools_are_not_promoted_by_later_success(self):
        updates = [
            {'sessionUpdate': 'tool_call_update', 'toolCallId': 'lost', 'status': 'failed'},
            {'sessionUpdate': 'tool_call', 'toolCallId': 'a', 'title': 'mcp__dtw__exec', 'status': 'in_progress'},
            {'sessionUpdate': 'tool_call_update', 'toolCallId': 'a', 'status': 'failed',
             'content': [{'content': {'type': 'text', 'text': 'Error: {"error":{"code":"execution_interrupted"},"effects":{"dispatch":{"unknown":1}}}\n[image unavailable]'}}]},
            {'sessionUpdate': 'tool_call', 'toolCallId': 'b', 'title': 'mcp__dtw__exec', 'status': 'in_progress'},
            {'sessionUpdate': 'tool_call_update', 'toolCallId': 'b', 'status': 'completed',
             'content': [{'content': {'type': 'image', 'data': 'not-a-pixel-oracle'}}]},
            {'sessionUpdate': 'tool_call', 'toolCallId': 'c', 'title': 'read', 'status': 'in_progress'},
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'acp.jsonl'
            path.write_text('\n'.join(json.dumps({'message': {'params': {'update': update}}})
                for update in updates) + '\nmalformed', encoding='utf-8')
            result = summarize(path)
        self.assertFalse(result['tool_error_free'])
        self.assertEqual(result['statuses'], {'failed': 1, 'completed': 1, 'in_progress': 1})
        self.assertEqual(result['failed_error_codes'], {'execution_interrupted': 1})
        self.assertEqual(result['admitted_image_parts'], 1)
        self.assertEqual(result['journal_parse_errors'], 1)
        self.assertEqual(result['unmatched_tool_updates'], 1)
        self.assertEqual(result['dispatch_counts'], {'unknown': 1})
        self.assertEqual(result['tools_with_unknown_dispatch'], 1)


if __name__ == '__main__':
    unittest.main()
