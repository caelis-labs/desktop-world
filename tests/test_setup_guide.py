"""The published guide must fetch the exact reviewed bootstrap, never a moving ref."""
import hashlib
from pathlib import Path
import unittest


class SetupGuideTests(unittest.TestCase):
    def test_source_blob_and_transport_checksum_match_each_launcher(self):
        root = Path(__file__).resolve().parents[1]
        guide = (root / 'install.md').read_text(encoding='utf-8')
        for script in ('bootstrap.sh', 'bootstrap.ps1'):
            with self.subTest(script=script):
                data = (root / 'install' / script).read_bytes()
                blob = hashlib.sha1(f'blob {len(data)}\0'.encode() + data).hexdigest()
                self.assertIn('https://api.github.com/repos/caelis-labs/desktop-world/git/blobs/' + blob, guide)
                self.assertIn(hashlib.sha256(data).hexdigest(), guide)
