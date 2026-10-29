#!/usr/bin/env python3
"""Owned local business UI for native-browser acceptance; no browser automation.

The model uses the rendered app only. The supervisor verifies state.json, which
is never exposed by HTTP, and retains every save/event rather than replaying.
"""
import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import threading
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', required=True)
    args = parser.parse_args()
    if not args.case.replace('-', '').isalnum():
        parser.error('case must contain only letters, digits and hyphens')
    args.output.mkdir(parents=True, exist_ok=True)
    items = [{'sku': f'DTW-{i:04}', 'name': f'Field kit {i:03}',
              'warehouse': ['North', 'South', 'Central'][i % 3],
              'quantity': 10 + i % 25, 'reviewed': False, 'note': ''} for i in range(1, 241)]
    state = {'case': args.case, 'items': items, 'saves': [], 'events': [], 'ready': False}
    lock = threading.Lock()
    html = Path(__file__).with_name('index.html').read_text().replace('__CASE__', args.case).encode()

    def persist():
        path = args.output / 'state.json'
        path.with_suffix('.tmp').write_text(json.dumps(state, ensure_ascii=False))
        path.with_suffix('.tmp').replace(path)

    persist()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def reply(self, status, body, kind='application/json'):
            self.send_response(status)
            self.send_header('Content-Type', kind + '; charset=utf-8')
            self.send_header('Content-Length', str(len(body)))
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path == '/':
                self.reply(200, html, 'text/html')
            elif self.path == '/api/items':
                with lock:
                    self.reply(200, json.dumps(items).encode())
            else:
                self.reply(404, b'{}')

        def do_POST(self):
            try:
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length <= 65536:
                    raise ValueError('body size')
                data = json.loads(self.rfile.read(length))
                if not isinstance(data, dict):
                    raise ValueError('object required')
                with lock:
                    if self.path == '/api/event':
                        if len(state['events']) >= 10000:
                            raise ValueError('event capacity')
                        state['events'].append({**data, 'unix_ms': int(time.time() * 1000)})
                        state['ready'] |= data.get('type') == 'ready'
                        result = {}
                    elif self.path == '/api/save':
                        item = next(x for x in items if x['sku'] == data.get('sku'))
                        if type(data.get('quantity')) is not int or not 0 <= data['quantity'] <= 10000:
                            raise ValueError('quantity')
                        if type(data.get('reviewed')) is not bool or not isinstance(data.get('note'), str) or len(data['note']) > 4096:
                            raise ValueError('values')
                        item.update({key: data[key] for key in ('quantity', 'reviewed', 'note')})
                        state['saves'].append({**item, 'unix_ms': int(time.time() * 1000)})
                        result = dict(item)
                    else:
                        raise ValueError('route')
                    persist()
                self.reply(200, json.dumps(result, ensure_ascii=False).encode())
            except (ValueError, KeyError, StopIteration):
                self.reply(400, b'{"error":"invalid input"}')

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    print(json.dumps({'url': f'http://127.0.0.1:{server.server_port}/', 'case': args.case}), flush=True)
    def stop_when_requested():
        while not (args.output / 'stop').exists():
            time.sleep(0.05)
        server.shutdown()
    threading.Thread(target=stop_when_requested, daemon=True).start()
    server.serve_forever()
    server.server_close()


if __name__ == '__main__':
    main()
