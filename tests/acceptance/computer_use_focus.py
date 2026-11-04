#!/usr/bin/env python3
"""Current MCP background contract and continuous, independently sampled focus proof.

Only run_background_contract/CLI starts owned GUI fixtures. Evaluators are pure.
Supervisor anchors supply geometry; this is direct-script integration evidence.
"""
import argparse
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import select
import subprocess
import time
import traceback

from app_client import AppClient, artifact_identity
from current_task_metrics import check_effects

NOTE = '后台 focus proof 中文🙂 café'
SENTINEL = 'witness sentinel'


def _number(value):
    return type(value) in (int, float) and math.isfinite(value)


def evaluate_focus(samples, start_ms, end_ms, expected_pid, maxgap=50):
    """Require bracketing samples, bounded actual gaps, and the exact witness PID.

    Activation rows retain their notification actor, including unknown actors.
    Nothing is sorted or interpolated: broken clocks/evidence fail explicitly.
    """
    failures = []
    result = {'passed': False, 'failures': failures, 'max_sample_gap_ms': None,
              'start_ms': start_ms, 'end_ms': end_ms, 'expected_pid': expected_pid}
    if (not _number(start_ms) or not _number(end_ms) or end_ms < start_ms
            or type(expected_pid) is not int or expected_pid <= 0
            or not _number(maxgap) or maxgap <= 0):
        failures.append({'reason': 'invalid_interval_or_expected_pid'})
        return result
    if not isinstance(samples, list):
        failures.append({'reason': 'invalid_samples'})
        return result
    previous = None
    for index, row in enumerate(samples):
        if (not isinstance(row, dict) or not _number(row.get('unix_ms'))
                or row.get('kind') not in ('sample', 'activation')):
            failures.append({'reason': 'invalid_row', 'index': index, 'row_repr': repr(row)})
            continue
        if previous is not None and row['unix_ms'] < previous:
            failures.append({'reason': 'nonmonotonic_evidence', 'index': index})
        previous = row['unix_ms']
    if failures:
        return result
    readings = [row for row in samples if row['kind'] == 'sample']
    before = [row for row in readings if row['unix_ms'] < start_ms]
    after = [row for row in readings if row['unix_ms'] > end_ms]
    if not before or not after:
        failures.append({'reason': 'missing_bracketing_sample',
                         'before': bool(before), 'after': bool(after)})
        return result
    left, right = before[-1]['unix_ms'], after[0]['unix_ms']
    covered = [row for row in readings if left <= row['unix_ms'] <= right]
    gaps = [b['unix_ms'] - a['unix_ms'] for a, b in zip(covered, covered[1:])]
    result['max_sample_gap_ms'] = max(gaps, default=0)
    result['sample_count'] = len(covered)
    result['bracket_ms'] = [left, right]
    if result['max_sample_gap_ms'] > maxgap:
        failures.append({'reason': 'sampling_gap', 'gap_ms': result['max_sample_gap_ms'], 'limit_ms': maxgap})
    for row in samples:
        if not left <= row['unix_ms'] <= right:
            continue
        pid = row.get('frontmost_pid')
        if type(pid) is not int or pid <= 0:
            failures.append({'reason': 'unknown_actor', 'row': row})
        elif pid != expected_pid:
            failures.append({'reason': 'foreign_foreground', 'row': row})
        if 'observed_frontmost_pid' in row and (type(row['observed_frontmost_pid']) is not int
                                               or row['observed_frontmost_pid'] != expected_pid):
            failures.append({'reason': 'activation_read_disagrees', 'row': row})
        if row.get('error'):
            failures.append({'reason': 'observer_read_error', 'row': row})
    result['passed'] = not failures
    return result


def exec_intervals(rows):
    """Pair original current exec request/response wire timestamps, including failures."""
    requests, responses, failures = {}, {}, []
    for index, row in enumerate(rows):
        message = row.get('message', {})
        if row.get('direction') == 'request' and message.get('method') == 'tools/call':
            params = message.get('params', {})
            if params.get('name') not in ('apps', 'exec'):
                failures.append({'reason': 'noncurrent_tool', 'index': index})
            if params.get('name') != 'exec':
                continue
            identity = message.get('id')
            if type(identity) not in (int, str) or identity in requests:
                failures.append({'reason': 'invalid_or_duplicate_exec_id', 'index': index})
                continue
            requests[identity] = row
        elif row.get('direction') == 'response' and 'id' in message:
            identity = message['id']
            if type(identity) not in (int, str):
                failures.append({'reason': 'invalid_response_id', 'index': index})
            elif identity in responses:
                failures.append({'reason': 'duplicate_response', 'id': identity})
            else:
                responses[identity] = row
    intervals = []
    for identity, row in requests.items():
        response = responses.get(identity)
        start, end = row.get('at_unix_ms'), response.get('at_unix_ms') if response else None
        if not _number(start) or not _number(end) or end < start:
            failures.append({'reason': 'missing_or_invalid_exec_response', 'id': identity})
            continue
        intervals.append({'request_id': identity, 'start_ms': start, 'end_ms': end,
                          'source': row['message']['params']['arguments'].get('source')})
    if not requests:
        failures.append({'reason': 'no_current_exec_intervals'})
    return intervals, failures


