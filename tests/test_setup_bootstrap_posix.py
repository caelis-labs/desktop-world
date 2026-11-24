"""Offline POSIX bootstrap contracts with real archive tooling and fake runtimes."""

import hashlib
import io
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import tarfile
import tempfile
import unittest
import zipfile


REPO = Path(__file__).resolve().parents[1]
BOOTSTRAP = REPO / 'install/bootstrap.sh'
LATEST = 'https://api.github.com/repos/caelis-labs/desktop-world/releases/latest'
DOWNLOAD = 'https://github.com/caelis-labs/desktop-world/releases/download/'
PACKAGE_ID = 'a' * 64
CONTEXT = '"host": "opencode", "profile": "default"'
RESTART = '{ "state": "restart_required", ' + CONTEXT + ', "next": "reload host" }\n'
VERIFY = '{ "state": "verify", ' + CONTEXT + ', "package_id": "' + PACKAGE_ID + '" }\n'

CURL = '''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import shutil
import sys

url = sys.argv[-1]
with open(os.environ['BOOTSTRAP_LOG'], 'a') as stream:
    stream.write('fetch ' + url + '\\n')
fixtures = Path(os.environ['BOOTSTRAP_FIXTURES'])
responses = json.loads((fixtures / 'routes.json').read_text())
if url not in responses:
    sys.exit('Unexpected network call: ' + url)
shutil.copyfile(fixtures / responses[url], sys.argv[sys.argv.index('--output') + 1])
'''

RUNTIME = '''#!/bin/sh
set -eu
printf '%s %s\\n' '@RUNTIME@' "$*" >> "$BOOTSTRAP_LOG"
case "$*" in
    --help)
        printf 'Usage:\\n  dtw --help\\n'
        [ "@RUNTIME@" = legacy ] || printf '  dtw setup resume|prepare|confirm [--store <absolute-directory>]\\n'
        ;;
    'setup --help') printf '{"setup_schema":%s}\\n' "${BOOTSTRAP_SCHEMA:-1}" ;;
    'setup resume opencode default')
        if [ "${BOOTSTRAP_RESUME_FAIL:-}" = true ]; then
            printf '{"code":"setup_error","message":"authorization denied"}\\n' >&2
            exit 2
        fi
        printf '%s\\n' "$BOOTSTRAP_RESUME" ;;
    'setup prepare opencode default @PACKAGE_ID@') printf '%s\\n' "$BOOTSTRAP_PREPARE" ;;
    'setup prepare opencode default @PACKAGE_ID@ '*)
        [ "@RUNTIME@" = downloaded ] && [ "$#" -eq 6 ] && [ "$0" = "$6/bin/dtw" ] && [ -f "$6/BUILD.json" ]
        printf '%s\\n' "$BOOTSTRAP_DOWNLOADED_PREPARE" ;;
    *) exit 99 ;;
esac
'''


def sha256(data):
    return hashlib.sha256(data).hexdigest()


