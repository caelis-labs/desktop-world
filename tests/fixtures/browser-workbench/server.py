#!/usr/bin/env python3
"""Owned local business UI for native-browser acceptance; no browser automation.

The model uses the rendered app only. The supervisor verifies state.json, which
is never exposed by HTTP, and retains every save/event rather than replaying.
The separate /reliability page exposes only its own diagnostic counts and events.
"""
import argparse
import hashlib
import json
import math
import secrets
import struct
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import threading
import time
import zlib


POINTER_EVENTS = frozenset(('pointerdown', 'pointerup', 'pointermove', 'mousedown',
                            'mouseup', 'mousemove', 'click', 'keydown', 'focus', 'blur',
                            'submit', 'ready', 'settling', 'mode_changed', 'icon', 'link'))

# A tiny fixed raster font keeps the image-channel control dependency-free.
# Exclude easily confused I/1, O/0, B/8, S/5, G/6, Q/9 and Z/2 pairs.
VISION_GLYPHS = {
    'A': ('01110','10001','10001','11111','10001','10001','10001'),
    'C': ('01111','10000','10000','10000','10000','10000','01111'),
    'D': ('11110','10001','10001','10001','10001','10001','11110'),
    'E': ('11111','10000','10000','11110','10000','10000','11111'),
    'F': ('11111','10000','10000','11110','10000','10000','10000'),
    'H': ('10001','10001','10001','11111','10001','10001','10001'),
    'J': ('00111','00010','00010','00010','10010','10010','01100'),
    'K': ('10001','10010','10100','11000','10100','10010','10001'),
    'M': ('10001','11011','10101','10101','10001','10001','10001'),
    'N': ('10001','11001','11001','10101','10011','10011','10001'),
    'P': ('11110','10001','10001','11110','10000','10000','10000'),
    'R': ('11110','10001','10001','11110','10100','10010','10001'),
    'T': ('11111','00100','00100','00100','00100','00100','00100'),
    'U': ('10001','10001','10001','10001','10001','10001','01110'),
    'V': ('10001','10001','10001','10001','10001','01010','00100'),
    'W': ('10001','10001','10001','10101','10101','11011','10001'),
    'X': ('10001','10001','01010','00100','01010','10001','10001'),
    'Y': ('10001','10001','01010','00100','00100','00100','00100'),
    '2': ('01110','10001','00001','00010','00100','01000','11111'),
    '3': ('11110','00001','00001','01110','00001','00001','11110'),
    '4': ('00010','00110','01010','10010','11111','00010','00010'),
    '7': ('11111','00001','00010','00100','01000','01000','01000'),
    '9': ('01110','10001','10001','01111','00001','00001','01110'),
}


def vision_png(token):
    """Render pixels only: no text chunk, token metadata or external font file."""
    width, height, scale = 640, 160, 10
    pixels = bytearray(b'\xff' * (width * height * 3))
    for index, char in enumerate(token):
        for row, bits in enumerate(VISION_GLYPHS[char]):
            for column, bit in enumerate(bits):
                if bit == '1':
                    for dy in range(scale):
                        start = ((45 + row*scale + dy)*width + 40 + index*70 + column*scale)*3
                        pixels[start:start+scale*3] = b'\x12\x20\x2b' * scale
    def chunk(kind, value):
        return struct.pack('>I', len(value)) + kind + value + struct.pack('>I', zlib.crc32(kind+value))
    rows = b''.join(b'\0'+pixels[y*width*3:(y+1)*width*3] for y in range(height))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