def evaluate_action(data, response=None, *, expected_rejection=None):
    """An expected policy rejection passes only its own case, never other cases."""
    response = response or {}
    error = data.get('error')
    effects = data.get('effects', {})
    effects_error = None
    try:
        check_effects(effects)
        preparation = effects.get('focus_preparation', {})
        check_effects(preparation)
        if any(preparation.get('dispatch', {}).values()):
            raise AssertionError('Background contract cannot attempt focus preparation')
    except AssertionError as exception:
        effects_error = str(exception)
    if expected_rejection is None:
        passed = (data.get('status') == 'completed' and not error
                  and not data.get('failure') and response.get('isError') is not True)
    else:
        dispatch = data.get('effects', {}).get('dispatch', {})
        passed = (data.get('status') == 'completed' and isinstance(error, dict)
                  and error.get('code') == expected_rejection
                  and type(dispatch.get('not_dispatched')) is int and dispatch['not_dispatched'] > 0
                  and all(type(count) is int and count == 0 for key, count in dispatch.items()
                          if key != 'not_dispatched'))
    return {'passed': passed and effects_error is None, 'expected_rejection': expected_rejection, 'error': error,
            'effects': data.get('effects'), 'effects_error': effects_error, 'execution_id': data.get('execution_id')}


def finalize_proof(proof):
    proof['passed'] = (not proof.get('failure') and bool(proof.get('cases'))
                       and all(row.get('passed') is True for row in proof['cases'])
                       and proof.get('focus', {}).get('passed') is True
                       and bool(proof['focus'].get('intervals'))
                       and proof.get('witness_no_input') is True
                       and proof.get('cleanup', {}).get('passed') is True)
    return proof


