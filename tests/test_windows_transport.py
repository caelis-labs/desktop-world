"""Current Windows MCP transport controls; no desktop input or legacy protocol."""
import importlib.util
import io
import json
from pathlib import Path
import queue
from types import SimpleNamespace
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'windows_transport', Path(__file__).parent / 'acceptance/windows/transport.py')
transport = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(transport)


class TransportTests(unittest.TestCase):
    def test_only_current_transport_methods_are_exposed(self):
        methods = {name for name, value in vars(transport.Mcp).items() if callable(value)}
        self.assertEqual(methods, {'__init__', 'send', 'begin', 'receive', 'rpc', 'close'})

    def test_rpc_keeps_out_of_order_responses_and_original_request_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            client = object.__new__(transport.Mcp)
            client.output = Path(directory)
            client.serial = 0
            client.pending = {}
            client.messages = queue.Queue()
            client.proc = SimpleNamespace(stdin=io.BytesIO())
            client.messages.put({'jsonrpc': '2.0', 'id': 2, 'result': {'later': True}})
            client.messages.put({'jsonrpc': '2.0', 'id': 1, 'result': {'tools': []}})
            self.assertEqual(client.rpc('tools/list', {}), {'tools': []})
            self.assertEqual(client.receive(2)['result'], {'later': True})
            expected = {'jsonrpc': '2.0', 'id': 1, 'method': 'tools/list', 'params': {}}
            self.assertEqual(json.loads(client.proc.stdin.getvalue()), expected)
            self.assertEqual(json.loads((client.output / 'requests.jsonl').read_text()), expected)

    def test_rpc_and_reader_failures_are_not_replayed(self):
        client = object.__new__(transport.Mcp)
        client.pending = {}
        client.messages = queue.Queue()
        sent = []
        client.begin = lambda method, params: sent.append((method, params)) or 1
        client.messages.put({'id': 1, 'error': {'code': -32601, 'message': 'no such method'}})
        with self.assertRaises(AssertionError):
            client.rpc('tools/call', {'name': 'exec'})
        self.assertEqual(len(sent), 1)
        client.messages.put(EOFError('MCP stdout closed'))
        with self.assertRaises(EOFError):
            client.receive(2)


if __name__ == '__main__':
    unittest.main()
