#!/usr/bin/env python3
"""Owned AppKit feasibility experiment, separate from the DTW product route.

Posts only to exact owned PIDs/windows; no HID post, cursor warp, activation,
replay, or DTW protocol change. Run inside cursor_audit.py for independent proof.
"""
import argparse
import json
from pathlib import Path
import plistlib
import shutil
import subprocess
import time
import uuid

from standard_agent_probe import require, wait_until, write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--peer', action='store_true', help='Keep an owned peer app in front of the target')
    parser.add_argument('--transport', choices=['public', 'skylight'], default='public')
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    out.chmod(0o700)
    fixtures = Path(__file__).resolve().parents[1] / 'fixtures'
    native = out / 'AXCompat'
    subprocess.run(['xcrun', 'clang', '-fobjc-arc', str(fixtures / 'ax-compat/main.m'),
                    '-framework', 'AppKit', '-framework', 'ApplicationServices', '-o', str(native)], check=True)
    directed = out / 'directed-pointer'
    subprocess.run(['xcrun', 'swiftc', '-module-cache-path', str(out / 'swift-cache'),
                    str(fixtures / 'directed-pointer/main.swift'), '-o', str(directed)], check=True)
    owned = []

    def state(root):
        try:
            return json.loads((root / 'state.json').read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            return None

    def start(name, profile):
        root = out / name
        bundle = root / 'Owned.app'
        exe = bundle / 'Contents/MacOS/Owned'
        exe.parent.mkdir(parents=True)
        shutil.copy2(native, exe)
        (root / 'profile').write_text(profile)
        with (bundle / 'Contents/Info.plist').open('wb') as stream:
            plistlib.dump({'CFBundleExecutable': 'Owned', 'CFBundleIdentifier': 'dev.caelis.dtw.directed.' + uuid.uuid4().hex,
                           'CFBundleName': name, 'CFBundlePackageType': 'APPL'}, stream)
        log = (root / 'fixture.log').open('w')
        child = subprocess.Popen([str(exe)], stdout=log, stderr=subprocess.STDOUT)
        owned.append((root, child, log))
        wait_until(lambda: state(root) and state(root)['pid'] == child.pid)
        return root

    report = {'scope': 'experimental direct PID delivery on owned AppKit; not DTW support', 'cases': []}
    try:
        target = start('target', 'opaque')
        peer = start('peer', 'occluder') if args.peer else None
        if peer:
            wait_until(lambda: state(peer)['frontmost_pid'] == state(peer)['pid'])
        expected_frontmost = state(peer or target)['pid']
        for operation in ['move', 'click', 'drag']:
            before = state(target)
            a, b = before['points']['pad-start'], before['points']['pad-end']
            if operation == 'click':
                a = before['points']['button']
            arguments = [str(directed), operation, str(before['pid']), str(before['window']),
                         *map(str, before['bounds'][:2]), *map(str, a), *map(str, b), '1', args.transport]
            started = time.time() * 1000
            subprocess.run(arguments, check=True, timeout=5)
            ended = time.time() * 1000
            after = state(target)
            gestures = after['gestures'][len(before['gestures']):]
            effect = {'move': 'move' in gestures, 'click': after['clicks'] == before['clicks'] + 1,
                      'drag': all(kind in gestures for kind in ['down', 'drag', 'up'])}[operation]
            row = {'operation': operation, 'transport': args.transport, 'start_unix_ms': started, 'end_unix_ms': ended,
                   'effect_proven': effect, 'target_active': after['active'],
                   'frontmost_pid': after['frontmost_pid'], 'expected_frontmost_pid': expected_frontmost,
                   'gestures': gestures, 'pressed_mouse_buttons': after['pressed_mouse_buttons'],
                   'peer_clicks': state(peer)['clicks'] if peer else 0,
                   'peer_gestures': state(peer)['gestures'] if peer else []}
            report['cases'].append(row)
            write_json(out / 'directed-actions.json', report)
            require(not row['peer_clicks'] and not row['peer_gestures'] and not after['pressed_mouse_buttons'], row)
    finally:
        for root, child, log in owned:
            (root / 'quit').touch()
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.terminate()
                child.wait(timeout=3)
            log.close()
        write_json(out / 'directed-actions.json', report)
    print(json.dumps({'effects_proven': sum(row['effect_proven'] for row in report['cases']),
                      'report': str(out / 'directed-actions.json')}))
    require(all(row['effect_proven'] and row['frontmost_pid'] == row['expected_frontmost_pid']
                for row in report['cases']), report)


if __name__ == '__main__':
    main()
