#!/usr/bin/env python3
"""HTTP/state controls only; does not launch a browser or automate a desktop."""
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path
import tempfile
import threading
import unittest
import struct
import zlib
from urllib.error import HTTPError
from urllib.request import Request, urlopen
from unittest.mock import patch


spec = importlib.util.spec_from_file_location('workbench_server', Path(__file__).with_name('server.py'))
workbench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workbench)


class WorkbenchHttpTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='DTW fixture 空格 ')
        self.output = Path(self.directory.name)
        self.server = workbench.create_server(self.output, 'http-controls')
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.base = f'http://127.0.0.1:{self.server.server_port}'

    def tearDown(self):
        self.server.shutdown()
        self.thread.join(timeout=5)
        self.server.server_close()
        self.directory.cleanup()

    def request(self, path, data=None):
        body = None if data is None else json.dumps(data, ensure_ascii=True).encode('utf-8')
        request = Request(self.base + path, data=body, headers={'Content-Type': 'application/json'})
        with urlopen(request, timeout=5) as response:
            content = response.read()
            return json.loads(content) if response.headers.get_content_type() == 'application/json' else content.decode('utf-8')

    def disk(self):
        return json.loads((self.output / 'state.json').read_text(encoding='utf-8'))

    def attempt(self, nonce, mode='normal', gesture=None, **changes):
        return self.request('/api/reliability/attempt', {
            'nonce': nonce, 'frame': 'frame-1', 'run': 'foreground-sendinput',
            'title': 'Owned record', 'mode': mode, 'ready': True,
            'gesture': gesture or {'down': False, 'up': False, 'click': False}, **changes})

    def test_existing_inventory_and_save_are_preserved(self):
        html = self.request('/')
        self.assertIn('Inventory review — http-controls', html)
        self.assertNotIn('__ATTACHMENT_SECTION__', html)
        self.assertNotIn('id="attachment"', html)
        items = self.request('/api/items')
        self.assertEqual(len(items), 240)
        self.assertEqual(items[-1]['sku'], 'DTW-0240')
        payload = {'sku': 'DTW-0240', 'quantity': 52, 'reviewed': True, 'note': 'Exact 中文 note 😀'}
        self.assertEqual(self.request('/api/save', payload)['note'], payload['note'])
        self.assertEqual(len(self.disk()['saves']), 1)
        # Full inventory supervisor state is not exposed through diagnostic HTTP.
        self.assertNotIn('items', self.request('/api/reliability'))
        with self.assertRaises(HTTPError) as error:
            self.request('/state.json')
        self.assertEqual(error.exception.code, 404)

    def test_original_two_file_instrumented_layout_remains_usable(self):
        copied = self.output / 'instrumented'
        copied.mkdir()
        source = Path(__file__).with_name('index.html')
        (copied / 'index.html').write_bytes(source.read_bytes())
        with patch.object(workbench, '__file__', str(copied / 'server.py')):
            server = workbench.create_server(copied / 'evidence', 'original-layout')
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            base = f'http://127.0.0.1:{server.server_port}'
            with urlopen(base + '/', timeout=5) as response:
                self.assertIn('Inventory review', response.read().decode('utf-8'))
            with self.assertRaises(HTTPError) as error:
                urlopen(base + '/reliability', timeout=5)
            self.assertEqual(error.exception.code, 404)
            with self.assertRaises(HTTPError) as error:
                urlopen(base + '/vision', timeout=5)
            self.assertEqual(error.exception.code, 404)
        finally:
            server.shutdown()
            thread.join(timeout=5)
            server.server_close()

    def test_isolated_page_and_event_journal(self):
        self.assertIn('Pointer delivery and traversal — http-controls', self.request('/reliability'))
        for kind in ('pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click', 'icon'):
            self.request('/api/reliability/event', {
                'kind': kind, 'run': 'posted-mouse', 'frame': 'frame-1',
                'trusted': True, 'button': 0, 'buttons': 0, 'detail': 1, 'x': 120, 'y': 240,
                'pointer_type': 'mouse', 'pointer_id': 1})
        state = self.disk()['reliability']
        self.assertEqual([x['sequence'] for x in state['events']], list(range(1, 7)))
        summary = self.request('/api/reliability')
        self.assertEqual(summary['event_counts']['pointerdown'], 1)
        self.assertEqual(summary['events'][0]['pointer_type'], 'mouse')
        self.assertEqual(summary['icon_activations'], 1)
        self.assertEqual(summary['attempt_count'], 0)

    def test_normal_and_trusted_pointer_modes_have_distinct_effects(self):
        normal = self.attempt('invoke-normal')
        self.assertTrue(normal['accepted'])
        blocked = self.attempt('invoke-gated', mode='trusted-pointer')
        self.assertFalse(blocked['accepted'])
        self.assertEqual(blocked['reason'], 'trusted_pointer_required')
        accepted = self.attempt('pointer-gated', mode='trusted-pointer', gesture={
            'down': True, 'up': True, 'click': True})
        self.assertTrue(accepted['accepted'])
        self.assertEqual(accepted['submission_count'], 2)
        not_ready = self.attempt('not-settled', ready=False)
        self.assertEqual(not_ready['reason'], 'not_ready')
        self.assertEqual(len(self.disk()['reliability']['submissions']), 2)

    def test_duplicate_request_is_recorded_without_hiding_two_activations(self):
        self.attempt('first-activation')
        repeat = self.attempt('first-activation')
        self.assertEqual(repeat['reason'], 'duplicate_nonce')
        self.assertFalse(repeat['accepted'])
        self.assertEqual(repeat['submission_count'], 1)
        # A second real click has a new nonce and is observable as a second effect.
        self.assertEqual(self.attempt('second-activation')['submission_count'], 2)
        self.assertEqual(len(self.disk()['reliability']['attempts']), 3)

    def test_concurrent_receipts_are_not_lost(self):
        with ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(lambda i: self.attempt(f'activation-{i}'), range(12)))
        self.assertTrue(all(x['accepted'] for x in results))
        journal = self.disk()['reliability']
        self.assertEqual(len(journal['submissions']), 12)
        self.assertEqual([x['sequence'] for x in journal['attempts']], list(range(1, 13)))

    def test_invalid_payload_cannot_create_a_receipt_or_poison_later_events(self):
        invalid = [
            {'nonce': 'bad', 'frame': 'f', 'run': 'r', 'title': 'x', 'mode': 'normal',
             'ready': True, 'gesture': {'down': 1, 'up': True, 'click': True}},
            {'nonce': 'bad', 'frame': 'f', 'run': 'r', 'title': 'x', 'mode': 'unknown',
             'ready': True, 'gesture': {'down': False, 'up': False, 'click': False}},
        ]
        for data in invalid:
            with self.assertRaises(HTTPError) as error:
                self.request('/api/reliability/attempt', data)
            self.assertEqual(error.exception.code, 400)
        with self.assertRaises(HTTPError):
            self.request('/api/reliability/event', {
                'kind': 'click', 'frame': 'f', 'run': 'r', 'x': float('nan')})
        self.assertEqual(len(self.disk()['reliability']['attempts']), 0)
        # A real input event may temporarily hold one UTF-16 surrogate.
        self.request('/api/event', {'type': 'input', 'value': '\ud83d'})
        self.request('/api/reliability/event', {'kind': 'ready', 'frame': 'f', 'run': 'r'})
        self.assertTrue(self.attempt('after-invalid')['accepted'])
        self.assertEqual(self.disk()['events'][-1]['value'], '\ud83d')

    def test_transient_windows_publication_lock_does_not_repeat_effects(self):
        replace = Path.replace
        failures = []
        def briefly_locked(path, target):
            if path == self.output / 'state.tmp' and not failures:
                failures.append(str(path))
                raise PermissionError('controlled transient destination lock')
            return replace(path, target)
        with patch.object(Path, 'replace', briefly_locked):
            receipt = self.attempt('publication-once')
        self.assertTrue(receipt['accepted'])
        self.assertEqual(len(failures), 1)
        state = self.disk()['reliability']
        self.assertEqual(len(state['attempts']), 1)
        self.assertEqual(len(state['submissions']), 1)
        self.assertFalse((self.output / 'state.tmp').exists())

    def test_vision_challenge_is_pixels_only_and_not_exposed_by_text_routes(self):
        token = self.disk()['vision']['expected_token']
        self.assertEqual(len(token), 8)
        self.assertTrue(set(token) <= set(workbench.VISION_GLYPHS))
        html = self.request('/vision')
        self.assertNotIn(token, html)
        self.assertIn('aria-label="Visual token challenge"', html)
        self.assertIn('<canvas', html)
        for route in ('/', '/reliability', '/api/items', '/api/reliability'):
            result = self.request(route)
            self.assertNotIn(token, result if isinstance(result, str) else json.dumps(result))
        for route in ('/state.json', '/api/vision', '/vision/expected_token'):
            with self.assertRaises(HTTPError):
                self.request(route)
        with urlopen(self.base + '/vision/challenge.png', timeout=5) as response:
            self.assertEqual(response.headers.get_content_type(), 'image/png')
            png = response.read()
        self.assertTrue(png.startswith(b'\x89PNG\r\n\x1a\n'))
        self.assertNotIn(token.encode('ascii'), png)
        chunks, position = [], 8
        while position < len(png):
            length = struct.unpack_from('>I', png, position)[0]
            kind = png[position+4:position+8]
            value = png[position+8:position+8+length]
            self.assertEqual(struct.unpack_from('>I', png, position+8+length)[0], zlib.crc32(kind+value))
            chunks.append((kind, value))
            position += 12+length
        self.assertEqual(position, len(png))
        self.assertEqual([kind for kind, _ in chunks], [b'IHDR', b'IDAT', b'IEND'])
        self.assertEqual(struct.unpack('>IIBBBBB', chunks[0][1]), (640, 160, 8, 2, 0, 0, 0))
        rows = zlib.decompress(chunks[1][1])
        self.assertEqual(len(rows), 160*(640*3+1))
        self.assertTrue(all(rows[y*(640*3+1)] == 0 for y in range(160)))
        self.assertIn(b'\x12\x20\x2b', rows)
        self.assertIn(b'\xff\xff\xff', rows)

    def test_vision_records_wrong_and_correct_attempts_without_returning_answer(self):
        token = self.disk()['vision']['expected_token']
        wrong = 'XXXXXXXX' if token != 'XXXXXXXX' else 'AAAAAAAA'
        first = self.request('/api/vision/verify', {'value': wrong})
        self.assertEqual(first, {'passed': False, 'attempt_count': 1})
        correct = self.request('/api/vision/verify', {'value': token})
        self.assertEqual(correct, {'passed': True, 'attempt_count': 2})
        self.assertNotIn(token, json.dumps(correct))
        attempts = self.disk()['vision']['attempts']
        self.assertEqual([row['submitted'] for row in attempts], [wrong, token])
        self.assertEqual([row['passed'] for row in attempts], [False, True])
        self.assertFalse(self.disk()['saves'])

    def test_vision_input_bounds_and_attempt_limit(self):
        for value in (None, '', 'A'*7, 'A'*9, 'A'*10000, '😀'*8, '<script>'):
            with self.assertRaises(HTTPError) as error:
                self.request('/api/vision/verify', {'value': value})
            self.assertEqual(error.exception.code, 400)
        self.assertFalse(self.disk()['vision']['attempts'])
        for count in range(1, 9):
            self.assertEqual(self.request('/api/vision/verify', {'value':'AAAAAAAA'})['attempt_count'], count)
        with self.assertRaises(HTTPError) as error:
            self.request('/api/vision/verify', {'value':'AAAAAAAA'})
        self.assertEqual(error.exception.code, 400)
        self.assertEqual(len(self.disk()['vision']['attempts']), 8)


if __name__ == '__main__':
    unittest.main()
