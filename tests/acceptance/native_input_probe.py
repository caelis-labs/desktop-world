#!/usr/bin/env python3
"""Opt-in real AppKit input through ordinary stdio MCP; exclusive desktop required.

Creates two owned apps and two independent DTW MCP processes. Native edit/save,
Unicode, shortcut, scroll, focus, setter, right/double click, cancellation and
cross-process routing are checked against fixture files/events. Never retries an
input, releases keys on the user's behalf, or changes Agent configuration.
Use --case to run one sample in fresh fixtures. This is GUI/MCP evidence, not
live-model or Harness approval evidence. All initial failures remain on disk.
The eof and parent-exit cases deliberately end one owned MCP process and are
opt-in only; neither is included in all.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import math
import os
from pathlib import Path
import signal
import sys
import threading
import time
import traceback
import uuid

from standard_agent_probe import Fixture, Mcp, process_table, require, save_png, successful, wait_until, write_json

SELECT = """
const matches=(await app.windows()).windows.filter(w=>w.title==='DTW Input Target');
if(matches.length!==1) throw new Error('Owned input window not unique');
const window_ref=matches[0].window_ref;
"""
NATIVE_CLEANUP_FAILURE = b'dtw native input cleanup did not complete before shutdown'


def execute(mcp, fixture, case, source, evidence):
    record, response = mcp.execute(fixture.app_id, case, source)
    evidence[case] = {"record": record, "fixture": fixture.state()}
    return successful(record), response


def initialized(binary, path):
    path.mkdir()
    mcp = Mcp(binary, path)
    mcp.rpc('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                          'clientInfo': {'name': 'native-input-probe', 'version': '1'}})
    mcp.notify('notifications/initialized')
    return mcp


def saved_equals(fixture, expected):
    def predicate():
        state = fixture.state()
        file = fixture.root / 'saved.txt'
        return state if state['saved'] == expected and file.exists() and file.read_text() == expected else None
    return wait_until(predicate, timeout=4)


def release_uptime(state):
    published = state.get('published_uptime')
    require(type(published) in (int, float) and math.isfinite(published) and published > 0,
            'Fixture release sample has no valid published_uptime')
    return published


def input_is_released(state):
    keys, buttons, flags = (state.get(name) for name in
                           ('keys_down_global', 'buttons_down_global', 'modifier_flags_global'))
    require(isinstance(keys, list) and isinstance(buttons, list)
            and type(flags) is int and flags >= 0, 'Fixture input state is incomplete')
    # Shift, Control, Option and Command are held modifiers. Caps Lock and the
    # non-coalesced event flag (536870912) do not prove that a key is held down.
    held_modifiers = flags & (0x00020000 | 0x00040000 | 0x00080000 | 0x00100000)
    return not keys and not buttons and not held_modifiers


def released(fixture, proof=None, timeout=3):
    """Require fresh publications spanning 100 ms after this post-terminal read."""
    proof = proof if proof is not None else {}
    baseline = fixture.state()
    previous = release_uptime(baseline)
    proof.update(baseline=baseline, last_sample=baseline, first_stable=None, last_stable=None,
                 fresh_samples=0, stable_samples=0, interrupted_intervals=[], released=False,
                 required_stable_seconds=0.1, maximum_publish_gap_seconds=0.06)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        state = fixture.state()
        published = release_uptime(state)
        proof['last_sample'] = state
        require(published >= previous, 'Fixture published_uptime moved backwards')
        if published > previous:
            gap = published - previous
            previous = published
            proof['fresh_samples'] += 1
            clear = input_is_released(state)
            if not clear or gap > proof['maximum_publish_gap_seconds']:
                if proof['first_stable'] is not None:
                    proof['interrupted_intervals'].append({
                        'first_uptime':proof['first_stable']['published_uptime'],
                        'last_uptime':proof['last_stable']['published_uptime'],
                        'first_sample':proof['first_stable'], 'last_sample':proof['last_stable'],
                        'samples':proof['stable_samples'], 'reason':'held_input' if not clear else 'publish_gap'})
                proof.update(first_stable=None, last_stable=None, stable_samples=0)
            if clear:
                if proof['first_stable'] is None: proof['first_stable'] = state
                proof['last_stable'] = state
                proof['stable_samples'] += 1
                elapsed = published - proof['first_stable']['published_uptime']
                if proof['stable_samples'] >= 2 and elapsed >= proof['required_stable_seconds']:
                    proof.update(released=True, stable_seconds=elapsed)
                    return state
        time.sleep(0.01)
    raise AssertionError('Input release was not confirmed by fresh, continuous 100 ms samples')


def event_pairs(fixture, proof=None):
    proof = proof if proof is not None else {}
    proof['release'] = {}
    state = released(fixture, proof['release'])
    # Read the journal after fresh release samples, so late delivered events
    # cannot fall between an earlier journal read and release confirmation.
    events = [json.loads(line) for line in (fixture.root / 'events.jsonl').read_text().splitlines()]
    down, up = {}, {}
    for event in events:
        if event['kind'] == 'key_down':
            key = str(event['key_code']); down[key] = down.get(key, 0) + 1
        elif event['kind'] == 'key_up':
            key = str(event['key_code']); up[key] = up.get(key, 0) + 1
    # Menu key equivalents can bypass a local keyDown monitor; extra keyUp is
    # therefore recorded, not invented as a missing down. Every observed down
    # must nevertheless have its release in this same fixture.
    proof.update(keydown=down, keyup=up, keys_down_global=state['keys_down_global'],
                 buttons_down_global=state['buttons_down_global'],
                 modifier_flags_global=state['modifier_flags_global'], events_count=len(events))
    require(all(up.get(key, 0) >= count for key, count in down.items()),
            {'unpaired_observed_key_down': down, 'key_up': up})
    return proof


def edit(mcp, target, peer, evidence):
    before = peer.state()['text']
    expected = 'DTW 中文 😀 café'
    value, _ = execute(mcp, target, 'unicode_shortcut_save', SELECT + f"""
