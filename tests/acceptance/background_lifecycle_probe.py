#!/usr/bin/env python3
"""Opt-in background input cancellation and owner-exit acceptance via stdio MCP.

DTW_AX_READINESS_TRACE=1 python3 tests/acceptance/background_lifecycle_probe.py \
  --dtw /path/to/signed/dtw --output /tmp/new-run --case all

Each case uses a fresh MCP process and reset owned AppKit fixtures. After actual
partial Unicode input, cancel the original request, close its stdin, or terminate
that exact MCP parent. The optional --foreground case uses one control-bearing
request to select automatic foreground fallback and verifies exact original-window
return. No explicit focus, pointer move, input replay or global user key release
is issued by the probe. Owned native events and partial text verify
effects; global activity is observation only. Closed transports have no invented
final ledger, and worker PID disappearance does not prove its exit code was zero.
This is native fixture/MCP evidence, not arbitrary-app or external-Agent coverage.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import sys
import time
import traceback
from types import SimpleNamespace
import uuid

from background_input_probe import front, fresh_end, journal, observe_desktop, observed_pairs, reset, select, require_exact_return
from native_input_probe import check_cleanup, close_mcp, recorded_workers, retained_exec_response
from standard_agent_probe import Fixture, Mcp, require, wait_until, write_json

CASES = ('cancel', 'eof', 'parent-exit')


def check_cancel_record(record, execution_id):
    require(record.get('execution_id') == execution_id and record.get('status') == 'completed',
            'Cancellation lost the original execution identity or terminal record')
    script = record.get('script', {})
    require(record.get('error') and script.get('failure', {}).get('kind') == 'cancelled', record)
    require(script.get('state_committed') is False and not script.get('shutdown_error'), record)
    steps = script.get('steps', [])
    typed = [step for step in steps if step['operation'] == 'typeText']
    require(len(typed) == 1 and not any(step['operation'] == 'keypress' for step in steps),
            'An unexecuted suffix ran, or the original input ledger was lost')
    receipt = typed[0].get('receipt') or {}
    require(receipt.get('outcome', {}).get('dispatch') == 'unknown',
            'Partially delivered cancellation must retain unknown dispatch')
    require(receipt.get('provider_outcome') is not None, 'Original provider evidence was discarded')


def check_effects(proof, payload, tag, suffix):
    after = proof['after']
    text = after['text']
    require(0 < len(text) < len(payload) and payload.startswith(text),
            'Stopped input is not a partial prefix of the original payload; never replay')
    require(len(text) >= len(proof['entered']['text']), 'The observed original prefix disappeared')
    require(suffix not in text and after['saves'] == proof['before']['saves'],
            'The unexecuted suffix or Save changed the target')
    events = proof['target_events']
    target_window = proof['target_window_id']
    key_events = [event for event in events if event['kind'] in ('key_down', 'key_up')]
    require(key_events and all(event['window_number'] == target_window for event in key_events),
            'Observed input reached a different owned target window')
    proof['paired_input'] = pairs = observed_pairs(key_events)
    require(pairs['down'].get('key:0', 0) > 0 and not pairs['unpaired'],
            {'unpaired_owned_target_input': pairs})
    delivered = ''.join(event['characters'] for event in key_events if event['kind'] == 'key_down')
    proof['observed_keydown_text'] = delivered
    require(delivered == text, 'Owned key-down journal does not match the original partial text')
    peer = proof['peer_after']
    peer_text = '\n'.join([peer['text'], peer.get('saved', ''),
                          ''.join(event.get('characters', '') for event in proof['peer_events']
                                  if event['kind'] == 'key_down')])
    require(tag not in peer_text, 'The unique input payload reached the peer app')
    proof['peer_observations'] = {
        'text_changed': peer['text'] != proof['peer_before']['text'],
        'saves_changed': peer['saves'] != proof['peer_before']['saves'],
        'meaning': 'Unrelated user input is permitted; only the unique payload indicates misrouting'}


def check_single_dispatch(mcp, proof, source):
    requests = [row['message'] for row in journal(SimpleNamespace(root=mcp.output), 'mcp.jsonl')
                if row.get('direction') == 'request' and row['message'].get('method') == 'tools/call'
                and row['message'].get('params', {}).get('name') == 'exec']
    proof['exec_requests'] = len(requests)
    require(len(requests) == 1 and requests[0]['id'] == proof['rpc_id']
            and requests[0]['params']['arguments'] == {'app': proof['app'], 'source': source},
            'The original input was duplicated or changed')


def run_case(binary, directory, target, peer, case, proof, clients, foreground=False, explicit_focus=False):
    directory.mkdir()
    reset(target); reset(peer)
    proof['desktop_before'] = front(peer)
    proof['before'], proof['peer_before'] = target.state(), peer.state()
    windows = [w for w in proof['before']['windows'] if w['title'] == 'DTW Input Target']
    require(len(windows) == 1, 'Owned target window missing or not unique')
    proof['target_window_id'] = windows[0]['id']
    tag = uuid.uuid4().hex
    payload = 'BG-'+tag+'-中文😀|' + ''.join(f'{tag}:{index:04d}:中文😀|' for index in range(50))
    if foreground:
        # One trailing control scalar selects the entire foreground route before
        # any prefix. Cancellation still happens in ordinary Unicode text.
        payload += '\n'
    suffix = 'NEVER-'+tag
    source = select() + ("await app.focus({window_ref});" if explicit_focus else "") + f"""
