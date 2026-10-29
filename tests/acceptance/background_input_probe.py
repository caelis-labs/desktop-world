#!/usr/bin/env python3
"""Opt-in owned AppKit background-input acceptance through standard stdio MCP.

Run on a real macOS desktop with a signed DTW candidate:
  python3 tests/acceptance/background_input_probe.py --dtw /path/to/dtw --output /tmp/new-run
Three owned apps and two MCP processes cover background keyboard/click delivery,
same-PID window routing, concurrent apps, focus return, and external focus change.
No action is retried. Independent fixture journals retain the initial failure.
User mouse movement and unrelated foreground activity are observations, not failures.
Position samples alone do not establish which actor moved the physical pointer.
This is native GUI/MCP evidence, not external LLM or arbitrary-app acceptance.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import math
from pathlib import Path
import sys
import subprocess
import threading
import time
import traceback
import uuid

from native_input_probe import check_cleanup, close_mcp
from standard_agent_probe import Fixture, Mcp, require, save_png, successful, wait_until, write_json

PRIMARY = 'DTW Input Target'
SECONDARY = 'DTW Input Secondary'


def command(fixture, name):
    # The fixture processes changed command bytes. Unique whitespace allows the
    # same owned setup command to be acknowledged twice, with no input retries.
    serial = getattr(fixture, 'background_command_serial', 0) + 1
    fixture.background_command_serial = serial
    return fixture.command(name + '\n' + ' ' * serial)


def front(fixture):
    owned_pid = fixture.child.pid
    require(fixture.child.poll() is None, 'Owned fixture exited before activation')
    command(fixture, 'front')
    # New macOS activation is cooperative. LaunchServices supplies this explicit
    # owned setup activation; it is never used by DTW or to retry an input.
    bundle = fixture.executable.parents[2]
    request = subprocess.run(['/usr/bin/open', '-a', str(bundle)], capture_output=True, text=True, timeout=5)
    require(request.returncode == 0, {'setup_activation_failed': request.stderr, 'app': str(bundle)})
    def ready():
        state = fixture.state()
        require(fixture.child.poll() is None and state['pid'] == owned_pid, 'Activation replaced the owned fixture PID')
        return state if (state['active'] and state['frontmost_pid'] == owned_pid
                         and state['key_window'] == state['windows'][0]['id']) else None
    state = wait_until(ready)
    state['setup_activation'] = {'method': 'single_launchservices_open', 'bundle': str(bundle), 'owned_pid': owned_pid}
    return state


def reset(fixture):
    command(fixture, 'reset')
    if len(fixture.state()['windows']) > 1:
        command(fixture, 'reset_secondary')
    require(fixture.state()['text'] == '', 'Primary text did not reset')
    require(fixture.state().get('secondary_text', '') == '', 'Secondary text did not reset')


def select(title=PRIMARY):
    return f"""
