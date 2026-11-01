"""Discover the fixture's non-GUI HTTP regression controls in the portable suite."""
import importlib.util
from pathlib import Path


_spec = importlib.util.spec_from_file_location(
    'dtw_browser_reliability_http_controls',
    Path(__file__).parent / 'fixtures/browser-workbench/test_server.py')
_controls = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_controls)
WorkbenchHttpTests = _controls.WorkbenchHttpTests
