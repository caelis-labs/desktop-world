"""Linux package contracts use real ELF headers, tar payloads and plugin entries."""
import importlib.util
import io
import json
from pathlib import Path
import stat
import shutil
import struct
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).parents[1]


def load(name, script):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / script)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


builder = load('linux_candidate', 'build-rewrite-candidate.py')
promoter = load('linux_promoter', 'promote-release.py')
checker = load('linux_package_checker', 'check-linux-candidate.py')
LINUX = ('x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu')


def elf(machine):
    data = bytearray(64)
    data[:7] = b'\x7fELF\x02\x01\x01'
    struct.pack_into('<HH', data, 16, 3, machine)
    return data


class LinuxDistributionTests(unittest.TestCase):
    def test_native_targets_and_host_staging_recognize_both_architectures(self):
        for target, machines in ((LINUX[0], ('x86_64', 'AMD64')), (LINUX[1], ('aarch64', 'arm64'))):
            for machine in machines:
                with self.subTest(target=target, machine=machine), patch.object(builder.platform, 'system', return_value='Linux'), patch.object(builder.platform, 'machine', return_value=machine):
                    builder.native_host(target)
                    self.assertEqual(builder.plugin_package.host_target('Linux', machine), target)
        with patch.object(builder.platform, 'system', return_value='Linux'), patch.object(builder.platform, 'machine', return_value='aarch64'):
            with self.assertRaises(ValueError):
                builder.native_host(LINUX[0])
        with self.assertRaises(ValueError):
            builder.plugin_package.host_target('Linux', 'riscv64')

    def test_elf_architecture_checks_do_not_claim_an_older_glibc_runtime(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'dtw'
            for target, machine in zip(LINUX, (62, 183)):
                path.write_bytes(elf(machine))
                result = builder.binary_format(path, target)
                self.assertEqual(result, {'format': 'ELF64', 'architecture': target.split('-')[0]})
                path.write_bytes(elf(183 if machine == 62 else 62))
                with self.assertRaisesRegex(ValueError, 'architecture'):
                    builder.binary_format(path, target)
            for invalid in (b'\x7fELF', bytes(64), elf(40)):
                path.write_bytes(invalid)
                with self.assertRaises(ValueError):
                    builder.binary_format(path, LINUX[0])

    def test_measured_glibc_requirement_comes_from_versioned_symbols(self):
        class Commands:
            def run(self, args, **kwargs):
                if '--version-info' in args:
                    return 'Name: GLIBC_2.9 Name: GLIBC_2.39 Name: GLIBC_2.3.4', ''
                return '(NEEDED) Shared library: [libc.so.6]\n(NEEDED) Shared library: [libgcc_s.so.1]', ''
        result = builder.linux_runtime_evidence(Path('/tmp/dtw'), Commands(), ROOT)
        self.assertEqual(result['required_glibc_symbol_version'], '2.39')
        self.assertEqual(result['dynamic_dependencies'], ['libc.so.6', 'libgcc_s.so.1'])
        self.assertEqual(result['minimum_os_runtime'], 'unverified')

    def make_package(self, root, target):
        package = root / f'desktop-world-0.5.3-{target}'
        (package / 'bin').mkdir(parents=True)
        (package / 'bin/dtw').write_bytes(elf(62 if target == LINUX[0] else 183))
        (package / 'BUILD.json').write_text(json.dumps({'target': target}))
        archive = root / (package.name + builder.archive_suffix(target))
        builder.archive_package(package, archive, 1700000000)
        return package, archive

    def test_gzip_archive_is_deterministic_and_preserves_inventory_and_execute_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for target in LINUX:
                first = root / (target + '-first'); first.mkdir()
                second = root / (target + '-second'); second.mkdir()
                package, archive = self.make_package(first, target)
                _, repeated = self.make_package(second, target)
                self.assertEqual(archive.read_bytes(), repeated.read_bytes())
                builder.verify_archive(archive, package.name)
                with tarfile.open(archive) as content:
                    binary = content.getmember(package.name + '/bin/dtw')
                    self.assertTrue(binary.isfile())
                    self.assertEqual(binary.mode, 0o755)
                self.assertEqual((archive.parent / (archive.name + '.sha256')).read_text(),
                                 f'{builder.digest(archive)}  {archive.name}\n')

    def test_gzip_audit_rejects_a_corrupt_trailer(self):
        with tempfile.TemporaryDirectory() as directory:
            package, archive = self.make_package(Path(directory), LINUX[0])
            data = bytearray(archive.read_bytes())
            data[-8] ^= 1
            archive.write_bytes(data)
            with self.assertRaises(OSError):
                builder.verify_archive(archive, package.name)

    def test_tar_audit_rejects_links_traversal_duplicates_missing_execute_and_tampering(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package, archive = self.make_package(root, LINUX[0])
            with tarfile.open(archive) as source:
                original = [(item, source.extractfile(item).read()) for item in source.getmembers()]
            for mutation in ('symlink', 'traversal', 'duplicate', 'mode', 'content'):
                changed = root / (mutation + '.tar.gz')
                with tarfile.open(changed, 'w:gz') as out:
                    for item, data in original:
                        item = tarfile.TarInfo(item.name)
                        item.mode = 0o755 if item.name.endswith('/bin/dtw') else 0o644
                        item.size = len(data)
                        if item.name.endswith('/bin/dtw'):
                            if mutation == 'symlink':
                                item.type = tarfile.SYMTYPE; item.linkname = '/outside'
                                item.size = 0
                            elif mutation == 'traversal':
                                item.name = package.name + '/../outside'
                            elif mutation == 'mode':
                                item.mode = 0o644
                            elif mutation == 'content':
                                data = b'changed'; item.size = len(data)
                        out.addfile(item, io.BytesIO(data))
                        if mutation == 'duplicate' and item.name.endswith('/bin/dtw'):
                            out.addfile(item, io.BytesIO(data))
                with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                    builder.verify_archive(changed, package.name)

    def test_final_package_check_rejects_sidecar_before_running_any_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package, archive = self.make_package(root, LINUX[0])
            candidate = {'version': '0.5.3', 'target': LINUX[0], 'archive': archive.name,
                         'archive_sha256': builder.digest(archive)}
            (root / 'CANDIDATE.json').write_text(json.dumps(candidate))
            (root / (archive.name + '.sha256')).write_text('incorrect sidecar')
            with patch.object(checker.builder, 'native_host'), patch.object(checker.subprocess, 'run') as run:
                with self.assertRaisesRegex(ValueError, 'sidecar'):
                    checker.check(root, root / 'checks')
                run.assert_not_called()

    def test_imported_recipe_is_bound_to_archive_inventory_binary_and_execute_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package, _ = self.make_package(root, LINUX[0])
            identity = builder.digest(package / 'SHA256SUMS')
            store = root / 'store'
            payload = store / 'packages' / identity / 'payload'
            shutil.copytree(package, payload)
            binary = payload / 'bin/dtw'; binary.chmod(0o755)
            candidate = {'target': LINUX[0], 'version': '0.5.3', 'binary_sha256': builder.digest(binary)}
            recipe = {'target': LINUX[0], 'version': '0.5.3', 'package_id': identity,
                      'package_path': str(payload), 'executable': str(binary), 'args': ['mcp']}
            self.assertEqual(checker.imported_binary(recipe, package, store, candidate), binary)
            with self.assertRaisesRegex(ValueError, 'recipe'):
                checker.imported_binary(dict(recipe, executable=str(package / 'bin/dtw')), package, store, candidate)
            binary.write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'binary hash'):
                checker.imported_binary(recipe, package, store, candidate)

    def test_check_binary_runs_headless_mcp_and_worker_controls(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / 'dtw'
            binary.write_bytes(elf(62))
            binary.chmod(0o755)
            output = root / 'out'
            with patch.object(checker.subprocess, 'check_output', return_value='dtw 0.6.1\n'), \
                    patch.object(checker.subprocess, 'run') as run_mcp, \
                    patch.object(checker, 'worker_controls', return_value=[{'mode': 'eof', 'exit_code': 0}]):
                report = checker.check_binary(binary, '0.6.1', output)
            self.assertEqual(report['version'], '0.6.1')
            self.assertEqual(report['executable'], str(binary.resolve()))
            self.assertEqual(report['worker_controls'], [{'mode': 'eof', 'exit_code': 0}])
            run_mcp.assert_called_once()
            cmd = run_mcp.call_args.args[0]
            self.assertTrue(cmd[1].endswith('tests/ci/run_packaged_mcp.py'))
            self.assertEqual(json.loads((output / 'report.json').read_text())['binary_sha256'], report['binary_sha256'])

    def test_promotion_stages_original_linux_gzip_bytes_from_authenticated_actions_zip(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for target in LINUX:
                work = root / target; work.mkdir()
                package, archive = self.make_package(work, target)
                binary = (package / 'bin/dtw').read_bytes()
                candidate = {'version': '0.5.3', 'target': target, 'source': {'commit': 'a' * 40},
                             'archive': archive.name, 'archive_sha256': builder.digest(archive),
                             'build_manifest_sha256': builder.digest(package / 'BUILD.json'),
                             'binary_sha256': promoter.digest(binary)}
                payload = io.BytesIO()
                with zipfile.ZipFile(payload, 'w') as outer:
                    outer.writestr('CANDIDATE.json', json.dumps(candidate))
                    outer.writestr(archive.name, archive.read_bytes())
                    outer.writestr(archive.name + '.sha256', (work / (archive.name + '.sha256')).read_bytes())
                data = payload.getvalue()
                output = work / 'staged'; output.mkdir()
                staged, native = promoter.stage_candidate(data, 'sha256:' + promoter.digest(data),
                                                          'a' * 40, 'v0.5.3', target, output)
                self.assertEqual(staged, candidate)
                self.assertIsNone(native)
                self.assertEqual((output / archive.name).read_bytes(), archive.read_bytes())
                self.assertEqual((output / f'BUILD-{target}.json').read_bytes(), (package / 'BUILD.json').read_bytes())


if __name__ == '__main__':
    unittest.main()
