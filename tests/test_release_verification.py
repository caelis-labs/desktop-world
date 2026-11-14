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

    def native_receipt(self):
        candidate = json.loads((self.root / f'CANDIDATE-{v.TARGETS[0]}.json').read_bytes())
        return dict(schema=1, kind='macos-package-verification', signature=dict(v.SIGNATURE),
                    **{key: candidate[key] for key in v.NATIVE_FIELDS})

    def macos_candidate(self):
        root = self.root / 'native-candidate'
        root.mkdir()
        candidate = json.loads((self.root / f'CANDIDATE-{v.TARGETS[0]}.json').read_bytes())
        (root / 'CANDIDATE.json').write_text(json.dumps(candidate))
        for name in [candidate['archive'], candidate['archive'] + '.sha256', *v.RECEIPTS]:
            (root / name).write_bytes((self.root / name).read_bytes())
        return root

    def test_native_builder_verifies_binary_from_final_zip_before_sealing_receipt(self):
        root = self.macos_candidate()
        def signing(path):
            self.assertEqual(path.read_bytes(), b'fake binary for offline signature unit tests')
            return dict(v.SIGNATURE)
        with patch.object(v.builder, 'binary_format'), patch.object(v, 'signature', side_effect=signing) as native:
            receipt = v.verify_macos_candidate(root)
        native.assert_called_once()
        self.assertEqual(receipt, self.native_receipt())
        self.assertEqual(json.loads((root / v.NATIVE_RECEIPT).read_bytes()), receipt)

    def test_native_signature_failure_never_produces_receipt(self):
        root = self.macos_candidate()
        with patch.object(v.builder, 'binary_format'), patch.object(v, 'signature', side_effect=ValueError('bad signature')):
            with self.assertRaisesRegex(ValueError, 'bad signature'):
                v.verify_macos_candidate(root)
        self.assertFalse((root / v.NATIVE_RECEIPT).exists())

    def test_native_receipt_requires_original_accepted_notary(self):
        root = self.macos_candidate()
        (root / v.RECEIPTS[2]).write_text(json.dumps({'id': 'other', 'status': 'Accepted'}))
        with patch.object(v.builder, 'binary_format'), patch.object(v, 'signature', return_value=dict(v.SIGNATURE)):
            with self.assertRaisesRegex(ValueError, 'notarization receipt mismatch'):
                v.verify_macos_candidate(root)
        self.assertFalse((root / v.NATIVE_RECEIPT).exists())

    def test_portable_verification_consumes_bound_native_receipt_without_codesign(self):
        with patch.object(v.builder, 'binary_format'), patch.object(v, 'signature') as native:
            result = v.verify(self.root, 'v0.2.0', self.tag_commit, native_verification=self.native_receipt())
        native.assert_not_called()
        self.assertEqual(result['macos_signature'], 'immutable_native_build_receipt')

    def test_receipt_cannot_be_reused_for_another_source_archive_binary_or_signer(self):
        for key in (*v.NATIVE_FIELDS, 'signature'):
            receipt = self.native_receipt()
            receipt[key] = {} if isinstance(receipt[key], dict) else 'other'
            with self.subTest(key=key), patch.object(v.builder, 'binary_format'), patch.object(v, 'signature') as native:
                with self.assertRaisesRegex(ValueError, 'Native verification receipt'):
                    v.verify(self.root, 'v0.2.0', self.tag_commit, native_verification=receipt)
                native.assert_not_called()

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

    def legacy_host_components(self, target):
        # Historical entries are independent of the current layout generator.
        binary = 'bin/dtw' if target == v.TARGETS[0] else 'bin/dtw.exe'
        identity = {'name': 'desktop-world', 'version': '0.2.0'}
        values = {
            '.claude-plugin/plugin.json': identity,
            'plugin.json': dict(identity, **{'$schema': 'https://agent-plugins.org/schemas/1.0.0/plugin.schema.json'}),
            '.antigravity-plugin/plugin.json': dict(identity, **{'$schema': 'https://antigravity.google/schemas/v1/plugin.json'}),
            '.mcp.json': {'mcpServers': {'desktop-world': {
                'command': '${CLAUDE_PLUGIN_ROOT}/' + binary, 'args': ['mcp']}}},
            'mcp.json': {'$schema': 'https://agent-plugins.org/schemas/1.0.0/mcp.schema.json',
                         'mcpServers': {'desktop-world': {'type': 'stdio', 'command': './' + binary, 'args': ['mcp']}}},
            'mcp_config.json': {'mcpServers': {'desktop-world': {
                'command': './' + binary, 'args': ['mcp'], 'cwd': '${PLUGIN_ROOT}'}}},
            'package.json': {'name': '@caelis-labs/desktop-world', 'version': '0.2.0', 'type': 'module',
                             'main': './cordis/index.mjs', 'exports': './cordis/index.mjs'},
        }
        components = {name: json.dumps(value).encode() for name, value in values.items()}
        components['cordis/index.mjs'] = b'// legacy adapter fixture'
        entries = {'target': target, 'binary': binary,
                   'formats': ['agent-plugins-1.0', 'claude-code', 'antigravity', 'dsh-cordis']}
        return entries, components

    def declare_multi_host_plugins(self, schema=2):
        for platform in self.release['platforms']:
            if schema in (None, 1):
                entries, components = self.legacy_host_components(platform['target'])
            else:
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
                if schema is not None:
                    build['package_store_schema'] = schema
                if schema == 2:
                    files['skills/dtw/SKILL.md'] = files.pop('skills/desktop-world/SKILL.md')
                files['BUILD.json'] = json.dumps(build).encode()
            self.rewrite_package(platform, add)

    def test_declared_multi_host_plugins_reuse_target_and_version(self):
        self.declare_multi_host_plugins()
        self.assertEqual(self.audit()['assets_verified'], 13)

    def test_schema1_multi_host_archive_remains_valid_and_unchanged(self):
        self.declare_multi_host_plugins(schema=1)
        before = {p.name: p.read_bytes() for p in self.root.iterdir()}
        self.assertEqual(self.audit()['assets_verified'], 13)
        self.assertEqual({p.name: p.read_bytes() for p in self.root.iterdir()}, before)

    def test_earlier_multi_host_archive_without_layout_schema_remains_valid(self):
        self.declare_multi_host_plugins(schema=None)
        self.assertEqual(self.audit()['assets_verified'], 13)

    def test_unknown_layout_schema_is_rejected_without_host_declaration(self):
        for schema in (0, 3, '2', None, True, 2.0):
            def change(files):
                build = json.loads(files['BUILD.json'])
                build['package_store_schema'] = schema
                files['BUILD.json'] = json.dumps(build).encode()
            self.rewrite_package(self.release['platforms'][0], change)
            with self.subTest(schema=schema), self.assertRaisesRegex(ValueError, 'Unsupported package_store_schema'):
                self.audit()

    def test_schema2_cannot_use_legacy_skill_directory(self):
        self.declare_multi_host_plugins()
        self.rewrite_package(self.release['platforms'][0], lambda files:
                             files.update({'skills/desktop-world/SKILL.md': files.pop('skills/dtw/SKILL.md')}))
        with self.assertRaisesRegex(ValueError, 'Missing runtime/Skill'):
            self.audit()

    def test_schema2_cannot_bundle_both_skill_directories(self):
        self.declare_multi_host_plugins()
        self.rewrite_package(self.release['platforms'][0], lambda files:
                             files.update({'skills/desktop-world/SKILL.md': b'skill'}))
        with self.assertRaisesRegex(ValueError, 'Skill layout mismatch'):
            self.audit()

    def test_layout_schema_cannot_be_inferred_from_new_host_entries(self):
        self.declare_multi_host_plugins()
        def remove_schema(files):
            build = json.loads(files['BUILD.json'])
            build.pop('package_store_schema')
            files['BUILD.json'] = json.dumps(build).encode()
            files['skills/desktop-world/SKILL.md'] = files.pop('skills/dtw/SKILL.md')
        self.rewrite_package(self.release['platforms'][0], remove_schema)
        with self.assertRaisesRegex(ValueError, 'Agent Plugins MCP entry mismatch'):
            self.audit()

    def test_schema2_rejects_old_mcp_keys_and_dual_aliases_for_each_host(self):
        self.declare_multi_host_plugins()
        platform = self.release['platforms'][0]
        for path in ('mcp.json', '.mcp.json', 'mcp_config.json'):
            for dual in (False, True):
                original = {}
                def change(files):
                    original[path] = files[path]
                    config = json.loads(files[path])
                    server = config['mcpServers']['dtw']
                    config['mcpServers'] = {'desktop-world': server}
                    if dual:
                        config['mcpServers']['dtw'] = server
                    files[path] = json.dumps(config).encode()
                self.rewrite_package(platform, change)
                with self.subTest(path=path, dual=dual), self.assertRaisesRegex(ValueError, 'MCP entry mismatch'):
                    self.audit()
                self.rewrite_package(platform, lambda files: files.update(original))

    def test_schema2_skill_digest_is_verified_from_new_path(self):
        self.declare_multi_host_plugins()
        self.rewrite_package(self.release['platforms'][0], lambda files:
                             files.update({'skills/dtw/SKILL.md': b'changed'}))
        with self.assertRaisesRegex(ValueError, 'Skill digest mismatch'):
            self.audit()

    def test_native_candidate_accepts_schema2_layout(self):
        self.declare_multi_host_plugins()
        root = self.macos_candidate()
        with patch.object(v.builder, 'binary_format'), patch.object(v, 'signature', return_value=dict(v.SIGNATURE)):
            v.verify_macos_candidate(root)

    def test_schema2_cannot_skip_host_validation_by_omitting_declaration(self):
        self.declare_multi_host_plugins()
        def remove_declaration(files):
            build = json.loads(files['BUILD.json'])
            build.pop('plugin_package')
            files['BUILD.json'] = json.dumps(build).encode()
        self.rewrite_package(self.release['platforms'][0], remove_declaration)
        with self.assertRaisesRegex(ValueError, 'Missing schema 2 plugin package declaration'):
            self.audit()

    def test_native_candidate_rejects_unknown_layout_schema(self):
        def change(files):
            build = json.loads(files['BUILD.json'])
            build['package_store_schema'] = 3
            files['BUILD.json'] = json.dumps(build).encode()
        self.rewrite_package(self.release['platforms'][0], change)
        root = self.macos_candidate()
        with patch.object(v.builder, 'binary_format'), patch.object(v, 'signature') as signature:
            with self.assertRaisesRegex(ValueError, 'Unsupported package_store_schema'):
                v.verify_macos_candidate(root)
        signature.assert_not_called()
        self.assertFalse((root / v.NATIVE_RECEIPT).exists())

    def test_declared_plugin_component_removal_is_rejected_even_after_rehashing(self):
        self.declare_multi_host_plugins()
        self.rewrite_package(self.release['platforms'][1], lambda files: files.pop('mcp_config.json'))
        with self.assertRaisesRegex(ValueError, 'Missing declared plugin component'):
            self.audit()

    def test_declared_plugin_cannot_redirect_to_external_executable(self):
        self.declare_multi_host_plugins()
        platform = self.release['platforms'][1]
        def replace_config(files):
            files['mcp_config.json'] = json.dumps({'mcpServers': {'dtw': {
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
