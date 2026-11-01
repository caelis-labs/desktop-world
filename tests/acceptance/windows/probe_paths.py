"""Resolve owned probe assets from a source checkout or extracted Windows ZIP."""
from pathlib import Path


def browser_workbench(probe_file, required=('server.py', 'index.html')):
    probe = Path(probe_file).resolve()
    candidates = (
        probe.parents[2] / 'fixtures/browser-workbench',  # tests/acceptance/windows
        probe.parents[1] / 'browser-workbench',  # acceptance/windows in a ZIP
    )
    missing = []
    for directory in candidates:
        absent = [name for name in required if not (directory / name).is_file()]
        if not absent:
            return directory
        missing.append(f'{directory} (missing {", ".join(absent)})')
    raise FileNotFoundError('Browser workbench fixture not found: ' + '; '.join(missing))
