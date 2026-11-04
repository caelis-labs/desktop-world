#!/usr/bin/env python3
"""Loopback-only portable acceptance UI. Private oracle and journal stay on disk."""
import argparse
import copy
from datetime import date
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import math
import os
from pathlib import Path
import secrets
import re
import threading
import time
from urllib.parse import urlsplit

MAX_BODY = 131072
MAX_EVENTS = 10000
MAX_MESSAGES = 300
MAX_STATE_BYTES = 8 * 1024 * 1024
LAYOUT_KEYS = frozenset(('web-note', 'web-commit', 'web-reference', 'web-checkpoint', 'web-catalogue',
                         'canvas-message-137', 'canvas-pin', 'canvas-edit', 'canvas-reply',
                         'canvas-compose', 'canvas-send'))
WEB = Path(__file__).with_name('web')


def encoded(value):
    return json.dumps(value, ensure_ascii=True, separators=(',', ':'), allow_nan=False).encode('utf-8')


def atomic_json(path, value):
    temporary = path.with_suffix('.tmp')
    with temporary.open('wb') as stream:
        stream.write(encoded(value))
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)


class Store:
    def __init__(self, output, case):
        self.output = output
        self.lock = threading.Lock()
        self.journal_error = None
        source = Path(__file__).parents[1] / 'browser-workbench' / 'server.py'
        spec = importlib.util.spec_from_file_location('acceptance_vision_source', source)
        helper = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(helper)
        token = ''.join(secrets.choice(tuple(helper.VISION_GLYPHS)) for _ in range(8))
        self.image = helper.vision_png(token)
        messages = [{'id': f'msg-{i}', 'text': f'{["Mira", "Tao", "Sam"][i % 3]}: Build lane {i:03d} — logs checked ✓', 'pinned': False} for i in range(25)]
        messages.insert(12, {'id': 'release-137', 'text': 'Release check 137', 'pinned': False})
        self.state = {'schema_version': 1, 'case': case, 'note': '', 'saves': [],
                      'checkpoint_hits': [], 'reference': ('Reference entry 中文🙂 — verify the release ledger.\n' * 230) + 'WEB-END-137-中文🙂',
                      'catalogue_loaded': False, 'records': [], 'slider': 35,
                      'chat': {'messages': messages, 'actions': []},
                      'vision': {'expected_token': token, 'attempts': []}, 'layout': {}, 'events': 0}
        atomic_json(output / 'state.json', self.state)
        (output / 'events.jsonl').touch(exist_ok=False)

    def public(self):
        with self.lock:
            return copy.deepcopy({key: self.state[key] for key in ('note', 'reference', 'catalogue_loaded', 'chat', 'slider')})

    def transact(self, kind, payload, effect=None, error=None):
        with self.lock:
            if self.journal_error:
                raise OSError(self.journal_error)
            if self.state['events'] >= MAX_EVENTS:
                raise OverflowError('event capacity reached')
            state = copy.deepcopy(self.state)
            result = effect(state) if effect else {}
            seq = state['events'] + 1
            event = {'seq': seq, 'type': kind, 'data': payload, 'error': error}
            state['events'] = seq
            if len(encoded(state)) > MAX_STATE_BYTES:
                raise OverflowError('state capacity reached')
            try:
                with (self.output / 'events.jsonl').open('ab') as stream:
                    stream.write(encoded(event) + b'\n')
                    stream.flush()
                    os.fsync(stream.fileno())
                atomic_json(self.output / 'state.json', state)
            except OSError as exc:
                self.journal_error = f'journal persistence failed: {exc}'
                raise
            self.state = state
            return result or {'ok': True, 'seq': seq}


def text(data, key, maximum=20000, required=False):
    value = data.get(key)
    if not isinstance(value, str) or len(value) > maximum or (required and not value.strip()):
        raise ValueError(f'invalid {key}')
    return value


