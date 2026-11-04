#!/usr/bin/env python3
"""GUI acceptance for one durable apps/exec foreground scope.

No GUI work happens on import. Build the fixture, focus observer and optional
identity-checked actor before the parent's serial GUI run. All business input
uses current apps/exec; the actor only injects an owned environment activation.
"""
import argparse
import hashlib
import json
from pathlib import Path
import queue
import subprocess
import time
import traceback

from app_client import AppClient, artifact_identity
from current_task_metrics import check_effects
from computer_use_boundary_cases import gesture_counts
from computer_use_focus import (_fixture_class, _image_point, _number, _ready,
                                _require, _rows, _wait, evaluate_focus, exec_intervals)

TEXT = 'Scope resident 中文🙂 café\nSecond line — Ελληνικά\nThird line'
SENTINEL = {'text': 'witness sentinel', 'key_events': 0, 'flags_events': 0}
HERE = Path(__file__).resolve()
FIXTURES = HERE.parents[1] / 'fixtures'


def scope_source(hover, multiline, *, case='normal', trigger='hover', path=None, external_return=False):
    increment = "await app.click({selector:{role:'button',text:'Increment counter'}});"
    lines = [increment, increment]
    if trigger == 'hover':
        lines += ['await app.move(' + json.dumps(hover) + ');', increment,
                  'await app.click(' + json.dumps(multiline) + ');']
    # The keyboard variant starts with the fixture's multiline editor already
    # focused. It isolates keyboard borrowing from background point-focus behavior.
    if trigger == 'hover':
        lines += ["await app.keypress('META+A');"]
    lines += ['await app.typeText(' + json.dumps(TEXT) + ');']
    if trigger == 'keyboard':
        lines += [increment]
    if case == 'external':
        # Catch the drag error so semantic, point and keyboard input are really
        # attempted after loss; merely aborting at drag would not prove denial.
        lines += ['const denied = [];',
                  'try { await app.drag({path:' + json.dumps(path) + '}); } catch (e) { denied.push({op:"drag",code:e.code}); }']
        if external_return:
            # Read-only calls keep this same exec alive for the owned target
            # return. The actor timestamp must precede suffix_unix_ms below;
            # a slow injection fails coverage rather than being waived.
            lines += ['const until = Date.now() + 750; while (Date.now() < until) { await app.look(); }',
                      'await app.capture();']
        lines += ['const suffix_unix_ms = Date.now();']
        for name, operation in (('semantic', increment), ('point', 'await app.click(' + json.dumps(multiline) + ');'),
                                ('keyboard', "await app.keypress('ESC');"), ('text', 'await app.typeText("FORBIDDEN");')):
            lines += ['try { ' + operation + ' denied.push({op:' + json.dumps(name) + ',code:"UNEXPECTED_SUCCESS"}); }'
                      ' catch (e) { denied.push({op:' + json.dumps(name) + ',code:e.code}); }']
        lines += ['return {denied,suffix_unix_ms};']
    else:
        if case == 'cancel':
            lines += ['await app.drag({path:' + json.dumps(path) + '});']
        # These operations must remain inside the same durable scope after the
        # resident physical type_text. Cancellation must block this suffix.
        lines += [increment, "await app.keypress('ESC');",
                  "await app.look({text:'Multiline note'});", 'await app.capture({full:true});']
    return '\n'.join(lines)


