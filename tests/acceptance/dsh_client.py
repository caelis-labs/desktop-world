"""DSH's shipped ACP profile, with one owned persistent model session.

This is a protocol client, not a model/tool executor. The installed DSH runtime
owns model requests, bundled Skills, the full Cordis plugin and MCP lifecycle.
Raw transcripts can contain model/private data and belong in ignored artifacts.
"""
import json
import os
from pathlib import Path
import queue
import subprocess
import threading
import time


class DshClient:
    def __init__(self, command, patch, output, cwd):
        self.output = Path(output).resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        self.counter = 0
        self.pending = {}
        self.methods = {}
        self.owned_sessions = set()
        self.controller_stop = threading.Event()
        self.events = []
        self.queue = queue.Queue()
        self.lock = threading.Lock()
        self.log = (self.output / 'acp.jsonl').open('w', encoding='utf-8')
        self.stderr = (self.output / 'stderr.log').open('wb')
        self.process = subprocess.Popen([str(command), '--profile', 'acp', '--patch', str(patch)],
            cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr,
            env={**os.environ, 'DTW_INPUT_TRACE': '1'})
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()
        self.controller = threading.Thread(target=self._control, daemon=True)
        self.controller.start()

    def _control(self):
        # A supervisor can stop this exact owned session through the public ACP
        # method without terminating the host or discovering a private RPC.
        marker = self.output / 'cancel-session'
        while not self.controller_stop.wait(.05):
            if not marker.exists():
                continue
            try:
                session = marker.read_text(encoding='utf-8-sig').strip()
                if session not in self.owned_sessions:
                    continue
                self.send({'jsonrpc': '2.0', 'method': 'session/cancel', 'params': {'sessionId': session}})
                return
            except (OSError, ValueError) as error:
                self._record('controller_error', str(error))
                return

    def _record(self, direction, value):
        with self.lock:
            self.log.write(json.dumps({'time_ms': time.time() * 1000, 'direction': direction,
                'message': value}, ensure_ascii=True) + '\n')
            self.log.flush()

    def _read(self):
        for line in self.process.stdout:
            try:
                message = json.loads(line.decode('utf-8'))
            except (UnicodeDecodeError, json.JSONDecodeError):
                self.queue.put(RuntimeError('DSH emitted non-JSON ACP stdout; see transcript'))
                self._record('invalid_stdout', line.decode('utf-8', errors='replace'))
                continue
            self._record('receive', message)
            self.queue.put(message)
        self.queue.put(EOFError('DSH ACP stdout closed'))

    def send(self, message):
        self._record('send', message)
        self.process.stdin.write((json.dumps(message, ensure_ascii=True) + '\n').encode('utf-8'))
        self.process.stdin.flush()

    def begin(self, method, params):
        self.counter += 1
        self.methods[self.counter] = method
        self.send({'jsonrpc': '2.0', 'id': self.counter, 'method': method, 'params': params})
        return self.counter

    def receive(self, request, timeout=30):
        until = time.monotonic() + timeout
        while request not in self.pending:
            message = self.queue.get(timeout=max(.001, until - time.monotonic()))
            if isinstance(message, BaseException):
                raise message
            if 'method' in message:
                self.events.append(message)
                if message['method'] == 'session/request_permission':
                    options = message['params']['options']
                    selected = next((option for option in options if option['kind'] == 'allow_once'), None)
                    if selected is None:
                        raise RuntimeError('DSH offered no one-time permission option')
                    self.send({'jsonrpc': '2.0', 'id': message['id'], 'result': {
                        'outcome': {'outcome': 'selected', 'optionId': selected['optionId']}}})
                elif 'id' in message:
                    self.send({'jsonrpc': '2.0', 'id': message['id'], 'error': {
                        'code': -32601, 'message': 'Unsupported acceptance-client request'}})
            elif 'id' in message:
                self.pending[message['id']] = message
        response = self.pending.pop(request)
        if 'error' in response:
            raise RuntimeError(response['error'])
        result = response['result']
        if self.methods.pop(request, None) in ('session/new', 'session/resume'):
            self.owned_sessions.add(result['sessionId'])
        return result

    def call(self, method, params, timeout=30):
        return self.receive(self.begin(method, params), timeout)

    def close(self):
        self.controller_stop.set()
        self.controller.join(timeout=2)
        input_error = None
        try:
            self.process.stdin.close()
        except OSError as error:
            # An externally stopped owned host can leave a broken pipe. Still
            # join its reader and record the real process exit, never success.
            input_error = str(error)
        forced = False
        try:
            result = self.process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            forced = True
            self.process.terminate()
            result = self.process.wait(timeout=5)
        self.reader.join(timeout=2)
        self.stderr.close()
        self.log.close()
        return {'pid': self.process.pid, 'exit_code': result, 'forced': forced,
            'reader_joined': not self.reader.is_alive(), 'controller_joined': not self.controller.is_alive(),
            'stdin_close_error': input_error}