def _fixture_class():
    path = Path(__file__).resolve().parents[1] / 'fixtures/computer-use-app/fixture.py'
    spec = importlib.util.spec_from_file_location('computer_use_focus_fixture', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.ComputerUseFixture


def _wait(predicate, timeout=2):
    until = time.monotonic() + timeout
    while True:
        value = predicate()
        if value:
            return value
        if time.monotonic() >= until:
            raise TimeoutError('Read-only state verification did not settle')
        time.sleep(.02)


def _require(condition, detail):
    if not condition:
        raise AssertionError(detail)


def _rows(path):
    return [json.loads(line) for line in Path(path).read_text(encoding='utf-8').splitlines() if line.strip()]


def _ready(child):
    until, data = time.monotonic() + 5, b''
    while b'\n' not in data and len(data) < 1024:
        readable, _, _ = select.select([child.stdout], [], [], max(0, until - time.monotonic()))
        if not readable:
            raise TimeoutError('Focus observer ready timeout')
        chunk = os.read(child.stdout.fileno(), 1024 - len(data))
        if not chunk:
            raise RuntimeError('Focus observer exited before ready')
        data += chunk
    _require(data == b'ready\n', {'observer_ready': repr(data)})


def _image_point(view, screen):
    image = view['image']
    left, top, width, height = image['bounds']
    _require(all(_number(v) for v in [left, top, width, height, *screen]) and width > 0 and height > 0,
             'Invalid capture geometry')
    x = (screen[0] - left) * image['width'] / width
    y = (screen[1] - top) * image['height'] / height
    _require(0 <= x < image['width'] and 0 <= y < image['height'], 'Anchor outside target capture')
    return {'x': x, 'y': y}


def run_background_contract(target_fixture, binary, observer_binary, output: Path, *, case='focus', input_policy='auto'):
    if input_policy not in ('auto', 'background-only'):
        raise ValueError('Focus contract requires auto or background-only input policy')
    output = Path(output)
    output.mkdir(parents=True, exist_ok=False)
    proof = {'schema_version': 1, 'case': case, 'passed': False, 'cases': [],
             'evidence_class': 'direct-script/current-MCP/' + input_policy,
             'input_policy': input_policy,
             'artifacts': {'focus': 'focus.jsonl', 'transcript': 'mcp/mcp.jsonl'},
             'focus': {'passed': False, 'intervals': [], 'failures': [], 'max_sample_gap_ms': None},
             'witness_no_input': False, 'cleanup': {'passed': False, 'errors': []}}
    witness = client = observer = observer_log = None
    expected_pid = None
    focus_path, transcript = output / 'focus.jsonl', output / 'mcp/mcp.jsonl'
    try:
        _require(target_fixture.state()['mode'] == 'native', 'Target must already be in native mode')
        witness = _fixture_class()(output / 'witness', case + '-witness')
        witness.prepare(activate=True, role='witness', compiled_native=target_fixture.executable)
        witness.start()
        expected_pid = witness.child.pid
        _wait(lambda: witness.state()['environment']['frontmost_pid'] == expected_pid, timeout=5)
        initial_witness = witness.state()
        _require(initial_witness['witness'] == {'text': SENTINEL, 'key_events': 0, 'flags_events': 0}, initial_witness)
        proof['witness_pid'] = expected_pid
        proof['target_pid'] = target_fixture.state()['pid']
        observer_log = (output / 'observer-stderr.log').open('wb')
        observer = subprocess.Popen([str(observer_binary), str(focus_path)], stdin=subprocess.PIPE,
                                    stdout=subprocess.PIPE, stderr=observer_log)
        _ready(observer)
        client = AppClient(binary, output / 'mcp',
                           mcp_args=() if input_policy == 'auto' else ('--input-policy', input_policy))

        def step(name, source=None, *, capture=None, expected_rejection=None, verify=None):
            _require(witness.state()['environment']['frontmost_pid'] == expected_pid,
                     'Owned witness lost the foreground; stop without reactivating it or sending another action')
            row = {'name': name, 'passed': False}
            proof['cases'].append(row)
            if capture:
                data, response = client.capture(target_fixture.bundle, capture), {}
            else:
                data, response = client.execute(target_fixture.bundle, source, allow_error=True)
            row.update(evaluate_action(data, response, expected_rejection=expected_rejection))
            row['response'] = data
            accepted, row['passed'] = row['passed'], False
            _require(accepted, row)
            if verify:
                row['verification'] = verify()
            row['passed'] = True
            return data

        before = target_fixture.state()['native']['counter']
        proof['counter_before'] = before
        step('known_counter_without_look', "await app.click({selector:{role:'button',text:'Increment counter'}});",
             verify=lambda: _wait(lambda: target_fixture.state()['native']['counter'] == before + 1))
        note_before = target_fixture.state()['native']['note']
        proof['native_note_before'] = note_before
        captured = step('capture_for_geometry', capture='native-note')
        point = _image_point(captured['view'], target_fixture.state()['anchors']['native-note'])
        step('background_coordinate_click', 'await app.click(' + json.dumps(point) + ');')
        selection = {'location': 0, 'length': len(note_before.encode('utf-16-le')) // 2, 'unit': 'utf16'}
        step('select_all_separate_keypress', "await app.keypress('META+A');",
             verify=lambda: _wait(lambda: target_fixture.state()['native'].get('note_selection') == selection))
        proof['expected_selection'] = selection
        proof['observed_selection'] = target_fixture.state()['native']['note_selection']
        step('exact_unicode_replace', 'await app.typeText(' + json.dumps(NOTE) + ');',
             verify=lambda: _wait(lambda: target_fixture.state()['native']['note'] == NOTE))
        step('readonly_look', "await app.look({role:'text_field',text:'Native note'});")
        captured = step('readonly_capture', capture='native-readonly')
        if input_policy == 'background-only':
            hover = _image_point(captured['view'], target_fixture.state()['anchors']['gesture-desk'])
            step('inactive_hover_rejected', 'await app.move(' + json.dumps(hover) + ');',
                 expected_rejection='foreground_required')
        # Automatic hover borrowing has its own exec-wide lifecycle acceptance
        # in computer_use_scope.py; this probe proves background-supported work.
        # Wait for independent publications and a post-response focus sample;
        # this performs no input and never retries a declined operation.
        time.sleep(.06)
        final = witness.state()
        proof['witness'] = final['witness']
        proof['witness_no_input'] = final['witness'] == {'text': SENTINEL, 'key_events': 0, 'flags_events': 0}
        _require(proof['witness_no_input'], final)
        proof['native_note_after'] = target_fixture.state()['native']['note']
        proof['counter_after'] = target_fixture.state()['native']['counter']
        _require(proof['native_note_after'] == NOTE, 'Replacement changed after read-only calls')
        _require(proof['counter_after'] == before + 1, 'Counter changed beyond one increment')
    except Exception as error:
        proof['failure'] = {'type': type(error).__name__, 'message': str(error), 'traceback': traceback.format_exc()}
    finally:
        # Observer, witness and client cleanup are independent, outside graded
        # intervals, and cannot overwrite the original contract failure.
        cleanup = proof['cleanup']
        if observer is not None:
            time.sleep(.06)  # Bracket the last response before cleanup changes focus.
        cleanup['started_unix_ms'] = time.time() * 1000
        if witness is not None:
            try:
                proof['witness'] = (witness.state() or {}).get('witness')
                proof['witness_no_input'] = proof['witness'] == {'text': SENTINEL, 'key_events': 0, 'flags_events': 0}
            except Exception as error:
                cleanup['errors'].append('witness state: ' + str(error))
            try:
                cleanup['witness'] = witness.close()
            except Exception as error:
                cleanup['errors'].append('witness: ' + str(error))
        if client is not None:
            try:
                cleanup['client'] = client.close()
            except Exception as error:
                cleanup['errors'].append('client: ' + str(error))
        if observer is not None:
            record = {'forced': []}
            try:
                observer.communicate(input=b'\n', timeout=3)
            except Exception as error:
                cleanup['errors'].append('observer: ' + str(error))
                try:
                    if observer.poll() is None:
                        record['forced'].append('terminate')
                        observer.terminate()
                        try:
                            observer.wait(timeout=2)
                        except subprocess.TimeoutExpired:
                            record['forced'].append('kill'); observer.kill(); observer.wait(timeout=2)
                except Exception as stop_error:
                    cleanup['errors'].append('observer forced cleanup: ' + str(stop_error))
            finally:
                record['returncode'] = observer.poll()
                cleanup['observer'] = record
                for stream in (observer.stdin, observer.stdout):
                    if stream is not None:
                        try:
                            stream.close()
                        except Exception as error:
                            cleanup['errors'].append('observer pipe: ' + str(error))
        if observer_log is not None:
            try:
                observer_log.close()
            except Exception as error:
                cleanup['errors'].append('observer log: ' + str(error))
        cleanup['ended_unix_ms'] = time.time() * 1000
        cleanup['passed'] = (not cleanup['errors'] and cleanup.get('observer', {}).get('returncode') == 0
                             and not cleanup.get('observer', {}).get('forced')
                             and cleanup.get('witness', {}).get('passed') is True
                             and cleanup.get('client', {}).get('returncode') == 0
                             and not cleanup.get('client', {}).get('forced')
                             and cleanup.get('client', {}).get('native_workers_reaped') is True)
        try:
            intervals, failures = exec_intervals(_rows(transcript))
            samples = _rows(focus_path)
            cleanup_rows = [row for row in samples if _number(row.get('unix_ms'))
                            and row['unix_ms'] >= cleanup['started_unix_ms']]
            cleanup['focus_sample_count'] = sum(row.get('kind') == 'sample' for row in cleanup_rows)
            cleanup['focus_activations'] = [row for row in cleanup_rows if row.get('kind') == 'activation']
            focus = proof['focus']
            focus['failures'].extend(failures)
            for interval in intervals:
                grade = evaluate_focus(samples, interval['start_ms'], interval['end_ms'], expected_pid)
                focus['intervals'].append({**interval, **grade})
            if intervals:
                span = evaluate_focus(samples, intervals[0]['start_ms'], intervals[-1]['end_ms'], expected_pid)
                focus['span'] = span
                focus['max_sample_gap_ms'] = span['max_sample_gap_ms']
                focus['failures'].extend(span['failures'])
            focus['passed'] = bool(intervals) and not focus['failures'] and all(row['passed'] for row in focus['intervals'])
        except Exception as error:
            proof['focus']['failures'].append({'reason': 'focus_evidence_unavailable', 'error': str(error)})
        finalize_proof(proof)
        (output / 'report.json').write_text(json.dumps(proof, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    return proof


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--observer', type=Path, required=True)
    parser.add_argument('--input-policy', choices=('auto', 'background-only'), default='auto')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    target = _fixture_class()(args.output / 'target', 'focus-target')
    proof = {'passed': False}
    try:
        target.prepare(activate=False).start()
        proof = run_background_contract(target, args.dtw, args.observer, args.output / 'background',
                                        input_policy=args.input_policy)
        proof.update(artifact_identity(args.dtw))
        proof['helper_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        proof['observer_sha256'] = hashlib.sha256(args.observer.read_bytes()).hexdigest()
    except Exception as error:
        proof['failure'] = {'type': type(error).__name__, 'message': str(error)}
    finally:
        try:
            proof['target_cleanup'] = target.close()
            proof['passed'] = proof.get('passed') is True and proof['target_cleanup']['passed']
        except Exception as error:
            proof['passed'] = False; proof['target_cleanup'] = {'passed': False, 'error': str(error)}
        (args.output / 'report.json').write_text(json.dumps(proof, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    return 0 if proof['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