def evaluate_scope_focus(rows, start, end, target, witness, *, case='normal', returned=False):
    """Require both event transitions and uninterrupted actual sample coverage."""
    failures = []
    result = {'passed': False, 'failures': failures, 'max_sample_gap_ms': None}
    if not (_number(start) and _number(end) and start <= end
            and type(target) is int and type(witness) is int and target > 0 and witness > 0 and target != witness):
        failures.append('invalid interval or owned identities'); return result
    if not isinstance(rows, list) or any(not isinstance(r, dict) or not _number(r.get('unix_ms'))
                                       or r.get('kind') not in ('sample', 'activation') for r in rows):
        failures.append('invalid observer rows'); return result
    if any(b['unix_ms'] < a['unix_ms'] for a, b in zip(rows, rows[1:])):
        failures.append('nonmonotonic observer evidence'); return result
    samples = [r for r in rows if r['kind'] == 'sample']
    before = [r for r in samples if r['unix_ms'] < start]
    after = [r for r in samples if r['unix_ms'] > end]
    if not before or not after:
        failures.append('missing bracketing samples'); return result
    left, right = before[-1]['unix_ms'], after[0]['unix_ms']
    covered = [r for r in rows if left <= r['unix_ms'] <= right]
    readings = [r for r in covered if r['kind'] == 'sample']
    gap = max((b['unix_ms'] - a['unix_ms'] for a, b in zip(readings, readings[1:])), default=0)
    result['max_sample_gap_ms'] = gap
    result['bracket_ms'] = [left, right]
    if gap > 50:
        failures.append('sample gap exceeds fixed 50ms limit')
    for row in covered:
        if row.get('frontmost_pid') not in (target, witness) or type(row.get('frontmost_pid')) is not int:
            failures.append({'unknown_or_foreign_actor': row})
        if row.get('error') or ('observed_frontmost_pid' in row and row['observed_frontmost_pid'] != row['frontmost_pid']):
            failures.append({'observer_error_or_disagreement': row})
    activations = [r for r in covered if r['kind'] == 'activation']
    expected = [target, witness] + ([target] if case == 'external' and returned else [])
    result['activations'] = activations
    if [r['frontmost_pid'] for r in activations] != expected:
        failures.append({'activation_sequence': [r['frontmost_pid'] for r in activations], 'expected': expected})
    sequence = []
    for row in readings:
        if not sequence or sequence[-1] != row['frontmost_pid']:
            sequence.append(row['frontmost_pid'])
    result['sample_sequence'] = sequence
    if sequence != [witness, *expected]:
        failures.append({'sample_sequence': sequence, 'expected': [witness, *expected]})
    result['passed'] = not failures
    return result


def async_outcome(client, request, source, timeout=8):
    """Retain the actual reply, error or absence; never synthesize a clean receipt."""
    started = time.monotonic()
    try:
        frame = client.transport.receive(request, timeout=timeout)
        outcome = {'outcome': 'response', 'frame': frame}
    except (TimeoutError, queue.Empty, EOFError) as error:
        frame = None
        outcome = {'outcome': 'no-response', 'error': type(error).__name__,
                   'observed_until_unix_ms': time.time() * 1000}
    envelope = (frame or {}).get('result', {})
    data = envelope.get('structuredContent', {})
    error = data.get('failure') or data.get('error') or (frame or {}).get('error')
    effects = data.get('effects', {}) if frame else {'dispatch': {'unknown': 1}, 'interrupted': 1}
    effects_error = None
    try:
        check_effects(effects)
        check_effects(effects.get('focus_preparation', {}))
    except AssertionError as exception:
        effects_error = str(exception)
    is_error = bool(not frame or envelope.get('isError') or error or effects_error
                    or data.get('status') != 'completed' or data.get('shutdown_error') or data.get('shutdown_errors'))
    metric = {'tool': 'exec', 'execution_id': data.get('execution_id') or 'async-request:' + str(request),
              'provider_execution_id': data.get('execution_id'), 'async_request_id': request,
              'is_error': is_error, 'error_code': (error or {}).get('code') if frame else 'async_outcome_unknown',
              'effects': effects, 'effects_error': effects_error, 'seconds': time.monotonic() - started,
              'source_bytes': len(source.encode()), 'json_bytes': len(json.dumps(outcome)),
              'image_count': sum(c.get('type') == 'image' for c in envelope.get('content', []))}
    client.metrics.append(metric)
    return {**outcome, 'metric': metric, 'data': data}


