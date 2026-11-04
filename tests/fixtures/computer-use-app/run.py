#!/usr/bin/env python3
"""Run the owned Computer Use app, or only its portable browser workspace."""
import argparse
import json
from pathlib import Path
import sys
import time

from fixture import ComputerUseFixture, write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True, help='New private case directory')
    parser.add_argument('--case', default='manual')
    parser.add_argument('--web-only', action='store_true', help='Portable loopback pages; no native build or launch')
    parser.add_argument('--foreground-preview', action='store_true', help='Explicitly activate the owned app for manual inspection')
    args = parser.parse_args()
    if not args.web_only and sys.platform not in ('darwin', 'win32'):
        parser.error('The native shell requires macOS or Windows; use --web-only for a browser workspace')
    if sys.platform == 'win32':
        sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'acceptance'))
    fixture = ComputerUseFixture(args.output, args.case)
    cleanup = None
    try:
        fixture.prepare(build=not args.web_only, activate=args.foreground_preview)
        if not args.web_only:
            fixture.start()
        print(json.dumps({'case': args.case, 'output': str(fixture.output),
            'app': str(fixture.bundle) if not args.web_only else None,
            'web_url': fixture.web_url, 'workbench_url': fixture.workbench_url,
            'stop_file': str(fixture.output / 'stop')}), flush=True)
        while not (fixture.output / 'stop').exists():
            if fixture.child is not None and fixture.child.poll() is not None:
                break
            time.sleep(0.1)
    except KeyboardInterrupt:
        pass
    finally:
        cleanup = fixture.close()
        if fixture.output.exists():
            write_json(fixture.output / 'cleanup.json', cleanup)
    if not cleanup['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
