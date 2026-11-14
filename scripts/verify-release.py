#!/usr/bin/env python3
"""Read-only release asset audit; draft preflight does not publish anything."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import tempfile
import zipfile

_spec = importlib.util.spec_from_file_location('candidate_builder', Path(__file__).with_name('build-rewrite-candidate.py'))
builder = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(builder)
TARGETS = ('aarch64-apple-darwin', 'x86_64-pc-windows-msvc')
TEAM = '64KZ67PM5J'
RECEIPTS = ('notarization-submit.json', 'notarization-wait.json', 'notarization-info.json')
NATIVE_RECEIPT = 'NATIVE-VERIFICATION.json'
NATIVE_FIELDS = ('version', 'target', 'source', 'archive', 'archive_sha256',
                 'binary_sha256', 'build_manifest_sha256', 'notarization')
SIGNATURE = {'status': 'verified', 'identifier': 'dev.caelis.dtw', 'team': TEAM,
             'hardened_runtime': True, 'timestamped': True}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def checksums(data):
    result = {}
    for line in data.decode('utf-8').splitlines():
        require(re.fullmatch(r'[0-9a-f]{64}  [^/\\:]+', line), 'Invalid asset checksum')
        digest, name = line.split('  ', 1)
        require(name not in result and name not in ('.', '..'), 'Duplicate/unsafe checksum')
        result[name] = digest
    return result


def signature(binary):
    checked = subprocess.run(['codesign', '--verify', '--strict', str(binary)], capture_output=True, text=True)
    require(checked.returncode == 0, 'codesign verification failed: ' + checked.stderr.strip())
    proc = subprocess.run(['codesign', '-d', '--verbose=4', str(binary)], check=True, capture_output=True, text=True)
    details = proc.stdout + proc.stderr
    require('Identifier=dev.caelis.dtw\n' in details and f'TeamIdentifier={TEAM}\n' in details
            and 'Authority=Developer ID Application:' in details and '(runtime)' in details
            and re.search(r'^Timestamp=.+$', details, re.M), 'Invalid release signing identity/runtime/timestamp')
    return dict(SIGNATURE)


def check_native_verification(receipt, candidate):
    """The publisher obtains this receipt only from its authenticated CI artifact."""
    require(isinstance(receipt, dict) and receipt.get('schema') == 1
            and receipt.get('kind') == 'macos-package-verification'
            and candidate.get('target') == TARGETS[0]
            and all(receipt.get(key) == candidate.get(key) for key in NATIVE_FIELDS)
            and receipt.get('signature') == SIGNATURE,
            'Native verification receipt does not match the immutable macOS candidate')


def verify_macos_candidate(root):
    """Verify the final ZIP on its native builder, before the artifact is sealed."""
    root = Path(root)
    candidate = json.loads((root / 'CANDIDATE.json').read_bytes())
    version = candidate.get('version', '')
    require(re.fullmatch(r'\d+\.\d+\.\d+', version)
            and candidate.get('schema') == 1 and candidate.get('kind') == 'unpublished-rewrite-candidate'
            and candidate.get('target') == TARGETS[0] and candidate.get('signing') == 'verified',
            'Expected a signed macOS candidate')
    package = f'desktop-world-{version}-{TARGETS[0]}'
    archive = package + '.zip'
    require(candidate.get('archive') == archive
            and sha((root / archive).read_bytes()) == candidate.get('archive_sha256'),
            'Native candidate archive mismatch')
    require(checksums((root / (archive + '.sha256')).read_bytes()) == {archive: candidate['archive_sha256']},
            'Native candidate sidecar mismatch')
    builder.verify_archive(root / archive, package)
    with zipfile.ZipFile(root / archive) as z:
        build_bytes = z.read(package + '/BUILD.json')
        build = json.loads(build_bytes)
        require(sha(build_bytes) == candidate.get('build_manifest_sha256')
                and all(build.get(key) == candidate.get(key) for key in ('schema', 'kind', 'version', 'target', 'source'))
                and build.get('signing', {}).get('status') == 'verified', 'Native candidate BUILD mismatch')
        binary = z.read(package + '/bin/dtw')
        require(build.get('binary', {}).get('path') == 'bin/dtw'
                and sha(binary) == build['binary'].get('sha256') == candidate.get('binary_sha256'),
                'Native candidate binary mismatch')
        plugin_package(z, package + '/', build)
        with tempfile.TemporaryDirectory(prefix='dtw-native-package-check-') as tmp:
            path = Path(tmp) / 'dtw'
            path.write_bytes(binary)
            builder.binary_format(path, TARGETS[0])
            signing = signature(path)
    notary = candidate.get('notarization', {})
    require(notary.get('requested') is True and notary.get('status') == 'Accepted'
            and notary.get('receipts') == list(RECEIPTS) and notary.get('submission_id'),
            'Native candidate lacks accepted notarization')
    for name in RECEIPTS:
        receipt = json.loads((root / name).read_bytes())
        require(receipt.get('id') == notary['submission_id']
                and (name == RECEIPTS[0] or receipt.get('status') == 'Accepted')
                and (name != RECEIPTS[2] or receipt.get('name') == archive), 'Native notarization receipt mismatch')
    receipt = dict(schema=1, kind='macos-package-verification', signature=signing,
                   **{key: candidate[key] for key in NATIVE_FIELDS})
    check_native_verification(receipt, candidate)
    (root / NATIVE_RECEIPT).write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
    return receipt


def plugin_package(archive, prefix, build):
    """Audit the schema-selected Skill and declared host entries, including legacy."""
    layout = builder.plugin_package.package_layout(build.get('package_store_schema', 1))
    skill = layout['skill_dir'] + '/SKILL.md'
    names = archive.namelist()
    require(prefix + skill in names, 'Missing runtime/Skill/plugin/notices')
    other_skill = 'skills/dtw/' if layout['mcp_server'] == 'desktop-world' else 'skills/desktop-world/'
    require(not any(name.startswith(prefix + other_skill) for name in names), 'Skill layout mismatch')
    require(sha(archive.read(prefix + skill)) == build.get('skill_sha256'), 'Skill digest mismatch')
    server = layout['mcp_server']
    if 'plugin_package' not in build:
        require(build.get('package_store_schema', 1) == 1, 'Missing schema 2 plugin package declaration')
        return  # Existing accepted archives did not declare this layout.
    target = build['target']
    binary = build['binary']['path']
    require(build['plugin_package'] == {
        'target': target, 'binary': binary,
        'formats': ['agent-plugins-1.0', 'claude-code', 'antigravity', 'dsh-cordis'],
    }, 'Plugin package declaration mismatch')

    def read(name):
        require(prefix + name in archive.namelist(), 'Missing declared plugin component: ' + name)
        value = json.loads(archive.read(prefix + name))
        require(isinstance(value, dict), 'Invalid declared plugin component: ' + name)
        return value

    for path in ('plugin.json', '.claude-plugin/plugin.json', '.antigravity-plugin/plugin.json'):
        manifest = read(path)
        require(manifest.get('name') == 'desktop-world' and manifest.get('version') == build['version'],
                'Plugin identity/version mismatch: ' + path)
        if path == 'plugin.json':
            require(manifest.get('$schema') == 'https://agent-plugins.org/schemas/1.0.0/plugin.schema.json',
                    'Agent Plugins schema mismatch')
        elif path.startswith('.antigravity-plugin/'):
            require(manifest.get('$schema') == 'https://antigravity.google/schemas/v1/plugin.json',
                    'Antigravity schema mismatch')
    standard = read('mcp.json')
    require(standard.get('$schema') == 'https://agent-plugins.org/schemas/1.0.0/mcp.schema.json'
            and standard.get('mcpServers') == {server: {
                'type': 'stdio', 'command': './' + binary, 'args': ['mcp']}},
            'Agent Plugins MCP entry mismatch')
    require(read('.mcp.json').get('mcpServers') == {server: {
        'command': '${CLAUDE_PLUGIN_ROOT}/' + binary, 'args': ['mcp']}}, 'Claude MCP entry mismatch')
    require(read('mcp_config.json').get('mcpServers') == {server: {
        'command': './' + binary, 'args': ['mcp'], 'cwd': '${PLUGIN_ROOT}'}}, 'Antigravity MCP entry mismatch')
    cordis = read('package.json')
    require(cordis.get('name') == '@caelis-labs/desktop-world' and cordis.get('version') == build['version']
            and cordis.get('type') == 'module' and cordis.get('main') == './cordis/index.mjs'
            and cordis.get('exports') == './cordis/index.mjs' and not cordis.get('dependencies')
            and not cordis.get('scripts'), 'Cordis package entry mismatch')
    require(prefix + 'cordis/index.mjs' in archive.namelist(), 'Missing declared Cordis adapter')


def verify(root, tag, tag_commit, *, native_verification=None):
    root = Path(root)
    require(re.fullmatch(r'v\d+\.\d+\.\d+', tag), 'Expected stable release tag')
    version = tag[1:]
    require(re.fullmatch(r'[0-9a-f]{40}', tag_commit), 'Invalid tag commit')
    names = {'RELEASE.json', 'SHA256SUMS', *RECEIPTS}
    for target in TARGETS:
        names.update({f'BUILD-{target}.json', f'CANDIDATE-{target}.json',
                      f'desktop-world-{version}-{target}.zip', f'desktop-world-{version}-{target}.zip.sha256'})
    require(all(p.is_file() and not p.is_symlink() for p in root.iterdir()), 'Nonregular release asset')
    require({p.name for p in root.iterdir()} == names, 'Unexpected or missing release asset')
    sums = checksums((root / 'SHA256SUMS').read_bytes())
    require(set(sums) == names - {'SHA256SUMS'}, 'Release checksum inventory mismatch')
    for name, digest in sums.items():
        require(sha((root / name).read_bytes()) == digest, f'Release checksum mismatch: {name}')
    read = lambda name: json.loads((root / name).read_text(encoding='utf-8'))
    release = read('RELEASE.json')
    require(release.get('version') == version and release.get('tag') == tag
            and release.get('tag_commit') == tag_commit, 'Release version/tag identity mismatch')
    platforms = release.get('platforms', [])
    require(len(platforms) == 2 and {p.get('target') for p in platforms} == set(TARGETS), 'Release platform matrix mismatch')
    for platform in platforms:
        target = platform['target']
        candidate = read(f'CANDIDATE-{target}.json')
        build_bytes = (root / f'BUILD-{target}.json').read_bytes()
        build = json.loads(build_bytes)
        for record in (candidate, build):
            require(record.get('schema') == 1 and record.get('kind') == 'unpublished-rewrite-candidate', 'Unsupported candidate schema')
            require(record.get('version') == version and record.get('target') == target, 'Candidate version/target mismatch')
            require(record.get('source') == platform.get('source'), 'Candidate source mismatch')
        source = platform['source']
        require(all(re.fullmatch(r'[0-9a-f]{40}', source.get(k, '')) for k in ('commit', 'tree'))
                and re.fullmatch(r'[0-9a-f]{64}', source.get('cargo_lock_sha256', '')), 'Invalid source identity')
        package = f'desktop-world-{version}-{target}'
        archive = package + '.zip'
        require(candidate.get('archive') == platform.get('archive') == archive, 'Archive name mismatch')
        archive_sha = sha((root / archive).read_bytes())
        require(candidate.get('archive_sha256') == platform.get('archive_sha256') == archive_sha, 'Archive digest mismatch')
        require(checksums((root / (archive + '.sha256')).read_bytes()) == {archive: archive_sha}, 'Archive sidecar mismatch')
        require(candidate.get('build_manifest_sha256') == sha(build_bytes), 'BUILD digest mismatch')
        builder.verify_archive(root / archive, package)
        with zipfile.ZipFile(root / archive) as z:
            prefix = package + '/'
            files = {n[len(prefix):] for n in z.namelist()}
            binary_name = 'bin/dtw' if target == TARGETS[0] else 'bin/dtw.exe'
            require({binary_name, 'BUILD.json', '.claude-plugin/plugin.json',
                     'notices/DEPENDENCIES.json'} <= files, 'Missing runtime/Skill/plugin/notices')
            require(z.read(prefix + 'BUILD.json') == build_bytes, 'Embedded BUILD differs')
            binary = z.read(prefix + binary_name)
            require(build.get('binary', {}).get('path') == binary_name, 'Binary path mismatch')
            require(sha(binary) == build['binary'].get('sha256') == candidate.get('binary_sha256')
                    == platform.get('binary_sha256'), 'Binary digest mismatch')
            require(sha(z.read(prefix + 'notices/DEPENDENCIES.json')) == build.get('dependencies_sha256'), 'Notices digest mismatch')
            plugin_package(z, prefix, build)
            with tempfile.TemporaryDirectory(prefix='dtw-release-check-') as tmp:
                path = Path(tmp) / Path(binary_name).name
                path.write_bytes(binary)
                builder.binary_format(path, target)
                if target == TARGETS[0]:
                    require(candidate.get('signing') == build.get('signing', {}).get('status') == 'verified', 'Unsigned macOS candidate')
                    if native_verification is None:
                        signature(path)
                    else:
                        check_native_verification(native_verification, candidate)
        require(candidate.get('notarization') == platform.get('notarization'), 'Notarization manifest mismatch')
        if target == TARGETS[0]:
            notary = candidate['notarization']
            require(notary.get('status') == 'Accepted' and notary.get('requested') is True
                    and notary.get('receipts') == list(RECEIPTS) and notary.get('submission_id'), 'Missing accepted notarization')
            for name in RECEIPTS:
                receipt = read(name)
                require(receipt.get('id') == notary['submission_id'], 'Notarization receipt identity mismatch')
                if name != RECEIPTS[0]:
                    require(receipt.get('status') == 'Accepted', 'Notarization not accepted')
            require(read(RECEIPTS[2]).get('name') == archive, 'Notarization archive mismatch')
    return {'tag': tag, 'assets_verified': len(names), 'platforms': list(TARGETS),
            'macos_signature': 'native_codesign' if native_verification is None else 'immutable_native_build_receipt',
            'gui_acceptance': 'not inferred; asset integrity and signature audit only'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--assets', type=Path)
    mode.add_argument('--macos-candidate', type=Path)
    parser.add_argument('--tag')
    parser.add_argument('--tag-commit')
    args = parser.parse_args()
    if args.macos_candidate:
        result = verify_macos_candidate(args.macos_candidate)
    else:
        if not args.tag or not args.tag_commit:
            parser.error('--assets requires --tag and --tag-commit')
        result = verify(args.assets, args.tag, args.tag_commit)
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
