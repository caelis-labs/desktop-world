#!/usr/bin/env python3
"""Select native checks; reuse a successful PR only for the identical Git tree."""
import io
import json
import os
from pathlib import Path
import subprocess
import zipfile


def select_checks(paths):
    macos = windows = linux = package_upgrade = linux_gui = False
    for path in paths:
        if path in ('install/bootstrap.sh', 'tests/test_setup_bootstrap_posix.py'):
            macos = True  # Portable job exercises the Linux shell path.
        elif path in ('.github/workflows/rewrite-linux-candidate.yml', 'tests/ci/install_linux_x11_deps.sh'):
            linux = True
            linux_gui = True
        elif path == 'scripts/check-linux-candidate.py':
            linux = True
        elif path == '.github/workflows/rewrite-macos-candidate.yml':
            macos = True
        elif path == '.github/workflows/rewrite-candidate.yml':
            windows = True
        elif (path.startswith('cmd/dtw/src/backends/macos/') or path.endswith('.swift')
                or path == 'tests/fixtures/app-scene/test_telemetry.py'):
            macos = True
        elif path == 'install/bootstrap.ps1' or path.startswith(('cmd/dtw/src/backends/windows/', 'tests/acceptance/windows/')):
            windows = True
        elif path.startswith(('cmd/dtw/src/backends/linux/', 'cmd/dtw/src/x11/', 'tests/acceptance/linux_')):
            linux = True
            linux_gui = True
        elif path.startswith(('cmd/dtw/', '.github/workflows/', '.github/actions/', 'tests/ci/', 'scripts/', 'packaging/', 'skills/dtw/', 'tests/acceptance/package_store_probe.py', '.cargo/', 'Cargo.', 'rust-toolchain')):
            macos = windows = linux = True

        if (path.startswith('cmd/dtw/') and not path.startswith(('cmd/dtw/src/backends/macos/', 'cmd/dtw/src/backends/windows/', 'cmd/dtw/src/package/'))) or path.startswith((
            'tests/acceptance/linux_',
            '.cargo/',
            'Cargo.',
            'rust-toolchain',
            '.github/actions/',
        )) or path in (
            'tests/ci/install_linux_x11_deps.sh',
            '.github/workflows/ci.yml',
            '.github/workflows/rewrite-linux-candidate.yml',
            'tests/ci/plan.py',
        ):
            linux_gui = True

        if (path.startswith('cmd/dtw/') and not path.startswith(('cmd/dtw/src/runtime/', 'cmd/dtw/src/computer/', 'cmd/dtw/src/x11/')) and not path.endswith('.swift')) or path.startswith((
            '.cargo/',
            'Cargo.',
            'rust-toolchain',
            'packaging/',
            'skills/dtw/',
            'plugins/',
            '.github/actions/',
        )) or path in (
            'tests/acceptance/package_store_probe.py',
            'tests/ci/check_package_upgrade.py',
            'scripts/plugin_package.py',
            'scripts/build-rewrite-candidate.py',
            'scripts/stage-agent-plugin.py',
            '.github/workflows/ci.yml',
            'tests/ci/plan.py',
        ):
            package_upgrade = True
    return macos, windows, linux, package_upgrade, linux_gui


def native_checks(paths):
    return select_checks(paths)[:3]


def gh(path, raw=False):
    data = subprocess.check_output(['gh', 'api', path], timeout=45)
    return data if raw else json.loads(data)


def reusable_run(repository, sha, tree, api=gh):
    """No artifact code is executed; any absent/expired evidence means fresh checks."""
    prefix = f'repos/{repository}'
    for pr in api(f'{prefix}/commits/{sha}/pulls'):
        if not pr.get('merged_at') or pr.get('merge_commit_sha') != sha:
            continue
        head = pr['head']['sha']
        runs = api(f'{prefix}/actions/workflows/ci.yml/runs?event=pull_request&head_sha={head}&status=success&per_page=10')
        for run in runs['workflow_runs']:
            if run['event'] != 'pull_request' or run['head_sha'] != head or run['conclusion'] != 'success':
                continue
            run_id, attempt = run['id'], run['run_attempt']
            artifacts = api(f'{prefix}/actions/runs/{run_id}/artifacts')['artifacts']
            name = f'ci-verification-{run_id}-{attempt}'
            for artifact in artifacts:
                if artifact['name'] != name or artifact['expired']:
                    continue
                data = api(f"{prefix}/actions/artifacts/{artifact['id']}/zip", raw=True)
                if len(data) > 1024 * 1024:
                    continue
                with zipfile.ZipFile(io.BytesIO(data)) as archive:
                    if archive.namelist() != ['verified.json'] or archive.getinfo('verified.json').file_size > 4096:
                        continue
                    record = json.loads(archive.read('verified.json'))
                if record == {'schema': 1, 'tree': tree, 'run_id': run_id, 'attempt': attempt,
                              'event': 'pull_request', 'checks': 'passed'}:
                    return str(run_id)
    return ''


def git(*args):
    return subprocess.check_output(['git', *args], text=True).strip()


def main():
    tree = git('rev-parse', 'HEAD^{tree}')
    event = os.environ['GITHUB_EVENT_NAME']
    reused = ''
    if event == 'push':
        try:
            reused = reusable_run(os.environ['GITHUB_REPOSITORY'], os.environ['GITHUB_SHA'], tree)
        except (OSError, ValueError, KeyError, zipfile.BadZipFile, subprocess.SubprocessError) as error:
            print(f'PR evidence unavailable ({type(error).__name__}); running fresh checks.')
    macos = windows = linux = package_upgrade = linux_gui = not bool(reused)
    if event == 'pull_request':
        payload = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text(encoding='utf-8'))
        base = payload['pull_request']['base']['sha']
        subprocess.run(['git', 'fetch', '--no-tags', '--depth=1', 'origin', base], check=True)
        changed = subprocess.check_output(['git', 'diff', '--no-renames', '--name-only', '-z', base, 'HEAD'])
        macos, windows, linux, package_upgrade, linux_gui = select_checks(changed.decode('utf-8').rstrip('\0').split('\0'))
    values = {
        'tree': tree,
        'reused_run': reused,
        'source': not bool(reused),
        'macos': macos,
        'windows': windows,
        'linux': linux,
        'package_upgrade': package_upgrade,
        'linux_gui': linux_gui,
    }
    with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
        for key, value in values.items():
            output.write(f'{key}={str(value).lower() if isinstance(value, bool) else value}\n')
    print(json.dumps(values))


if __name__ == '__main__':
    main()