def apply_action(state, route, data):
    if route == '/api/note':
        note = text(data, 'note')
        state['note'] = note
        state['saves'].append({'note': note, 'activation': len(state['saves']) + 1})
    elif route == '/api/reference':
        state['reference'] = text(data, 'reference', 50000)
    elif route == '/api/catalogue':
        state['catalogue_loaded'] = True
    elif route == '/api/checkpoint':
        if type(data.get('id')) is not int or data['id'] != 137:
            raise ValueError('invalid checkpoint')
        state['checkpoint_hits'].append({'id': 137, 'activation': len(state['checkpoint_hits']) + 1})
    elif route == '/api/record':
        note = text(data, 'note', 1000, True)
        number = data.get('number')
        if type(number) is not int or not 1 <= number <= 999 or data.get('approved') is not True or data.get('priority') not in ('Normal', 'Urgent'):
            raise ValueError('complete the required record fields')
        date.fromisoformat(text(data, 'date', 10, True))
        state['records'].append(dict(data, note=note))
    elif route == '/api/slider':
        if type(data.get('value')) is not int or not 0 <= data['value'] <= 100:
            raise ValueError('invalid slider value')
        state['slider'] = data['value']
    elif route == '/api/chat':
        action = data.get('action')
        if action not in ('pin', 'edit', 'reply', 'send'):
            raise ValueError('invalid chat action')
        messages = state['chat']['messages']
        target = next((item for item in messages if item['id'] == data.get('id')), None)
        if action != 'send' and target is None:
            raise ValueError('unknown message')
        if action == 'pin':
            target['pinned'] = True
        elif action == 'edit':
            target['text'] = text(data, 'text', 2000, True)
        else:
            if len(messages) >= MAX_MESSAGES:
                raise OverflowError('message capacity reached')
            messages.append({'id': f'sent-{len(state["chat"]["actions"]) + 1}', 'text': text(data, 'text', 2000, True), 'pinned': False,
                             'reply_to': target['id'] if target else None})
        state['chat']['actions'].append(dict(data))
    elif route == '/api/vision':
        value = text(data, 'value', 32, True)
        passed = value == state['vision']['expected_token']
        state['vision']['attempts'].append({'submitted': value, 'passed': passed})
        return {'passed': passed}
    elif route == '/api/layout':
        viewport, anchors = data.get('viewport'), data.get('anchors')
        def point(value, viewport=False):
            return (isinstance(value, list) and len(value) == 2 and
                    all(type(v) in (int, float) and math.isfinite(v) and
                        (0 < v <= 20000 if viewport else -100000 <= v <= 100000) for v in value))
        if (set(data) != {'viewport', 'anchors'} or not point(viewport, True) or
                not isinstance(anchors, dict) or not set(anchors) <= LAYOUT_KEYS or
                not all(point(value) for value in anchors.values())):
            raise ValueError('invalid layout')
        state['layout'] = copy.deepcopy(data)
    elif route == '/api/event':
        if set(data) != {'action', 'target'}:
            raise ValueError('invalid event')
        text(data, 'action', 80, True)
        text(data, 'target', 120)
    else:
        raise KeyError('unknown route')


