#!/usr/bin/env python3
"""Verify, extract and import final native Linux candidate bytes; no desktop access."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import tarfile
import threading

_spec = importlib.util.spec_from_file_location('linux_candidate_builder', Path(__file__).with_name('build-rewrite-candidate.py'))
builder = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(builder)
require = builder.require


def imported_binary(recipe, package, store, candidate):
    identity = builder.digest(package / 'SHA256SUMS')
    payload = store / 'packages' / identity / 'payload'
    binary = payload / 'bin/dtw'
    require(recipe.get('package_id') == identity and recipe.get('package_path') == str(payload)
            and recipe.get('executable') == str(binary) and recipe.get('args') == ['mcp']
            and recipe.get('target') == candidate['target'] and recipe.get('version') == candidate['version'],
            'Imported package recipe differs from verified archive identity')
    require(builder.digest(payload / 'SHA256SUMS') == identity
            and builder.digest(binary) == candidate['binary_sha256'] and os.access(binary, os.X_OK),
            'Managed inventory, binary hash or execute mode mismatch')
    return binary


def worker_controls(binary, output):
    cases = []
    environment = os.environ.copy()
    for key in ('DISPLAY', 'WAYLAND_DISPLAY', 'XDG_SESSION_TYPE'):
        environment.pop(key, None)
    for mode in ('eof', 'malformed', 'sigterm', 'wayland'):
        worker_env = environment.copy()
        expected_error = 'display_unavailable'
        if mode == 'wayland':
            worker_env['XDG_SESSION_TYPE'] = 'wayland'
            expected_error = 'wayland_unsupported'
        with (output / ('worker-' + mode + '.stderr')).open('w') as error:
            process = subprocess.Popen([str(binary), '--dtw-native-worker', str(os.getpid())],
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=error,
                                       text=True, encoding='utf-8', env=worker_env)
            messages = queue.Queue()
            reader = threading.Thread(target=lambda: messages.put(process.stdout.readline()), daemon=True)
            reader.start()
            try:
                request = {'request_id': mode, 'operation': 'release', 'binding': 'never-created-candidate-control'}
                process.stdin.write(json.dumps(request) + '\n')
                process.stdin.flush()
                reply = json.loads(messages.get(timeout=5))
                require(reply.get('request_id') == mode
                        and reply.get('result', {}).get('error') == expected_error
                        and isinstance(reply['result'].get('message'), str) and reply['result']['message'],
                        f'Worker must return {expected_error} in {mode} control')
                if mode in ('eof', 'wayland'):
                    process.stdin.close()
                elif mode == 'malformed':
                    process.stdin.write('{invalid-json}\n')
                    process.stdin.flush()
                else:
                    process.terminate()
                require(process.wait(timeout=5) == 0, 'Worker shutdown failed: ' + mode)
                require(process.stdout.read() == '', 'Worker emitted unexpected extra output')
                cases.append({'mode': mode, 'exit_code': process.returncode, 'reply': reply})
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                if not process.stdin.closed:
                    process.stdin.close()
                reader.join(timeout=2)
                process.stdout.close()
    return cases


def check(candidate_root, output):
    candidate_root = candidate_root.resolve(strict=True)
    output = output.resolve()
    output.mkdir()
    candidate = json.loads((candidate_root / 'CANDIDATE.json').read_text())
    target, version = candidate['target'], candidate['version']
    require(target.endswith('linux-gnu'), 'Expected Linux candidate')
    builder.native_host(target)
    package_name = f'desktop-world-{version}-{target}'
    name = package_name + '.tar.gz'
    require(candidate.get('archive') == name, 'Unexpected candidate archive name')
    archive = candidate_root / name
    require(builder.digest(archive) == candidate['archive_sha256'], 'Candidate archive digest mismatch')
    require((candidate_root / (name + '.sha256')).read_text() == f'{candidate["archive_sha256"]}  {name}\n',
            'Candidate archive sidecar mismatch')
    builder.verify_archive(archive, package_name)
    extracted = output / 'extracted'
    extracted.mkdir()
    # The audit above admits only regular, uniquely named members inside this
    # package root. Extract these exact bytes without tar's link handling.
    with tarfile.open(archive, 'r:gz') as content:
        for item in content.getmembers():
            destination = extracted / item.name
            destination.parent.mkdir(parents=True, exist_ok=True)
            with content.extractfile(item) as source, destination.open('xb') as dest:
                shutil.copyfileobj(source, dest)
            destination.chmod(item.mode & 0o777)
    package = extracted / package_name
    require(builder.digest(package / 'BUILD.json') == candidate['build_manifest_sha256'], 'Candidate BUILD digest mismatch')
    require(builder.digest(package / 'bin/dtw') == candidate['binary_sha256'], 'Extracted candidate binary mismatch')
    store = output / 'managed-store'
    imported = subprocess.run([str(package / 'bin/dtw'), 'package', 'import', str(package), '--store', str(store)],
                              capture_output=True, text=True, encoding='utf-8', check=True, timeout=30)
    recipe = json.loads(imported.stdout)
    (output / 'import.json').write_text(json.dumps(recipe, indent=2) + '\n')
    binary = imported_binary(recipe, package, store, candidate)
    require(subprocess.check_output([str(binary), '--version'], text=True, timeout=10).strip() == 'dtw ' + version,
            'Managed executable version mismatch')
    subprocess.run([sys.executable, str(candidate_root / 'source/tests/ci/run_packaged_mcp.py'),
                    '--dtw', str(binary), '--version', version, '--output', str(output / 'packaged-mcp')], check=True)
    workers = worker_controls(binary, output)
    report = {'archive': name, 'archive_sha256': candidate['archive_sha256'], 'package_id': recipe['package_id'],
              'executable': str(binary), 'target': target, 'version': version,
              'scope': 'final archive extraction, managed import, MCP metadata/EOF and Linux worker shutdown; no desktop access',
              'worker_controls': workers}
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True, help='New verification directory')
    args = parser.parse_args()
    print(json.dumps(check(args.candidate, args.output)))


if __name__ == '__main__':
    main()
