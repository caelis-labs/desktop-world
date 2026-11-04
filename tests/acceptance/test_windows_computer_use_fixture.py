"""No-GUI lifecycle controls for the Windows shell of the shared workbench."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('windows_shared_fixture',
    Path(__file__).resolve().parents[1] / 'fixtures/computer-use-app/fixture.py')
fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixture)


class SharedWindowsFixtureTests(unittest.TestCase):
    def test_windows_native_identity_seed_and_reused_source_are_preserved(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(fixture.platform, 'system', return_value='Windows'):
            root = Path(directory)
            binary = root / 'owned-compiled.exe'
            binary.write_bytes(b'owned-fixture-compile-control')
            owned = fixture.ComputerUseFixture(root / 'case', 'windows-unit')
            owned.prepare(build=False)
            try:
                owned.build(compiled_native=binary)
                self.assertEqual(owned.bundle, owned.output / 'ComputerUse.exe')
                self.assertEqual(owned.executable.read_bytes(), binary.read_bytes())
                text = (owned.output / 'native-reference.txt').read_bytes().decode('utf-8')
                self.assertGreater(len(text.encode('utf-16-le')) // 2, 20000)
                self.assertTrue(text.endswith('END-OF-NATIVE-REFERENCE-137🚀'))
                self.assertTrue(any(name.endswith('windows/Fixture.cs') for name in fixture.source_identity()))
            finally:
                self.assertTrue(owned.close()['passed'])

    def test_existing_output_is_rejected_before_any_new_server(self):
        with tempfile.TemporaryDirectory() as directory:
            owned = fixture.ComputerUseFixture(Path(directory), 'fresh-required')
            with self.assertRaises(FileExistsError):
                owned.prepare(build=False)
            self.assertEqual(owned.servers, [])


if __name__ == '__main__':
    unittest.main()