def create_server(output, case, file_task=False):
    """Create a loopback-only fixture, without opening a browser or operating UI."""
    output = Path(output)
    if not case.replace('-', '').isalnum():
        raise ValueError('case must contain only letters, digits and hyphens')
    output.mkdir(parents=True, exist_ok=True)
    items = [{'sku': f'DTW-{i:04}', 'name': f'Field kit {i:03}',
              'warehouse': ['North', 'South', 'Central'][i % 3],
              'quantity': 10 + i % 25, 'reviewed': False, 'note': ''} for i in range(1, 241)]
    reliability = {'events': [], 'attempts': [], 'submissions': [], 'icon_activations': 0}
    state = {'case': case, 'items': items, 'saves': [], 'events': [], 'uploads': [],
             'ready': False, 'reliability': reliability}
    lock = threading.Lock()
    html = Path(__file__).with_name('index.html').read_text(encoding='utf-8').replace('__CASE__', case)
    # Existing acceptance probes copy only server.py/index.html before adding
    # instrumentation. Keep that original layout usable without this extra page.
    probe_path = Path(__file__).with_name('reliability.html')
    probe_html = (probe_path.read_text(encoding='utf-8').replace('__CASE__', case).encode('utf-8')
                  if probe_path.exists() else None)
    vision_path = Path(__file__).with_name('vision.html')
    vision_html = (vision_path.read_text(encoding='utf-8').replace('__CASE__', case).encode('utf-8')
                   if vision_path.exists() else None)
    vision = None
    challenge = None
    if vision_html is not None:
        vision = {'expected_token': ''.join(secrets.choice(tuple(VISION_GLYPHS)) for _ in range(8)), 'attempts': []}
        state['vision'] = vision
        challenge = vision_png(vision['expected_token'])
    attachment = '''<section aria-label="Review attachment"><h2>Review attachment</h2>
<label>Attach review evidence <input id="attachment" type="file" accept=".txt"></label>
<p id="attachment-status" role="status">No attachment received</p></section>'''
    script = '''byId('attachment').onchange=async()=>{const file=byId('attachment').files[0];if(!file)return;
const response=await fetch('/api/attachment',{method:'POST',headers:{'Content-Type':'application/json'},
body:JSON.stringify({name:file.name,text:await file.text()})});
byId('attachment-status').textContent=response.ok?'Received attachment: '+file.name:'Attachment failed';};'''
    html = html.replace('__ATTACHMENT_SECTION__', attachment if file_task else '')
    html = html.replace('__ATTACHMENT_SCRIPT__', script if file_task else '').encode('utf-8')

    def persist():
        path = output / 'state.json'
        # Browser strings are UTF-16. A real keyboard input event can contain
        # one surrogate before the next event completes an emoji. JSON escapes
        # preserve that exact event without poisoning all later UTF-8 writes.
        path.with_suffix('.tmp').write_text(json.dumps(state, ensure_ascii=True), encoding='utf-8')
        # Windows indexing/antivirus/readers can briefly hold the destination
        # without FILE_SHARE_DELETE. Retry this same publication only, never an
        # input, event append or submit effect. A persistent failure stays visible.
        for attempt in range(21):
            try:
                path.with_suffix('.tmp').replace(path)
                break
            except PermissionError:
                if attempt == 20:
                    raise
                time.sleep(0.025)

    def text(data, key, limit):
        value = data.get(key)
        if not isinstance(value, str) or not value or len(value) > limit:
            raise ValueError(key)
        return value

    def pointer_event(data):
        kind = text(data, 'kind', 32)
        if kind not in POINTER_EVENTS:
            raise ValueError('event kind')
        result = {'kind': kind, 'run': text(data, 'run', 80), 'frame': text(data, 'frame', 96)}
        for key in ('trusted', 'primary'):
            if key in data:
                if type(data[key]) is not bool:
                    raise ValueError(key)
                result[key] = data[key]
        for key in ('button', 'buttons', 'detail', 'pointer_id', 'x', 'y', 'client_ms'):
            if key in data:
                value = data[key]
                if type(value) not in (int, float) or not math.isfinite(value) or abs(value) > 1e12:
                    raise ValueError(key)
                result[key] = value
        for key in ('key', 'mode', 'target'):
            if key in data:
                result[key] = text(data, key, 96)
        if 'pointer_type' in data:
            value = data['pointer_type']
            if not isinstance(value, str) or len(value) > 32:
                raise ValueError('pointer_type')
            result['pointer_type'] = value
        return result

    def attempt(data):
        result = {key: text(data, key, limit) for key, limit in
                  (('nonce', 96), ('frame', 96), ('run', 80), ('title', 128), ('mode', 32))}
        if result['mode'] not in ('normal', 'trusted-pointer') or type(data.get('ready')) is not bool:
            raise ValueError('attempt mode/ready')
        gesture = data.get('gesture')
        if (not isinstance(gesture, dict)
                or any(type(gesture.get(key)) is not bool for key in ('down', 'up', 'click'))):
            raise ValueError('gesture')
        result['ready'] = data['ready']
        result['gesture'] = {key: gesture[key] for key in ('down', 'up', 'click')}
        result['duplicate'] = any(x['nonce'] == result['nonce'] for x in reliability['attempts'])
        result['accepted'] = (not result['duplicate'] and result['ready']
                              and (result['mode'] == 'normal' or all(result['gesture'].values())))
        result['reason'] = ('duplicate_nonce' if result['duplicate'] else
                            'not_ready' if not result['ready'] else
                            'accepted' if result['accepted'] else 'trusted_pointer_required')
        return result

    persist()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def reply(self, status, body, kind='application/json'):
            self.send_response(status)
            self.send_header('Content-Type', kind if kind == 'image/png' else kind + '; charset=utf-8')
            self.send_header('Content-Length', str(len(body)))
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path == '/':
                self.reply(200, html, 'text/html')
            elif self.path in ('/reliability', '/reliability/') and probe_html is not None:
                self.reply(200, probe_html, 'text/html')
            elif self.path in ('/vision', '/vision/') and vision_html is not None:
                self.reply(200, vision_html, 'text/html')
            elif self.path == '/vision/challenge.png' and challenge is not None:
                self.reply(200, challenge, 'image/png')
            elif self.path == '/api/reliability':
                with lock:
                    counts = {}
                    for row in reliability['events']:
                        counts[row['kind']] = counts.get(row['kind'], 0) + 1
                    result = {'case': case, 'event_counts': counts,
                              'attempt_count': len(reliability['attempts']),
                              'submission_count': len(reliability['submissions']),
                              'icon_activations': reliability['icon_activations'],
                              'attempts': reliability['attempts'][-20:],
                              'events': reliability['events'][-40:]}
                    self.reply(200, json.dumps(result, ensure_ascii=True).encode('utf-8'))
            elif self.path == '/api/items':
                with lock:
                    self.reply(200, json.dumps(items).encode('utf-8'))
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
                    elif self.path == '/api/reliability/event':
                        if len(reliability['events']) >= 10000:
                            raise ValueError('reliability event capacity')
                        row = pointer_event(data)
                        row.update(sequence=len(reliability['events']) + 1, unix_ms=int(time.time() * 1000))
                        reliability['events'].append(row)
                        if row['kind'] == 'icon':
                            reliability['icon_activations'] += 1
                        result = {'sequence': row['sequence']}
                    elif self.path == '/api/reliability/attempt':
                        if len(reliability['attempts']) >= 1000:
                            raise ValueError('reliability attempt capacity')
                        row = attempt(data)
                        row.update(sequence=len(reliability['attempts']) + 1, unix_ms=int(time.time() * 1000))
                        reliability['attempts'].append(row)
                        if row['accepted']:
                            reliability['submissions'].append(dict(row))
                        result = {**row, 'submission_count': len(reliability['submissions'])}
                    elif self.path == '/api/vision/verify' and vision is not None:
                        value = data.get('value')
                        if (not isinstance(value, str) or len(value) != 8 or not value.isascii()
                                or not value.isalnum() or len(vision['attempts']) >= 8):
                            raise ValueError('visual token')
                        vision_attempt = {'submitted': value, 'passed': value == vision['expected_token'],
                                          'unix_ms': int(time.time() * 1000)}
                        vision['attempts'].append(vision_attempt)
                        result = {'passed': vision_attempt['passed'], 'attempt_count': len(vision['attempts'])}
                    elif self.path == '/api/attachment' and file_task:
                        if (not isinstance(data.get('name'), str) or not data['name']
                                or len(data['name']) > 256 or '/' in data['name'] or '\\' in data['name']
                                or not isinstance(data.get('text'), str)
                                or len(data['text'].encode('utf-8')) > 4096):
                            raise ValueError('attachment')
                        result = {'name': data['name'],
                                  'sha256': hashlib.sha256(data['text'].encode('utf-8')).hexdigest()}
                        state['uploads'].append({**result, 'unix_ms': int(time.time() * 1000)})
                    else:
                        raise ValueError('route')
                    persist()
                self.reply(200, json.dumps(result, ensure_ascii=True).encode('utf-8'))
            except (ValueError, KeyError, StopIteration):
                self.reply(400, b'{"error":"invalid input"}')

    return ThreadingHTTPServer(('127.0.0.1', 0), Handler)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', required=True)
    parser.add_argument('--file-task', action='store_true', help='Enable the real OS file-picker task')
    args = parser.parse_args()
    if not args.case.replace('-', '').isalnum():
        parser.error('case must contain only letters, digits and hyphens')
    server = create_server(args.output, args.case, args.file_task)
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
