"""Offline promotion audit contracts; platform signatures are mocked, ZIP integrity is real."""
import importlib.util
import json
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location('release_verifier', Path(__file__).parents[1] / 'scripts/verify-release.py')
v = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v)


class ReleaseVerificationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.tag_commit = 'a' * 40
        self.release = {'version': '0.2.0', 'tag': 'v0.2.0', 'tag_commit': self.tag_commit, 'platforms': []}
        for index, target in enumerate(v.TARGETS):
            source = {'commit': str(index + 1) * 40, 'tree': 'b' * 40, 'cargo_lock_sha256': 'c' * 64}
            package = f'desktop-world-0.2.0-{target}'
            binary_name = 'bin/dtw' if index == 0 else 'bin/dtw.exe'
            binary = b'fake binary for offline signature unit tests'
            notary = {'status': 'unverified', 'requested': False, 'stapled': False}
            if index == 0:
                notary = {'status': 'Accepted', 'requested': True, 'stapled': False, 'submission_id': 'original-id', 'receipts': list(v.RECEIPTS)}
            build = {'schema': 1, 'kind': 'unpublished-rewrite-candidate', 'version': '0.2.0', 'target': target,
                     'source': source, 'binary': {'path': binary_name, 'sha256': v.sha(binary)},
                     'skill_sha256': v.sha(b'skill'), 'dependencies_sha256': v.sha(b'{}'),
                     'signing': {'status': 'verified' if index == 0 else 'not_requested'},
                     'notarization': {'status': 'unverified'}}
            build_bytes = json.dumps(build).encode()
            self.write(f'BUILD-{target}.json', build_bytes)
            files = {binary_name: binary, 'BUILD.json': build_bytes, 'skills/desktop-world/SKILL.md': b'skill',
                     '.claude-plugin/plugin.json': b'{}', 'notices/DEPENDENCIES.json': b'{}'}
            files['SHA256SUMS'] = ''.join(f'{v.sha(data)}  {name}\n' for name, data in files.items()).encode()
            archive = package + '.zip'
            with zipfile.ZipFile(self.root / archive, 'w') as z:
                for name, data in files.items():
                    info = zipfile.ZipInfo(package + '/' + name)
                    info.external_attr = (stat.S_IFREG | 0o755) << 16
                    z.writestr(info, data)
            digest = v.sha((self.root / archive).read_bytes())
            self.write(archive + '.sha256', f'{digest}  {archive}\n'.encode())
            platform = {'target': target, 'source': source, 'archive': archive, 'archive_sha256': digest,
                        'binary_sha256': v.sha(binary), 'notarization': notary}
            self.release['platforms'].append(platform)
            self.write_json(f'CANDIDATE-{target}.json', dict(platform, schema=1, kind=build['kind'], version='0.2.0',
                            build_manifest_sha256=v.sha(build_bytes), signing=build['signing']['status'], gui='unverified'))
        for name in v.RECEIPTS:
            self.write_json(name, {'id': 'original-id', 'status': 'Accepted', 'name': self.release['platforms'][0]['archive']})
        self.write_json('RELEASE.json', self.release)
        self.resum()

    def write(self, name, data):
        (self.root / name).write_bytes(data)

    def write_json(self, name, data):
        self.write(name, json.dumps(data).encode())

    def resum(self):
        self.write('SHA256SUMS', ''.join(f'{v.sha(p.read_bytes())}  {p.name}\n' for p in sorted(self.root.iterdir()) if p.name != 'SHA256SUMS').encode())

    def audit(self):
        with patch.object(v.builder, 'binary_format'), patch.object(v, 'signature') as signature:
            result = v.verify(self.root, 'v0.2.0', self.tag_commit)
            signature.assert_called_once()
            return result

    def mutate_candidate(self, key, value):
        name = f'CANDIDATE-{v.TARGETS[0]}.json'
        candidate = json.loads((self.root / name).read_bytes())
        candidate[key] = value
        self.write_json(name, candidate)
        self.resum()

    def test_historical_format_different_sources_and_unverified_embedded_notary(self):
        self.assertEqual(self.audit()['assets_verified'], 13)

    def rewrite_package(self, platform, mutate):
        archive = self.root / platform['archive']
        prefix = archive.stem + '/'
        with zipfile.ZipFile(archive) as z:
            files = {name[len(prefix):]: z.read(name) for name in z.namelist()}
        mutate(files)
        files.pop('SHA256SUMS', None)
        files['SHA256SUMS'] = ''.join(f'{v.sha(data)}  {name}\n' for name, data in files.items()).encode()
        with zipfile.ZipFile(archive, 'w') as z:
            for name, data in files.items():
                info = zipfile.ZipInfo(prefix + name)
                info.external_attr = (stat.S_IFREG | 0o755) << 16
                z.writestr(info, data)
        target = platform['target']
        self.write(f'BUILD-{target}.json', files['BUILD.json'])
        platform['archive_sha256'] = v.sha(archive.read_bytes())
        self.write(archive.name + '.sha256', f'{platform["archive_sha256"]}  {archive.name}\n'.encode())
        candidate = json.loads((self.root / f'CANDIDATE-{target}.json').read_bytes())
        candidate.update(archive_sha256=platform['archive_sha256'], build_manifest_sha256=v.sha(files['BUILD.json']))
        self.write_json(f'CANDIDATE-{target}.json', candidate)
        self.write_json('RELEASE.json', self.release)
        self.resum()

    def declare_multi_host_plugins(self):
        for platform in self.release['platforms']:
            with tempfile.TemporaryDirectory() as temporary:
                package = Path(temporary)
                entries = v.builder.plugin_package.write_plugin_entries(
                    Path(__file__).parents[1], package, '0.2.0', platform['target'])
                components = {p.relative_to(package).as_posix(): p.read_bytes()
                              for p in package.rglob('*') if p.is_file()}
            def add(files):
                files.update(components)
                build = json.loads(files['BUILD.json'])
                build['plugin_package'] = entries
                files['BUILD.json'] = json.dumps(build).encode()
            self.rewrite_package(platform, add)

    def test_declared_multi_host_plugins_reuse_target_and_version(self):
        self.declare_multi_host_plugins()
        self.assertEqual(self.audit()['assets_verified'], 13)

    def test_declared_plugin_component_removal_is_rejected_even_after_rehashing(self):
        self.declare_multi_host_plugins()
        self.rewrite_package(self.release['platforms'][1], lambda files: files.pop('mcp_config.json'))
        with self.assertRaisesRegex(ValueError, 'Missing declared plugin component'):
            self.audit()

    def test_declared_plugin_cannot_redirect_to_external_executable(self):
        self.declare_multi_host_plugins()
        platform = self.release['platforms'][1]
        def replace_config(files):
            files['mcp_config.json'] = json.dumps({'mcpServers': {'desktop-world': {
                'command': 'C:/another-install/dtw.exe', 'args': ['mcp'], 'cwd': '${PLUGIN_ROOT}'}}}).encode()
        self.rewrite_package(platform, replace_config)
        with self.assertRaisesRegex(ValueError, 'Antigravity MCP entry mismatch'):
            self.audit()

    def test_declared_plugin_cannot_advertise_another_version(self):
        self.declare_multi_host_plugins()
        def replace_version(files):
            manifest = json.loads(files['.antigravity-plugin/plugin.json'])
            manifest['version'] = '9.9.9'
            files['.antigravity-plugin/plugin.json'] = json.dumps(manifest).encode()
        self.rewrite_package(self.release['platforms'][1], replace_version)
        with self.assertRaisesRegex(ValueError, 'Plugin identity/version mismatch'):
            self.audit()

    def test_wrong_candidate_version_target_source_and_digests(self):
        for key, value in [('version', '0.9.0'), ('target', v.TARGETS[1]), ('source', {}),
                           ('archive_sha256', '0' * 64), ('binary_sha256', '0' * 64),
                           ('build_manifest_sha256', '0' * 64), ('signing', 'not_requested')]:
            with self.subTest(key=key):
                name = f'CANDIDATE-{v.TARGETS[0]}.json'
                before = (self.root / name).read_bytes()
                self.mutate_candidate(key, value)
                with self.assertRaises(ValueError): self.audit()
                self.write(name, before)
                self.resum()

    def test_wrong_receipt(self):
        self.write_json(v.RECEIPTS[2], {'id': 'another-id', 'status': 'Accepted'})
        self.resum()
        with self.assertRaisesRegex(ValueError, 'receipt identity'): self.audit()

    def test_corrupt_asset_not_masked_by_manifest(self):
        self.write('RELEASE.json', b'{}')
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'): self.audit()

    def test_missing_extra_and_wrong_tag_assets(self):
        self.write('unexpected.zip', b'bad')
        with self.assertRaisesRegex(ValueError, 'missing release asset'): self.audit()
        (self.root / 'unexpected.zip').unlink()
        self.release['tag_commit'] = 'b' * 40
        self.write_json('RELEASE.json', self.release)
        self.resum()
        with self.assertRaisesRegex(ValueError, 'tag identity'): self.audit()

    def test_signature_failure_is_not_ignored(self):
        with patch.object(v.builder, 'binary_format'), patch.object(v, 'signature', side_effect=ValueError('invalid signature')):
            with self.assertRaisesRegex(ValueError, 'invalid signature'): v.verify(self.root, 'v0.2.0', self.tag_commit)

    def test_wrong_binary_architecture_is_rejected(self):
        with patch.object(v.builder, 'binary_format', side_effect=ValueError('wrong architecture')), patch.object(v, 'signature'):
            with self.assertRaisesRegex(ValueError, 'wrong architecture'):
                v.verify(self.root, 'v0.2.0', self.tag_commit)

    def test_archive_integrity_is_checked_even_when_outer_hashes_are_updated(self):
        platform = self.release['platforms'][0]
        archive = self.root / platform['archive']
        with zipfile.ZipFile(archive, 'a') as z:
            info = zipfile.ZipInfo(archive.stem + '/extra')
            info.external_attr = (stat.S_IFREG | 0o644) << 16
            z.writestr(info, b'not in inner checksums')
        digest = v.sha(archive.read_bytes())
        platform['archive_sha256'] = digest
        self.write_json('RELEASE.json', self.release)
        self.write(archive.name + '.sha256', f'{digest}  {archive.name}\n'.encode())
        self.mutate_candidate('archive_sha256', digest)
        with self.assertRaisesRegex(ValueError, 'Archive inventory'):
            self.audit()

    def test_checksums_reject_duplicates_and_paths(self):
        for data in [('0' * 64 + '  ../escape\n'), ('0' * 64 + '  x\n') * 2]:
            with self.assertRaises(ValueError): v.checksums(data.encode())


if __name__ == '__main__':
    unittest.main()