const matches=(await app.windows()).windows.filter(w=>w.title==={json.dumps(title)});
if(matches.length!==1) throw new Error('Owned window not unique');
const window_ref=matches[0].window_ref;
"""


def execute(mcp, fixture, case, source, evidence):
    record, response = mcp.execute(fixture.app_id, case, source)
    evidence.update(record=record, fixture=fixture.state())
    return successful(record), response


def text_equals(fixture, expected, secondary=False):
    key = 'secondary_text' if secondary else 'text'
    def ready():
        state = fixture.state()
        return state if state.get(key) == expected else None
    return wait_until(ready)


def journal(fixture, name):
    # A writer may be appending the next row. A completed snapshot is published
    # only after its journal row, so ignoring an unfinished final row is safe.
    return [json.loads(line) for line in (fixture.root / name).read_bytes().splitlines(keepends=True)
            if line.endswith(b'\n')]


def observe_desktop(observer, before, after, proof, *, forbidden_pids=()):
    """Retain desktop activity without inferring its actor from position samples.

    forbidden_pids identifies intended background targets for coverage reporting;
    seeing one in front cannot prove a DTW takeover while the user is active.
    Missing final samples remain an evidence failure; publication gaps make
    background coverage unproven without failing independently verified input.
    """
    start, end = before['published_uptime'], after['published_uptime']
    samples = [row for row in journal(observer, 'desktop.jsonl') if start < row['uptime'] <= end]
    activations = [row for row in journal(observer, 'focus.jsonl') if start < row['uptime'] <= end]
    proof.update(before=before, after=after, samples=samples, activations=activations,
                 pointer_evidence='20 ms fixture sampling; shorter excursions are not excluded; '
                                  'ambient pointer changes cannot be attributed to DTW from positions alone')
    require(len(samples) >= 2 and samples[-1]['uptime'] == end, 'Missing fresh desktop samples')
    timestamps = [start] + [row['uptime'] for row in samples]
    gaps = [b-a for a,b in zip(timestamps, timestamps[1:])]
    require(all(gap > 0 for gap in gaps), 'Desktop sample timestamps did not advance')
    proof['largest_sample_gap_seconds'] = max(gaps)
    proof['sampling_coverage'] = {
        'status': 'complete' if proof['largest_sample_gap_seconds'] <= 0.2 else 'not_established',
        'maximum_gap_seconds': 0.2,
        'reason': None if proof['largest_sample_gap_seconds'] <= 0.2 else 'publication_gap'}
    pointer, frontmost = before['pointer_screen'], before['frontmost_pid']
    proof['pointer_deltas'], proof['foreground_changes'] = [], []
    for row in samples:
        current = row['pointer_screen']
        if current != pointer:
            proof['pointer_deltas'].append({'uptime': row['uptime'], 'from': pointer, 'to': current,
                                           'delta': [current[i]-pointer[i] for i in range(2)]})
        if row['frontmost_pid'] != frontmost:
            proof['foreground_changes'].append({'uptime': row['uptime'], 'from': frontmost,
                                                 'to': row['frontmost_pid']})
        pointer, frontmost = current, row['frontmost_pid']
    targets = set(forbidden_pids)
    observations = [{'source': source, **row} for source, rows in
                    [('baseline', [dict(before, uptime=start)]), ('sample', samples), ('activation', activations)]
                    for row in rows if row['frontmost_pid'] in targets]
    proof['target_foreground_observations'] = observations
    proof['background_coverage'] = {
        'status': 'not_requested' if not targets else
                  'not_established' if observations or proof['sampling_coverage']['status'] != 'complete' else 'established',
        'sampling_coverage': proof['sampling_coverage'],
        'target_pids': sorted(targets),
        'attribution': 'Target foreground observations cannot identify their actor; '
                       'no DTW takeover is inferred from desktop snapshots alone.'}
    return proof


def fresh_end(fixture):
    baseline = fixture.state()['published_uptime']
    require(type(baseline) in (int, float) and math.isfinite(baseline) and baseline > 0,
            'Fixture has no valid publication timestamp')
    def fresh():
        state = fixture.state()
        published = state['published_uptime']
        require(type(published) in (int, float) and math.isfinite(published) and published >= baseline,
                'Fixture publication timestamp moved backwards or became invalid')
        return state if published-baseline >= 0.1 else None
    return wait_until(fresh, timeout=3)


def observed_pairs(events):
    """Pair received events in the owned target; global user input is unrelated."""
    down, up, pending = {}, {}, {}
    modifier_flags = 0
    for event in events:
        kind = event['kind']
        if kind in ('key_down', 'key_up', 'flags_changed'):
            modifier_flags = event.get('flags', 0) & 0x001e0000
        if kind in ('key_down', 'key_up'):
            key, is_down = 'key:' + str(event['key_code']), kind == 'key_down'
        elif kind in ('left_down', 'left_up', 'right_down', 'right_up'):
            key, is_down = kind.split('_')[0], kind.endswith('_down')
        else:
            continue
        counts = down if is_down else up
        counts[key] = counts.get(key, 0) + 1
        pending[key] = pending.get(key, 0)+1 if is_down else max(0, pending.get(key, 0)-1)
    # Menu equivalents can bypass the local keyDown monitor. Extra keyUp is
    # retained, but cannot discharge a later, unmatched keyDown.
    unpaired = {key: count for key, count in pending.items() if count}
    if modifier_flags:
        unpaired['received_modifier_flags'] = modifier_flags
    return {'down': down, 'up': up, 'unpaired': unpaired, 'received_modifier_flags': modifier_flags}


def owned_input_pairs(fixture, before, proof):
    after = fresh_end(fixture)
    events = [event for event in journal(fixture, 'events.jsonl')
              if event['sequence'] > before['events_count']]
    proof.update(observed_pairs(events), events=events, after=after,
                 scope='Received events in this action target; global held input is observation only')
    require(not proof['unpaired'], {'unpaired_owned_input': proof['unpaired']})
    return proof


def no_marker_leak(marker, fixtures, proof):
    """Only this probe's unique text identifies misdelivery to another app."""
    proof['marker'] = marker
    proof['peer_states'] = [fixture.state() for fixture in fixtures]
    for state in proof['peer_states']:
        require(not any(marker in state.get(key, '') for key in ('text', 'secondary_text', 'saved')),
                {'probe_marker_reached_wrong_app': marker, 'pid': state['pid']})


def finish_background(observer, before, targets, proof):
    return observe_desktop(observer, before, fresh_end(observer), proof,
                           forbidden_pids=tuple(target.child.pid for target in targets))


