import unittest
import json
from pathlib import Path
import tempfile

from cursor_audit import independence_violation, summarize, trace


class CursorOracleTests(unittest.TestCase):
    def test_restored_endpoint_does_not_hide_transient_motion(self):
        rows = [{'unix_ms': index * 2, 'x': x, 'y': -100}
                for index, x in enumerate([-200, -160, -200])]
        result = trace(rows)
        self.assertEqual(result['before'], result['after'])
        self.assertEqual(result['status'], 'moved')
        self.assertEqual(result['max_displacement_points'], 40)
        self.assertEqual(result['transitions'], 2)

    def test_unavailable_oracle_never_proves_independence(self):
        self.assertEqual(trace([])['status'], 'oracle_unavailable')
        self.assertEqual(trace([{'unix_ms': 0, 'x': 0, 'y': 0}])['status'], 'oracle_unavailable')
        self.assertEqual(trace([{'unix_ms': 0, 'error': 'read_failed'}])['status'], 'oracle_unavailable')

    def test_stable_cursor(self):
        rows = [{'unix_ms': index * 2, 'x': -20, 'y': 100} for index in range(3)]
        self.assertEqual(trace(rows)['status'], 'stable')

    def test_route_contract_does_not_accept_restore_or_arbitrary_fallback(self):
        moved={'status':'moved','before':[0,0],'after':[0,0]}
        self.assertIsNotNone(independence_violation({'cursor':moved,'routes':[{'route':'addressed','reason':'pid_window_event'}]}))
        self.assertIsNotNone(independence_violation({'cursor':moved,'routes':[{'route':'borrowed','reason':'children_unavailable'}]}))
        self.assertIsNone(independence_violation({'cursor':moved,'routes':[{'route':'borrowed','reason':'native_hover_tracking'}]}))

    def test_cancelled_action_keeps_missing_receipt_and_measures_cleanup(self):
        def frame(direction, at, message):
            return {'direction': direction, 'at_unix_ms': at, 'message': message}
        def execute(number, source):
            return {'id': number, 'method': 'tools/call', 'params': {'name': 'exec', 'arguments': {'source': source}}}
        frames = [frame('request', 0, execute(1, 'await app.drag({path:[]});')),
                  frame('request', 2, {'method': 'notifications/cancelled', 'params': {'requestId': 1}}),
                  frame('request', 4, execute(2, 'await app.capture();')),
                  frame('response', 8, {'id': 2, 'result': {'structuredContent': {'status': 'completed'}}})]
        samples = [{'unix_ms': index * 2, 'x': x, 'y': 0} for index, x in enumerate([0, 20, 40, 20, 0])]
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / 'mcp.jsonl').write_text('\n'.join(json.dumps(f) for f in frames))
            rows = summarize(root, samples)
        cancelled = next(row for row in rows if row['request_id'] == 1)
        self.assertEqual(cancelled['completion'], 'no_reply_after_cancel')
        self.assertEqual(cancelled['cursor']['status'], 'moved')
        self.assertNotIn('tool_error', cancelled)


if __name__ == '__main__':
    unittest.main()