await app.focus({{window_ref}});
await app.keypress({{window_ref,keys:['META','A']}});
await app.typeText({{window_ref,text:{json.dumps(expected)}}});
const save=await app.keypress({{window_ref,keys:['CMD','S']}});
return {{save}};
""", evidence)
    evidence['unicode_saved'] = saved_equals(target, expected)
    require(peer.state()['text'] == before, 'Unicode crossed into the peer app')
    replacement = 'Native setter 设置'
    execute(mcp, target, 'setter_and_semantic_save', SELECT + f"""
let p=await app.observe({{window_ref,role:'text_field',text:'Fixture Text',limit:10}});
const fields=p.elements.filter(e=>e.actions.includes('setValue'));
if(fields.length!==1) throw new Error('No unique writable native text field');
const set=await app.setValue({{window_ref,observation_ref:p.observation_ref,element_ref:fields[0].element_ref,value:{json.dumps(replacement)}}});
p=await app.observe({{window_ref,role:'button',text:'Save',limit:10}});
const buttons=p.elements.filter(e=>e.name==='Save'&&e.can_press);
if(buttons.length!==1) throw new Error('No unique native Save action');
const save=await app.press({{window_ref,observation_ref:p.observation_ref,element_ref:buttons[0].element_ref}});
return {{set,save}};
""", evidence)
    evidence['setter_saved'] = saved_equals(target, replacement)
    require(peer.state()['text'] == before, 'Setter/save crossed into the peer app')
    scroll(mcp, target, evidence)


def scroll(mcp, target, evidence):
    evidence['scroll_before'] = target.state()
    scroll_before = evidence['scroll_before']['scroll_y']
    execute(mcp, target, 'scroll', SELECT + "await app.focus({window_ref});return await app.scroll({window_ref,delta_y:6});", evidence)
    evidence['scroll_after'] = wait_until(lambda: target.state() if target.state()['scroll_y'] > scroll_before else None)


def capture(mcp, target, case, evidence, focus=True):
    source = SELECT + ('await app.focus({window_ref});' if focus else '') + "state.window=window_ref;state.shot=await app.capture({window_ref});return state.shot;"
    shot, response = execute(mcp, target, case, source, evidence)
    directory = mcp.output / case; directory.mkdir()
    evidence[case]['png'] = save_png(directory, response, shot)
    state = target.state(); bounds = state['window_bounds']; point = state['pad_screen_point']
    x = (point[0] - bounds[0]) / bounds[2] * shot['image']['width']
    y = (point[1] - bounds[1]) / bounds[3] * shot['image']['height']
    require(0 < x < shot['image']['width'] and 0 < y < shot['image']['height'], 'Fixture pad outside capture')
    return x, y


def click(mcp, target, case, point, button, count, evidence):
    x, y = point
    return execute(mcp, target, case, f"return await app.click({{window_ref:state.window,observation_ref:state.shot.observation_ref,x:{x},y:{y},button:'{button}',count:{count}}});", evidence)


def visual(mcp, target, evidence):
    before = target.state()['pad']
    point = capture(mcp, target, 'right_capture', evidence)
    click(mcp, target, 'right_click', point, 'right', 1, evidence)
    wait_until(lambda: target.state()['pad']['right_up'] == before['right_up'] + 1)
    point = capture(mcp, target, 'double_capture', evidence)
    click(mcp, target, 'double_click', point, 'left', 2, evidence)
    after = wait_until(lambda: target.state() if target.state()['pad']['left_up'] == before['left_up'] + 2 else None)
    require(after['pad']['double_down'] == before['double_down'] + 1, after['pad'])
    evidence['visual_independent_state'] = after


def ownership(clients, fixtures, evidence):
    for fixture in fixtures: fixture.command('reset')
    barrier = threading.Barrier(3)
    def task(index):
        mcp, fixture = clients[index], fixtures[index]
        marker = ('OWNER-A-' + 'a'*80) if index == 0 else ('OWNER-B-' + 'b'*80)
        source = SELECT + f"""
