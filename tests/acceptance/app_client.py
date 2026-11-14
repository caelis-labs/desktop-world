"""Two-tool acceptance client. No model, result polling or Agent Session API.

Uses owned stdio transports on each platform. Windows packages include only
the current transport helper, without the historical execution/result helpers.
"""
import base64
import hashlib
import json
import os
from pathlib import Path
import time


METHODS = ['look', 'capture', 'readText', 'wait', 'waitForNewWindow', 'click', 'setValue', 'move', 'drag', 'typeText', 'keypress', 'scroll', 'focus', 'quit']


def require(condition, detail):
    if not condition:
        raise AssertionError(detail)


def package_layout(package):
    """Select names from distribution/stage metadata, never from Skill paths."""
    package = Path(package)
    if (package / 'BUILD.json').is_file():
        metadata = json.loads((package / 'BUILD.json').read_text(encoding='utf-8'))
        require(type(metadata.get('schema')) is int and metadata['schema'] == 1
                and metadata.get('kind') == 'unpublished-rewrite-candidate',
                'Expected an unpublished rewrite candidate BUILD manifest')
    else:
        metadata = json.loads((package / 'build-info.json').read_text(encoding='utf-8'))
        require(metadata.get('layout') == 'multi-agent-plugin'
                and metadata.get('distribution') == 'local-development-stage',
                'Expected local development stage metadata')
    # Both recognized formats predate package_store_schema; their original
    # layout is legacy 1. Explicit unknown or malformed schema values fail.
    schema = metadata.get('package_store_schema', 1)
    require(type(schema) is int and schema in (1, 2), 'Unsupported BUILD package_store_schema')
    return ('desktop-world', 'desktop-world') if schema == 1 else ('dtw', 'dtw')


def artifact_identity(binary, package=None):
    binary = Path(binary).resolve(strict=True)
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
        'scope': 'development binary; no installed-package claim'}
    if package is not None:
        package = Path(package).resolve(strict=True)
        executable = package / 'bin' / ('dtw.exe' if os.name == 'nt' else 'dtw')
        require(executable.resolve(strict=True) == binary, 'binary is not this installed package executable')
        skill_name, _ = package_layout(package)
        files = ['skills/' + skill_name + '/' + name for name in ['SKILL.md', 'API.md', 'PATTERNS.md']]
        if (package / 'build-info.json').is_file():
            files.append('build-info.json')
        result.update(scope='exact installed binary/Skill bytes; signing and source receipts verified separately',
            package=str(package), package_files={name: hashlib.sha256((package / name).read_bytes()).hexdigest() for name in files})
    return result


class AppClient:
    def __init__(self, binary, output, *, mcp_args=()):
        self.output = Path(output)
        self.metrics = []
        self.output.mkdir(parents=True, exist_ok=True)
        self.transport = None
        try:
            if os.name == 'nt':
                from windows.transport import Mcp
                self.transport = Mcp(binary, self.output / 'mcp', options=mcp_args)
            else:
                from standard_agent_probe import Mcp
                self.output.mkdir(parents=True, exist_ok=True)
                self.transport = Mcp(binary, self.output, mcp_args=mcp_args)
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
        started_unix_ms = time.time() * 1000
        # Both transports retain the original wire frames. Decode the same
        # envelope here rather than using legacy execute()/receipt() helpers.
        response = self.transport.rpc('tools/call', {'name': name, 'arguments': arguments})
        data = response.get('structuredContent')
        texts = [c['text'] for c in response.get('content', []) if c.get('type') == 'text']
        require(isinstance(data, dict) and len(texts) == 1 and json.loads(texts[0]) == data, response)
        self.metrics.append({'tool': name, 'seconds': time.monotonic() - started,
            'started_unix_ms': started_unix_ms, 'ended_unix_ms': time.time() * 1000,
            'source_bytes': len(arguments.get('source', '').encode()),
            'json_bytes': len(json.dumps(data, ensure_ascii=False, separators=(',', ':')).encode()),
            'image_count': sum(c.get('type') == 'image' for c in response.get('content', [])),
            'is_error': response.get('isError') is True,
            'execution_id': data.get('execution_id'), 'error_code': data.get('error', {}).get('code'),
            'effects': data.get('effects', {})})
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