def background(mcp, target, peer, foreground, evidence):
    reset(target)
    before = front(foreground)
    marker = 'BG-'+uuid.uuid4().hex[:8]
    proof = evidence.setdefault('background_keyboard_and_click', {'before': target.state(), 'marker': marker})
    expected = marker+' 后台 Unicode 😀 café!'
    keyboard = proof.setdefault('keyboard', {})
    execute(mcp, target, 'background-keyboard', select() + f"""
await app.typeText({{window_ref,text:{json.dumps(expected + 'Z')}}});
return await app.keypress({{window_ref,keys:['BACKSPACE']}});
""", keyboard)
    proof['keyboard_state'] = text_equals(target, expected)
    require(marker not in proof['keyboard_state']['secondary_text'], 'Primary marker reached secondary window')
    owned_input_pairs(target, proof['before'], keyboard.setdefault('paired_input', {}))
    no_marker_leak(marker, (peer, foreground), keyboard.setdefault('routing', {}))
    finish_background(foreground, before, (target,), keyboard.setdefault('desktop', {}))
    before = foreground.state()
    capture = proof.setdefault('capture', {})
    shot, response = execute(mcp, target, 'background-capture', select() +
        'state.window=window_ref;state.shot=await app.capture({window_ref});return state.shot;', capture)
    directory = mcp.output / 'background-capture'; directory.mkdir()
    capture['png'] = save_png(directory, response, shot)
    finish_background(foreground, before, (target,), capture.setdefault('desktop', {}))
    state = target.state(); bounds = state['window_bounds']; point = state['pad_screen_point']
    x = (point[0]-bounds[0])/bounds[2]*shot['image']['width']
    y = (point[1]-bounds[1])/bounds[3]*shot['image']['height']
    require(0 < x < shot['image']['width'] and 0 < y < shot['image']['height'], 'Pad outside image')
    clicks = state['pad']['left_up']
    before = foreground.state()
    click = proof.setdefault('click', {})
    execute(mcp, target, 'background-click', f"""
return await app.click({{window_ref:state.window,observation_ref:state.shot.observation_ref,x:{x},y:{y}}});
""", click)
    def clicked():
        after = target.state()
        return after if after['pad']['left_up'] == clicks+1 else None
    proof['click_state'] = wait_until(clicked)
    pairs = owned_input_pairs(target, state, click.setdefault('paired_input', {}))
    require(pairs['down'].get('left') == 1 and pairs['up'].get('left') == 1,
            'Expected one owned pad down/up pair for the one DTW click')
    finish_background(foreground, before, (target,), click.setdefault('desktop', {}))
    click['pointer_attribution'] = 'Actual pad hit and received pair verified; source audit of directed '
    click['pointer_attribution'] += 'delivery is separate evidence. Position deltas cannot identify DTW as the mover.'


def multiwindow(mcp, target, peer, foreground, evidence):
    reset(target)
    before = front(foreground)
    proof = evidence.setdefault('same_pid_multiple_windows', {'before': target.state()})
    marker = 'SECOND-'+uuid.uuid4().hex[:8]
    expected = marker+'-末'
    secondary = proof.setdefault('secondary', {})
    execute(mcp, target, 'secondary-keyboard', select(SECONDARY) + f"""
await app.typeText({{window_ref,text:{json.dumps(marker+'-Z')}}});
await app.keypress({{window_ref,keys:['BACKSPACE']}});
return await app.typeText({{window_ref,text:'末'}});
""", secondary)
    proof['secondary_state'] = text_equals(target, expected, secondary=True)
    require(marker not in proof['secondary_state']['text'], 'Secondary marker reached primary window')
    owned_input_pairs(target, proof['before'], secondary.setdefault('paired_input', {}))
    no_marker_leak(marker, (peer, foreground), secondary.setdefault('routing', {}))
    finish_background(foreground, before, (target,), secondary.setdefault('desktop', {}))
    before, target_before = foreground.state(), target.state()
    marker = 'PRIMARY-'+uuid.uuid4().hex[:8]+'-独立'
    primary = proof.setdefault('primary', {})
    execute(mcp, target, 'primary-keyboard', select() +
            f"return await app.typeText({{window_ref,text:{json.dumps(marker)}}});", primary)
    proof['primary_state'] = text_equals(target, target_before['text']+marker)
    require(marker not in proof['primary_state']['secondary_text'], 'Primary marker reached secondary window')
    owned_input_pairs(target, target_before, primary.setdefault('paired_input', {}))
    no_marker_leak(marker, (peer, foreground), primary.setdefault('routing', {}))
    proof['events'] = [event for event in journal(target, 'events.jsonl')
                       if event['sequence'] > proof['before']['events_count']]
    require(len({window['id'] for window in proof['primary_state']['windows']}) == 2,
            'Fixture did not create two windows in the same PID')
    finish_background(foreground, before, (target,), primary.setdefault('desktop', {}))


