"""Two-tool acceptance client. No model, result polling or Agent Session API.

Uses the existing owned stdio transports on each platform. Legacy execution
helpers on those transports are deliberately not part of this client.
"""
import base64
import hashlib
import json
import os
from pathlib import Path
import time


METHODS = ['look', 'capture', 'click', 'move', 'drag', 'typeText', 'keypress', 'scroll', 'quit']


def require(condition, detail):
    if not condition:
        raise AssertionError(detail)


def artifact_identity(binary, package=None):
    binary = Path(binary).resolve(strict=True)
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
        'scope': 'development binary; no installed-package claim'}
    if package is not None:
        package = Path(package).resolve(strict=True)
        executable = package / 'bin' / ('dtw.exe' if os.name == 'nt' else 'dtw')
        require(executable.resolve(strict=True) == binary, 'binary is not this installed package executable')
        files = ['skills/desktop-world/' + name for name in ['SKILL.md', 'API.md', 'PATTERNS.md']]
        if (package / 'build-info.json').is_file():
            files.append('build-info.json')
        result.update(scope='exact installed binary/Skill bytes; signing and source receipts verified separately',
            package=str(package), package_files={name: hashlib.sha256((package / name).read_bytes()).hexdigest() for name in files})
    return result


class AppClient:
    def __init__(self, binary, output):
        self.output = Path(output)
        self.metrics = []
        self.output.mkdir(parents=True, exist_ok=True)
        self.transport = None
        try:
            if os.name == 'nt':
                from windows.cursor_probe import Mcp
                self.transport = Mcp(binary, self.output / 'mcp')
            else:
                from standard_agent_probe import Mcp
                self.output.mkdir(parents=True, exist_ok=True)
                self.transport = Mcp(binary, self.output)
                self.transport.rpc('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                    'clientInfo': {'name': 'dtw-app-protocol-acceptance', 'version': '1'}})
                self.transport.notify('notifications/initialized')
            tools = self.transport.rpc('tools/list', {})['tools']
            require([t['name'] for t in tools] == ['apps', 'exec'], tools)
            execution = tools[1]['inputSchema']
            require(execution['required'] == ['app', 'source'], execution)
            require(set(execution['properties']) == {'app', 'source'}, execution)
        except BaseException:
            if self.transport:
                self.transport.close()
            raise

    def tool(self, name, arguments):
        started = time.monotonic()
        # Both transports retain the original wire frames. Decode the same
        # envelope here rather than using legacy execute()/receipt() helpers.
        response = self.transport.rpc('tools/call', {'name': name, 'arguments': arguments})
        data = response.get('structuredContent')
        texts = [c['text'] for c in response.get('content', []) if c.get('type') == 'text']
        require(isinstance(data, dict) and len(texts) == 1 and json.loads(texts[0]) == data, response)
        self.metrics.append({'tool': name, 'seconds': time.monotonic() - started,
            'source_bytes': len(arguments.get('source', '').encode()),
            'json_bytes': len(json.dumps(data, ensure_ascii=False, separators=(',', ':')).encode()),
            'image_count': sum(c.get('type') == 'image' for c in response.get('content', [])),
            'is_error': response.get('isError') is True})
        return data, response

    def execute(self, app, source, *, allow_error=False):
        data, response = self.tool('exec', {'app': str(app), 'source': source})
        if not allow_error:
            require(response.get('isError') is not True and data.get('status') == 'completed', data)
        return data, response

    def capture(self, app, name):
        data, response = self.execute(app, 'await app.capture({full:true});')
        require('error' not in data.get('view', {}).get('image', {}), data)
        images = [c for c in response['content'] if c['type'] == 'image']
        require(len(images) == 1, response)
        payload = base64.b64decode(images[0]['data'], validate=True)
        require(payload.startswith(b'\x89PNG\r\n\x1a\n'), 'invalid PNG')
        (self.output / (name + '.png')).write_bytes(payload)
        return data

    def begin_execute(self, app, source):
        return self.transport.begin('tools/call', {'name': 'exec',
            'arguments': {'app': str(app), 'source': source}})

    def cancel(self, request_id):
        params = {'requestId': request_id, 'reason': 'owned acceptance interruption'}
        if os.name == 'nt':
            self.transport.send({'jsonrpc': '2.0', 'method': 'notifications/cancelled', 'params': params})
        else:
            self.transport.notify('notifications/cancelled', params)

    def close(self):
        try:
            (self.output / 'protocol-metrics.json').write_text(json.dumps(self.metrics, indent=2) + '\n')
        finally:
            result = self.transport.close()
        return result
