#!/usr/bin/env python3
"""Cross-platform setup, package CLI and MCP lease probe without desktop interaction."""
import argparse, hashlib, io, json, os, platform, queue, shutil, stat, subprocess, sys, tempfile, threading, time, uuid, zipfile
from contextlib import contextmanager
from pathlib import Path
from urllib.parse import unquote, urlsplit
from urllib.request import urlopen
from standard_agent_probe import Mcp, stop_owned

class PackageClient(Mcp):
    """This probe initializes MCP only; it never creates native child workers.

    Reap the exact owned process without the GUI probe's Unix process scanner.
    Native worker lifetime is checked independently below on both platforms.
    """
    def close(self):
        try:
            if not self.child.stdin.closed:self.child.stdin.close()
            cleanup=stop_owned(self.child)
            self.reader.join(timeout=2)
            assert not self.reader.is_alive(),"MCP output reader did not stop"
            assert cleanup['returncode']==0 and not cleanup['forced'],cleanup
            return cleanup
        finally:
            self.child.stdout.close();self.stderr.close();self.transcript.close()
            # Release the completed process object/OS handle as well as its pipes.
            self.child=None

def native_worker_command(executable):
    command=[executable,'--dtw-native-worker']
    if os.name=='nt':
        import ctypes
        from ctypes import wintypes as w
        kernel=ctypes.WinDLL('kernel32',use_last_error=True)
        kernel.GetCurrentProcess.restype=w.HANDLE
        kernel.GetProcessTimes.argtypes=[w.HANDLE,*([ctypes.POINTER(w.FILETIME)]*4)]
        kernel.GetProcessTimes.restype=w.BOOL
        created,exited,cpu_kernel,cpu_user=(w.FILETIME() for _ in range(4))
        if not kernel.GetProcessTimes(kernel.GetCurrentProcess(),*(ctypes.byref(v) for v in [created,exited,cpu_kernel,cpu_user])):
            raise ctypes.WinError(ctypes.get_last_error())
        birth=(created.dwHighDateTime<<32)|created.dwLowDateTime
        command.extend([str(os.getpid()),str(birth)])
    else:
        image=Path(executable).stat()
        command[1]+=f'@{image.st_dev}:{image.st_ino}'
        if sys.platform=='linux':
            command.append(str(os.getpid()))
    return command

def host_target():
    if sys.platform=='linux':
        return {'x86_64':'x86_64-unknown-linux-gnu','aarch64':'aarch64-unknown-linux-gnu'}[platform.machine().lower()]
    return 'x86_64-pc-windows-msvc' if os.name=='nt' else 'aarch64-apple-darwin'

def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def write(path,value): path.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')


def isolated_environment(root):
    # Pass a clean environment to setup. In particular, DSH profile overrides
    # and paths to installed host commands must never reach the subprocess.
    env = {}
    if os.name == 'nt':
        env['SYSTEMROOT'] = os.environ['SYSTEMROOT']
    home = root / 'home'
    directories = {
        'HOME': home,
        'USERPROFILE': home,
        'APPDATA': home / 'appdata',
        'LOCALAPPDATA': home / 'localappdata',
        'XDG_CONFIG_HOME': home / 'config',
        'XDG_DATA_HOME': home / 'data',
        'TMP': home / 'tmp',
        'TEMP': home / 'tmp',
        'TMPDIR': home / 'tmp',
        'PATH': home / 'empty-path',
    }
    real_homes = {Path.home().resolve()}
    for key in ('HOME', 'USERPROFILE'):
        if os.environ.get(key):
            real_homes.add(Path(os.environ[key]).resolve())
    for key, path in directories.items():
        resolved = path.resolve()
        if resolved != path or not resolved.is_relative_to(root) or resolved in real_homes:
            raise RuntimeError(f'Unsafe setup probe directory for {key}: {resolved}')
        path.mkdir(mode=0o700, exist_ok=True)
        env[key] = str(path)
    return env


def file_state(path):
    info = path.stat()
    return (sha(path) if path.is_file() else None, info.st_ino, info.st_mtime_ns, info.st_mode)


def host_files(home):
    # File identity and modification time reveal an identical-content rewrite.
    return {path.relative_to(home): file_state(path) for path in home.rglob('*')}