def concurrent(clients, target, peer, foreground, evidence):
    for fixture in (target, peer): reset(fixture)
    before = front(foreground)
    proof = evidence.setdefault('two_mcp_processes_distinct_apps', {})
    require(clients[0].child.pid != clients[1].child.pid, 'MCP clients share a process')
    barrier = threading.Barrier(3)
    def worker(index, fixture):
        # Keep the original eight calls / ~200 characters per app workload.
        marker = ('A-甲-' if index == 0 else 'B-乙-') + uuid.uuid4().hex[:20]
        source = select() + f"for(let i=0;i<8;i++) await app.typeText({{window_ref,text:{json.dumps(marker)}}});return true;"
        row = {'mcp_pid': clients[index].child.pid, 'app': fixture.app_id,
               'before': fixture.state(), 'marker': marker}
        proof[str(index)] = row
        barrier.wait(timeout=5)
        row['started'] = time.monotonic()
        execute(clients[index], fixture, 'concurrent-background', source, row)
        row['finished'] = time.monotonic()
        row['after'] = text_equals(fixture, marker*8)
        events = [event for event in journal(fixture, 'events.jsonl')
                  if event['sequence'] > row['before']['events_count'] and event['kind'] in ('key_down', 'key_up')]
        require(events, 'No independent native keyboard journal for concurrent app')
        row['native_event_interval'] = [min(e['uptime'] for e in events), max(e['uptime'] for e in events)]
        owned_input_pairs(fixture, row['before'], row.setdefault('paired_input', {}))
        return row
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures = [pool.submit(worker, index, fixture) for index, fixture in enumerate((target, peer))]
        barrier.wait(timeout=5)
        rows = [future.result(timeout=35) for future in futures]
    require(max(row['started'] for row in rows) < min(row['finished'] for row in rows),
            'The two MCP executions did not overlap')
    intervals = [row['native_event_interval'] for row in rows]
    proof['native_event_intervals_overlap'] = max(row[0] for row in intervals) < min(row[1] for row in intervals)
    for index, fixture in enumerate((target, peer)):
        row = rows[index]
        require(row['marker'] not in fixture.state().get('secondary_text', ''),
                'Concurrent primary marker reached secondary window')
        no_marker_leak(row['marker'], ((peer if index == 0 else target), foreground),
                       row.setdefault('routing', {}))
    finish_background(foreground, before, (target, peer), proof.setdefault('desktop', {}))


def focus_return_coverage(observer, before, target, proof):
    after = fresh_end(observer)
    desktop = observe_desktop(observer, before, after, proof.setdefault('desktop', {}))
    target_pid, original_pid = target.child.pid, before['frontmost_pid']
    rows = sorted(desktop['samples']+desktop['activations'], key=lambda row: row['uptime'])
    require(any(row['frontmost_pid'] == target_pid for row in rows),
            'Focus never independently reached the target app')
    external = [row for row in rows if row['frontmost_pid'] not in (target_pid, original_pid)]
    # A new activation after a completed return can be a user's next choice.
    # It is not evidence that cleanup failed to return the first borrow.
    seen_target, seen_return = False, False
    for row in rows:
        pid = row['frontmost_pid']
        if pid == target_pid:
            if seen_return: external.append(row)
            seen_target = True
        elif pid == original_pid and seen_target:
            seen_return = True
    proof['after'] = after
    continuous = desktop['sampling_coverage']['status'] == 'complete'
    proof['ownership_coverage'] = {'status': 'established' if continuous and not external else 'not_established',
        'sampling_coverage': desktop['sampling_coverage'],
        'external_foreground_observations': external,
        'attribution': 'Unscheduled foreground activity makes the normal return branch unproven; '
                       'pointer movement is unrelated to focus ownership.'}
    if continuous and not external:
        require(after['frontmost_pid'] == original_pid, 'Uninterrupted focus borrow was not returned')
    return proof['ownership_coverage']


