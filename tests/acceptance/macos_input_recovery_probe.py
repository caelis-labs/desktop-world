#!/usr/bin/env python3
"""Serial owned-GUI regression for background input and app.focus/windowless recovery."""
import argparse
import json
from pathlib import Path
import subprocess
import time
import traceback

from app_client import AppClient, artifact_identity
from computer_use_focus import _fixture_class, _image_point, _ready, _rows, _wait, evaluate_focus
from computer_use_scope import evaluate_scope_focus


def require(condition, detail):
    if not condition:
        raise AssertionError(detail)


def run(args):
    root = args.output.resolve()
    root.mkdir(mode=0o700, exist_ok=False)
    proof = {'identity': artifact_identity(args.dtw), 'cases': {}, 'functional_pass': False,
             'geometry_source': 'Owned fixture anchors; not model vision evidence.'}
    target = _fixture_class()(root / 'target', 'input-recovery-target')
    cover = _fixture_class()(root / 'cover', 'input-recovery-cover')
    clients, client, observer, observer_log = [], None, None, None
    closed = {}

    def publish():
        (root / 'report.json').write_text(json.dumps(proof, ensure_ascii=False, indent=2))

    def connect(directory):
        value = AppClient(args.dtw, root / directory)
        clients.append(value)
        return value

    def close_client(value):
        if value not in closed:
            closed[value] = value.close()
        return closed[value]

    def foreground():
        return int(subprocess.check_output([str(args.observer), '--current'], text=True).strip())

    def step(name, source, *, borrowed=False, rejection=None, prefix_dispatches=0):
        start = time.time() * 1000
        data, frame = client.execute(target.bundle, source, allow_error=True)
        end = time.time() * 1000
        time.sleep(.2)
        row = {'source': source, 'reply': data, 'is_error': frame.get('isError', False),
               'start_ms': start, 'end_ms': end, 'borrowed': borrowed,
               'target': target.state(), 'cover': cover.state()}
        proof['cases'][name] = row
        publish()
        effects = data.get('effects', {})
        require(not effects.get('dispatch', {}).get('unknown') and not effects.get('interrupted')
                and not data.get('shutdown_error'), row)
        if rejection:
            require(data.get('error', {}).get('code') == rejection and frame.get('isError') is True, row)
            require(effects.get('dispatch', {}).get('not_dispatched') == 1
                    and effects.get('dispatch', {}).get('dispatched', 0) == prefix_dispatches, row)
        else:
            require(not data.get('error') and not frame.get('isError'), row)
        require(row['cover']['native']['counter'] == 0
                and row['cover']['native']['note'] == proof['baseline']['cover']['native']['note'], row)
        require(foreground() == cover.child.pid, row)
        return row

    try:
        # Both apps use normal make-key behavior for newly created windows. The
        # cover starts last and fully occludes the target before any measurement.
        for fixture, active in ((target, not args.new_window_no_key), (cover, True)):
            fixture.prepare(activate=active, compiled_native=args.fixture_binary)
            path = fixture.output / 'config.json'
            config = json.loads(path.read_text())
            config.update(window_x=100, window_y=150)
            path.write_text(json.dumps(config))
            fixture.start()
        time.sleep(.4)
        proof['baseline'] = {'target': target.state(), 'cover': cover.state()}
        _wait(lambda: foreground() == cover.child.pid, timeout=5)
        require(target.state()['windows'][0]['bounds'] == cover.state()['windows'][0]['bounds'],
                'The background target must be fully covered')
        observer_log = (root / 'observer-stderr.log').open('wb')
        observer = subprocess.Popen([str(args.observer), str(root / 'focus.jsonl')],
                                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=observer_log)
        _ready(observer)
        client = connect('mcp')
        view = client.capture(target.bundle, 'covered-target')['view']
        point = _image_point(view, target.state()['anchors']['Increment counter'])
        row = step('covered_coordinate_click', 'await app.click(' + json.dumps(point) + ');')
        require(row['target']['native']['counter'] == 1, row)
        row = step('background_plain_text', 'await app.typeText("bg-中文-proof");')
        require(row['target']['native']['note'] == 'bg-中文-proof', row)
        row = step('public_focus', 'return {focus:typeof app.focus,methods:Object.keys(app)};')
        require(row['reply']['result']['focus'] == 'function', row)
        view = client.capture(target.bundle, 'before-multiline')['view']
        point = _image_point(view, target.state()['anchors']['native-multiline-note'])
        step('background_multiline_click', 'await app.click(' + json.dumps(point) + ');')
        step('background_multiline_select_all', 'await app.keypress("META+A");')
        row = step('explicit_focus_and_input',
                   'await app.focus(); await app.typeText("focused-proof");', borrowed=True)
        require(row['target']['native']['multiline_note'] == 'focused-proof', row)
        row = step('close_owned_window', 'await app.keypress("META+W");', borrowed=True)
        require(not row['target']['windows'], row)
        close_client(client)
        client = connect('mcp-windowless')
        step('windowless_focus', 'await app.focus();', borrowed=True)
        row = step('windowless_new',
                   'await app.keypress("META+N"); await app.typeText("after-new-proof");', borrowed=True,
                   rejection='focus_unproven' if args.new_window_no_key else None,
                   prefix_dispatches=1 if args.new_window_no_key else 0)
        require(len(row['target']['windows']) == 1
                and row['target']['native']['peers'][0]['note'] == ('' if args.new_window_no_key else 'after-new-proof'), row)
        if args.new_window_no_key:
            row = step('explicit_recovery_after_unkey_window',
                       'await app.focus(); await app.typeText("explicit-recovery");', borrowed=True)
            require(row['target']['native']['peers'][0]['note'] == 'explicit-recovery', row)
        proof['functional_pass'] = True
    except BaseException as error:
        proof['failure'] = {'message': str(error), 'traceback': traceback.format_exc()}
    finally:
        cleanup = proof['cleanup'] = {'clients': []}
        for value in clients:
            cleanup['clients'].append(close_client(value))
        if observer:
            try:
                observer.communicate(input=b'\n', timeout=3)
            except subprocess.TimeoutExpired:
                observer.terminate()
                observer.wait(timeout=3)
                cleanup['observer_forced'] = True
            cleanup['observer_returncode'] = observer.returncode
        if observer_log:
            observer_log.close()
        samples = _rows(root / 'focus.jsonl') if (root / 'focus.jsonl').exists() else []
        for row in proof['cases'].values():
            row['focus'] = (evaluate_scope_focus(samples, row['start_ms'], row['end_ms'],
                                                target.child.pid, cover.child.pid)
                            if row['borrowed'] else
                            evaluate_focus(samples, row['start_ms'], row['end_ms'], cover.child.pid))
        cleanup['target'], cleanup['cover'] = target.close(), cover.close()
        cleanup['passed'] = (not cleanup.get('observer_forced') and cleanup.get('observer_returncode') == 0
                             and cleanup['target'].get('passed') and cleanup['cover'].get('passed')
                             and all(c.get('returncode') == 0 and not c.get('forced')
                                     and c.get('native_workers_reaped') for c in cleanup['clients']))
        proof['functional_pass'] = (proof['functional_pass'] and cleanup['passed']
                                    and all(r['focus']['behavior_passed'] is not False for r in proof['cases'].values()))
        proof['sampling_pass'] = bool(proof['cases']) and all(
            r['focus']['sampling_quality']['passed'] for r in proof['cases'].values())
        proof['passed'] = proof['functional_pass'] and proof['sampling_pass']
        proof['status'] = 'failed' if not proof['functional_pass'] else 'passed' if proof['passed'] else 'inconclusive'
        publish()
    return proof


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('dtw', 'fixture-binary', 'observer', 'output'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--new-window-no-key', action='store_true',
                        help='Negative control: refuse an unkeyed new window, then recover through explicit focus')
    args = parser.parse_args()
    proof = run(args)
    print(json.dumps({k: proof.get(k) for k in ('status', 'functional_pass', 'sampling_pass', 'failure')}))
    raise SystemExit(0 if proof['passed'] else 1)


if __name__ == '__main__':
    main()