def make_source(root, name, binary, version, *, schema=1):
    skill = root / 'skills' / ('desktop-world' if schema == 1 else 'dtw')
    skill.mkdir(parents=True)
    (root / 'cordis').mkdir()
    (root / 'bin').mkdir()
    executable = root / 'bin' / ('dtw.exe' if os.name == 'nt' else 'dtw')
    shutil.copy2(binary, executable)
    for guide in ['SKILL.md', 'API.md', 'PATTERNS.md']:
        (skill / guide).write_text(f'{name} {guide}', encoding='utf-8')
    for manifest in ['plugin.json', '.claude-plugin/plugin.json', '.antigravity-plugin/plugin.json', 'package.json']:
        path = root / manifest
        path.parent.mkdir(parents=True, exist_ok=True)
        write(path, {'name': 'desktop-world', 'version': version})
    (root / 'cordis/index.mjs').write_text('export{}', encoding='utf-8')
    write(root / 'BUILD.json', {
        'package_store_schema': schema,
        'version': version,
        'target': host_target(),
        'binary': {'path': executable.relative_to(root).as_posix(), 'sha256': sha(executable)},
        'skill_sha256': sha(skill / 'SKILL.md'),
        'agy_live_swap': 2,
    })
    sums = [f'{sha(path)}  {path.relative_to(root).as_posix()}\n'
            for path in sorted(root.rglob('*')) if path.is_file()]
    (root / 'SHA256SUMS').write_text(''.join(sums), encoding='utf-8')
    return executable


@contextmanager
def running_package_mcp(binary, output, version, *, env, cwd):
    output.mkdir()
    client = PackageClient(binary, output, env=env, cwd=cwd)
    try:
        initialized = client.rpc('initialize', {
            'protocolVersion': '2025-06-18', 'capabilities': {},
            'clientInfo': {'name': 'agy-swap-probe', 'version': '1'},
        })
        client.notify('notifications/initialized')
        server = initialized['serverInfo']
        assert (server['name'], server['version']) == ('dtw', version), initialized
        yield client
    finally:
        client.close()