def create_server(output: Path, case: str, *, workbench_url=None):
    """Create an unstarted server; output must be a fresh child of an existing directory."""
    if not isinstance(case, str) or not re.fullmatch(r'[A-Za-z0-9_-]{1,100}', case):
        raise ValueError('invalid case')
    if workbench_url:
        parts = urlsplit(workbench_url)
        if parts.scheme != 'http' or parts.hostname not in ('127.0.0.1', 'localhost') or parts.query or parts.fragment:
            raise ValueError('workbench URL must be loopback HTTP')
    output = Path(output)
    output.mkdir(exist_ok=False)
    store = Store(output, case)

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def reply(self, status, body, mime='application/json'):
            if not isinstance(body, bytes):
                body = encoded(body)
            self.send_response(status)
            self.send_header('Content-Type', mime + ('; charset=utf-8' if mime != 'image/png' else ''))
            self.send_header('Content-Length', str(len(body)))
            self.send_header('Cache-Control', 'no-store')
            self.send_header('X-Content-Type-Options', 'nosniff')
            self.send_header('Content-Security-Policy', "default-src 'self'; style-src 'self'; img-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'self'")
            self.end_headers()
            self.wfile.write(body)

        def failure(self, status, message):
            try:
                store.transact('failure', {'method': self.command, 'path': self.path[:200]}, error=message)
            except (OSError, OverflowError) as exc:
                status, message = 503, f'journal unavailable: {exc}'
            self.reply(status, {'error': message})

        def do_GET(self):
            routes = {'/': ('index.html', 'text/html'), '/canvas': ('canvas.html', 'text/html'),
                      '/vision': ('vision.html', 'text/html'), '/app.js': ('app.js', 'text/javascript'),
                      '/canvas.js': ('canvas.js', 'text/javascript'), '/style.css': ('style.css', 'text/css')}
            if self.path in routes:
                name, mime = routes[self.path]
                self.reply(200, (WEB / name).read_bytes(), mime)
            elif self.path == '/api/data':
                self.reply(200, store.public())
            elif self.path == '/api/config':
                self.reply(200, {'case': case, 'workbench_url': workbench_url})
            elif self.path == '/vision/challenge.png':
                self.reply(200, store.image, 'image/png')
            else:
                self.failure(404, 'unknown route')

        def do_POST(self):
            # Disallow cross-origin form or script writes to the local fixture.
            origin = self.headers.get('Origin')
            if origin and origin != self.server.fixture_url.rstrip('/'):
                self.failure(403, 'foreign origin')
                return
            try:
                if self.headers.get('Transfer-Encoding'):
                    raise ValueError('transfer encoding unsupported')
                length = int(self.headers.get('Content-Length', '-1'))
                if length < 0:
                    raise ValueError('content length required')
                if length > MAX_BODY:
                    self.failure(413, 'request too large')
                    return
                if self.headers.get_content_type() != 'application/json':
                    raise ValueError('JSON required')
                self.connection.settimeout(5)
                data = json.loads(self.rfile.read(length), parse_constant=lambda _value: (_ for _ in ()).throw(ValueError('invalid number')))
                if not isinstance(data, dict):
                    raise ValueError('JSON object required')
                result = store.transact(self.path, data, lambda state: apply_action(state, self.path, data))
                self.reply(200, result)
            except KeyError:
                self.failure(404, 'unknown route')
            except (ValueError, UnicodeError, TimeoutError) as exc:
                self.failure(400, str(exc))
            except (OSError, OverflowError) as exc:
                self.reply(503, {'error': f'journal unavailable: {exc}'})

    class FixtureServer(ThreadingHTTPServer):
        request_queue_size = 64
        daemon_threads = False

        def get_request(self):
            connection, address = super().get_request()
            # Bound idle clients before BaseHTTPRequestHandler reads any headers.
            connection.settimeout(1)
            return connection, address

    server = FixtureServer(('127.0.0.1', 0), Handler)
    server.fixture_url = f'http://127.0.0.1:{server.server_port}/'
    server.store = store
    server.workbench_url = workbench_url
    return server


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--case', required=True)
    parser.add_argument('--workbench-url')
    args = parser.parse_args()
    server = create_server(args.output, args.case, workbench_url=args.workbench_url)
    worker = threading.Thread(target=server.serve_forever)
    worker.start()
    print(json.dumps({'url': server.fixture_url, 'canvas_url': server.fixture_url + 'canvas', 'state': str(args.output / 'state.json')}), flush=True)
    try:
        while not (args.output / 'stop').exists():
            time.sleep(.1)
    except KeyboardInterrupt:
        pass
    finally:
        server.shutdown()
        worker.join()
        server.server_close()


if __name__ == '__main__':
    main()