def foreground_fallback(mcp, target, foreground, evidence):
    before = front(foreground)
    proof = evidence.setdefault('background_unsupported_automatically_borrows_foreground', {})
    shot, response = execute(mcp, target, 'fallback-capture', select() +
        'state.fallbackWindow=window_ref;state.fallbackShot=await app.capture({window_ref});return state.fallbackShot;',
        proof.setdefault('capture', {}))
    directory = mcp.output/'fallback-capture'; directory.mkdir()
    proof['capture']['png'] = save_png(directory, response, shot)
    state = target.state(); bounds = state['window_bounds']; point = state['pad_screen_point']
    x = (point[0]-bounds[0])/bounds[2]*shot['image']['width']
    y = (point[1]-bounds[1])/bounds[3]*shot['image']['height']
    require(0 < x < shot['image']['width'] and 0 < y < shot['image']['height'], 'Pad outside image')
    execute(mcp, target, 'automatic-foreground-right-double-click', f"""
return await app.click({{window_ref:state.fallbackWindow,observation_ref:state.fallbackShot.observation_ref,
                        x:{x},y:{y},button:'right',count:2}});
""", proof.setdefault('click', {}))
    def clicked():
        after = target.state()
        return after if (after['pad']['right_down'] == state['pad']['right_down']+2
                         and after['pad']['right_up'] == state['pad']['right_up']+2) else None
    proof['after_click'] = wait_until(clicked)
    pairs = owned_input_pairs(target, state, proof.setdefault('paired_input', {}))
    require(pairs['down'].get('right') == 2 and pairs['up'].get('right') == 2,
            'Automatic fallback did not deliver exactly two owned right down/up pairs')
    require([event['click_count'] for event in pairs['events'] if event['kind'] == 'right_down'] == [1, 2],
            'Fallback did not carry native double-click counts')
    focus_return_coverage(foreground, before, target, proof)


def focus_return(mcp, target, foreground, evidence):
    for count, key in [(1, 'explicit_focus_returns_after_script'), (2, 'repeated_focus_returns_after_script'),
                       (3, 'explicit_sibling_focus_returns_after_script')]:
        reset(target)
        before = front(foreground)
        proof = evidence.setdefault(key, {'before': before, 'explicit_focus_count': count})
        focus = 'await app.focus({window_ref});' * count
        if count == 3:
            focus = f"""
await app.focus({{window_ref}});
const sibling=(await app.windows()).windows.find(w=>w.title==={json.dumps(SECONDARY)});
if(!sibling) throw new Error('Sibling window is missing');
await app.focus({{window_ref:sibling.window_ref}});
await app.focus({{window_ref}});
"""
        marker = 'FOCUS-'+uuid.uuid4().hex[:8]+'-归还'
        target_before = target.state()
        source = select() + focus + f"return await app.typeText({{window_ref,text:{json.dumps(marker)}}});"
        execute(mcp, target, f'focus-return-{count}', source, proof)
        proof['target_after'] = text_equals(target, marker)
        owned_input_pairs(target, target_before, proof.setdefault('paired_input', {}))
        no_marker_leak(marker, (foreground,), proof.setdefault('routing', {}))
        focus_return_coverage(foreground, before, target, proof)


def focus_change(mcp, target, peer, foreground, evidence):
    before = front(foreground)
    proof = evidence.setdefault('external_focus_change_is_preserved', {'before': before})
    source = select() + """
await app.focus({window_ref});
const until=Date.now()+2000;while(Date.now()<until) {} return true;
"""
    with ThreadPoolExecutor(max_workers=1) as pool:
        future = pool.submit(execute, mcp, target, 'external-focus-change', source, proof)
        def borrowed():
            state = target.state()
            return state if state['active'] and state['frontmost_pid'] == target.child.pid else None
        proof['borrowed'] = wait_until(borrowed)
        # Owned fixture control represents a user/other Agent's independent
        # activation; it is deliberately outside this DTW/MCP execution.
        proof['external_activation'] = front(peer)
        future.result(timeout=35)
    observe_desktop(peer, proof['external_activation'], fresh_end(peer), proof.setdefault('after_external_change', {}))
    require(peer.state()['frontmost_pid'] == peer.child.pid, 'DTW took focus back from the external choice')


def focus_leave_return(mcp, target, peer, foreground, evidence):
    before = front(foreground)
    proof = evidence.setdefault('external_focus_leave_then_return_is_preserved', {'before': before})
    source = select() + """
await app.focus({window_ref});
const hold_started_unix_ms=Date.now();
while(Date.now()<hold_started_unix_ms+5000) {}
return {hold_started_unix_ms,hold_ended_unix_ms:Date.now()};
"""
    with ThreadPoolExecutor(max_workers=1) as pool:
        future = pool.submit(execute, mcp, target, 'external-focus-leave-return', source, proof)
        def borrowed():
            state = target.state()
            return state if state['active'] and state['frontmost_pid'] == target.child.pid else None
        proof['borrowed'] = wait_until(borrowed)
        proof['external_leave'] = front(peer)
        proof['external_return'] = front(target)
        proof['external_return_unix_ms'] = int(time.time()*1000)
        hold, _ = future.result(timeout=35)
        proof['script_hold'] = hold
    require(hold['hold_started_unix_ms'] <= proof['external_return_unix_ms'] < hold['hold_ended_unix_ms'],
            'External leave/return did not finish while the script still held its borrow')
    observe_desktop(target, proof['external_return'], fresh_end(target),
                      proof.setdefault('after_external_return', {}))
    require(target.state()['frontmost_pid'] == target.child.pid,
            'Finalizer restored stale foreground after an external leave/return')


