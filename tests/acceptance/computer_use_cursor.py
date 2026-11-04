#!/usr/bin/env python3
"""Read-only cursor proof after current AppClient pointer input.

Compile tests/fixtures/cursor-overlay-observer.swift once, then immediately call:
    proof = observe_cursor(client, observer_binary, 'drag-end', (screen_x, screen_y))
    assert proof['passed'], proof
No MCP calls, input, focus changes or helper control commands are issued here.
Exact-window capture proves rendered marker content, not visibility through occluders.
"""
import hashlib
import json
import math
import os
from pathlib import Path
import socket
import stat
import struct
import subprocess
import time
import uuid


def owned_helpers(parent_pid, binary):
    expected = str(Path(binary).resolve()) + ' --dtw-overlay-worker'
    rows = subprocess.check_output(['ps', '-axo', 'pid=,ppid=,command='], text=True)
    result = set()
    for row in rows.splitlines():
        fields = row.strip().split(None, 2)
        if len(fields) == 3 and int(fields[1]) == parent_pid and fields[2] == expected:
            result.add(int(fields[0]))
    return result


def discover_identity(parent_pid, binary):
    """Challenge only the exact executable child owned by this current MCP."""
    children = owned_helpers(parent_pid, binary)
    if len(children) > 1:
        raise AssertionError('One MCP owns multiple cursor helpers')
    root = Path('/private/tmp') / ('dtw-overlays-' + str(os.getuid()))
    if not children or not root.exists():
        return None
    pid = next(iter(children))
    for directory in root.iterdir():
        try:
            ds = directory.lstat()
            endpoint = directory / 'identity'
            ss = endpoint.lstat()
            if (not stat.S_ISDIR(ds.st_mode) or ds.st_uid != os.getuid()
                    or stat.S_IMODE(ds.st_mode) != 0o700 or not stat.S_ISSOCK(ss.st_mode)
                    or ss.st_uid != os.getuid() or stat.S_IMODE(ss.st_mode) != 0o600):
                continue
            nonce = uuid.uuid4().hex
            with socket.socket(socket.AF_UNIX) as peer:
                peer.settimeout(.2)
                peer.connect(str(endpoint))
                # Darwin SOL_LOCAL / LOCAL_PEERPID: socket reply alone is no proof.
                if struct.unpack('i', peer.getsockopt(0, 2, 4))[0] != pid:
                    continue
                peer.sendall((json.dumps({'nonce': nonce}) + '\n').encode())
                data = b''
                while b'\n' not in data and len(data) < 4096:
                    chunk = peer.recv(4096 - len(data))
                    if not chunk:
                        break
                    data += chunk
            reply = json.loads(data)
            identity = reply.get('identity', {})
            windows = identity.get('windows')
            session = reply.get('session')
            if (reply.get('nonce') != nonce or type(identity.get('pid')) is not int
                    or identity['pid'] != pid or type(identity.get('birth')) is not int
                    or identity['birth'] <= 0 or not isinstance(windows, list) or len(windows) != 1
                    or type(windows[0]) is not int or windows[0] <= 0
                    or not isinstance(session, str) or len(session) != 32
                    or any(c not in '0123456789abcdefABCDEF' for c in session)):
                continue
            if owned_helpers(parent_pid, binary) != children:
                continue
            return dict(identity, session=session, instance=directory.name)
        except (OSError, ValueError):
            continue
    return None


def follow_proof(snapshot, identity, expected_screen_point, tolerance=2):
    """Use actual WindowServer bounds and captured pixels, never private state."""
    if not math.isfinite(tolerance) or tolerance < 0:
        raise ValueError('Cursor tolerance must be finite and nonnegative')
    x, y = expected_screen_point
    if not all(math.isfinite(value) for value in (x, y)):
        raise ValueError('Expected screen point must be finite')
    capture = snapshot.get('capture', {})
    window_id = identity['windows'][0]

    def exact_window(state):
        if state.get('identity_matches') is not True:
            return None
        rows = [row for row in state.get('windows', [])
                if row.get('id') == window_id and row.get('on_screen') is True]
        return rows[0] if len(rows) == 1 else None

    before = exact_window(snapshot)
    after = exact_window(snapshot.get('after_capture', {}))
    bounds = before.get('bounds') if before else None
    stable = bounds is not None and after is not None and bounds == after.get('bounds')
    # OverlayCursorView's 14pt hotspot is relative to its top-left CG origin.
    hotspot = [bounds['x'] + 14, bounds['y'] + 14] if bounds else None
    distance = math.hypot(hotspot[0] - x, hotspot[1] - y) if hotspot else None
    positioned = stable and distance <= tolerance
    rendered = (stable and capture.get('exact_owned_window') == window_id
                and capture.get('status') == 'captured'
                and capture.get('colored_pixels', 0) >= 20
                and capture.get('opaque_pixels', 0) >= 20)
    return {'passed': positioned and rendered, 'position_verified': positioned,
            'rendering_verified': rendered, 'identity': identity,
            'expected_screen_point': [x, y], 'hotspot': hotspot, 'distance': distance,
            'tolerance': tolerance, 'window_bounds': bounds, 'snapshot': snapshot}


def observe_cursor(client, observer_binary, case, expected_screen_point, *, tolerance=2):
    """Call immediately AFTER a current AppClient click/move/drag/scroll returns.

    observer_binary is the parent's compiled cursor-overlay-observer.swift.
    Returns and saves a proof; the caller must require proof['passed'].
    """
    if not case or Path(case).name != case:
        raise ValueError('Cursor case must be a single filename component')
    child = client.transport.child
    binary = child.args[0]
    until = time.monotonic() + .75
    identity = None
    while identity is None:
        identity = discover_identity(child.pid, binary)
        if identity is not None or time.monotonic() >= until:
            break
        time.sleep(.02)
    root = Path(client.output)
    if identity is None:
        proof = {'passed': False, 'position_verified': False, 'rendering_verified': False,
                 'identity': None, 'reason': 'No authenticated owned cursor helper'}
    else:
        png = root / (case + '-cursor.png')
        result = subprocess.run([str(observer_binary), json.dumps(identity), str(png)],
                                text=True, capture_output=True, timeout=10, check=True)
        snapshot = json.loads(result.stdout)
        proof = follow_proof(snapshot, identity, expected_screen_point, tolerance)
        if proof['rendering_verified']:
            proof['capture_sha256'] = hashlib.sha256(png.read_bytes()).hexdigest()
    (root / (case + '-cursor.json')).write_text(json.dumps(proof, indent=2) + '\n')
    return proof