@unittest.skipUnless(platform.system() in {'Darwin', 'Linux'}, 'POSIX bootstrap targets macOS/Linux')
class PosixBootstrapTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        self.log = self.root / 'calls.log'
        self.tools = self.root / 'tools'
        self.tools.mkdir()
        self.fixtures = self.root / 'assets'
        self.fixtures.mkdir()
        self.home = self.root / 'home'
        self.home.mkdir()
        self.tag = 'v1.2.3'
        self.os_name = platform.system()
        self.machine = 'arm64' if self.os_name == 'Darwin' else platform.machine()
        linux_arch = 'aarch64' if self.machine in {'aarch64', 'arm64'} else 'x86_64'
        self.target = 'aarch64-apple-darwin' if self.os_name == 'Darwin' else f'{linux_arch}-unknown-linux-gnu'
        self.suffix = '.zip' if self.os_name == 'Darwin' else '.tar.gz'
        self.write_executable(self.tools / 'curl', CURL)
        self.write_executable(self.tools / 'uname', f'#!/bin/sh\ncase $1 in\n-s) echo {self.os_name} ;;\n-m) echo {self.machine} ;;\nesac\n')
        self.installed = self.root / 'registered runtime' / 'dtw'
        self.installed.parent.mkdir()
        self.write_executable(self.installed, self.runtime('installed'))
        self.env = dict(os.environ, PATH=f'{self.tools}{os.pathsep}{os.environ["PATH"]}',
                        HOME=str(self.home), XDG_DATA_HOME=str(self.home), XDG_CONFIG_HOME=str(self.home),
                        TMPDIR=str(self.root), BOOTSTRAP_LOG=str(self.log), BOOTSTRAP_FIXTURES=str(self.fixtures),
                        BOOTSTRAP_RESUME=json.dumps(dict(state='no_pending', host='opencode', profile='default')),
                        BOOTSTRAP_PREPARE=json.dumps(dict(state='needs_download', host='opencode', profile='default', package_id=PACKAGE_ID)),
                        BOOTSTRAP_DOWNLOADED_PREPARE=VERIFY.rstrip('\n'))
        self.env.pop('DTW_STAGING_DIR', None)

    def runtime(self, name):
        return RUNTIME.replace('@RUNTIME@', name).replace('@PACKAGE_ID@', PACKAGE_ID)

    def write_executable(self, path, text):
        path.write_text(text)
        path.chmod(0o755)

    def release(self, *, legacy=False, invalid=None, sidecar_has_api_digest=True):
        if legacy:
            self.tag = 'v0.6.0'
        version = self.tag[1:]
        package = f'desktop-world-{version}-{self.target}'
        archive = package + self.suffix
        binary = self.runtime('downloaded').encode()
        files = [(f'{package}/bin/dtw', binary, 0o755), (f'{package}/BUILD.json', b'{}', 0o644)]
        if invalid == 'traversal':
            files.append((f'{package}/../../escaped', b'unsafe member', 0o644))
        if invalid == 'symlink':
            files.append((f'{package}/link', b'/tmp', 0o777))
        path = self.fixtures / archive
        if self.suffix == '.zip':
            with zipfile.ZipFile(path, 'w') as output:
                for name, data, mode in files:
                    member = zipfile.ZipInfo(name)
                    member.create_system = 3
                    kind = stat.S_IFLNK if name.endswith('/link') else stat.S_IFREG
                    member.external_attr = (kind | mode) << 16
                    output.writestr(member, data)
        else:
            with tarfile.open(path, 'w:gz') as output:
                for name, data, mode in files:
                    member = tarfile.TarInfo(name)
                    member.size = len(data)
                    member.mode = mode
                    if name.endswith('/link'):
                        member.type = tarfile.SYMTYPE
                        member.linkname = data.decode()
                        member.size = 0
                    output.addfile(member, io.BytesIO(data))

        digest = sha256(path.read_bytes())
        sidecar_sum = '0' * 64 if invalid == 'sidecar' else digest
        sidecar = archive + '.sha256'
        (self.fixtures / sidecar).write_text(f'{sidecar_sum}  {archive}\n')
        payload = dict(target=self.target, archive=archive, archive_sha256=digest, binary_sha256=sha256(binary))
        if not legacy and invalid != 'missing package ID':
            payload['package_id'] = PACKAGE_ID
        if invalid == 'binary':
            payload['binary_sha256'] = '0' * 64
        manifest = self.fixtures / 'RELEASE.json'
        manifest.write_text(json.dumps(dict(tag=self.tag, version=version, platforms=[payload])))
        self.urls = {name: DOWNLOAD + self.tag + '/' + name for name in ('RELEASE.json', sidecar, archive)}
        self.archive = archive
        self.sidecar = sidecar
        assets = [dict(name=name, browser_download_url=url,
                       digest='sha256:' + sha256((self.fixtures / name).read_bytes()))
                  for name, url in self.urls.items()]
        if not sidecar_has_api_digest:
            assets[1]['digest'] = None
        if invalid == 'archive API digest':
            assets[-1]['digest'] = 'sha256:' + '0' * 64
        if invalid == 'archive URL':
            assets[-1]['browser_download_url'] = DOWNLOAD + 'v9.9.9/' + archive
        (self.fixtures / 'latest.json').write_text(json.dumps(dict(tag_name=self.tag, draft=False, prerelease=False, assets=assets)))
        (self.fixtures / 'routes.json').write_text(json.dumps({LATEST: 'latest.json', **{url: name for name, url in self.urls.items()}}))
        if invalid == 'manifest':
            manifest.write_text(manifest.read_text().replace(PACKAGE_ID, 'b' * 64))
        if invalid == 'archive':
            path.write_bytes(path.read_bytes() + b'mutated after resolution')

    def bootstrap(self, *, installed=True):
        if self.log.exists():
            self.log.unlink()
        args = [str(self.installed)] if installed else []
        result = subprocess.run([str(BOOTSTRAP), 'opencode', 'default', *args], cwd=self.root,
                                env=self.env, text=True, capture_output=True, timeout=20)
        self.calls = self.log.read_text().splitlines() if self.log.is_file() else []
        self.network = [line.removeprefix('fetch ') for line in self.calls if line.startswith('fetch ')]
        return result

    def test_pending_runtime_stops_before_network(self):
        self.env['BOOTSTRAP_RESUME'] = RESTART.rstrip('\n')
        result = self.bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, RESTART)
        self.assertEqual(self.calls, ['installed --help', 'installed setup --help', 'installed setup resume opencode default'])

    def test_cached_package_uses_authenticated_manifest_and_avoids_archive(self):
        self.release()
        self.env['BOOTSTRAP_PREPARE'] = VERIFY.rstrip('\n')
        result = self.bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, VERIFY)
        self.assertEqual(self.network, [LATEST, self.urls['RELEASE.json']])
        self.assertEqual(self.calls[-1], f'installed setup prepare opencode default {PACKAGE_ID}')

    def test_download_runs_new_verified_runtime_with_exact_package_id_and_source(self):
        self.release()
        result = self.bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, VERIFY)
        self.assertEqual(self.network, [LATEST, self.urls['RELEASE.json'], self.urls[self.sidecar], self.urls[self.archive]])
        self.assertIn(f'installed setup prepare opencode default {PACKAGE_ID}', self.calls)
        source = Path(self.calls[-1].split(f' {PACKAGE_ID} ', 1)[1])
        self.assertTrue(self.calls[-1].startswith('downloaded setup prepare opencode default '))
        self.assertTrue(source.is_absolute())
        self.assertNotIn(self.root, source.resolve().parents)
        self.assertFalse(source.exists(), 'private staging should be cleaned up')

    def test_fresh_bootstrap_checks_sidecar_bytes_without_sidecar_api_digest(self):
        self.release(sidecar_has_api_digest=False)
        result = self.bootstrap(installed=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, VERIFY)
        self.assertEqual(self.network, [LATEST, self.urls['RELEASE.json'], self.urls[self.sidecar], self.urls[self.archive]])
        self.assertTrue(self.calls[-1].startswith(f'downloaded setup prepare opencode default {PACKAGE_ID} /tmp/'))

    def test_legacy_installed_runtime_can_bootstrap_managed_release(self):
        self.release()
        self.write_executable(self.installed, self.runtime('legacy'))
        result = self.bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, VERIFY)
        self.assertEqual([line for line in self.calls if line.startswith('legacy ')], ['legacy --help'])
        self.assertEqual(self.network.count(LATEST), 1)

    def test_legacy_release_has_explicit_instructions_and_never_prepares(self):
        self.release(legacy=True)
        self.write_executable(self.installed, self.runtime('legacy'))
        result = self.bootstrap()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, '')
        self.assertIn('unsupported for managed setup', result.stderr)
        self.assertIn('/blob/v0.6.0/install.md', result.stderr)
        self.assertEqual(self.calls, ['legacy --help', 'fetch ' + LATEST, 'fetch ' + self.urls['RELEASE.json']])

    def test_unknown_or_failed_probe_and_resume_do_not_fall_back(self):
        for variables, message, code in (
            ({'BOOTSTRAP_SCHEMA': '2'}, 'Unsupported setup capability', 1),
            ({'BOOTSTRAP_RESUME': '{"state":"unexpected", ' + CONTEXT + '}'}, 'Unexpected setup resume state', 1),
            ({'BOOTSTRAP_RESUME': 'invalid JSON'}, 'Invalid setup resume response', 1),
            ({'BOOTSTRAP_RESUME_FAIL': 'true'}, 'authorization denied', 2),
        ):
            with self.subTest(variables=variables):
                self.env.update(variables)
                result = self.bootstrap()
                self.assertEqual(result.returncode, code)
                self.assertIn(message, result.stderr)
                self.assertEqual(self.network, [])
                for variable in variables:
                    self.env.pop(variable)

    def test_unknown_prepare_receipt_does_not_download(self):
        self.release()
        self.env['BOOTSTRAP_PREPARE'] = '{"state":"unexpected", ' + CONTEXT + ', "package_id": "' + PACKAGE_ID + '"}'
        result = self.bootstrap()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Unexpected setup prepare state', result.stderr)
        self.assertEqual(self.network, [LATEST, self.urls['RELEASE.json']])

    def test_terminal_receipts_must_match_setup_context(self):
        self.env['BOOTSTRAP_RESUME'] = RESTART.replace('default', 'other').rstrip('\n')
        result = self.bootstrap()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, '')
        self.assertEqual(self.network, [])
        self.assertIn('requested host and profile', result.stderr)

        self.release()
        self.env['BOOTSTRAP_DOWNLOADED_PREPARE'] = VERIFY.replace(PACKAGE_ID, 'b' * 64).rstrip('\n')
        result = self.bootstrap(installed=False)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, '')
        self.assertIn('requested package ID', result.stderr)

    def test_custom_staging_root_must_be_absolute(self):
        self.env['DTW_STAGING_DIR'] = 'relative'
        result = self.bootstrap(installed=False)
        self.assertEqual(result.returncode, 1)
        self.assertIn('DTW_STAGING_DIR must be an approved absolute path', result.stderr)
        self.assertEqual(self.calls, [])

        with tempfile.TemporaryDirectory(prefix='approved staging ') as stage:
            self.env['DTW_STAGING_DIR'] = stage
            self.release()
            result = self.bootstrap(installed=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, VERIFY)
            self.assertIn(' ' + stage + '/dtw-bootstrap.', self.calls[-1])
            self.assertEqual(list(Path(stage).iterdir()), [])

    def test_invalid_release_bytes_or_archive_members_never_execute(self):
        for invalid, calls, message in (
            ('manifest', 2, 'SHA-256 mismatch for RELEASE.json'),
            ('missing package ID', 2, 'Missing or invalid release package_id'),
            ('archive API digest', 2, 'archive digests disagree'),
            ('archive URL', 2, 'Unexpected download URL'),
            ('sidecar', 3, 'Archive sidecar disagrees'),
            ('archive', 4, 'SHA-256 mismatch for desktop-world-'),
            ('binary', 4, 'SHA-256 mismatch for dtw'),
            ('traversal', 4, 'Unsafe release archive members'),
            ('symlink', 4, 'Unsafe release archive members'),
        ):
            with self.subTest(invalid=invalid):
                self.release(invalid=invalid)
                result = self.bootstrap(installed=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, '')
                self.assertIn(message, result.stderr)
                self.assertEqual(len(self.network), calls)
                self.assertEqual(len(self.calls), len(self.network), self.calls)


if __name__ == '__main__':
    unittest.main()