def focus_window_leave_return(mcp, target, foreground, evidence):
    before = front(foreground)
    proof = evidence.setdefault('external_sibling_window_leave_then_return_is_preserved', {'before': before})
    source = select() + """
await app.focus({window_ref});
const hold_started_unix_ms=Date.now();
while(Date.now()<hold_started_unix_ms+5000) {}
return {hold_started_unix_ms,hold_ended_unix_ms:Date.now()};
"""
    def selected(index):
        def ready():
            state = target.state()
            return state if (state['frontmost_pid'] == target.child.pid
                             and state['key_window'] == state['windows'][index]['id']) else None
        return wait_until(ready)
    with ThreadPoolExecutor(max_workers=1) as pool:
        future = pool.submit(execute, mcp, target, 'external-sibling-window-leave-return', source, proof)
        proof['borrowed'] = selected(0)
        command(target, 'front_secondary')
        proof['external_sibling'] = selected(1)
        # Retain independent publications across several bounded observer samples.
        # This is setup observation, not a retry of any DTW action.
        start = proof['external_sibling']['published_uptime']
        wait_until(lambda: target.state()['published_uptime'] - start >= 0.15)
        command(target, 'front')
        proof['external_return'] = selected(0)
        proof['external_return_unix_ms'] = int(time.time()*1000)
        hold, _ = future.result(timeout=35)
        proof['script_hold'] = hold
    require(hold['hold_started_unix_ms'] <= proof['external_return_unix_ms'] < hold['hold_ended_unix_ms'],
            'Sibling window leave/return did not finish while the script held its borrow')
    observe_desktop(target, proof['external_return'], fresh_end(target),
                      proof.setdefault('after_external_return', {}))
    require(target.state()['key_window'] == target.state()['windows'][0]['id'],
            'Finalizer overwrote the external window selection')


def close_record_outcome(record, app, app_ref):
    """Recognize the exact post-close target-change result without rewriting it."""
    require(record.get('app') == app and record.get('app_ref') == app_ref,
            'Close returned a different app owner')
    require(record.get('status') == 'completed' and record.get('context_reset') is False
            and record.get('state_reset') is False, 'Close lost owner continuity')
    pending = [record]
    while pending:
        value = pending.pop()
        if isinstance(value, dict):
            for key, child in value.items():
                if key in ('cleanup_error', 'shutdown_error'):
                    require(child is None, 'Close left a cleanup or shutdown failure')
                else:
                    pending.append(child)
        elif isinstance(value, list):
            pending.extend(value)
    script = record.get('script', {})
    steps = script.get('steps', [])
    operations = [step.get('operation') for step in steps]
    require(operations in (['keypress'], ['windows', 'keypress']),
            'Close must contain exactly one keypress and no other write')
    require(all(step.get('status') == 'completed' for step in steps), 'Close has unfinished native work')
    require(all(not step.get('error') and not step.get('receipt') for step in steps[:-1]),
            'Close preparation failed before input')
    step = steps[-1]
    receipt = step.get('receipt') or {}
    require(receipt.get('interruption') is None, 'Close was interrupted')
    outcomes = [receipt.get(key) or {} for key in ('outcome', 'provider_outcome')]
    if record.get('error') is None:
        successful(record)
        require(script.get('failure') is None and step.get('error') is None
                and all(row.get('dispatch') == 'dispatched' and row.get('error') is None for row in outcomes),
                'Successful close lacks one clean dispatched receipt')
        return 'completed'
    # Retain the exact original unknown. New candidates preserve the native
    # diagnosis: an invalid retained AX window after close is distinct from a
    # busy AX provider (cannotComplete), which is never accepted here.
    expected_message = (step.get('error') or {}).get('message')
    require(expected_message in (
        'input target changed; refresh app/window state',
        'target_changed: WindowServer no longer proves the requested surface owner',
        'ax_unavailable: AX attribute AXModal failed: -25202',
    ), 'Close failed for a reason other than the exact post-input target change')
    expected_code = 'background_input_failed'
    failure, error = script.get('failure') or {}, step.get('error') or {}
    require(record['error'].get('code') == 'script_exception'
            and record['error'].get('message') == 'keypress failed: '+expected_message
            and script.get('state_committed') is False
            and failure.get('kind') == 'exception' and failure.get('operation') == 'keypress'
            and failure.get('code') == expected_code and failure.get('call') == step.get('number')
            and error.get('code') == expected_code and error.get('message') == expected_message
            and error.get('details', {}).get('provider_code') == expected_code,
            'Close failed for a reason other than the exact post-input target change')
    require(all(row.get('dispatch') == 'unknown' and row.get('error') == error for row in outcomes),
            'Post-close target change must retain both original unknown outcomes')
    return 'target_changed_unknown'


