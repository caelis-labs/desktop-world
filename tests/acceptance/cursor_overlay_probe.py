#!/usr/bin/env python3
"""Opt-in owned AppKit cursor overlay acceptance; no desktop-idle requirement.

Only real app input drives the marker. Identity sockets receive read-only nonce
challenges; the probe never writes synthetic presentation events. Rendering is
proved by exact helper-window pixels, separately from WindowServer visibility,
input effects and sampled desktop stability. Missing pixels are an explicit gap.
This is an MCP client test, not a live LLM-host test. Failed runs are retained.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import signal
import socket
import stat
import struct
import subprocess
import sys
import threading
import time
import traceback
import uuid

from standard_agent_probe import Fixture, Mcp, process_table, require, save_png, wait_until, write_json
from native_input_probe import check_cleanup, close_mcp, recorded_workers, retained_exec_response
from background_input_probe import command, execute, finish_background, front, fresh_end, journal, owned_input_pairs, reset, select
from background_lifecycle_probe import check_cancel_record


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def owned_helpers(table, parent, binary):
    expected = str(Path(binary).resolve()) + ' --dtw-overlay-worker'
    return {pid: cmd for pid, (ppid, cmd) in table.items() if ppid == parent and cmd == expected}


def validate_identity(reply, nonce, pid):
    require(isinstance(reply, dict) and reply.get('nonce') == nonce, 'Identity nonce mismatch')
    value = reply.get('identity', {})
    require(type(value.get('pid')) is int and value['pid'] == pid, 'Identity is not the exact helper child')
    require(type(value.get('birth')) is int and value['birth'] > 0, 'Missing kernel birth')
    windows = value.get('windows')
    require(isinstance(windows, list) and len(windows) == 1 and type(windows[0]) is int and windows[0] > 0,
            'Missing exact helper window')
    return {key: value[key] for key in ('pid', 'birth', 'windows')}


def logical_session(reply):
    value = reply.get('session')
    require(isinstance(value, str) and len(value) == 32 and all(c in '0123456789abcdefABCDEF' for c in value),
            'Missing stable logical session identity')
    return value


def require_helper_continuity(known, current, replacement_of=None):
    if replacement_of is not None:
        require(known == replacement_of, 'Replacement does not name the recorded original helper')
        require(current['session'] == known['session'] and current['pid'] != known['pid']
                and set(current['windows']).isdisjoint(known['windows']) and current['instance'] != known['instance'],
                'Replacement lost logical session or retained original process/window/instance')
    elif known is not None:
        require(current == known, 'Session helper identity changed across ordinary app operations')


def visible(snapshot):
    return snapshot.get('identity_matches') is True and any(w.get('on_screen') is True for w in snapshot.get('windows', []))


def rendering_proof(snapshot):
    capture = snapshot.get('capture', {})
    window = capture.get('exact_owned_window')
    before_ids = {w['id'] for w in snapshot.get('windows', []) if w.get('on_screen')}
    after_ids = {w['id'] for w in snapshot.get('after_capture', {}).get('windows', []) if w.get('on_screen')}
    return (type(window) is int and window > 0 and window in before_ids and window in after_ids
            and visible(snapshot) and visible(snapshot.get('after_capture', {}))
            and capture.get('status') == 'captured' and capture.get('colored_pixels', 0) >= 20
            and capture.get('opaque_pixels', 0) >= 20)


def require_route(record, operation, route):
    receipts = [step['receipt'] for step in record.get('script', {}).get('steps', [])
                if step['operation'] == operation and step.get('receipt')]
    require(len(receipts) == 1, 'Missing unique input receipt')
    outcome = receipts[0].get('provider_outcome') or receipts[0]['outcome']
    require(outcome.get('delivery', {}).get('route') == route, outcome)
    if route == 'background':
        require(outcome['delivery']['foreground_borrowed'] is False, 'Background receipt borrowed foreground')
    return outcome['delivery']


def require_strict_focus_rejection(outcome):
    delivery = outcome.get('delivery', {})
    require(outcome.get('dispatch') == 'not_dispatched'
            and outcome.get('error', {}).get('code') == 'foreground_required'
            and delivery.get('route') is None and delivery.get('reason') == 'explicit_focus'
            and delivery.get('foreground_borrowed') is False, outcome)


def derived_fixture(base, controls):
    marker = 'switch next.trimmingCharacters(in:.whitespacesAndNewlines) {'
    state_marker = 'try? JSONSerialization.data(withJSONObject:value,options:[.sortedKeys]).write(to:output.appendingPathComponent("state.json"),options:.atomic)'
    require(base.count(marker) == 1 and base.count(state_marker) == 1,
            'Owned fixture command/state hook changed; inspect before adapting')
    source = base.replace(marker, 'cursorOverlayProbeCommand(next.trimmingCharacters(in:.whitespacesAndNewlines))\n                ' + marker)
    source = source.replace(state_marker, 'value["overlay_probe_pad_bounds"] = cursorOverlayProbePadBounds()\n        ' + state_marker)
    return source + '\n' + controls


class Observer:
    def __init__(self, binary, root):
        self.binary, self.root = binary, root
        self.gaps = []

    def snapshot(self, identity, case=None):
        args = [str(self.binary), json.dumps(identity)]
        if case:
            args.append(str(self.root / (case + '.png')))
        result = subprocess.run(args, text=True, capture_output=True, timeout=10, check=True)
        value = json.loads(result.stdout)
        require('error' not in value, value)
        if case:
            value['rendering_verified'] = rendering_proof(value)
            if value['rendering_verified']:
                value['capture']['sha256'] = sha(args[-1])
            else:
                self.gaps.append({'case': case, 'kind': 'owned_marker_pixels_unverified', 'capture': value.get('capture')})
        return value

    def hidden(self, identity):
        def sample():
            state = self.snapshot(identity)
            return state if not visible(state) else None
        return wait_until(sample, timeout=3)


def marker_roi(baseline, current, marker_snapshots):
    before, after = baseline['fixture'], current['fixture']
    require(before['pid'] == after['pid'] and before['windows'][0]['id'] == after['windows'][0]['id'],
            'Capture comparison changed owned target identity')
    require(before['window_bounds'] == after['window_bounds']
            and before['overlay_probe_pad_bounds'] == after['overlay_probe_pad_bounds'], 'Capture geometry changed')
    a, b = baseline['png'], current['png']
    require([a['width'], a['height']] == [b['width'], b['height']], 'Capture dimensions changed')
    require(len(marker_snapshots) == 2 and all(visible(row) and len(row['windows']) == 1 for row in marker_snapshots),
            'Pixel isolation requires two exact on-screen markers')
    bounds = [row['windows'][0]['bounds'] for row in marker_snapshots]
    left, top = min(r['x'] for r in bounds)-2, min(r['y'] for r in bounds)-2
    right = max(r['x']+r['width'] for r in bounds)+2
    bottom = max(r['y']+r['height'] for r in bounds)+2
    px, py, pw, ph = before['overlay_probe_pad_bounds']
    require(px <= left < right <= px+pw and py <= top < bottom <= py+ph,
            'Marker ROI escaped the static owned ClickPad')
    wx, wy, ww, wh = before['window_bounds']; sx, sy = a['width']/ww, a['height']/wh
    x, y = math.floor((left-wx)*sx), math.floor((top-wy)*sy)
    end_x, end_y = math.ceil((right-wx)*sx), math.ceil((bottom-wy)*sy)
    require(0 <= x < end_x <= a['width'] and 0 <= y < end_y <= a['height'], 'ROI outside captured image')
    return [x, y, end_x-x, end_y-y]


def require_equal_roi(comparison):
    require(comparison.get('pixels', 0) > 0 and comparison.get('baseline_opaque_pixels', 0) >= 20
            and comparison.get('different_pixels') == 0 and comparison.get('different_channels') == 0
            and comparison.get('maximum_channel_delta') == 0
            and isinstance(comparison.get('before_rgba_sha256'), str)
            and len(comparison['before_rgba_sha256']) == 64
            and comparison['before_rgba_sha256'] == comparison.get('after_rgba_sha256'),
            {'capture_roi_contaminated_or_unproven': comparison})


def capture(mcp, fixture, case, proof):
    shot, response = execute(mcp, fixture, case, select() +
        'state.window=window_ref;state.shot=await app.capture({window_ref});return state.shot;', proof)
    directory = mcp.output/case; directory.mkdir()
    proof['png'] = save_png(directory, response, shot)
    return shot


def capture_isolation(mcp, target, observer, identities, baseline, evidence):
    proof = evidence.setdefault('app_capture_excludes_own_and_peer_markers', {})
    proof['baseline'] = baseline
    proof['markers_before'] = [observer.snapshot(identity) for identity in identities]
    require(all(visible(row) for row in proof['markers_before']), 'Capture isolation began without both markers')
    current = proof.setdefault('with_markers', {})
    capture(mcp, target, 'capture-with-own-and-peer-markers', current)
    proof['markers_after'] = [observer.snapshot(identity) for identity in identities]
    for before, after in zip(proof['markers_before'], proof['markers_after']):
        require(visible(after) and before['windows'] == after['windows'], 'Marker changed or hid across target capture')
    roi = marker_roi(baseline, current, proof['markers_before']); proof['roi'] = roi
    result = subprocess.run([str(observer.binary), '--compare-roi', baseline['png']['path'], current['png']['path'],
                             *map(str, roi)], text=True, capture_output=True, timeout=10, check=True)
    proof['comparison'] = json.loads(result.stdout)
    require_equal_roi(proof['comparison'])
    proof['scope'] = 'Actual app.capture PNG RGBA equality in the static owned ClickPad under both marker window bounds; changing title/body text excluded.'


def helper(mcp, binary, observer, *, replacement_of=None):
    """Discover only this MCP's exact executable child and challenge its socket."""
    root = Path('/private/tmp') / ('dtw-overlays-' + str(os.getuid()))
    def discover():
        children = owned_helpers(process_table(), mcp.child.pid, binary)
        require(len(children) <= 1, 'One session spawned multiple overlay helpers')
        if not children or not root.exists():
            return None
        pid, executable = next(iter(children.items()))
        for directory in root.iterdir():
            try:
                ds = directory.lstat(); endpoint = directory / 'identity'; ss = endpoint.lstat()
                if (not stat.S_ISDIR(ds.st_mode) or ds.st_uid != os.getuid() or stat.S_IMODE(ds.st_mode) != 0o700
                        or not stat.S_ISSOCK(ss.st_mode) or ss.st_uid != os.getuid() or stat.S_IMODE(ss.st_mode) != 0o600):
                    continue
                nonce = uuid.uuid4().hex
                with socket.socket(socket.AF_UNIX) as client:
                    client.settimeout(.2); client.connect(str(endpoint))
                    # macOS SOL_LOCAL / LOCAL_PEERPID. Reject another session before its reply.
                    peer = struct.unpack('i', client.getsockopt(0, 2, 4))[0]
                    if peer != pid:
                        continue
                    client.sendall((json.dumps({'nonce': nonce}) + '\n').encode())
                    data = b''
                    while b'\n' not in data and len(data) < 4096:
                        chunk = client.recv(4096-len(data))
                        if not chunk: break
                        data += chunk
                reply = json.loads(data)
                identity = validate_identity(reply, nonce, pid)
                session = logical_session(reply)
                require(observer.snapshot(identity)['identity_matches'], 'Helper kernel identity changed')
                require(owned_helpers(process_table(), mcp.child.pid, binary).get(pid) == executable,
                        'Helper exited during discovery')
                value = dict(identity, session=session, instance=directory.name, command=executable)
                known = getattr(mcp, 'probe_overlay_identity', None)
                require_helper_continuity(known, value, replacement_of)
                mcp.probe_overlay_identity = value
                return value
            except (OSError, ValueError):
                continue
        return None
    return wait_until(discover, timeout=3)


