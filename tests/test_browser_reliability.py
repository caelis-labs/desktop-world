"""Portable browser fixture HTTP and Windows probe layout regression controls."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


_spec = importlib.util.spec_from_file_location(
    'dtw_browser_reliability_http_controls',
    Path(__file__).parent / 'fixtures/browser-workbench/test_server.py')
_controls = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_controls)
WorkbenchHttpTests = _controls.WorkbenchHttpTests


_ENTRY_PREFLIGHT = r'''
import ctypes, importlib, json, subprocess, sys
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0, sys.argv[1])
expected = Path(sys.argv[2]).resolve()
checked = []
class BeforeDesktop(Exception):
    pass
with patch.object(subprocess, 'Popen', side_effect=AssertionError('No subprocess allowed')), \
     patch.object(subprocess, 'run', side_effect=AssertionError('No subprocess allowed')), \
     patch.object(ctypes, 'WinDLL', side_effect=AssertionError('No desktop API allowed'), create=True):
    for name in ('browser_reliability_probe', 'browser_mouse_probe', 'browser_delivery_probe'):
        probe = importlib.import_module(name)
        resolve = probe.browser_workbench
        def preflight(*args):
            assert resolve(*args) == expected
            checked.append(name)
            raise BeforeDesktop()
        with patch.object(probe, 'browser_workbench', preflight), \
             patch.object(sys, 'argv', [name, '--dtw', 'missing.exe', '--chrome', 'missing-chrome.exe', '--output', 'unused']):
            try:
                probe.main()
            except BeforeDesktop:
                pass
            else:
                raise AssertionError('Probe did not perform fixture preflight first')
print(json.dumps(checked))
'''


class WindowsBrowserProbeLayoutTests(unittest.TestCase):
    def preflight(self, scripts, fixture, cwd):
        result = subprocess.run([sys.executable, '-X', 'utf8', '-c', _ENTRY_PREFLIGHT,
                                 str(scripts), str(fixture)], cwd=cwd,
                                capture_output=True, text=True, encoding='utf-8', timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout),
                         ['browser_reliability_probe', 'browser_mouse_probe', 'browser_delivery_probe'])
        self.assertFalse((cwd / 'unused').exists())

    def test_repository_entry_points_resolve_before_desktop_from_another_cwd(self):
        repo = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as directory:
            self.preflight(repo / 'tests/acceptance/windows', repo / 'tests/fixtures/browser-workbench',
                           Path(directory))

    def test_actual_packaged_assets_resolve_from_a_unicode_directory_and_other_cwd(self):
        repo = Path(__file__).resolve().parents[1]
        spec = importlib.util.spec_from_file_location('dtw_probe_layout_candidate_builder',
                                                      repo / 'scripts/build-rewrite-candidate.py')
        candidate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(candidate)
        with tempfile.TemporaryDirectory() as directory:
            package = Path(directory) / 'candidate package 中文'
            package.mkdir()
            version = (repo / 'version.txt').read_text(encoding='utf-8').strip()
            candidate.package_assets(repo, package, version, 'x86_64-pc-windows-msvc')
            unrelated = Path(directory) / 'other cwd'
            unrelated.mkdir()
            self.assertFalse((package / 'fixtures/browser-workbench').exists())
            self.preflight(package / 'acceptance/windows', package / 'acceptance/browser-workbench', unrelated)

    def test_partial_fixture_reports_missing_files_in_both_supported_locations(self):
        spec = importlib.util.spec_from_file_location('dtw_probe_layout_paths',
            Path(__file__).parent / 'acceptance/windows/probe_paths.py')
        paths = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(paths)
        with tempfile.TemporaryDirectory() as directory:
            package = Path(directory)
            probe = package / 'acceptance/windows/browser_reliability_probe.py'
            probe.parent.mkdir(parents=True)
            probe.touch()
            fixture = package / 'acceptance/browser-workbench'
            fixture.mkdir()
            for name in ('server.py', 'index.html'):
                (fixture / name).touch()
            self.assertEqual(paths.browser_workbench(probe), fixture)
            with self.assertRaises(FileNotFoundError) as failure:
                paths.browser_workbench(probe, ('server.py', 'index.html', 'reliability.html'))
            self.assertIn(str(fixture), str(failure.exception))
            self.assertIn(str(package / 'fixtures/browser-workbench'), str(failure.exception))
            self.assertIn('missing reliability.html', str(failure.exception))
