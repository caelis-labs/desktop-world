"""Owned multi-engine acceptance fixture lifecycle; no DTW or UI input shortcuts."""
import hashlib
import importlib.util
import json
from pathlib import Path
import platform
import plistlib
import re
import subprocess
import threading
import time
import uuid


HERE = Path(__file__).resolve().parent


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def read_json(path):
    try:
        return json.loads(Path(path).read_text(encoding='utf-8'))
    except (FileNotFoundError, json.JSONDecodeError):
        return None  # Atomic publications may not exist before startup.


def write_json(path, value):
    path = Path(path)
    temporary = path.with_suffix('.tmp')
    temporary.write_text(json.dumps(value, ensure_ascii=True, indent=2) + '\n', encoding='utf-8')
    temporary.replace(path)


def source_identity():
    paths = sorted(p for p in HERE.rglob('*') if p.is_file()
                   and '__pycache__' not in p.parts and p.suffix in {'.py', '.swift', '.html', '.js', '.json', '.css'})
    workbench = HERE.parent / 'browser-workbench'
    paths.extend(workbench / name for name in ('server.py', 'index.html', 'reliability.html', 'vision.html'))
    return {str(path.relative_to(HERE.parent)): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}


class ComputerUseFixture:
    def __init__(self, output, case):
        if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9-]{0,63}', case):
            raise ValueError('case must be 1–64 ASCII letters, digits or hyphens')
        self.output = Path(output).resolve()
        self.case = case
        self.bundle = self.output / 'ComputerUse.app'
        self.executable = self.bundle / 'Contents/MacOS/ComputerUse'
        self.servers = []
        self.child = None
        self.log = None
        self.web_url = None
        self.workbench_url = None
        self.cleanup = None

    def _serve(self, server):
        # Both the reused workbench and new pages own their request handlers.
        # Install the deadline at accept time, before even partial HTTP headers.
        accept = server.get_request
        def bounded_request():
            connection, address = accept()
            connection.settimeout(1)
            return connection, address
        server.get_request = bounded_request
        server.daemon_threads = False
        thread = threading.Thread(target=server.serve_forever, name='computer-use-http', daemon=True)
        self.servers.append((server, thread))
        thread.start()

    def prepare(self, *, build=True, activate=False, role='target', compiled_native=None):
        self.output.mkdir(parents=True, exist_ok=False)
        self.output.chmod(0o700)
        try:
            original = load_module('computer_use_original_workbench', HERE.parent / 'browser-workbench/server.py')
            workbench = original.create_server(self.output / 'workbench', self.case, file_task=True)
            self.workbench_url = f'http://127.0.0.1:{workbench.server_port}/'
            self._serve(workbench)
            web = load_module('computer_use_web_server', HERE / 'web_server.py')
            server = web.create_server(self.output / 'web', self.case, workbench_url=self.workbench_url)
            self.web_url = server.fixture_url
            self._serve(server)
            write_json(self.output / 'config.json', {'schema_version': 1, 'case': self.case,
                'web_url': self.web_url, 'workbench_url': self.workbench_url, 'activate': activate, 'role': role})
            write_json(self.output / 'fixture-source.json', source_identity())
            if build:
                self.build(compiled_native=compiled_native)
            return self
        except BaseException:
            self.close()
            raise

    def build(self, *, compiled_native=None):
        self.executable.parent.mkdir(parents=True, exist_ok=False)
        metadata = {'CFBundleExecutable': self.executable.name,
            'CFBundleIdentifier': 'dev.caelis.dtw.computeruse.' + uuid.uuid4().hex,
            'CFBundleName': 'DTW Computer Use', 'CFBundlePackageType': 'APPL',
            'CFBundleVersion': '1', 'NSPrincipalClass': 'NSApplication',
            'NSHighResolutionCapable': True,
            'NSAppTransportSecurity': {'NSAllowsLocalNetworking': True}}
        with (self.bundle / 'Contents/Info.plist').open('wb') as stream:
            plistlib.dump(metadata, stream)
        if compiled_native is not None:
            import shutil
            shutil.copy2(Path(compiled_native).resolve(strict=True), self.executable)
            return self.bundle
        sources = sorted((HERE / 'macos').glob('*.swift'))
        if not sources:
            raise RuntimeError('native fixture sources are missing')
        with (self.output / 'build.log').open('w') as log:
            subprocess.run(['xcrun', 'swiftc', '-target', platform.machine() + '-apple-macos14.2',
                '-module-cache-path', str(self.output / 'swift-cache'),
                *map(str, sources), '-framework', 'AppKit', '-framework', 'WebKit',
                '-o', str(self.executable)], stdout=log, stderr=subprocess.STDOUT, check=True, timeout=120)
        return self.bundle

    def start(self):
        if self.child is not None:
            raise RuntimeError('fixture already started')
        self.log = (self.output / 'native.log').open('w')
        self.child = subprocess.Popen([str(self.executable)], stdout=self.log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if self.child.poll() is not None:
                raise RuntimeError(f'fixture exited during startup: {self.child.returncode}')
            state = self.state()
            if state and state.get('ready'):
                return state
            time.sleep(0.02)
        raise TimeoutError('native fixture did not publish ready state')

    def state(self):
        state = read_json(self.output / 'native-state.json')
        if state is not None:
            if self.child is not None and state['pid'] != self.child.pid:
                raise RuntimeError('native state belongs to a different process')
            if state.get('write_error'):
                raise RuntimeError('native fixture evidence write failed: ' + str(state['write_error']))
        return state

    def web_state(self):
        return read_json(self.output / 'web/state.json')

    def close(self):
        if self.cleanup is not None:
            return self.cleanup
        cleanup = {'native': {'started': self.child is not None, 'forced': []}, 'servers': [], 'errors': []}
        try:
            if self.child is not None:
                if self.child.poll() is None:
                    (self.output / 'command').write_text('quit', encoding='utf-8')
                try:
                    self.child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    cleanup['native']['forced'].append('terminate')
                    self.child.terminate()
                    try:
                        self.child.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        cleanup['native']['forced'].append('kill')
                        self.child.kill()
                        self.child.wait(timeout=2)
                cleanup['native'].update(pid=self.child.pid, returncode=self.child.returncode)
        except Exception as error:
            cleanup['errors'].append(str(error))
        finally:
            if self.log is not None:
                self.log.close()
            for server, thread in reversed(self.servers):
                try:
                    server.shutdown()
                    server.server_close()
                    thread.join(timeout=2)
                    cleanup['servers'].append({'port': server.server_port, 'stopped': not thread.is_alive()})
                except Exception as error:
                    cleanup['errors'].append(str(error))
        cleanup['passed'] = (not cleanup['errors'] and not cleanup['native']['forced']
            and cleanup['native'].get('returncode', 0) == 0
            and all(row['stopped'] for row in cleanup['servers']))
        self.cleanup = cleanup
        return cleanup