def _activate(actor, fixture, output, label):
    state = fixture.state()
    identity = [str(state['pid']), str(state['process_birth']), str(fixture.bundle.resolve())]
    completed = subprocess.run([str(actor), *identity], capture_output=True, text=True, timeout=5)
    record = {'argv': [str(actor), *identity], 'returncode': completed.returncode,
              'stdout': completed.stdout, 'stderr': completed.stderr}
    (output / (label + '.json')).write_text(json.dumps(record, indent=2) + '\n')
    _require(completed.returncode == 0, record)
    rows = [json.loads(line) for line in completed.stdout.splitlines()]
    _require(len(rows) == 1 and rows[0]['pid'] == state['pid']
             and rows[0]['process_birth'] == state['process_birth'], record)
    return rows[0]


def _partial_drag(rows, terminal, full_samples):
    counts = gesture_counts(rows)
    _require(counts['down'] == counts['up'] == 1 and 0 < counts['drag'] < full_samples,
             {'reason': 'not a real partial drag with paired release', 'counts': counts})
    observed = [r for r in rows if r.get('phase') in ('drag', 'up')]
    for row in observed:
        _require(_number(row.get('screen_x')) and _number(row.get('screen_y')), row)
        _require(abs(row['screen_x'] - terminal[0]) > 1 or abs(row['screen_y'] - terminal[1]) > 1,
                 {'reason': 'terminal sentinel reached', 'row': row})
    return {'counts': counts, 'terminal_screen': terminal, 'full_path_samples': full_samples,
            'terminal_reached': False, 'events': rows}