def setup_transition(root, out, binaries, packages, versions):
    env = isolated_environment(root)
    home = root / 'home'
    store = root / 'setup store 中文'
    host = profile_id = 'dsh-desktop'
    profile = home / '.dsh' / 'profiles' / 'desktop'
    for directory in (profile.parent.parent, profile.parent, profile):
        directory.mkdir(mode=0o700)
    patch = profile / 'cordis.patch.yml'
    initial_patch = b"- insert:\n    - id: probe-other\n      name: '@example/other' # preserve this row\n"
    patch.write_bytes(initial_patch)
    patch.chmod(0o600)
    backups = set()
    initial_files = host_files(home)
    writes_profile = os.name != 'nt'
    prepared_state = 'restart_required' if writes_profile else 'host_action_required'
    ready_state = 'verify' if writes_profile else 'host_action_required'
    source_a, source_b = out / 'source A', out / 'source B'
    bootstrap_a, bootstrap_b = (
        source / 'bin' / Path(package['executable']).name
        for source, package in zip((source_a, source_b), packages)
    )
    binary_a, binary_b = binaries
    id_a, id_b = (package['package_id'] for package in packages)
    version_a, version_b = versions

    def run(binary, *arguments):
        if any(Path(env[key]).resolve(strict=True) != home for key in ('HOME', 'USERPROFILE')):
            raise RuntimeError('Setup probe lost its disposable HOME or USERPROFILE')
        if any(path.resolve() != path for path in (profile, patch, store)):
            raise RuntimeError('Setup probe profile or store points outside its disposable directory')
        return subprocess.run([str(binary), *map(str, arguments)], env=env, cwd=root,
                              capture_output=True, encoding='utf-8', timeout=20)

    def setup(binary, version, command, *arguments, state, package_id=None):
        result = run(binary, 'setup', command, host, profile_id, *arguments, '--store', store)
        assert result.returncode == 0, (command, result.stdout, result.stderr)
        receipt = json.loads(result.stdout)
        assert (receipt['setup_schema'], receipt['store_schema'], receipt['manager_version']) == (1, 1, version), receipt
        assert (receipt['state'], receipt['host'], receipt['profile']) == (state, host, profile_id), receipt
        assert receipt.get('package_id') == package_id, receipt
        return receipt

    def binding(binary, *, current, pending, rollback):
        result = run(binary, 'package', 'status', '--store', store)
        assert result.returncode == 0, result.stderr
        binding, = json.loads(result.stdout)['bindings']
        assert (binding['host'], binding['setup_host']) == (profile_id, host), binding
        assert (binding['current'], binding['pending'], binding['rollback']) == (current, pending, rollback), binding

    def checked_executable(receipt, package, build_binary):
        install = receipt['install']
        registered = Path(install['executable'])
        assert registered.is_absolute(), install
        executable = registered.resolve(strict=True)
        payload = store / 'packages' / package['package_id'] / 'payload'
        expected = payload / 'bin' / Path(package['executable']).name
        # Rust's canonical Windows path may carry a verbatim prefix. Check the
        # exact managed directory and file identity, not their DOS spelling.
        assert executable.parent.samefile(expected.parent) and executable.name == expected.name, install
        assert executable.samefile(expected), install
        assert install['package_id'] == package['package_id'], install
        if writes_profile:
            url = urlsplit(install['cordis_url'])
            assert url.scheme == 'file' and not (url.netloc or url.query or url.fragment), install
            assert Path(unquote(url.path)) == payload / 'cordis' / 'index.mjs', install
        assert sha(executable) == sha(build_binary), install
        result = run(executable, '--version')
        assert result.returncode == 0 and result.stdout.strip() == f"dtw {package['version']}", result
        return executable

    def check_patch(receipt, prior, expected):
        if not writes_profile:
            assert host_files(home) == initial_files
            return
        entries = set(profile.iterdir())
        backup, = entries - backups - {patch}
        assert Path(receipt['registration']['backup']) == backup, receipt
        backups.add(backup)
        assert entries == backups | {patch}, entries
        assert backup.resolve(strict=True) == backup and backup.is_file(), backup
        assert backup.name.startswith('cordis.patch.yml.dtw-') and backup.suffix == '.bak', backup
        assert backup.read_bytes() == prior and (backup.stat().st_mode & 0o777) == 0o600, backup
        assert patch.read_bytes() == expected, patch

    help_result = run(bootstrap_a, 'setup', '--help')
    assert help_result.returncode == 0, (help_result.stdout, help_result.stderr)
    assert json.loads(help_result.stdout)['setup_schema'] == 1
    setup(bootstrap_a, version_a, 'resume', state='no_pending')
    setup(bootstrap_a, version_a, 'prepare', id_a, state='needs_download', package_id=id_a)
    cold = setup(bootstrap_a, version_a, 'prepare', id_a, source_a, state=prepared_state, package_id=id_a)
    binding(bootstrap_a, current=None, pending=id_a, rollback=None)
    managed_a = checked_executable(cold, packages[0], binary_a)
    url_a = cold['install']['cordis_url'].encode('ascii')
    patch_a = initial_patch + b"- insert:\n    - id: desktop-world-plugin\n      name: '" + url_a + b"'\n"
    check_patch(cold, initial_patch, patch_a)
    setup(managed_a, version_a, 'confirm', id_a, state='completed', package_id=id_a)
    binding(managed_a, current=id_a, pending=None, rollback=None)

    upgrade = setup(bootstrap_b, version_b, 'prepare', id_b, source_b, state=prepared_state, package_id=id_b)
    binding(bootstrap_b, current=id_a, pending=id_b, rollback=None)
    managed_b = checked_executable(upgrade, packages[1], binary_b)
    patch_b = patch_a.replace(url_a, upgrade['install']['cordis_url'].encode('ascii'))
    check_patch(upgrade, patch_a, patch_b)

    host_before = host_files(home)
    pin = store / 'hosts' / f'{profile_id}.json'
    pin_before = file_state(pin)
    # Hide the extraction so the installed A manager must resume through managed B.
    parked = out / 'source B parked'
    source_b.rename(parked)
    try:
        resumed = setup(managed_a, version_b, 'resume', state=ready_state, package_id=id_b)
    finally:
        parked.rename(source_b)
    assert resumed['install'] == upgrade['install'], resumed
    assert host_files(home) == host_before and file_state(pin) == pin_before

    mismatch = run(managed_b, 'setup', 'confirm', host, profile_id, id_a, '--store', store)
    assert mismatch.returncode == 2, (mismatch.stdout, mismatch.stderr)
    error = json.loads(mismatch.stderr)['error']
    assert error['code'] == 'setup_error' and error['details']['command'] == 'confirm', error
    assert error['details']['phase'] == 'run_command' and id_a in error['message'] and id_b in error['message'], error
    binding(managed_b, current=id_a, pending=id_b, rollback=None)
    assert host_files(home) == host_before and file_state(pin) == pin_before

    confirmed = setup(managed_b, version_b, 'confirm', id_b, state='completed', package_id=id_b)
    assert confirmed['binding']['current'] == id_b and confirmed['binding']['rollback'] == id_a, confirmed
    binding(managed_b, current=id_b, pending=None, rollback=id_a)
    pin_before = file_state(pin)
    setup(managed_b, version_b, 'prepare', id_b, state=ready_state, package_id=id_b)
    assert host_files(home) == host_before and file_state(pin) == pin_before

    rollback = setup(managed_b, version_a, 'prepare', id_a, state=prepared_state, package_id=id_a)
    binding(managed_a, current=id_b, pending=id_a, rollback=id_a)
    assert checked_executable(rollback, packages[0], binary_a) == managed_a
    check_patch(rollback, patch_b, patch_a)
    setup(managed_a, version_a, 'confirm', id_a, state='completed', package_id=id_a)
    binding(managed_a, current=id_a, pending=None, rollback=id_b)
    return {
        'scope': 'Disposable DSH profile patch and CLI package checks' if writes_profile else
                 'Disposable DSH profile, host action receipt and CLI package checks',
        'live_host_verified': False,
        'versions': [version_a, version_b],
        'version_changed': version_a != version_b,
        'cases': ['cold prepare', 'source-free resume with no writes', 'exact confirm mismatch',
                  'verified package switch and rollback', 'same package with no host writes'],
    }