const typed=await app.typeText({{window_ref,text:{json.dumps(payload)}}});
await app.typeText({{window_ref,text:{json.dumps(suffix)}}});
await app.keypress({{window_ref,keys:['META','S']}});return typed;
"""
    proof.update(app=target.app_id, payload_tag=tag, payload_characters=len(payload),
                 payload_sha256=hashlib.sha256(payload.encode()).hexdigest(), suffix=suffix,
                 replayed=False, dispatch_attempts=0, errors=[])
    mcp = Mcp(binary, directory); clients.append(mcp)
    mcp.probe_require_stderr_diagnostics = True
    errors = []
    def attempt(phase, action):
        try:
            return action()
        except BaseException as error:
            proof['errors'].append({'phase':phase, 'type':type(error).__name__,
                                    'message':str(error), 'traceback':traceback.format_exc()})
            errors.append(error)
            return None

    def start_and_interrupt():
        mcp.rpc('initialize', {'protocolVersion':'2025-06-18', 'capabilities':{},
                              'clientInfo':{'name':'background-lifecycle-probe', 'version':'1'}})
        mcp.notify('notifications/initialized')
        proof['dispatch_attempts'] = 1
        proof['rpc_id'] = mcp.begin('tools/call', {'name':'exec', 'arguments':{'app':target.app_id,'source':source}})
        def entered():
            state = target.state()
            return state if len(state['text']) >= len(tag)+10 else None
        proof['entered'] = wait_until(entered, timeout=7)
        text = proof['entered']['text']
        require(0 < len(text) < len(payload) and payload.startswith(text),
                'Did not observe an in-flight original prefix; sample will not be retried')
        proof['running'] = mcp.receipt(target.app_id, case+'_original')
        proof['execution_id'] = proof['running']['execution_id']
        require(proof['running']['status'] == 'running', 'Input ended before interruption; no retry')
        proof['native_workers'] = recorded_workers(mcp)
        require(mcp.child.poll() is None, 'Owned MCP exited before intentional interruption')
        proof['interrupted_unix_ms'] = int(time.time()*1000)
        if case == 'cancel':
            mcp.notify('notifications/cancelled', {'requestId':proof['rpc_id'],
                       'reason':'Owned background partial-Unicode cancellation sample'})
            def terminal():
                record = mcp.receipt(target.app_id, case+'_original')
                return record if record.get('status') == 'completed' else None
            proof['record'] = wait_until(terminal, timeout=8)
            check_cancel_record(proof['record'], proof['execution_id'])
            if explicit_focus:
                steps = proof['record']['script']['steps']
                focused = [step for step in steps if step['operation'] == 'focus']
                require(len(focused) == 1 and focused[0]['receipt']['provider_outcome']['delivery']['focus_return'] == 'pending',
                        'Cancellation rewrote the original explicit-focus receipt')
                require(proof['record']['script'].get('focus_return') == 'unconfirmed',
                        'Stopped focus owner lost its unconfirmed final return evidence')
            proof['retained_record'] = mcp.receipt(target.app_id, case+'_original')
            require(proof['retained_record'] == proof['record'], 'Original cancellation receipt changed')
        else:
            proof['expected_exit'] = {'pid':mcp.child.pid, 'reason':'stdin EOF' if case == 'eof' else 'intentional SIGTERM',
                                      'returncode':0 if case == 'eof' else -signal.SIGTERM, 'sent':False}
            mcp.probe_expected_exit = proof['expected_exit']
            mcp.probe_expected_exit_child = mcp.child
            if case == 'eof': mcp.child.stdin.close()
            else: mcp.child.terminate()
            proof['expected_exit']['sent'] = True
    attempt('start_and_interrupt', start_and_interrupt)

    def close_client():
        try:
            check_cleanup(mcp, close_mcp(mcp))
        finally:
            proof['mcp_close'] = getattr(mcp, 'probe_close_result', None)
    attempt('close', close_client)
    if 'execution_id' in proof and case != 'cancel':
        attempt('original_transport_response', lambda: retained_exec_response(mcp, proof))
    if 'rpc_id' in proof:
        attempt('single_dispatch', lambda: check_single_dispatch(mcp, proof, source))

    def independent_effects():
        proof['after_stop'] = fresh_end(target)
        proof['after'], proof['peer_after'] = fresh_end(target), fresh_end(peer)
        proof['target_events'] = [event for event in journal(target, 'events.jsonl')
            if event['sequence'] > proof['before']['events_count'] and event['uptime'] <= proof['after']['published_uptime']]
        proof['peer_events'] = [event for event in journal(peer, 'events.jsonl')
            if event['sequence'] > proof['peer_before']['events_count'] and event['uptime'] <= proof['peer_after']['published_uptime']]
        observe_desktop(peer, proof['desktop_before'], proof['peer_after'], proof.setdefault('desktop', {}),
                        forbidden_pids=() if foreground or explicit_focus else (target.child.pid,))
        if foreground or explicit_focus:
            require_exact_return(proof['peer_after'], peer.child.pid, proof['desktop_before']['key_window'])
            proof['exact_foreground_return_verified'] = True
        require(proof['after']['text'] == proof['after_stop']['text'], 'Target text continued changing after owner cleanup')
        if 'entered' in proof:
            check_effects(proof, payload, tag, suffix)
    attempt('independent_effects', independent_effects)
    proof['passed'] = not errors
    if errors: raise errors[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', choices=('all',)+CASES, default='all')
    parser.add_argument('--explicit-focus', action='store_true', help='Borrow foreground explicitly before background input and retain uncertain finalizer evidence on cancellation')
    parser.add_argument('--foreground', action='store_true', help='Exercise automatic foreground fallback and exact return during interruption')
    args = parser.parse_args()
    require(sys.platform == 'darwin', 'This probe requires real macOS')
    require(os.environ.get('DTW_AX_READINESS_TRACE') == '1',
            'Set DTW_AX_READINESS_TRACE=1 before setup so worker cleanup failures remain visible')
    binary = args.dtw.resolve(strict=True)
    output = args.output.absolute(); output.mkdir(mode=0o700, parents=True, exist_ok=False)
    source = Path(__file__).resolve().parents[1]/'fixtures/native-input/main.swift'
    evidence = {'kind':'owned-AppKit-background-lifecycle-standard-MCP', 'case':args.case,
                'foreground_fallback':args.foreground,'explicit_focus':args.explicit_focus,
                'live_llm_host':False, 'release_artifact':False, 'binary_path':str(binary),
                'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
                'fixture_source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
                'started_unix_ms':int(time.time()*1000), 'cases':{}, 'cleanup':[],
                'passed_scope':'partial input, cancellation/owner cleanup and no replay; background coverage is separate',
                'limitations':['Global pointer/foreground/input changes have no inferred actor',
                               'Worker PID/command disappearance does not establish exit code zero',
                               'A closed MCP transport cannot supply a final authoritative ledger']}
    fixtures, clients, failure = [], [], None
    try:
        for name, x in [('target',100), ('peer',700)]:
            fixture = Fixture.prepare(output/name, 'dev.dtw.bglifecycle.'+uuid.uuid4().hex, source,
                                      compiled=fixtures[0].executable if fixtures else None)
            fixtures.append(fixture)
            write_json(fixture.root/'fixture.json', {'background_probe':True, 'x':x})
        evidence['initial'] = [fixture.start() for fixture in fixtures]
        target, peer = fixtures
        for case in CASES:
            if args.case in ('all',case):
                proof = evidence['cases'].setdefault(case, {})
                run_case(binary, output/('mcp-'+case), target, peer, case, proof, clients, foreground=args.foreground, explicit_focus=args.explicit_focus)
        evidence['after'] = [fixture.state() for fixture in fixtures]
    except BaseException as error:
        failure = {'type':type(error).__name__, 'message':str(error), 'traceback':traceback.format_exc()}
    finally:
        for name, resource in [(f'mcp-{i}',c) for i,c in enumerate(clients)] + [(f'fixture-{i}',f) for i,f in enumerate(fixtures)]:
            row = {'resource':name}; evidence['cleanup'].append(row)
            try:
                result = close_mcp(resource) if isinstance(resource,Mcp) else resource.close()
                row.update(result); check_cleanup(resource,result)
            except BaseException as error:
                row.update(error={'type':type(error).__name__, 'message':str(error)},
                           retained=getattr(resource,'probe_close_result',None))
                failure = failure or {'type':'CleanupFailure','message':str(error)}
        evidence['binary_unchanged'] = hashlib.sha256(binary.read_bytes()).hexdigest() == evidence['binary_sha256']
        evidence.update(failure=failure, passed=failure is None and evidence['binary_unchanged'],
                        finished_unix_ms=int(time.time()*1000))
        write_json(output/'evidence.json',evidence)
    print(json.dumps({'passed':evidence['passed'], 'evidence':str(output/'evidence.json'), 'failure':failure},ensure_ascii=False))
    return 0 if evidence['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())
