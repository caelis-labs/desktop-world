#!/usr/bin/env python3
"""Headless checks of exact packaged CLI/MCP bytes; never enumerate a desktop."""
import argparse
import hashlib
import json
from pathlib import Path
import queue
import subprocess
import threading
import time


MAX_OUTPUT = 4 * 1024 * 1024
MAX_LINE = 1024 * 1024
PROTOCOL = '2025-06-18'
EOF = object()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(65536), b''):
            result.update(block)
    return result.hexdigest()


class OwnedStdio:
    """All pipe operations run on bounded, joinable workers, including writes."""
    def __init__(self, command, directory, *, rpc, popen):
        self.directory = directory
        self.rpc_mode = rpc
        self.messages = queue.Queue(maxsize=32)
        self.writes = queue.Queue(maxsize=8)
        self.faults = []
        self.lock = threading.Lock()
        self.stdin_closed = threading.Event()
        self.proc = popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, bufsize=0)
        self.threads = [
            threading.Thread(target=self.read, args=(self.proc.stdout, 'stdout', rpc), daemon=True),
            threading.Thread(target=self.read, args=(self.proc.stderr, 'stderr', False), daemon=True),
            threading.Thread(target=self.write, daemon=True),
        ]
        for thread in self.threads:
            thread.start()

    def fault(self, message):
        with self.lock:
            if message not in self.faults:
                self.faults.append(message)

    def check(self):
        with self.lock:
            require(not self.faults, '; '.join(self.faults))

    def put(self, item):
        try:
            self.messages.put_nowait(item)
        except queue.Full:
            self.fault('Too many MCP output frames')

    def read(self, stream, name, rpc):
        total = 0
        pending = b''
        try:
            with (self.directory / (name + '.bin')).open('wb') as log:
                while True:
                    block = stream.read(65536)
                    if not block:
                        break
                    log.write(block[:max(0, MAX_OUTPUT - total)])
                    log.flush()
                    total += len(block)
                    if total > MAX_OUTPUT:
                        self.fault(name + ' exceeds retained output limit')
                        continue
                    if not rpc:
                        continue
                    pending += block
                    while b'\n' in pending:
                        line, pending = pending.split(b'\n', 1)
                        require(len(line) <= MAX_LINE, 'MCP frame exceeds limit')
                        value = json.loads(line)
                        require(isinstance(value, dict), 'MCP frame is not an object')
                        self.put(value)
                    require(len(pending) <= MAX_LINE, 'MCP frame exceeds limit')
                require(not rpc or not pending, 'MCP stdout ended with a partial frame')
        except BaseException as error:
            self.fault(name + ': ' + str(error))
        finally:
            stream.close()
            if rpc:
                self.put(EOF)

    def write(self):
        try:
            with (self.directory / 'requests.jsonl').open('wb') as log:
                while True:
                    task = self.writes.get()
                    if task is EOF:
                        break
                    payload, finished = task
                    try:
                        log.write(payload)
                        log.flush()
                        remaining = memoryview(payload)
                        while remaining:
                            count = self.proc.stdin.write(remaining)
                            require(count is not None and count > 0, 'MCP stdin did not advance')
                            remaining = remaining[count:]
                    except BaseException as error:
                        self.fault('stdin: ' + str(error))
                        break
                    finally:
                        finished.set()
        except BaseException as error:
            self.fault('stdin writer: ' + str(error))
        finally:
            try:
                self.proc.stdin.close()
            except OSError as error:
                self.fault('stdin close: ' + str(error))
            self.stdin_closed.set()

    def send(self, value, until):
        payload = (json.dumps(value, separators=(',', ':')) + '\n').encode('utf-8')
        require(len(payload) < 4096, 'Unexpected large metadata request')
        finished = threading.Event()
        self.writes.put_nowait((payload, finished))
        while not finished.wait(timeout=min(.05, remaining(until))):
            self.check()
        self.check()

    def rpc(self, identifier, method, params, until):
        self.send({'jsonrpc': '2.0', 'id': identifier, 'method': method, 'params': params}, until)
        while True:
            self.check()
            try:
                value = self.messages.get(timeout=min(.05, remaining(until)))
            except queue.Empty:
                continue
            require(value is not EOF, 'MCP stdout closed before its response')
            require(value.get('jsonrpc') == '2.0', 'Invalid JSON-RPC version')
            if notification(value):
                continue  # Notifications remain retained in the raw stdout log.
            require(type(value.get('id')) is int and value['id'] == identifier,
                    'Unexpected MCP response identity')
            require('error' not in value and isinstance(value.get('result'), dict),
                    'MCP request failed: ' + json.dumps(value))
            return value['result']

    def close(self, grace):
        errors = []
        forced = False
        try:
            self.writes.put_nowait(EOF)
        except queue.Full:
            errors.append('MCP writer did not accept EOF')
        try:
            code = self.proc.wait(timeout=max(.01, grace))
        except subprocess.TimeoutExpired:
            forced = True
            try:
                self.proc.kill()  # Only this exact owned Popen handle.
                code = self.proc.wait(timeout=2)
            except (OSError, subprocess.TimeoutExpired) as error:
                errors.append('Owned process reclamation failed: ' + str(error))
                code = self.proc.poll()
        for thread in self.threads:
            thread.join(timeout=1)
        joined = all(not thread.is_alive() for thread in self.threads)
        if not joined:
            errors.append('Owned pipe workers did not join')
        if self.rpc_mode and joined:
            while True:
                try:
                    value = self.messages.get_nowait()
                except queue.Empty:
                    break
                if value is not EOF and not notification(value):
                    self.fault('Unexpected MCP response after metadata completion')
        return {'pid': self.proc.pid, 'command': self.proc.args, 'exit_code': code,
                'forced': forced, 'stdin_closed': self.stdin_closed.is_set(),
                'pipe_workers_joined': joined, 'errors': errors, 'io_faults': list(self.faults)}