await app.focus({{window_ref}});await app.keypress({{window_ref,keys:['META','A']}});
await app.typeText({{window_ref,text:{json.dumps(marker)}}});
return await app.keypress({{window_ref,keys:['META','S']}});
"""
        barrier.wait(timeout=5); started = time.monotonic()
        record, _ = mcp.execute(fixture.app_id, 'concurrent_owner', source)
        finished = time.monotonic()
        return {'started': started, 'finished': finished, 'marker': marker, 'record': record,
                'state': fixture.state(), 'server_pid': mcp.child.pid}
    with ThreadPoolExecutor(max_workers=2) as pool:
        jobs = [pool.submit(task, index) for index in range(2)]
        barrier.wait(timeout=5)
        results = [job.result(timeout=20) for job in jobs]
    evidence['concurrent_owners'] = results
    for index, result in enumerate(results):
        successful(result['record'])
        result['state'] = saved_equals(fixtures[index], result['marker'])
    require(max(r['started'] for r in results) < min(r['finished'] for r in results), 'Independent MCP calls did not overlap')
    require(results[0]['server_pid'] != results[1]['server_pid'], 'Not independent MCP owners')
    for index, fixture in enumerate(fixtures):
        require(fixture.state()['text'] == results[index]['marker'], 'Foreign text entered an owned document')
        proof = {}; evidence[f'owner_{index}_key_pairs'] = proof
        event_pairs(fixture, proof)


def surface(mcp, target, evidence):
    evidence['surface_setup_command'] = target.command('front')
    evidence['surface_setup'] = wait_until(lambda: target.state() if target.state()['active'] and
        target.state()['key_window'] == target.state()['windows'][0]['id'] else None)
    if 'double_click' in evidence:
        setup = evidence['surface_setup']
        require(all(abs(a-b) <= 2 for a,b in zip(setup['pointer_screen'], setup['pad_screen_point'])),
                'Full matrix must retain its earlier click cursor over the target pad')
        evidence['surface_cursor_already_on_target'] = True
    target.command('ax_empty')
    before = target.state()['pad']['left_up']
    point = capture(mcp, target, 'ax_empty_capture', evidence, focus=False)
    value, _ = execute(mcp, target, 'ax_empty_semantic_negative', SELECT + "try {await app.observe({window_ref});return {rejected:false};}catch(e){return {rejected:true,code:e.code};}", evidence)
    require(value['rejected'] and value.get('code'), value)
    click(mcp, target, 'ax_empty_visible_click', point, 'left', 1, evidence)
    evidence['ax_empty_independent_state'] = wait_until(lambda: target.state() if target.state()['pad']['left_up'] == before + 1 else None)
    require(evidence['ax_empty_independent_state']['ax_hidden'], 'Fixture did not keep AXWindows explicitly empty')
    target.command('ax_restore')


def double_intercept(mcp, target, evidence):
    point = capture(mcp, target, 'intercept_capture', evidence)
    before = target.state()['pad']['left_down']
    evidence['intercept_setup'] = target.command('cover_after_first')
    require(evidence['intercept_setup']['cover_after_first'], 'Overlay fixture was not armed')
    x, y = point
    source = f"return await app.click({{window_ref:state.window,observation_ref:state.shot.observation_ref,x:{x},y:{y},button:'left',count:2}});"
    record, _ = mcp.execute(target.app_id, 'intercepted_double', source)
    evidence['intercepted_double'] = record
    evidence['intercept_observed_state'] = target.state()
    require(record.get('error'), 'Second click was not rejected after the first opened an overlay')
    steps = [s for s in record['script']['steps'] if s['operation'] == 'click']
    require(len(steps) == 1 and steps[0]['receipt']['outcome']['dispatch'] == 'unknown', record)
    require(steps[0]['receipt']['provider_outcome']['dispatch'] == 'unknown', record)
    evidence['intercepted_double_release'] = {}
    state = released(target, evidence['intercepted_double_release'])
    require(state['pad']['left_down'] == before + 1, state)
    require(state['pad'].get('overlay_left_down', 0) == 0, 'Second down reached the new overlay')
    evidence['intercepted_double_independent_state'] = state


def begin_inflight_input(mcp, target, peer, evidence, case, prefix):
    proof = evidence.setdefault(case, {})
    proof.update(replayed=False, dispatch_attempts=0, release={})
    proof['reset'] = target.command('reset')
    require(proof['reset']['text'] == '', 'Owned fixture did not reset before input; no retry')
    proof['peer_before'] = peer.state()
    payload = prefix + 'q'*1000
    proof['payload_characters'] = len(payload)
    proof['payload_sha256'] = hashlib.sha256(payload.encode()).hexdigest()
    source = SELECT + f"await app.focus({{window_ref}});return await app.typeText({{window_ref,text:{json.dumps(payload)}}});"
    proof['dispatch_attempts'] = 1
    rpc_id = mcp.begin('tools/call', {'name':'exec','arguments':{'app':target.app_id,'source':source}})
    proof['rpc_id'] = rpc_id
    def entered():
        state = target.state()
        return state if len(state['text']) >= 12 else None
    proof['entered'] = wait_until(entered, timeout=4)
    require(0 < len(proof['entered']['text']) < len(payload)
            and payload.startswith(proof['entered']['text']), 'Sample missed in-flight Unicode input; no retry')
    proof['running'] = mcp.receipt(target.app_id, case+'_original')
    proof['execution_id'] = proof['running']['execution_id']
    require(proof['running']['status'] == 'running', 'Input completed before interruption; no retry')
    return proof, payload


def cancel(mcp, target, peer, evidence):
    proof, payload = begin_inflight_input(mcp, target, peer, evidence, 'cancellation', 'CANCEL-')
    rpc_id = proof['rpc_id']
    mcp.notify('notifications/cancelled', {'requestId':rpc_id,'reason':'Owned in-flight Unicode cancellation sample'})
    def terminal():
        record = mcp.receipt(target.app_id, 'cancellation_original')
        return record if record.get('status') == 'completed' else None
    record = wait_until(terminal, timeout=5)
    proof['record'] = record
    require(record.get('error') and record['script']['failure']['kind'] == 'cancelled', record)
    steps = [s for s in record['script']['steps'] if s['operation'] == 'typeText']
    require(len(steps) == 1 and steps[0]['receipt']['outcome']['dispatch'] == 'unknown', record)
    require(steps[0]['receipt'].get('provider_outcome') is not None, 'Original native evidence lost')
    after = released(target, proof['release'])
    proof['after'] = after
    require(0 < len(after['text']) < len(payload) and payload.startswith(after['text']),
            'Cancelled input was changed, replayed or sample missed')
    require(peer.state()['text'] == proof['peer_before']['text'], 'Cancelled input reached the other app')
    require(mcp.receipt(target.app_id, 'cancellation_original') == record, 'Terminal cancellation receipt changed')


def recorded_workers(mcp):
    workers = {pid:command for pid, (parent, command) in process_table().items()
               if parent == mcp.child.pid and any(flag in command for flag in
                    ('--dtw-native-worker', '--dtw-scene-worker'))}
    require(workers, 'No owned native worker recorded during in-flight input')
    mcp.probe_native_workers = workers
    return [{'pid':pid, 'parent_pid':mcp.child.pid, 'command':command,
             'returncode':None, 'exit_status':'unavailable'} for pid, command in workers.items()]


def close_mcp(mcp):
    # Reuse normal parent reaping, reader joining and transcript cleanup. Keep
    # the result if a subsequent orphan-worker check fails; finally rechecks it.
    result = getattr(mcp, 'probe_close_result', None)
    if result is None:
        result = mcp.close()
        mcp.probe_close_result = result
    workers = getattr(mcp, 'probe_native_workers', {})
    if workers:
        proof = result.setdefault('recorded_worker_exit', {'exited':False, 'polls':0,
            'exit_status':'unavailable', 'meaning':'PID/command disappearance does not establish exit code 0'})
        def exited():
            current = process_table()
            proof['polls'] += 1
            proof['remaining'] = [pid for pid, command in workers.items()
                                  if current.get(pid, (None, None))[1] == command]
            proof['exited'] = not proof['remaining']
            return proof['exited']
        wait_until(exited, timeout=5)
        if getattr(mcp, 'probe_require_stderr_diagnostics', False):
            path = mcp.output/'dtw-stderr.log'
            data = path.read_bytes()
            matches = [index for index, line in enumerate(data.splitlines(), 1)
                       if line == NATIVE_CLEANUP_FAILURE]
            result['native_worker_stderr'] = {
                'path':str(path), 'trace_enabled':True, 'checked_after_recorded_workers_exited':True,
                'bytes':len(data), 'sha256':hashlib.sha256(data).hexdigest(),
                'cleanup_failure_marker_lines':matches,
                'meaning':'Marker absence is not a worker exit status or proof of exit code 0'}
            require(not matches, 'Native worker reported incomplete input cleanup before shutdown')
    return result


def check_cleanup(resource, result):
    require(not result.get('forced'), result)
    expected = getattr(resource, 'probe_expected_exit', None)
    if expected is not None:
        require(expected['sent'] and resource.child is resource.probe_expected_exit_child
                and result.get('pid') == expected['pid'], 'Expected exit does not belong to this exact child')
    require(result.get('returncode', 0) == (expected['returncode'] if expected else 0), result)
    if isinstance(resource, Mcp):
        require(result.get('native_workers_reaped') is True, 'Normal MCP cleanup did not prove worker exit')
    if getattr(resource, 'probe_native_workers', None):
        require(result.get('recorded_worker_exit', {}).get('exited') is True,
                'Recorded native workers did not exit')


def retained_exec_response(mcp, proof):
    # The connection is already closed: never invent a final ledger or query a
    # replacement server. A response already recorded for the original RPC is
    # retained separately from the unavailable full ledger.
    final = {'execution_id':proof['execution_id'], 'ledger':'unavailable',
             'reason':'MCP transport closed; the original running receipt remains authoritative evidence',
             'response':None}
    proof['final_receipt'] = final
    responses = []
    for line in (mcp.output/'mcp.jsonl').read_text().splitlines():
        entry = json.loads(line)
        if entry.get('direction') == 'response' and entry.get('message', {}).get('id') == proof['rpc_id']:
            responses.append(entry['message'])
    if not responses:
        final['availability'] = 'unavailable'
        return
    final.update(availability='recorded_transport_response_only', response=responses[0])
    require(all(response == responses[0] for response in responses), 'Original exec response changed')
    response = responses[0]
    require(not response.get('error'), 'Original exec returned a JSON-RPC error during shutdown')
    record = mcp.decode(response['result']); final['record'] = record
    require(record.get('execution_id') == proof['execution_id'], 'Shutdown response changed execution identity')
    require(not record.get('shutdown_error') and not record.get('shutdown_errors')
            and not record.get('script', {}).get('shutdown_error'), 'Shutdown response contains cleanup failure')


def transport_exit(mcp, target, peer, evidence, case):
    require(case in ('eof', 'parent-exit'), 'Unknown transport interruption case')
    trace = os.environ.get('DTW_AX_READINESS_TRACE') == '1'
    evidence[case] = {'native_worker_stderr_trace':{'enabled':trace, 'environment':'DTW_AX_READINESS_TRACE=1'}}
    require(trace, 'Transport-exit cases require DTW_AX_READINESS_TRACE=1 before input dispatch')
    mcp.probe_require_stderr_diagnostics = True
    proof, payload = begin_inflight_input(mcp, target, peer, evidence, case, case.upper()+'-中文😀-')
    proof['native_workers'] = recorded_workers(mcp)
    require(mcp.child.poll() is None, 'Owned MCP exited before the intentional interruption')
    proof['expected_exit'] = {'pid':mcp.child.pid, 'reason':'stdin EOF' if case == 'eof' else 'intentional SIGTERM',
                              'returncode':0 if case == 'eof' else -signal.SIGTERM, 'sent':False}
    mcp.probe_expected_exit = proof['expected_exit']
    mcp.probe_expected_exit_child = mcp.child
    errors = []
    def check(phase, action):
        try: action()
        except BaseException as error:
            proof.setdefault('errors', []).append({'phase':phase, 'type':type(error).__name__, 'message':str(error)})
            errors.append(error)
    def interrupt():
        if case == 'eof': mcp.child.stdin.close()
        else: mcp.child.terminate()
        proof['expected_exit']['sent'] = True
    check('interrupt', interrupt)
    def closed():
        try:
            result = close_mcp(mcp)
            check_cleanup(mcp, result)
        finally:
            proof['mcp_close'] = getattr(mcp, 'probe_close_result', None)
    check('close', closed)
    check('original_response', lambda:retained_exec_response(mcp, proof))
    def independent_release():
        proof['after'] = released(target, proof['release'])
        proof['peer_after'] = peer.state()
        text = proof['after']['text']
        require(0 < len(text) < len(payload) and payload.startswith(text), 'Input completed, changed or replayed after transport exit')
        require(proof['peer_after']['text'] == proof['peer_before']['text'], 'Transport-exit input reached the peer app')
    check('independent_release', independent_release)
    if errors: raise errors[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', choices=['all','editing','scroll','visual','ownership','surface','cancel','double-intercept','eof','parent-exit'], default='all')
    parser.add_argument('--pointer-start-peer', action='store_true', help='Owned setup control: start the physical cursor over the peer app')
    args = parser.parse_args()
    require(sys.platform == 'darwin', 'Requires an exclusive real macOS GUI')
    if args.case in ('eof', 'parent-exit'):
        require(os.environ.get('DTW_AX_READINESS_TRACE') == '1',
                'Transport-exit cases require DTW_AX_READINESS_TRACE=1 before GUI setup')
    output = args.output.absolute(); output.mkdir(mode=0o700)
    binary = args.dtw.resolve(strict=True)
    source = Path(__file__).resolve().parents[1] / 'fixtures/native-input/main.swift'
    evidence = {'kind':'owned-AppKit-native-input-standard-MCP','live_llm_host':False,'case':args.case,
                'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
                'fixture_source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
                'started_unix_ms':int(time.time()*1000)}
    fixtures, clients = [], []; failure = None
    try:
        target = Fixture.prepare(output/'fixture', 'dev.dtw.input.'+uuid.uuid4().hex, source); fixtures.append(target)
        peer = Fixture.prepare(output/'peer', 'dev.dtw.input.peer.'+uuid.uuid4().hex, source, compiled=target.executable); fixtures.append(peer)
        evidence['before'] = [fixture.start() for fixture in fixtures]
        if args.pointer_start_peer:
            peer.command('park_pointer')
            b=peer.state()['window_bounds']
            evidence['pointer_setup'] = wait_until(lambda: peer.state() if
                abs(peer.state()['pointer_screen'][0]-(b[0]+b[2]/2)) <= 2 and
                abs(peer.state()['pointer_screen'][1]-(b[1]+b[3]/2)) <= 2 else None)
        require(all(input_is_released(state) for state in evidence['before']), 'A physical input was already held before the owned sample')
        for name in ['mcp-a','mcp-b']: clients.append(initialized(binary, output/name))
        selected = lambda name: args.case in ('all', name)
        if selected('editing'): edit(clients[0], target, peer, evidence)
        if args.case == 'scroll': scroll(clients[0], target, evidence)
        if selected('visual'): visual(clients[0], target, evidence)
        if selected('ownership'): ownership(clients, fixtures, evidence)
        if selected('surface'): surface(clients[0], target, evidence)
        if selected('cancel'): cancel(clients[0], target, peer, evidence)
        if selected('double-intercept'): double_intercept(clients[0], target, evidence)
        # These end a client deliberately and never run as part of the all case.
        if args.case in ('eof', 'parent-exit'): transport_exit(clients[0], target, peer, evidence, args.case)
        evidence['after'] = [fixture.state() for fixture in fixtures]
        evidence['key_pairs'] = []
        for fixture in fixtures:
            proof = {}; evidence['key_pairs'].append(proof)
            event_pairs(fixture, proof)
    except BaseException as error:
        failure = {'type':type(error).__name__,'message':str(error)}
        (output/'failure.txt').write_text(traceback.format_exc())
    finally:
        evidence['cleanup'] = []
        for name, resource in [(f'mcp-{i}', c) for i,c in enumerate(clients)] + [(f'fixture-{i}', f) for i,f in enumerate(fixtures)]:
            row = {'resource':name}; evidence['cleanup'].append(row)
            try:
                result = close_mcp(resource) if isinstance(resource, Mcp) else resource.close()
                row.update(result)
                check_cleanup(resource, result)
            except BaseException as error:
                row['error'] = {'type':type(error).__name__,'message':str(error)}
                retained = getattr(resource, 'probe_close_result', None)
                if retained is not None: row.update(retained)
                failure = failure or {'type':'CleanupFailure','message':str(error)}
        evidence['failure'] = failure; evidence['passed'] = failure is None
        evidence['finished_unix_ms'] = int(time.time()*1000)
        write_json(output/'evidence.json', evidence)
    print(json.dumps({'passed':evidence['passed'],'evidence':str(output/'evidence.json'),'failure':failure},ensure_ascii=False))
    return 0 if evidence['passed'] else 1


if __name__ == '__main__': sys.exit(main())