def run_scope_contract(target, binary, observer_binary, output, *, case='normal', actor=None,
                       trigger='hover', external_return=False):
    _require(case in ('normal', 'external', 'cancel'), 'unknown case')
    _require(trigger in ('hover', 'keyboard'), 'unknown trigger')
    _require(case != 'external' or actor is not None, 'external requires identity-checked actor binary')
    _require(not external_return or case == 'external', 'external-return requires external case')
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    proof = {'schema_version': 1, 'case': case, 'trigger': trigger, 'contract_pass': False,
             'passed': False, 'clean_run': False, 'external_return': external_return,
             'artifacts': {'focus': 'focus.jsonl', 'transcript': 'mcp/mcp.jsonl',
                           'target_journal': str(target.output / 'native-events.jsonl')},
             'cleanup': {'passed': False, 'errors': []}}
    witness = client = observer = observer_log = None
    request = source = None
    outcome = None
    try:
        _require(target.state()['mode'] == 'native', 'target must be native')
        witness = _fixture_class()(output / 'witness', 'scope-witness')
        witness.prepare(activate=True, role='witness', compiled_native=target.executable)
        witness.start()
        wp, tp = witness.child.pid, target.state()['pid']
        _wait(lambda: witness.state()['environment']['frontmost_pid'] == wp, timeout=5)
        _require(witness.state()['witness'] == SENTINEL, witness.state())
        proof.update(witness_pid=wp, target_pid=tp,
                     target_identity={'pid': tp, 'process_birth': target.state()['process_birth'],
                                      'bundle': str(target.bundle.resolve())})
        observer_log = (output / 'observer-stderr.log').open('wb')
        observer = subprocess.Popen([str(observer_binary), str(output / 'focus.jsonl')],
                                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=observer_log)
        _ready(observer)
        client = AppClient(binary, output / 'mcp')
        captured = client.capture(target.bundle, 'scope-geometry')
        _require(witness.state()['environment']['frontmost_pid'] == wp, 'geometry capture moved focus')
        state = target.state()
        if trigger == 'keyboard':
            _require(state['native'].get('multiline_focused') is True,
                     'Keyboard borrow fixture must start in its multiline editor')
        hover = _image_point(captured['view'], state['anchors']['gesture-desk'])
        multiline = _image_point(captured['view'], state['anchors']['native-multiline-note'])
        points = [{'x': hover['x'] + i, 'y': hover['y']} for i in range(64)]
        _require(points[-1]['x'] < captured['view']['image']['width'], 'drag outside capture')
        left, top, width, height = captured['view']['image']['bounds']
        terminal = [left + points[-1]['x'] * width / captured['view']['image']['width'],
                    top + points[-1]['y'] * height / captured['view']['image']['height']]
        before = state['native']['counter']
        expected_text = (state['native']['multiline_note'] if trigger == 'keyboard' else '') + TEXT
        proof['expected_text'] = expected_text
        journal = target.output / 'native-events.jsonl'
        offset = len(_rows(journal))
        source = scope_source(hover, multiline, case=case, trigger=trigger, path=points,
                              external_return=external_return)
        (output / 'scope.js').write_text(source + '\n')
        request = client.begin_execute(target.bundle, source)
        proof['request_id'] = request
        if case != 'normal':
            def held():
                rows = [r for r in _rows(journal)[offset:] if r.get('kind') == 'gesture']
                counts = gesture_counts(rows)
                _require(counts['up'] == 0, {'reason': 'drag completed before interruption', 'events': rows})
                return counts if counts['down'] == 1 and counts['drag'] > 0 else None
            proof['held_at_interruption'] = _wait(held, timeout=8)
            _require(target.state()['environment']['frontmost_pid'] == tp, 'borrow absent during held drag')
            if case == 'external':
                proof['injection'] = _activate(actor, witness, output, 'external-witness')
                _wait(lambda: witness.state()['environment']['frontmost_pid'] == wp)
                if external_return:
                    # Let the independent observer and resident scope record loss
                    # before returning. No business input is sent by this helper.
                    time.sleep(.06)
                    current_frames = _rows(output / 'mcp/mcp.jsonl')
                    _require(not any(r['direction'] == 'response' and r['message'].get('id') == request
                                     for r in current_frames), 'exec already replied before external target return')
                    proof['return_injection'] = _activate(actor, target, output, 'external-target')
                    _wait(lambda: target.state()['environment']['frontmost_pid'] == tp)
            else:
                proof['cancel_unix_ms'] = time.time() * 1000
                client.cancel(request)
        outcome = async_outcome(client, request, source)
        proof['execution'] = outcome
        if case == 'normal':
            _require(outcome['outcome'] == 'response' and not outcome['metric']['is_error'], outcome)
        else:
            _require(outcome['metric']['is_error'], 'interrupted execution falsely reported clean success')
        if case == 'cancel':
            deadline = time.monotonic() + 5
            while True:
                recovery, envelope = client.execute(target.bundle, 'await app.look();', allow_error=True)
                if recovery.get('error', {}).get('code') != 'busy':
                    break
                _require(time.monotonic() < deadline, 'cancelled execution never settled')
                time.sleep(.02)
            _require(envelope.get('isError') is not True and recovery.get('status') == 'completed'
                     and not recovery.get('error') and not recovery.get('failure'), recovery)
            proof['cleanup_read'] = recovery
        if case == 'external':
            denied = outcome['data'].get('result', {}).get('denied')
            proof['remaining_input_denials'] = denied
            _require(isinstance(denied, list) and [r.get('op') for r in denied] ==
                     ['drag', 'semantic', 'point', 'keyboard', 'text'], outcome)
            _require(all(r.get('code') == 'focus_changed' for r in denied), denied)
            effects = outcome['data'].get('effects', {})
            _require(type(effects.get('dispatch', {}).get('not_dispatched')) is int
                     and effects['dispatch']['not_dispatched'] >= 4, effects)
            if external_return:
                suffix_at = outcome['data']['result'].get('suffix_unix_ms')
                returned_at = proof['return_injection']['ended_unix_ms']
                _require(_number(suffix_at) and returned_at < suffix_at,
                         {'reason': 'target return did not precede suffix attempts',
                          'suffix_unix_ms': suffix_at, 'return_ended_unix_ms': returned_at})
                proof['return_before_suffix'] = True
        _wait(lambda: target.state()['pressed_mouse_buttons'] == 0, timeout=5)
        expected_front = tp if external_return else wp
        _wait(lambda: witness.state()['environment']['frontmost_pid'] == expected_front)
        # Independent sample after response and before any owned cleanup.
        proof['graded_end_unix_ms'] = time.time() * 1000
        time.sleep(.06)
        final = target.state()
        events = _rows(journal)[offset:]
        (output / 'target-events.json').write_text(json.dumps(events, ensure_ascii=False, indent=2) + '\n')
        increments = [r for r in events if r.get('kind') == 'increment']
        _require(len(increments) == (4 if case == 'normal' else 3), increments)
        _require(final['native']['counter'] == before + len(increments), final)
        _require(final['native']['multiline_note'] == expected_text, final['native'])
        _require(all(r.get('frontmost_pid') == wp for r in increments[:2]), increments[:2])
        _require(all(r.get('frontmost_pid') == tp for r in increments[2:]), increments[2:])
        _require(witness.state()['witness'] == SENTINEL, witness.state())
        if case != 'normal':
            proof['partial_drag'] = _partial_drag([r for r in events if r.get('kind') == 'gesture'], terminal, 63)
        frames = _rows(output / 'mcp/mcp.jsonl')
        intervals, failures = exec_intervals(frames)
        # A cancellation may have no reply. Keep that unknown in execution and
        # clean_run; independent focus/effect proof still has a bounded end.
        proof['exec_intervals'] = intervals
        req = next(r for r in frames if r['direction'] == 'request' and r['message'].get('id') == request)
        start = req['at_unix_ms']
        proof['wire_failures'] = failures
        _require(not failures or (case != 'normal' and outcome['outcome'] == 'no-response'
                                 and failures == [{'reason': 'missing_or_invalid_exec_response', 'id': request}]), failures)
        samples = _rows(output / 'focus.jsonl')
        proof['focus'] = evaluate_scope_focus(samples, start, proof['graded_end_unix_ms'], tp, wp,
                                               case=case, returned=external_return)
        geometry = next(r for r in intervals if r['request_id'] != request)
        proof['geometry_focus'] = evaluate_focus(samples, geometry['start_ms'], geometry['end_ms'], wp)
        _require(proof['geometry_focus']['passed'], proof['geometry_focus'])
        proof['background_prefix'] = evaluate_focus(samples, start, increments[1]['unix_ms'], wp)
        _require(proof['background_prefix']['passed'], proof['background_prefix'])
        _require(proof['focus']['passed'], proof['focus'])
        proof['counter'] = {'before': before, 'after': final['native']['counter'], 'events': increments}
        proof['exact_text'] = final['native']['multiline_note']
        proof['witness'] = witness.state()['witness']
        proof['final_frontmost_pid'] = witness.state()['environment']['frontmost_pid']
        proof['held_mouse_buttons'] = final['pressed_mouse_buttons']
        proof['contract_pass'] = True
        proof['clean_run'] = outcome['outcome'] == 'response' and not outcome['metric']['is_error']
    except Exception as error:
        proof['failure'] = {'type': type(error).__name__, 'message': str(error), 'traceback': traceback.format_exc()}
    finally:
        cleanup = proof['cleanup']
        cleanup['started_unix_ms'] = time.time() * 1000
        # Preserve an in-flight failure's eventual original reply/absence too.
        if client is not None and request is not None and outcome is None:
            try:
                proof['execution'] = async_outcome(client, request, source, timeout=1)
            except Exception as error:
                cleanup['errors'].append('async evidence: ' + str(error))
        if observer is not None:
            try:
                observer.communicate(input=b'\n', timeout=3)
            except Exception as error:
                cleanup['errors'].append('observer: ' + str(error))
                observer.terminate()
                try:
                    observer.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    observer.kill(); observer.wait(timeout=2)
            cleanup['observer_returncode'] = observer.poll()
        if observer_log is not None:
            observer_log.close()
        for name, owned in (('client', client), ('witness', witness)):
            if owned is not None:
                try:
                    cleanup[name] = owned.close()
                except Exception as error:
                    cleanup['errors'].append(name + ': ' + str(error))
        cleanup['passed'] = (not cleanup['errors'] and cleanup.get('observer_returncode') == 0
                             and cleanup.get('witness', {}).get('passed') is True
                             and cleanup.get('client', {}).get('returncode') == 0
                             and not cleanup.get('client', {}).get('forced')
                             and cleanup.get('client', {}).get('native_workers_reaped') is True)
        proof['passed'] = proof['contract_pass'] and cleanup['passed']
        (output / 'report.json').write_text(json.dumps(proof, ensure_ascii=False, indent=2) + '\n')
    return proof


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--observer', type=Path, required=True)
    parser.add_argument('--actor', type=Path)
    parser.add_argument('--fixture-binary', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', choices=('normal', 'external', 'cancel'), default='normal')
    parser.add_argument('--trigger', choices=('hover', 'keyboard'), default='hover')
    parser.add_argument('--external-return', action='store_true')
    args = parser.parse_args()
    if args.case == 'external' and args.actor is None:
        parser.error('--case external requires --actor')
    if args.external_return and args.case != 'external':
        parser.error('--external-return requires --case external')
    args.output.mkdir(parents=True, exist_ok=False)
    target = _fixture_class()(args.output / 'target', 'scope-target')
    proof = {'passed': False, 'contract_pass': False, 'clean_run': False}
    try:
        proof.update(artifact_identity(args.dtw))
        identities = {'harness': HERE, 'observer_binary': args.observer,
                      'observer_source': FIXTURES / 'focus-observer/main.swift',
                      'actor_source': FIXTURES / 'focus-scope-actor.swift'}
        if args.actor:
            identities['actor_binary'] = args.actor
        proof['sha256'] = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in identities.items()}
        proof['helper_source_sha256'] = {name: hashlib.sha256((HERE.parent / name).read_bytes()).hexdigest()
                                        for name in ('app_client.py', 'computer_use_focus.py',
                                                     'computer_use_boundary_cases.py', 'current_task_metrics.py',
                                                     'standard_agent_probe.py')}
        target.prepare(activate=False, compiled_native=args.fixture_binary)
        if args.trigger == 'keyboard':
            config_path = target.output / 'config.json'
            config = json.loads(config_path.read_text())
            config['initial_multiline_focus'] = True
            config_path.write_text(json.dumps(config, indent=2) + '\n')
        proof['initial_multiline_focus'] = args.trigger == 'keyboard'
        proof['fixture_source_sha256'] = json.loads((target.output / 'fixture-source.json').read_text())
        proof['fixture_binary_sha256'] = hashlib.sha256(target.executable.read_bytes()).hexdigest()
        target.start()
        proof.update(run_scope_contract(target, args.dtw, args.observer, args.output / 'scope',
                                        case=args.case, actor=args.actor, trigger=args.trigger,
                                        external_return=args.external_return))
    except Exception as error:
        proof['failure'] = {'type': type(error).__name__, 'message': str(error), 'traceback': traceback.format_exc()}
    finally:
        try:
            proof['target_cleanup'] = target.close()
        except Exception as error:
            proof['target_cleanup'] = {'passed': False, 'error': str(error)}
        proof['passed'] = proof.get('passed') is True and proof['target_cleanup']['passed']
        (args.output / 'report.json').write_text(json.dumps(proof, ensure_ascii=False, indent=2) + '\n')
    # Contract acceptance is distinct from a clean uninterrupted execution.
    return 0 if proof['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