class ChildWatch:
    """Bounded sampled no-spawn evidence; never records unrelated processes."""
    def __init__(self, mcp, binary):
        self.stop = threading.Event(); self.samples = []; self.error = None
        def sample():
            try:
                while not self.stop.is_set():
                    self.samples.append({'at': time.monotonic(), 'helpers': list(owned_helpers(process_table(), mcp.child.pid, binary))})
                    self.stop.wait(.02)
            except BaseException as error: self.error = repr(error)
        self.thread = threading.Thread(target=sample, daemon=True); self.thread.start()

    def finish(self):
        self.stop.set(); self.thread.join(timeout=2)
        require(not self.thread.is_alive() and self.error is None, 'Helper observer failed')
        gaps = [b['at']-a['at'] for a, b in zip(self.samples, self.samples[1:])]
        return {'samples': self.samples, 'maximum_gap_seconds': max(gaps, default=0),
                'coverage': 'established' if len(self.samples) >= 2 and max(gaps, default=0) <= .2 else 'unavailable',
                'helper_seen': any(row['helpers'] for row in self.samples)}


def open_client(binary, output, clients, policy='auto'):
    output.mkdir()
    mcp = Mcp(binary, output, mcp_args=('--input-policy', policy)); clients.append(mcp)
    mcp.rpc('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                          'clientInfo': {'name': 'cursor-overlay-probe', 'version': '1'}})
    mcp.notify('notifications/initialized')
    return mcp


def click(mcp, fixture, case, proof, *, foreground=False):
    shot = capture(mcp, fixture, case+'-capture', proof.setdefault('capture', {}))
    before = fixture.state(); bounds = before['window_bounds']; point = before['pad_screen_point']
    x = (point[0]-bounds[0])/bounds[2]*shot['image']['width']
    y = (point[1]-bounds[1])/bounds[3]*shot['image']['height']
    require(0 < x < shot['image']['width'] and 0 < y < shot['image']['height'], 'Owned pad outside capture')
    suffix = ",button:'right',count:2" if foreground else ''
    action = proof.setdefault('action', {})
    execute(mcp, fixture, case, f'return await app.click({{window_ref:state.window,observation_ref:state.shot.observation_ref,x:{x},y:{y}{suffix}}});', action)
    key, count = ('right_up', 2) if foreground else ('left_up', 1)
    proof['after'] = wait_until(lambda: (state if (state := fixture.state())['pad'][key] == before['pad'][key]+count else None))
    proof['delivery'] = require_route(action['record'], 'click', 'foreground' if foreground else 'background')
    proof['pairs'] = owned_input_pairs(fixture, before, {})
    button = 'right' if foreground else 'left'
    require(proof['pairs']['down'].get(button) == count and proof['pairs']['up'].get(button) == count,
            'Owned business journal does not contain exactly the expected clicks')
    if foreground:
        require([e['click_count'] for e in proof['pairs']['events'] if e['kind'] == 'right_down'] == [1, 2],
                'Native fallback did not retain double-click semantics')


def background(clients, fixtures, binary, observer, evidence):
    a, b = clients[:2]; target, peer, foreground = fixtures
    front(foreground)
    proof = evidence.setdefault('background_and_sessions', {})
    table = process_table()
    proof['baseline_helper_pids'] = [list(owned_helpers(table, client.child.pid, binary)) for client in clients]
    require(not any(proof['baseline_helper_pids']), 'Baseline capture must precede any owned marker helper')
    start = fresh_end(foreground)
    click(a, target, 'background-target', proof.setdefault('first_app', {}))
    first = helper(a, binary, observer); proof['first_identity'] = first
    proof['first_pixels'] = observer.snapshot(first, 'first-app-marker')
    finish_background(foreground, start, [target], proof['first_app'])
    scroll = proof.setdefault('second_app_scroll', {}); start = fresh_end(foreground); before = peer.state()
    execute(a, peer, 'background-peer-scroll', select() + 'return await app.scroll({window_ref,delta_y:6});', scroll)
    scroll['after'] = wait_until(lambda: (state if (state := peer.state())['scroll_y'] > before['scroll_y'] else None))
    scroll['delivery'] = require_route(scroll['record'], 'scroll', 'background')
    scroll['journal'] = [row for row in journal(peer, 'events.jsonl') if row['sequence'] > before['events_count'] and row['kind'] == 'scroll']
    require(bool(scroll['journal']), 'Scroll business position changed without owned wheel journal')
    require(helper(a, binary, observer) == first, 'Switching apps replaced the session helper')
    scroll['pixels'] = observer.snapshot(first, 'second-app-marker')
    finish_background(foreground, start, [peer], scroll)
    click(a, target, 'returned-app-click', proof.setdefault('returned_app', {}))
    require(helper(a, binary, observer) == first, 'Returning to app replaced helper')
    click(b, target, 'second-session-click', proof.setdefault('second_session', {}))
    second = helper(b, binary, observer)
    require(second['pid'] != first['pid'] and set(second['windows']).isdisjoint(first['windows']), 'Sessions share overlay helper/window')
    proof['second_identity'] = second
    proof['both_visible'] = [observer.snapshot(first, 'session-a-marker'), observer.snapshot(second, 'session-b-marker')]
    require(all(visible(row) for row in proof['both_visible']), 'Independent session markers are not simultaneously visible')
    capture_isolation(a, target, observer, [first, second], proof['first_app']['capture'], evidence)


def strict(binary, output, clients, target, observer, evidence):
    mcp = open_client(binary, output/'mcp-strict', clients, 'background-only')
    proof = evidence.setdefault('semantic_and_rejected_no_new_overlay', {})
    watcher = ChildWatch(mcp, binary)
    try:
        replacement = 'DTW overlay semantic ' + uuid.uuid4().hex[:8]
        execute(mcp, target, 'semantic-set-and-press', select() + f"""
let p=await app.observe({{window_ref,role:'text_field',text:'Fixture Text',limit:10}});
const fields=p.elements.filter(e=>e.actions.includes('setValue'));
if(fields.length!==1) throw Error('No unique owned text field');
await app.setValue({{window_ref,observation_ref:p.observation_ref,element_ref:fields[0].element_ref,value:{json.dumps(replacement)}}});
p=await app.observe({{window_ref,role:'button',text:'Save',limit:10}});
const buttons=p.elements.filter(e=>e.name==='Save'&&e.can_press);
if(buttons.length!==1) throw Error('No unique owned Save');
return await app.press({{window_ref,observation_ref:p.observation_ref,element_ref:buttons[0].element_ref}});
""", proof.setdefault('semantic', {}))
        proof['semantic_effect'] = wait_until(lambda: (state if (state := target.state())['saved'] == replacement else None))
        require_route(proof['semantic']['record'], 'setValue', 'semantic')
        require_route(proof['semantic']['record'], 'press', 'semantic')
        rejected, _ = mcp.execute(target.app_id, 'strict-focus', select()+'return await app.focus({window_ref});')
        proof['rejected'] = rejected
        require(rejected.get('error'), 'Strict explicit focus unexpectedly completed')
        step = [s for s in rejected['script']['steps'] if s['operation'] == 'focus'][-1]
        outcome = step['receipt']['outcome']
        require_strict_focus_rejection(outcome)
    finally:
        proof['helper_observation'] = watcher.finish()
    require(not proof['helper_observation']['helper_seen'] and not owned_helpers(process_table(), mcp.child.pid, binary),
            'Semantic or rejected action spawned an overlay helper')
    if proof['helper_observation']['coverage'] != 'established':
        observer.gaps.append({'case': 'semantic-strict', 'kind': 'no_spawn_sampling_unavailable'})


def exclusions(clients, fixtures, binary, observer, evidence):
    a, b = clients[:2]; target, _, foreground = fixtures; front(foreground)
    proof = evidence.setdefault('own_and_peer_exact_exclusion', {})
    isolation_needed = 'app_capture_excludes_own_and_peer_markers' not in evidence
    if isolation_needed:
        table = process_table()
        proof['baseline_helper_pids'] = [list(owned_helpers(table, client.child.pid, binary)) for client in clients]
        require(not any(proof['baseline_helper_pids']), 'Baseline capture must precede any owned marker helper')
    click(a, target, 'exclusion-seed-a', proof.setdefault('seed_a', {}))
    first = helper(a, binary, observer)
    click(b, target, 'exclusion-seed-b', proof.setdefault('seed_b', {}))
    second = helper(b, binary, observer)
    proof['visible_before'] = [observer.snapshot(first), observer.snapshot(second)]
    require(all(visible(row) for row in proof['visible_before']), 'Peer exclusion was not tested with two visible owned markers')
    point = target.state()['pad_screen_point']
    for row in proof['visible_before']:
        bounds = row['windows'][0]['bounds']
        require(bounds['x'] <= point[0] < bounds['x']+bounds['width'] and bounds['y'] <= point[1] < bounds['y']+bounds['height'],
                'Known overlay did not cover the action point')
    if isolation_needed:
        capture_isolation(a, target, observer, [first, second], proof['seed_a']['capture'], evidence)
    click(a, target, 'physical-double-through-owned-markers', proof.setdefault('fallback', {}), foreground=True)
    proof['after_pixels'] = observer.snapshot(first, 'foreground-double-marker')
    proof['scope'] = 'Exact own/peer marker windows covered the input point; native double-click effect and foreground route succeeded. Pure Scene controls cover spoof identities.'


def close_client(mcp, observer):
    result = close_mcp(mcp); check_cleanup(mcp, result)
    identity = getattr(mcp, 'probe_overlay_identity', None)
    if identity:
        def reaped():
            table = process_table(); snap = observer.snapshot(identity)
            gone = table.get(identity['pid'], (None, None))[1] != identity['command']
            return {'process_reaped': True, 'window_snapshot': snap} if gone and not snap['identity_matches'] and not snap['windows'] else None
        result['overlay'] = wait_until(reaped, timeout=5)
    return result


def cancelled_execution_evidence(mcp, app, case, execution_id, rpc):
    # rmcp may intentionally omit the canceled tools/call response. The original
    # execution ledger, not transport silence, proves terminal cancellation.
    def terminal():
        record = mcp.receipt(app, case)
        return record if record['status'] == 'completed' else None
    record = wait_until(terminal, timeout=8)
    check_cancel_record(record, execution_id)
    retained = mcp.receipt(app, case)
    require(retained == record, 'Original cancellation receipt changed')
    response = mcp.pending.get(rpc)
    return {'record': record, 'retained_record': retained,
            'exec_response': {'availability': 'recorded' if response is not None else 'unavailable',
                              'response': response,
                              'meaning': 'Canceled RPC response is optional; only the original terminal execution ledger proves cancellation'}}


def begin_inflight(mcp, app, source, proof):
    proof['dispatch_attempts'] = 1
    proof['rpc_id'] = mcp.begin('tools/call', {'name': 'exec', 'arguments': {'app': app, 'source': source}})
    return proof['rpc_id']


def lifecycle(binary, output, clients, target, observer, evidence):
    require(os.environ.get('DTW_AX_READINESS_TRACE') == '1', 'Lifecycle requires DTW_AX_READINESS_TRACE=1 for native cleanup diagnostics')
    for mode in ('cancel', 'eof', 'parent-exit'):
        reset(target)
        mcp = open_client(binary, output/('mcp-'+mode), clients)
        proof = evidence.setdefault('lifecycle', {}).setdefault(mode, {'replayed': False})
        proof['visible_target_setup'] = front(target)
        mcp.probe_require_stderr_diagnostics = True
        click(mcp, target, mode+'-marker', proof.setdefault('pointer', {}))
        identity = helper(mcp, binary, observer)
        proof['identity'] = identity; proof['visible_before'] = observer.snapshot(identity)
        require(visible(proof['visible_before']), 'Lifecycle began without a visible real-input marker')
        tag = 'OVERLAY-'+uuid.uuid4().hex[:8]+'-'
        payload = tag + ''.join(f'{index:04d}:中文😀|' for index in range(250))
        before = target.state(); proof['before'] = before
        rpc = begin_inflight(mcp, target.app_id,
              select()+f'await app.typeText({{window_ref,text:{json.dumps(payload)}}});throw Error("unexpected suffix executed");', proof)
        entered = wait_until(lambda: (state if len((state := target.state())['text']) >= len(tag)+10 else None))
        require(len(entered['text']) < len(payload), 'Cancellation missed the in-flight input; do not replay')
        proof['entered'] = entered; proof['payload_sha256'] = hashlib.sha256(payload.encode()).hexdigest()
        proof['running'] = mcp.receipt(target.app_id, mode+'-original')
        proof['execution_id'] = proof['running']['execution_id']
        require(proof['running']['status'] == 'running', 'Input ended before interruption; no retry')
        proof['native_workers'] = recorded_workers(mcp)
        require(mcp.child.poll() is None, 'MCP exited before intentional interruption')
        if mode == 'cancel':
            mcp.notify('notifications/cancelled', {'requestId': rpc, 'reason': 'owned overlay lifecycle probe'})
            proof.update(cancelled_execution_evidence(mcp, target.app_id, mode+'-original', proof['execution_id'], rpc))
            proof['hidden_after_cancel'] = observer.hidden(identity)
            require(proof['hidden_after_cancel']['identity_matches'], 'Ordinary cancellation killed the session helper')
            require(helper(mcp, binary, observer) == identity, 'Cancellation replaced the session helper')
        else:
            expected = {'pid': mcp.child.pid, 'reason': mode, 'returncode': 0 if mode == 'eof' else -signal.SIGTERM, 'sent': False}
            mcp.probe_expected_exit = expected; mcp.probe_expected_exit_child = mcp.child; proof['expected_exit'] = expected
            if mode == 'eof': mcp.child.stdin.close()
            else: mcp.child.terminate()
            expected['sent'] = True
        proof['cleanup'] = close_client(mcp, observer)
        after = fresh_end(target); proof['after'] = after
        require(0 < len(after['text']) < len(payload) and payload.startswith(after['text']), 'Interrupted input is not an original partial prefix')
        proof['pairs'] = owned_input_pairs(target, before, {})
        if mode != 'cancel': retained_exec_response(mcp, proof)


def require_owned_signal_target(identity, snapshot, table, parent, binary):
    require(snapshot.get('identity_matches') is True
            and owned_helpers(table, parent, binary).get(identity['pid']) == identity['command'],
            'Helper termination requires fresh exact kernel identity, parent and executable proof')


def helper_recovery(binary, output, clients, target, observer, evidence):
    proof = evidence.setdefault('helper_exit_new_action_recovers_same_session', {'replayed': False})
    proof['visible_target_setup'] = front(target)
    mcp = open_client(binary, output/'mcp-helper-recovery', clients)
    click(mcp, target, 'before-owned-helper-exit', proof.setdefault('first_action', {}))
    old = helper(mcp, binary, observer)
    proof['original_identity'] = old; proof['original_marker'] = observer.snapshot(old, 'before-helper-exit-marker')
    require(visible(proof['original_marker']), 'Helper recovery began without a visible real marker')
    signal_snapshot = observer.snapshot(old); table = process_table()
    require_owned_signal_target(old, signal_snapshot, table, mcp.child.pid, binary)
    proof['termination'] = {'pid': old['pid'], 'signal': 'SIGTERM', 'owned_parent': mcp.child.pid,
                            'identity_snapshot': signal_snapshot, 'sent': False}
    os.kill(old['pid'], signal.SIGTERM); proof['termination']['sent'] = True
    def stopped():
        snap = observer.snapshot(old)
        status = subprocess.run(['/bin/ps', '-p', str(old['pid']), '-o', 'stat='],
                                text=True, capture_output=True, timeout=2)
        # A dead child may remain a zombie until its MCP owner next reaps it.
        # Do not mistake that for a live helper and do not signal it again.
        dead = not snap['identity_matches'] or status.returncode == 1 or status.stdout.lstrip().startswith('Z')
        return {'snapshot': snap, 'process_status': status.stdout.strip(), 'exit_not_yet_reap': True} if dead and not snap['windows'] else None
    proof['stopped_before_new_action'] = wait_until(stopped, timeout=5)
    # A new request with a fresh capture, never a retry of the completed click.
    click(mcp, target, 'new-action-after-helper-exit', proof.setdefault('new_action', {}))
    new = helper(mcp, binary, observer, replacement_of=old); proof['replacement_identity'] = new
    proof['replacement_marker'] = observer.snapshot(new, 'replacement-helper-marker')
    require(visible(proof['replacement_marker']), 'Fresh action did not display a replacement marker')
    if proof['original_marker']['rendering_verified'] and proof['replacement_marker']['rendering_verified']:
        require(proof['original_marker']['capture']['dominant_color_bin'] == proof['replacement_marker']['capture']['dominant_color_bin'],
                'Stable logical session changed its marker color')
    def reaped():
        snap = observer.snapshot(old)
        return snap if not snap['identity_matches'] and not snap['windows'] else None
    proof['original_reaped_after_new_action'] = wait_until(reaped, timeout=5)


def target_changes(clients, target, binary, observer, evidence):
    mcp = clients[0]; proof = evidence.setdefault('target_move_and_close_hide', {})
    proof['visible_target_setup'] = front(target)
    for change in ('move', 'close'):
        click(mcp, target, 'before-target-'+change, proof.setdefault(change, {}).setdefault('pointer', {}))
        identity = helper(mcp, binary, observer)
        proof[change]['visible_before'] = observer.snapshot(identity)
        require(visible(proof[change]['visible_before']), 'Target change began without visible marker')
        command(target, 'overlay_'+change)
        proof[change]['fixture_after'] = target.state()
        proof[change]['hidden_after'] = observer.hidden(identity)
        require(proof[change]['hidden_after']['identity_matches'], 'Target change killed the session helper')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True); parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', choices=['all', 'background', 'strict', 'exclusions', 'target-change', 'lifecycle', 'helper-recovery'], default='all')
    args = parser.parse_args(); require(sys.platform == 'darwin', 'Requires macOS GUI')
    output = args.output.absolute(); output.mkdir(mode=0o700); binary = args.dtw.resolve(strict=True)
    fixtures_root = Path(__file__).resolve().parents[1]/'fixtures'
    base = fixtures_root/'native-input/main.swift'; controls = fixtures_root/'cursor-overlay-controls.swift'
    observer_source = fixtures_root/'cursor-overlay-observer.swift'
    source = output/'main.swift'; source.write_text(derived_fixture(base.read_text(), controls.read_text()))
    observer_binary = output/'cursor-overlay-observer'
    subprocess.run(['swiftc', '-parse-as-library', str(observer_source), '-o', str(observer_binary)], check=True)
    observer = Observer(observer_binary, output)
    evidence = {'kind': 'owned-native-cursor-overlay-standard-MCP', 'case': args.case,
                'binary_sha256': sha(binary), 'fixture_base_sha256': sha(base), 'fixture_controls_sha256': sha(controls),
                'generated_fixture_sha256': sha(source), 'observer_source_sha256': sha(observer_source),
                'live_llm_host': False, 'global_desktop_idle_required': False,
                'scope': 'Actual input effects and exact owned marker-window rendered pixels; window capture does not prove visibility through unrelated occluding windows. Sampled pointer/foreground stability is separate coverage, not actor attribution.',
                'synthetic_overlay_events': False, 'replay': False}
    clients, fixtures, failure = [], [], None
    try:
        for name, x in [('target', 100), ('peer', 700), ('foreground', 1050)]:
            fixture = Fixture.prepare(output/name, 'dev.dtw.overlay.'+uuid.uuid4().hex, source,
                                      compiled=fixtures[0].executable if fixtures else None)
            fixtures.append(fixture); write_json(fixture.root/'fixture.json', {'background_probe': True, 'x': x})
            fixture.start()
        for name in ('mcp-a', 'mcp-b'): open_client(binary, output/name, clients)
        cases = {'strict': lambda: strict(binary, output, clients, fixtures[0], observer, evidence),
                 'background': lambda: background(clients, fixtures, binary, observer, evidence),
                 'exclusions': lambda: exclusions(clients, fixtures, binary, observer, evidence),
                 'helper-recovery': lambda: helper_recovery(binary, output, clients, fixtures[0], observer, evidence),
                 'lifecycle': lambda: lifecycle(binary, output, clients, fixtures[0], observer, evidence),
                 'target-change': lambda: target_changes(clients, fixtures[0], binary, observer, evidence)}
        for name, run in cases.items():
            if args.case in ('all', name): run()
    except BaseException as error:
        failure = {'error': repr(error), 'traceback': traceback.format_exc()}
    finally:
        cleanup = evidence.setdefault('cleanup', [])
        for resource in reversed(clients):
            try:
                cleanup.append(close_client(resource, observer))
            except BaseException as error:
                cleanup.append({'error': repr(error)}); failure = failure or {'error': 'MCP/helper cleanup failed'}
        for resource in reversed(fixtures):
            try:
                result = resource.close(); check_cleanup(resource, result); cleanup.append(result)
            except BaseException as error:
                cleanup.append({'error': repr(error)}); failure = failure or {'error': 'Fixture cleanup failed'}
        evidence.update(functional_passed=failure is None, rendering_gaps=observer.gaps,
                        passed=failure is None and not observer.gaps, failure=failure)
        write_json(output/'evidence.json', evidence)
    print(json.dumps({'passed': evidence['passed'], 'functional_passed': evidence['functional_passed'],
                      'gaps': len(observer.gaps), 'evidence': str(output/'evidence.json')}))
    return 0 if evidence['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