def agy_swap_transition(root, out, binaries, packages, versions):
    env = isolated_environment(root)
    home = root / 'home'
    work = home / 'work'
    work.mkdir(mode=0o700)
    store = root / 'agy store 中文'
    host, profile_id = 'agy-app', 'agy'
    config = home / '.gemini' / 'config'
    plugins = config / 'plugins'
    slot = plugins / 'desktop-world'
    exe_name = 'dtw.exe' if os.name == 'nt' else 'dtw'
    source_a, source_b = out / 'source A', out / 'source B'
    bootstrap_a = source_a / 'bin' / Path(packages[0]['executable']).name
    bootstrap_b = source_b / 'bin' / Path(packages[1]['executable']).name
    binary_a, binary_b = binaries
    id_a, id_b = (package['package_id'] for package in packages)
    version_a, version_b = versions

    def run(binary, *arguments):
        return subprocess.run([str(binary), *map(str, arguments)], env=env, cwd=work,
                              capture_output=True, encoding='utf-8', timeout=20)

    def setup(binary, version, command, *arguments, state, package_id=None):
        result = run(binary, 'setup', command, host, profile_id, *arguments, '--store', store)
        assert result.returncode == 0, (command, result.stdout, result.stderr)
        receipt = json.loads(result.stdout)
        assert (receipt['setup_schema'], receipt['store_schema'], receipt['manager_version']) == (1, 1, version), receipt
        assert (receipt['state'], receipt['host'], receipt['profile']) == (state, host, profile_id), receipt
        assert receipt.get('package_id') == package_id, receipt
        return receipt

    cold = setup(bootstrap_a, version_a, 'prepare', id_a, source_a, state='host_action_required', package_id=id_a)
    managed_a = Path(cold['install']['executable']).resolve(strict=True)
    assert slot.is_dir() and [p.name for p in plugins.iterdir()] == ['desktop-world']
    (config / 'config.json').write_text(json.dumps({'plugins': {'desktop-world': {'enabled': True}}}) + '\n', encoding='utf-8')
    setup(managed_a, version_a, 'resume', state='verify', package_id=id_a)
    setup(managed_a, version_a, 'confirm', id_a, state='completed', package_id=id_a)

    slot_binary = slot / 'bin' / exe_name
    with running_package_mcp(slot_binary, out / 'agy-old-mcp', version_a, env=env, cwd=work) as old_mcp:
        upgrade = setup(bootstrap_b, version_b, 'prepare', id_b, source_b, state='host_action_required', package_id=id_b)
        managed_b = Path(upgrade['install']['executable']).resolve(strict=True)
        assert list(plugins.iterdir()) == [slot], list(plugins.iterdir())

        if os.name == 'nt':
            assert sha(slot_binary) == sha(binary_a)
            return {
                'scope': 'Disposable agy-app Windows non-destructive occupied slot',
                'versions': [version_a, version_b],
                'cases': ['cold slot install', 'enabled verify', 'occupied slot preserved on Windows'],
            }

        assert 'restart its dtw MCP server' in upgrade.get('next', ''), upgrade
        assert sha(slot_binary) == sha(binary_b)
        backup = Path(upgrade['registration']['backup']).resolve(strict=True)
        assert backup.is_relative_to(config) and not backup.is_relative_to(plugins), upgrade
        backup_binary = backup / 'bin' / exe_name
        assert sha(backup_binary) == sha(binary_a)
        # If stale-image rejection regresses, neither image can run a worker and
        # reach desktop discovery. The guard itself uses the image's identity.
        modes = {path: stat.S_IMODE(path.stat().st_mode) for path in (slot_binary, backup_binary)}
        try:
            for path, mode in modes.items():
                path.chmod(mode & ~0o111)
            apps_data, apps_result = old_mcp.tool('apps', {'state': 'running'})
            assert apps_result.get('isError') is True, (apps_data, apps_result)
            message = apps_data.get('error', {}).get('message', '')
            assert 'replaced on disk' in message and 'restart the MCP server' in message, (apps_data, apps_result)
        finally:
            for path, mode in modes.items():
                path.chmod(mode)

    with running_package_mcp(slot_binary, out / 'agy-new-mcp', version_b, env=env, cwd=work):
        setup(managed_b, version_b, 'resume', state='verify', package_id=id_b)
        confirmed = setup(managed_b, version_b, 'confirm', id_b, state='completed', package_id=id_b)
        assert confirmed['binding']['current'] == id_b and confirmed['binding']['rollback'] == id_a, confirmed
    return {
        'scope': 'Disposable agy-app Unix live swap, stale MCP rejection and reload',
        'versions': [version_a, version_b],
        'cases': ['cold slot install', 'enabled verify', 'atomic swap under running old MCP',
                  'old MCP rejects post-swap worker spawn', 'fresh MCP loads new version', 'confirm after reload'],
    }


