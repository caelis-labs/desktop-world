"""Owned copied Chrome lifecycle; never authorizes the installed bundle or profile."""
import json
import os
from pathlib import Path
import plistlib
import shutil
import signal
import subprocess
import tempfile
import time
from urllib.parse import urlsplit

from app_client import require


class ChromeIsolationError(RuntimeError):
    pass


def loopback_url(url):
    parsed = urlsplit(url)
    require(parsed.scheme == 'http' and parsed.hostname in ('127.0.0.1', '::1')
            and parsed.port is not None and not parsed.username and not parsed.password,
            'Chrome accepts only the owned loopback fixture URL')
    return url


class IsolatedChrome:
    """Copy/profile are retained as evidence; cleanup signals only our launch PID+birth.

    start() must complete before public apps/exec, and verify() precedes every
    execution. No retry after an input failure, no app activation or app.quit.
    """
    def __init__(self, output, url, *, source=Path('/Applications/Google Chrome.app')):
        self.output = Path(output).resolve()
        require(not any(p.suffix.lower() == '.app' for p in (self.output, *self.output.parents)),
                'Isolated Chrome root must not be nested in an application bundle')
        self.url = loopback_url(url)
        self.source = Path(source).resolve(strict=True)
        self.root = None
        self.child = None
        self.birth = None
        self.log = None
        self.evidence = {'classification': 'should_fill_acceptance_harness', 'cleanup': None}

    def publish(self):
        if self.root is not None:
            path = self.root / 'isolation.json'
            temporary = path.with_suffix('.tmp')
            temporary.write_text(json.dumps(self.evidence, indent=2) + '\n')
            temporary.replace(path)

    def observe(self):
        result = subprocess.run([str(self.observer), str(self.child.pid), str(self.bundle)],
                                capture_output=True, text=True, check=True, timeout=10)
        return json.loads(result.stdout)

    def verify(self):
        require(self.child.poll() is None, 'Owned Chrome exited: possible startup handoff')
        observed = self.observe()
        self.evidence['latest_observation'] = observed
        self.publish()
        require(observed['stable'] is True and type(observed['birth']) is int
                and observed['birth'] == self.birth
                and observed['pid'] == self.child.pid
                and observed['executable'] == str(self.executable)
                and observed.get('argv') == self.evidence['argv']
                and isinstance(observed.get('bundle_path'), str)
                and Path(observed['bundle_path']).is_dir()
                and Path(observed['bundle_path']).resolve(strict=True) == self.bundle
                and observed['instances'] == [self.child.pid],
                {'isolation_failure': observed})
        return observed

    def start(self):
        self.output.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix='chrome-owned-', dir=self.output)).resolve()
        self.bundle = self.root / 'Chrome.app'
        self.profile = self.root / 'profile'
        self.observer = self.root / 'chrome-instance-observer'
        self.evidence.update(root=str(self.root), target=str(self.bundle), profile=str(self.profile))
        try:
            source = Path(__file__).resolve().parents[1] / 'fixtures/chrome-instance-observer.swift'
            build = subprocess.run(['xcrun', 'swiftc', str(source), '-framework', 'AppKit',
                                    '-module-cache-path', str(self.root / 'swift-cache'),
                                    '-o', str(self.observer)], capture_output=True, text=True, timeout=120)
            (self.root / 'observer-build.log').write_text(build.stdout + build.stderr)
            require(build.returncode == 0, 'Read-only Chrome observer compilation failed')
            clone = subprocess.run(['/bin/cp', '-cR', str(self.source), str(self.bundle)],
                                   capture_output=True, text=True, timeout=180)
            self.evidence['clone'] = {'returncode': clone.returncode, 'stderr': clone.stderr}
            if clone.returncode:
                unsupported = any(s in clone.stderr.lower() for s in
                                  ('illegal option', 'invalid option', 'not supported'))
                require(unsupported, {'copy_failed': self.evidence['clone']})
                if self.bundle.exists():
                    shutil.rmtree(self.bundle)  # only this freshly allocated owned destination
                full = subprocess.run(['/bin/cp', '-R', str(self.source), str(self.bundle)],
                                      capture_output=True, text=True, timeout=300)
                self.evidence['full_copy'] = {'returncode': full.returncode, 'stderr': full.stderr}
                require(full.returncode == 0, {'full_copy_failed': self.evidence['full_copy']})
            with (self.bundle / 'Contents/Info.plist').open('rb') as stream:
                name = plistlib.load(stream)['CFBundleExecutable']
            require(Path(name).name == name, 'Chrome executable must be a bundle-local filename')
            self.executable = self.bundle / 'Contents/MacOS' / name
            original = self.source / 'Contents/MacOS' / name
            require(self.executable.is_file() and not self.executable.is_symlink()
                    and self.executable.resolve() == self.executable
                    and not os.path.samefile(self.executable, original),
                    'Copied executable must be independent of installed Chrome')
            for directory, dirs, files in os.walk(self.bundle):
                for name in dirs + files:
                    path = Path(directory) / name
                    if path.is_symlink():
                        require(path.resolve().is_relative_to(self.bundle),
                                'Copied Chrome contains an external symlink: ' + str(path))
            self.profile.mkdir(mode=0o700)
            argv = [str(self.executable), '--user-data-dir=' + str(self.profile),
                    '--no-first-run', '--no-default-browser-check', '--disable-background-networking',
                    '--disable-component-update', '--disable-sync', '--disable-default-apps',
                    '--disable-session-crashed-bubble', '--password-store=basic',
                    '--use-mock-keychain', '--force-renderer-accessibility', '--new-window', self.url]
            self.evidence['argv'] = argv
            self.log = (self.root / 'chrome.log').open('w')
            self.child = subprocess.Popen(argv, stdout=self.log, stderr=subprocess.STDOUT)
            # Capture the kernel identity immediately, before waiting for LaunchServices.
            first = self.observe()
            require(first['stable'] is True and type(first['birth']) is int
                    and first['executable'] == str(self.executable), {'launch_identity': first})
            self.birth = first['birth']
            self.evidence.update(pid=self.child.pid, birth=self.birth, launch_observation=first)
            require(first.get('argv') == argv, {'launch_arguments': first})
            deadline = time.monotonic() + 20
            while True:
                require(self.child.poll() is None, 'Chrome exited or handed off startup')
                observed = self.observe()
                if (observed.get('instances') == [self.child.pid] and isinstance(observed.get('bundle_path'), str)
                        and Path(observed['bundle_path']).is_dir()
                        and Path(observed['bundle_path']).resolve(strict=True) == self.bundle):
                    break
                require(time.monotonic() < deadline, {'bundle_registration_timeout': observed})
                time.sleep(0.1)
            self.verify()
            self.evidence['isolated'] = True
            self.publish()
            return self
        except Exception as error:
            self.evidence.update(isolated=False, blocked=str(error), classification='technical_isolation_failure')
            self.publish()
            raise ChromeIsolationError(str(error)) from error

    def close(self):
        if self.evidence['cleanup'] is not None:
            return self.evidence['cleanup']
        cleanup = {'signals': [], 'forced': False, 'errors': [], 'retained_owned_files': True}
        self.evidence['cleanup'] = cleanup
        try:
            if self.child is not None and self.child.poll() is None:
                for sig, timeout in ((signal.SIGTERM, 5), (signal.SIGKILL, 3)):
                    observed = self.observe()
                    require(self.birth is not None and observed['stable'] is True
                            and observed['birth'] == self.birth
                            and observed['executable'] == str(self.executable),
                            'Cleanup refused: owned PID birth/executable no longer proven')
                    os.kill(self.child.pid, sig)
                    cleanup['signals'].append(sig.name)
                    cleanup['forced'] = sig == signal.SIGKILL
                    try:
                        self.child.wait(timeout=timeout)
                        break
                    except subprocess.TimeoutExpired:
                        continue
                require(self.child.poll() is not None, 'Owned Chrome survived cleanup')
            cleanup['returncode'] = self.child.poll() if self.child else None
        except Exception as error:
            cleanup['errors'].append(str(error))
        finally:
            if self.log:
                self.log.close()
            self.publish()
        return cleanup