def notification(value):
    return (isinstance(value, dict) and value.get('jsonrpc') == '2.0'
            and isinstance(value.get('method'), str) and 'id' not in value
            and 'result' not in value and 'error' not in value)


def remaining(until):
    left = until - time.monotonic()
    if left <= 0:
        raise TimeoutError('Packaged metadata probe deadline expired')
    return left


def clean(record):
    return (record['exit_code'] == 0 and not record['forced'] and record['stdin_closed']
            and record['pipe_workers_joined'] and not record['errors'] and not record['io_faults'])


def check(binary, version, output, *, timeout=20, cleanup_timeout=2, popen=subprocess.Popen):
    binary = Path(binary).resolve(strict=True)
    output = Path(output).resolve()
    require(not output.exists(), 'Use a new output directory; evidence is never overwritten')
    output.mkdir(parents=True)
    until = time.monotonic() + timeout
    report = {'passed': False, 'binary': str(binary), 'binary_sha256': digest(binary),
              'expected_version': version, 'scope': 'CLI and MCP metadata only; no GUI acceptance',
              'desktop_tools_called': [], 'cleanup': []}
    active = None
    try:
        directory = output / 'version'
        directory.mkdir()
        active = OwnedStdio([str(binary), '--version'], directory, rpc=False, popen=popen)
        status = active.close(min(5, remaining(until)))
        active = None
        report['cleanup'].append({'role': 'version', **status})
        require(clean(status), 'Packaged --version process did not exit cleanly')
        observed = (directory / 'stdout.bin').read_bytes().decode('utf-8').strip()
        report['observed_version'] = observed
        require(observed == 'dtw ' + version, 'Packaged CLI version mismatch: ' + observed)

        directory = output / 'mcp'
        directory.mkdir()
        active = OwnedStdio([str(binary), 'mcp'], directory, rpc=True, popen=popen)
        initialized = active.rpc(1, 'initialize', {'protocolVersion': PROTOCOL, 'capabilities': {},
            'clientInfo': {'name': 'dtw-packaged-headless-check', 'version': '1'}}, until)
        require(initialized.get('protocolVersion') == PROTOCOL, 'Unexpected MCP protocol negotiation')
        require(initialized.get('serverInfo', {}).get('name') == 'desktop-world'
                and initialized['serverInfo'].get('version') == version, 'Packaged MCP identity/version mismatch')
        require(isinstance(initialized.get('capabilities', {}).get('tools'), dict), 'MCP tool capability missing')
        active.send({'jsonrpc': '2.0', 'method': 'notifications/initialized'}, until)
        listed = active.rpc(2, 'tools/list', {}, until)
        tools = listed.get('tools')
        require(isinstance(tools, list) and all(isinstance(tool, dict) for tool in tools), 'Invalid MCP tools list')
        names = [tool.get('name') for tool in tools]
        require(all(isinstance(name, str) and name for name in names) and len(names) == len(set(names))
                and {'apps', 'api', 'exec', 'result'} <= set(names) and not listed.get('nextCursor'),
                'Missing, duplicate or incomplete packaged MCP tools')
        response = active.rpc(3, 'tools/call', {'name': 'api', 'arguments': {'methods': ['windows']}}, until)
        require(response.get('isError') is not True, 'Metadata API returned an error result')
        data = response.get('structuredContent')
        content = response.get('content')
        require(isinstance(data, dict) and isinstance(content, list) and len(content) == 1
                and content[0].get('type') == 'text' and json.loads(content[0]['text']) == data,
                'Metadata API text/structured projections disagree')
        method = data.get('methods', {}).get('windows', {})
        require(set(data.get('methods', {})) == {'windows'} and 'parameters' in method and method['parameters'] is None
                and isinstance(method.get('returns'), dict) and 'error' not in data,
                'Packaged metadata schema missing or invalid')
        report['metadata'] = {'server': initialized['serverInfo'], 'protocol': PROTOCOL,
                              'tools': names, 'api_methods': ['windows']}
        require(active.proc.poll() is None, 'MCP server exited before client EOF')
        status = active.close(min(cleanup_timeout, remaining(until)))
        active = None
        report['cleanup'].append({'role': 'mcp', **status})
        require(clean(status), 'Packaged MCP EOF cleanup was not clean')
        require(digest(binary) == report['binary_sha256'], 'Packaged binary changed during probe')
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
    finally:
        if active is not None:
            report['cleanup'].append({'role': 'interrupted', **active.close(cleanup_timeout)})
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    report = check(args.dtw, args.version, args.output)
    print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