def close_window(mcp, target, foreground, evidence):
    before = front(foreground)
    target_before = target.state()
    secondary = target_before['windows'][1]['id']
    require(target_before['windows'][1]['visible'], 'Secondary window was not open before close')
    proof = evidence.setdefault('background_close_window_keeps_owner', {'before': target_before})
    marker = uuid.uuid4().hex
    committed, _ = execute(mcp, target, 'commit-before-background-close', select(SECONDARY) + f"""
state.closeWindowMarker={json.dumps(marker)};state.closeWindowRef=window_ref;
return {{marker:state.closeWindowMarker,window_ref}};
""", proof.setdefault('marker_commit', {}))
    require(committed['marker'] == marker, 'Close marker was not committed before input')
    owner = proof['marker_commit']['record']['app_ref']
    require(isinstance(owner, str) and owner, 'Missing original app owner')
    proof['owner'] = {'app_ref':owner, 'mcp_pid':mcp.child.pid, 'app_pid':target_before['pid']}
    close = proof.setdefault('close', {'dispatch_attempts':1, 'replayed':False})
    # Exactly one input execution. A closed target can fail the final validation
    # after dispatch; its original error/unknown receipt remains authoritative.
    record, _ = mcp.execute(target.app_id, 'background-close-secondary',
                           "return await app.keypress({window_ref:state.closeWindowRef,keys:['CMD','W']});")
    close.update(record=record, fixture=target.state())
    close['terminal_outcome'] = close_record_outcome(record, target.app_id, owner)
    def closed():
        state = target.state()
        return state if (secondary in state['closed_window_ids']
                         and not state['windows'][1]['visible'] and state['windows'][0]['visible']) else None
    proof['closed_state'] = wait_until(closed)
    proof['close_events'] = [event for event in journal(target, 'events.jsonl')
                            if event['sequence'] > target_before['events_count']
                            and event['kind'] == 'window_closed' and event['window_number'] == secondary]
    require(len(proof['close_events']) == 1, 'Expected one independent NSWindow close notification')
    owned_input_pairs(target, target_before, proof.setdefault('paired_input', {}))
    # willClose precedes WindowServer's removal/animation. Observe only, retaining
    # every inventory and its receipt; no close or input is repeated.
    convergence = proof.setdefault('inventory_convergence', {'samples':[], 'read_only':True, 'budget_seconds':1.0})
    started = time.monotonic()
    while True:
        sample = {'elapsed_seconds':time.monotonic()-started}
        convergence['samples'].append(sample)
        state, _ = execute(mcp, target, 'read-after-background-close-'+str(len(convergence['samples'])), f"""
if(state.closeWindowMarker!=={json.dumps(marker)}) throw new Error('App state was reset');
if(state.closeWindowRef!=={json.dumps(committed['window_ref'])}) throw new Error('Original window state was reset');
return {{windows:(await app.windows()).windows,marker:state.closeWindowMarker}};
""", sample)
        sample['windows'] = state['windows']
        require(sum(w['title']==PRIMARY for w in state['windows']) == 1, 'Primary window was lost')
        if not any(w['window_ref']==committed['window_ref'] or w['title']==SECONDARY for w in state['windows']):
            convergence['converged_seconds'] = time.monotonic()-started
            proof['after_close'] = sample
            break
        require(time.monotonic()-started < convergence['budget_seconds'], 'Closed surface did not leave WindowServer inventory within the read-only convergence budget')
        time.sleep(0.05)
    proof['remaining_windows'] = state
    for operation in ('marker_commit', 'close', 'after_close'):
        record = proof[operation]['record']
        require(record['app_ref'] == owner and record['context_reset'] is False and record['state_reset'] is False,
                'Closing a sibling window reset the native owner or app state')
        require(not record.get('script', {}).get('shutdown_error'), 'Window close left a cleanup failure')
    require(mcp.child.pid == proof['owner']['mcp_pid'] and mcp.child.poll() is None,
            'Window close replaced or stopped the MCP owner')
    require(target.state()['pid'] == target_before['pid'], 'Window close replaced the app incarnation')
    proof['independent_close_verified'] = True
    finish_background(foreground, before, (target,), proof.setdefault('desktop', {}))


