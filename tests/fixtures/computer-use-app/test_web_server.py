"""Portable web fixture contracts, exercised through real loopback HTTP."""
import concurrent.futures
import importlib.util
import json
from pathlib import Path
import socket
import tempfile
import threading
import unittest
from unittest.mock import patch
import urllib.error
import urllib.request

SPEC = importlib.util.spec_from_file_location('computer_web', Path(__file__).with_name('web_server.py'))
web = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(web)


class WebServerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.output = Path(self.temp.name) / 'web'
        self.server = web.create_server(self.output, 'case-137', workbench_url='http://127.0.0.1:9876/')
        self.thread = threading.Thread(target=self.server.serve_forever)
        self.thread.start()

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
        self.temp.cleanup()

    def request(self, path, body=None, raw=None):
        data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
        request = urllib.request.Request(self.server.fixture_url.rstrip('/') + path, data=data,
                                         headers={'Content-Type': 'application/json'})
        try:
            with urllib.request.urlopen(request, timeout=5) as response:
                return response.status, response.read()
        except urllib.error.HTTPError as error:
            return error.code, error.read()

    def state(self):
        return json.loads((self.output / 'state.json').read_text())

    def test_partial_headers_do_not_block_owned_handler_shutdown(self):
        parsing = threading.Event()
        closed = threading.Event()
        original_parse = self.server.RequestHandlerClass.parse_request

        def observe_parse(handler):
            parsing.set()
            return original_parse(handler)

        def close_server():
            self.server.shutdown()
            self.server.server_close()
            closed.set()

        closer = threading.Thread(target=close_server)
        with patch.object(self.server.RequestHandlerClass, 'parse_request', observe_parse):
            peer = socket.create_connection(self.server.server_address, timeout=2)
            try:
                peer.sendall(b'GET / HTTP/1.1\r\nHost: localhost\r\nX-Incomplete: ')
                self.assertTrue(parsing.wait(2), 'handler never began parsing partial headers')
                self.assertFalse(self.server.daemon_threads)
                closer.start()
                completed_with_peer_open = closed.wait(3)
            finally:
                peer.close()
                if closer.ident is not None:
                    closer.join(3)
        self.assertTrue(completed_with_peer_open, 'server_close waited for the peer to close')
        self.assertFalse(closer.is_alive())

    def test_normal_flow_and_independent_saves(self):
        self.assertEqual(self.request('/api/note', {'note': '中文🙂'})[0], 200)
        self.assertEqual(self.request('/api/note', {'note': '中文🙂'})[0], 200)
        self.assertEqual(len(self.state()['saves']), 2)
        self.assertEqual(self.state()['note'], '中文🙂')
        self.request('/api/catalogue', {})
        self.request('/api/checkpoint', {'id': 137})
        self.assertEqual(len(self.state()['checkpoint_hits']), 1)
        self.request('/api/chat', {'action': 'pin', 'id': 'release-137'})
        state = self.state()
        target = next(m for m in state['chat']['messages'] if m['id'] == 'release-137')
        self.assertTrue(target['pinned'])
        self.assertEqual(len(state['chat']['actions']), 1)
        self.assertGreater(len(state['reference'].encode('utf-16-le')) // 2, 8000)
        self.assertTrue(state['reference'].endswith('WEB-END-137-中文🙂'))
        events = [json.loads(line) for line in (self.output / 'events.jsonl').read_text().splitlines()]
        self.assertEqual([e['seq'] for e in events], list(range(1, len(events) + 1)))

    def test_visual_secret_is_fresh_for_identical_cases(self):
        second = web.create_server(Path(self.temp.name) / 'second', 'case-137')
        try:
            first_state = self.state()
            second_state = json.loads((Path(self.temp.name) / 'second' / 'state.json').read_text())
            self.assertNotEqual(first_state['vision']['expected_token'], second_state['vision']['expected_token'])
            self.assertEqual(first_state['chat'], second_state['chat'])
            self.assertEqual(first_state['reference'], second_state['reference'])
        finally:
            second.server_close()

    def test_transient_utf16_surrogate_is_preserved_without_poisoning_saves(self):
        note = 'transient \\ud83d'
        self.assertEqual(self.request('/api/note', {'note': note})[0], 200)
        self.assertEqual(self.state()['note'], note)
        self.assertIn(b'\\\\ud83d', (self.output / 'state.json').read_bytes())
        self.assertEqual(self.request('/api/note', {'note': 'complete 🙂'})[0], 200)
        self.assertEqual([save['note'] for save in self.state()['saves']], [note, 'complete 🙂'])
        events = [json.loads(line) for line in (self.output / 'events.jsonl').read_text().splitlines()]
        self.assertEqual(events[0]['data']['note'], note)

    def test_concurrent_state_is_serialized(self):
        with concurrent.futures.ThreadPoolExecutor(max_workers=10) as pool:
            results = list(pool.map(lambda i: self.request('/api/note', {'note': str(i)})[0], range(30)))
        self.assertEqual(results, [200] * 30)
        self.assertEqual(len(self.state()['saves']), 30)
        self.assertEqual(self.state()['events'], 30)
        self.assertEqual(len((self.output / 'events.jsonl').read_text().splitlines()), 30)

    def test_invalid_requests_are_visible_and_no_oracle_is_served(self):
        for path in ['/state.json', '/events.jsonl', '/../state.json', '/api/state', '/missing']:
            self.assertEqual(self.request(path)[0], 404)
        self.assertEqual(self.request('/api/note', raw=b'{')[0], 400)
        self.assertEqual(self.request('/api/note', raw=b'[]')[0], 400)
        self.assertEqual(self.request('/api/note', {'note': 2})[0], 400)
        self.assertEqual(self.request('/api/note', raw=b'x' * (web.MAX_BODY + 1))[0], 413)
        self.assertEqual(self.request('/api/unknown', {})[0], 404)
        self.assertEqual(self.request('/api/chat', {'action': 'pin', 'id': 'missing'})[0], 400)
        self.assertEqual(len(self.state()['saves']), 0)
        self.assertGreater(self.state()['events'], 0)

    def test_image_contains_no_token_and_public_data_has_no_oracle(self):
        token = self.state()['vision']['expected_token'].encode()
        for path in ['/', '/canvas', '/vision', '/app.js', '/api/data', '/api/config', '/vision/challenge.png']:
            code, body = self.request(path)
            self.assertEqual(code, 200, path)
            self.assertNotIn(token, body, path)
            self.assertNotIn(b'expected_token', body, path)
        self.assertTrue(self.request('/vision/challenge.png')[1].startswith(b'\x89PNG'))
        self.assertEqual(self.request('/api/vision', {'value': 'WRONG'})[0], 200)
        self.assertFalse(self.state()['vision']['attempts'][0]['passed'])

    def test_layout_validation_and_action_preserves_geometry(self):
        layout = {'viewport': [900, 700], 'anchors': {'web-note': [100.5, 80]}}
        self.assertEqual(self.request('/api/layout', layout)[0], 200)
        self.request('/api/note', {'note': 'geometry survives'})
        self.assertEqual(self.state()['layout'], layout)
        for invalid in [
            {'viewport': [900, 700], 'anchors': {'unknown': [0, 0]}},
            {'viewport': [900, 700], 'anchors': {'web-note': [float('inf'), 0]}},
            {'viewport': [0, 700], 'anchors': {}},
        ]:
            self.assertEqual(self.request('/api/layout', invalid)[0], 400)
        self.assertEqual(self.state()['layout'], layout)

    def test_record_validation_reference_edit_and_capacity(self):
        record = {'note': 'review 中文🙂', 'number': 12, 'approved': True,
                  'priority': 'Urgent', 'date': '2026-01-03'}
        self.assertEqual(self.request('/api/record', dict(record, approved=False))[0], 400)
        self.assertEqual(self.request('/api/record', record)[0], 200)
        self.assertEqual(self.state()['records'], [record])
        reference = self.state()['reference'] + '\nTail edited 中文🙂'
        self.assertEqual(self.request('/api/reference', {'reference': reference})[0], 200)
        self.assertEqual(self.state()['reference'], reference)
        self.server.store.state['events'] = web.MAX_EVENTS
        self.assertEqual(self.request('/api/note', {'note': 'over capacity'})[0], 503)
        self.assertEqual(self.state()['saves'], [])

    def test_real_journal_failure_stops_mutation(self):
        with patch.object(Path, 'open', side_effect=OSError('disk unavailable')):
            code, body = self.request('/api/note', {'note': 'lost'})
        self.assertEqual(code, 503)
        self.assertIn(b'journal', body)
        self.assertEqual(self.state()['note'], '')
        self.assertEqual(self.request('/api/note', {'note': 'still blocked'})[0], 503)

    def test_fresh_output_and_failure_visibility(self):
        with self.assertRaises(FileExistsError):
            web.create_server(self.output, 'case-137')
        self.server.store.journal_error = 'simulated journal failure'
        code, body = self.request('/api/note', {'note': 'must fail'})
        self.assertEqual(code, 503)
        self.assertIn(b'journal', body)
        self.assertEqual(self.state()['saves'], [])


if __name__ == '__main__':
    unittest.main()