def extract_published_v061(root):
    package = 'desktop-world-0.6.1-aarch64-apple-darwin'
    url = f'https://github.com/caelis-labs/desktop-world/releases/download/v0.6.1/{package}.zip'
    # Pin the published ZIP digest recorded by GitHub and its release sidecar.
    expected = 'cd99a1ee7e25b41bd7fb4e7bc6444a6458f981a06ed586a4f80672fe6a04007e'
    with urlopen(url, timeout=30) as response:
        if urlsplit(response.url).scheme != 'https':
            raise RuntimeError(f'Expected an HTTPS release download: {response.url}')
        archive = response.read(5 * 1024 * 1024)
    digest = hashlib.sha256(archive).hexdigest()
    if digest != expected:
        raise RuntimeError(f'Published v0.6.1 ZIP SHA-256 mismatch: expected {expected}, got {digest}')
    # The pinned bytes contain a regular-file-only ZIP. Restore executable mode
    # after extraction because zipfile does not apply POSIX permissions.
    with zipfile.ZipFile(io.BytesIO(archive)) as verified:
        verified.extractall(root)
    source = root / package
    (source / 'bin/dtw').chmod(0o755)
    return source, digest


def published_agy_transition(root, out, candidate, candidate_version):
    env = isolated_environment(root)
    work = root / 'home/work'
    work.mkdir(mode=0o700)
    store = root / 'published upgrade store 中文'
    config = root / 'home/.gemini/config'
    plugins = config / 'plugins'
    slot = plugins / 'desktop-world'
    slot_binary = slot / 'bin/dtw'
    settings = config / 'config.json'
    previous_source, archive_sha256 = extract_published_v061(root)
    previous_binary = previous_source / 'bin/dtw'
    previous_version = '0.6.1'
    candidate_source = root / 'candidate source'
    candidate_binary = make_source(candidate_source, 'candidate', candidate, candidate_version, schema=2)
    previous_id = sha(previous_source / 'SHA256SUMS')
    candidate_id = sha(candidate_source / 'SHA256SUMS')
    previous_digest, candidate_digest = sha(previous_binary), sha(candidate_binary)
    assert previous_digest != candidate_digest, 'Use a candidate binary distinct from published v0.6.1'
    assert previous_id != candidate_id

    def run(binary, *arguments):
        result = subprocess.run([str(binary), *map(str, arguments), '--store', str(store)],
                                env=env, cwd=work, capture_output=True, encoding='utf-8', timeout=20)
        assert result.returncode == 0, (arguments, result.stdout, result.stderr)
        return json.loads(result.stdout)

    def setup(binary, version, command, *arguments, state, package_id):
        receipt = run(binary, 'setup', command, 'agy-app', 'agy', *arguments)
        assert (receipt['setup_schema'], receipt['store_schema'], receipt['manager_version']) == (1, 1, version), receipt
        assert (receipt['state'], receipt['host'], receipt['profile'], receipt['package_id']) == (state, 'agy-app', 'agy', package_id), receipt
        return receipt

    def check_binding(binary, current, pending, rollback):
        row, = run(binary, 'package', 'status')['bindings']
        assert (row['host'], row['setup_host']) == ('agy', 'agy-app'), row
        assert (row['current'], row['pending'], row['rollback']) == (current, pending, rollback), row

    def package_tree(directory):
        entries = host_files(directory)
        assert all(not (directory / path).is_symlink() for path in entries), directory
        return {path: (digest, mode) for path, (digest, _, _, mode) in entries.items()}

    setup(previous_binary, previous_version, 'prepare', previous_id, previous_source,
          state='host_action_required', package_id=previous_id)
    write(settings, {'plugins': {'desktop-world': {'enabled': True}}})
    settings_before = file_state(settings)
    assert sha(slot_binary) == previous_digest
    assert package_tree(slot) == package_tree(previous_source)

    with running_package_mcp(slot_binary, out / 'published-agy-old-mcp', previous_version, env=env, cwd=work) as old_mcp:
        old_process = old_mcp.child
        old_tools = old_mcp.rpc('tools/list', {})
        assert [tool['name'] for tool in old_tools['tools']] == ['apps', 'exec'], old_tools
        # The released manager leaves enabled App setup requiring host action.
        # Establish its pin after checking the isolated MCP connection.
        setup(previous_binary, previous_version, 'confirm', previous_id, state='completed', package_id=previous_id)
        check_binding(previous_binary, previous_id, None, None)
        original_slot = file_state(slot), host_files(slot)
        prepared = setup(candidate_binary, candidate_version, 'prepare', candidate_id, candidate_source,
                         state='host_action_required', package_id=candidate_id)
        assert 'restart its dtw MCP server' in prepared['next'], prepared
        backup = Path(prepared['registration']['backup'])
        assert backup.resolve(strict=True) == backup, prepared
        assert backup.is_relative_to(config) and not backup.is_relative_to(plugins), prepared
        assert list(plugins.iterdir()) == [slot], list(plugins.iterdir())
        assert (file_state(backup), host_files(backup)) == original_slot, prepared
        assert not slot.samefile(backup), prepared
        assert file_state(settings) == settings_before, prepared

        install = prepared['install']
        managed = Path(install['executable'])
        payload = store / 'packages' / candidate_id / 'payload'
        assert install['package_id'] == candidate_id and managed == payload / 'bin/dtw', prepared
        assert sha(managed) == sha(slot_binary) == candidate_digest, prepared
        assert package_tree(slot) == package_tree(payload) == package_tree(candidate_source), prepared
        check_binding(candidate_binary, previous_id, candidate_id, None)
        assert old_mcp.child is old_process and old_process.poll() is None
        assert old_mcp.rpc('tools/list', {}) == old_tools

        # v0.6.1 spawned these roles with bare flags. Replay only their argv
        # against the new image and pass EOF so no desktop request can dispatch.
        for role in ('native', 'scene', 'overlay'):
            rejected = subprocess.run([str(slot_binary), f'--dtw-{role}-worker'], env=env, cwd=work,
                                      stdin=subprocess.DEVNULL, capture_output=True, encoding='utf-8', timeout=5)
            assert rejected.returncode == 2 and not rejected.stdout, rejected
            assert json.loads(rejected.stderr)['error']['code'] == 'invalid_arguments', rejected

    with running_package_mcp(slot_binary, out / 'published-agy-new-mcp', candidate_version, env=env, cwd=work) as new_mcp:
        new_tools = new_mcp.rpc('tools/list', {})
        assert [tool['name'] for tool in new_tools['tools']] == ['apps', 'exec'], new_tools
        resumed = setup(managed, candidate_version, 'resume', state='verify', package_id=candidate_id)
        assert resumed['install'] == install, resumed
        setup(managed, candidate_version, 'confirm', candidate_id, state='completed', package_id=candidate_id)
    check_binding(managed, candidate_id, None, previous_id)
    assert file_state(settings) == settings_before
    assert (file_state(backup), host_files(backup)) == original_slot
    return {
        'versions': [previous_version, candidate_version],
        'published_archive_sha256': archive_sha256,
        'executable_sha256': [previous_digest, candidate_digest],
        'package_ids': [previous_id, candidate_id],
        'cases': ['complete slot switch with the published MCP still connected',
                  'original slot retained and settings preserved', 'new image rejects legacy worker flags',
                  'verify after MCP-only reload', 'confirm with rollback pin'],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--next-dtw', type=Path)
    parser.add_argument('--published-v0.6.1', dest='published_v061', action='store_true',
                        help='also test the published arm64 macOS package against --dtw')
    args = parser.parse_args()
    if args.published_v061 and (sys.platform != 'darwin' or platform.machine() != 'arm64'):
        parser.error('--published-v0.6.1 requires native arm64 macOS')
    binary=args.dtw.resolve();manager=(args.next_dtw or args.dtw).resolve();out=args.output.resolve()
    store=out/'managed store 中文';version=subprocess.check_output([str(binary),'--version'],encoding='utf-8').strip().split()[-1]
    manager_version=subprocess.check_output([str(manager),'--version'],encoding='utf-8').strip().split()[-1]
    if args.next_dtw and manager_version == version:
        parser.error('--next-dtw must report a different version from --dtw')
    out.mkdir(parents=True,exist_ok=False)
    def cli(*arguments,ok=True):
        result=subprocess.run([str(manager),'package',*map(str,arguments),'--store',str(store)],capture_output=True,encoding='utf-8',timeout=20)
        assert (result.returncode==0)==ok,(arguments,result.stdout,result.stderr)
        if not ok:return result.stderr
        value=json.loads(result.stdout);assert value['manager_version']==manager_version and value['store_schema']==1,value
        return value
    def source(name,executable_source=binary,*,schema=1):
        package_version=subprocess.check_output([str(executable_source),'--version'],encoding='utf-8').strip().split()[-1]
        root = out / ('source ' + name)
        make_source(root, name, executable_source, package_version, schema=schema)
        return cli('import',root)
    a=source('A');b=source('B',manager,schema=2);c=source('C');assert cli('import',out/'source A')['package_id']==a['package_id']
    with tempfile.TemporaryDirectory(prefix='dtw-setup-probe-') as temporary:
        root = Path(temporary).resolve(strict=True)
        setup_report = setup_transition(root, out, (binary, manager), (a, b), (version, manager_version))
    with tempfile.TemporaryDirectory(prefix='dtw-agy-setup-probe-') as temporary:
        root = Path(temporary).resolve(strict=True)
        agy_report = agy_swap_transition(root, out, (binary, manager), (a, b), (version, manager_version))
    cli('bind','agy',a['package_id']);cli('activate','agy');cli('bind','dsh-desktop',a['package_id']);cli('activate','dsh-desktop')
    cli('bind','dsh-desktop',b['package_id']);staged=cli('status')['bindings'];dsh=next(r for r in staged if r['host']=='dsh-desktop');assert dsh['current']==a['package_id'] and dsh['pending']==b['package_id']
    fresh=out/'fresh-version';fresh.mkdir();verified=PackageClient(Path(b['executable']),fresh)
    try:
        initialized=verified.rpc('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'updated-runtime-verifier','version':'1'}});verified.notify('notifications/initialized')
        assert initialized['serverInfo']['version']==b['version'] and initialized['serverInfo']['name']=='dtw',initialized
        assert [t['name'] for t in verified.rpc('tools/list',{})['tools']]==['apps','exec']
    finally:verified.close()
    cli('activate','dsh-desktop');live=out/'live';live.mkdir();m=PackageClient(Path(c['executable']),live)
    try:
        old_runtime=m.rpc('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'managed-package-lease','version':'1'}});m.notify('notifications/initialized')
        assert old_runtime['serverInfo']['version']==a['version'],old_runtime
        assert [t['name'] for t in m.rpc('tools/list',{})['tools']]==['apps','exec']
        for package in (store/'packages').iterdir():
            path=package/'PACKAGE.json';record=json.loads(path.read_text(encoding='utf-8'));record['imported_at']=int(time.time())-8*86400;write(path,record)
        rows=cli('gc','--apply')['packages'];assert next(r for r in rows if r['package_id']==c['package_id'])['retained_by']=='running',rows
        assert all(next(r for r in rows if r['package_id']==p['package_id'])['retained_by']=='host_pin' for p in [a,b])
    finally:m.close()
    # A worker protects itself after the MCP supervisor is already gone. Release
    # of a nonexistent binding performs no AX/SCK/input/desktop operation.
    with (out/'worker-stderr.log').open('w',encoding='utf-8') as error:
        worker_env=os.environ.copy()
        if sys.platform=='linux':
            for key in ('DISPLAY','WAYLAND_DISPLAY','XDG_SESSION_TYPE'):worker_env.pop(key,None)
        worker=subprocess.Popen(native_worker_command(c['executable']),stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=error,text=True,encoding='utf-8',env=worker_env)
        try:
            request_id=str(uuid.uuid4());worker.stdin.write(json.dumps({'request_id':request_id,'operation':'release','binding':'never-created-'+str(uuid.uuid4())})+'\n');worker.stdin.flush()
            messages=queue.Queue();output_reader=threading.Thread(target=lambda:messages.put(worker.stdout.readline()),daemon=True);output_reader.start()
            line=messages.get(timeout=5);output_reader.join(timeout=2)
            if not line:
                worker.wait(timeout=5);error.flush();raise AssertionError({'exit':worker.returncode,'stderr':Path(error.name).read_text(encoding='utf-8',errors='replace')[-4096:]})
            reply=json.loads(line)
            if sys.platform=='linux':
                assert reply.get('request_id')==request_id and reply.get('result',{}).get('error')=='display_unavailable' and reply['result'].get('message'),reply
            else:assert reply=={'request_id':request_id,'result':{'released':False}}
            rows=cli('gc','--apply')['packages'];assert next(r for r in rows if r['package_id']==c['package_id'])['retained_by']=='running',rows
        finally:
            worker.stdin.close()
            try:worker.wait(timeout=5)
            except subprocess.TimeoutExpired:worker.kill();worker.wait(timeout=5);raise
            worker.stdout.close()
        assert worker.returncode==0,worker.returncode
        del worker
    rows=cli('gc','--apply')['packages'];assert next(r for r in rows if r['package_id']==c['package_id'])['action']=='removed',rows
    assert Path(a['package_path']).exists() and Path(b['package_path']).exists() and not Path(c['package_path']).exists()
    (out/'source A/skills/desktop-world/API.md').write_text('corrupt',encoding='utf-8');assert 'checksum' in cli('import',out/'source A',ok=False)
    cli('unbind','agy');cli('unbind','dsh-desktop');collected=cli('gc','--apply');assert not cli('status')['packages']
    report={'passed':True,'binary_sha256':sha(binary),'manager_sha256':sha(manager),'baseline_version':a['version'],'target_version':b['version'],'scope':'synthetic complete packages, disposable setup profile and standalone MCP process/locks','cases':['Unicode/space paths','complete-package deduplication','two host pins','schema1 baseline and schema2 upgrade','manager version check','standalone MCP runtime version verification','pending failure protection','rollback protection','live MCP shared lease','independent native worker lease','collection after process exit','corruption rejection','owned cleanup'],'collection':collected,'setup':setup_report,'agy_setup':agy_report}
    if args.published_v061:
        with tempfile.TemporaryDirectory(prefix='dtw-published-upgrade-') as temporary:
            root = Path(temporary).resolve(strict=True)
            report['published_agy_setup'] = published_agy_transition(root, out, binary, version)
    write(out/'report.json',report);print(json.dumps(report,ensure_ascii=True))
if __name__=='__main__':main()