def coverage_summary(evidence):
    result = {}
    def visit(value, path):
        if not isinstance(value, dict): return
        for key, child in value.items():
            name = path+'/'+key if path else key
            if key in ('background_coverage', 'ownership_coverage') and isinstance(child, dict):
                if child['status'] != 'not_requested': result[name] = child['status']
            else:
                visit(child, name)
    visit(evidence, '')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', choices=['all', 'background', 'foreground-fallback', 'multiwindow', 'concurrent', 'focus-return', 'focus-change', 'focus-leave-return', 'focus-window-leave-return', 'close-window'], default='all')
    args = parser.parse_args()
    require(sys.platform == 'darwin', 'Requires a real macOS GUI desktop')
    output = args.output.absolute(); output.mkdir(mode=0o700)
    binary = args.dtw.resolve(strict=True)
    source = Path(__file__).resolve().parents[1] / 'fixtures/native-input/main.swift'
    evidence = {'kind': 'owned-AppKit-background-input-standard-MCP', 'live_llm_host': False,
                'case': args.case, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                'fixture_source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                'started_unix_ms': int(time.time()*1000),
                'acceptance_policy': 'action_scoped_v2', 'global_desktop_idle_required': False,
                'superseded_assertion': 'Unchanged pointer/foreground for every action was overly broad. '
                    'Old samples, including 20261029-h, stay preserved and are not relabeled as passing.',
                'pass_scope': 'Functional effects, owned input pairing and cleanup; background and focus '
                    'ownership coverage are reported separately. Desktop positions do not identify their actor.'}
    fixtures, clients, failure = [], [], None
    try:
        for name, x, secondary in [('target', 100, True), ('peer', 700, False), ('foreground', 380, False)]:
            fixture = Fixture.prepare(output/name, 'dev.dtw.background.'+uuid.uuid4().hex, source,
                                      compiled=fixtures[0].executable if fixtures else None)
            fixtures.append(fixture)
            write_json(fixture.root/'fixture.json', {'background_probe': True, 'secondary_window': secondary, 'x': x})
        evidence['before'] = [fixture.start() for fixture in fixtures]
        target, peer, foreground = fixtures
        for name in ('mcp-a', 'mcp-b'):
            directory = output/name; directory.mkdir()
            mcp = Mcp(binary, directory); clients.append(mcp)
            mcp.rpc('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                                  'clientInfo': {'name': 'background-input-probe', 'version': '1'}})
            mcp.notify('notifications/initialized')
        cases = {'background': lambda: background(clients[0], target, peer, foreground, evidence),
                 'foreground-fallback': lambda: foreground_fallback(clients[0], target, foreground, evidence),
                 'multiwindow': lambda: multiwindow(clients[0], target, peer, foreground, evidence),
                 'concurrent': lambda: concurrent(clients, target, peer, foreground, evidence),
                 'focus-return': lambda: focus_return(clients[0], target, foreground, evidence),
                 'focus-change': lambda: focus_change(clients[0], target, peer, foreground, evidence),
                 'focus-leave-return': lambda: focus_leave_return(clients[0], target, peer, foreground, evidence),
                 'focus-window-leave-return': lambda: focus_window_leave_return(clients[0], target, foreground, evidence),
                 'close-window': lambda: close_window(clients[0], target, foreground, evidence)}
        for name, run in cases.items():
            if args.case in ('all', name): run()
        evidence['after'] = [fixture.state() for fixture in fixtures]
    except BaseException as error:
        failure = {'type': type(error).__name__, 'message': str(error)}
        (output/'failure.txt').write_text(traceback.format_exc())
    finally:
        evidence['cleanup'] = []
        for name, resource in [(f'mcp-{i}', c) for i,c in enumerate(clients)] + [(f'fixture-{i}', f) for i,f in enumerate(fixtures)]:
            row = {'resource': name}; evidence['cleanup'].append(row)
            try:
                result = close_mcp(resource) if isinstance(resource, Mcp) else resource.close()
                row.update(result); check_cleanup(resource, result)
            except BaseException as error:
                row['error'] = {'type': type(error).__name__, 'message': str(error)}
                failure = failure or {'type': 'CleanupFailure', 'message': str(error)}
        evidence.update(failure=failure, passed=failure is None, functional_passed=failure is None,
                        finished_unix_ms=int(time.time()*1000))
        evidence['coverage'] = coverage_summary(evidence)
        write_json(output/'evidence.json', evidence)
    print(json.dumps({'passed': evidence['passed'], 'evidence': str(output/'evidence.json'), 'failure': failure}, ensure_ascii=False))
    return 0 if evidence['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
