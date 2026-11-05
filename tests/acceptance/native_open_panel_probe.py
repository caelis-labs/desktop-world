#!/usr/bin/env python3
"""Owned NSRemoteView Open-panel GUI regression with a capture-bound visual handoff.

The supervisor inspects mcp/open-panel.png and writes point.json containing
x, y and capture_sha256. Business input is exclusively current apps/exec.
"""
import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import shutil
import subprocess
import time
import traceback
import uuid

from app_client import AppClient, artifact_identity
from computer_use_focus import _fixture_class, _ready, _rows, evaluate_focus
from computer_use_scope import evaluate_scope_focus
from macos_input_recovery_probe import require
from standard_agent_probe import process_table


def wait_for(predicate, seconds=5):
    until = time.monotonic() + seconds
    while not predicate():
        require(time.monotonic() < until, 'Owned GUI postcondition did not settle')
        time.sleep(.05)


def run(args):
    root = args.output.resolve()
    root.mkdir(mode=0o700, exist_ok=False)
    bundle = root / 'DocumentProbe.app'
    executable = bundle / 'Contents/MacOS/DocumentProbe'
    executable.parent.mkdir(parents=True)
    shutil.copy2(args.document_binary, executable)
    with (bundle / 'Contents/Info.plist').open('wb') as stream:
        plistlib.dump({'CFBundleIdentifier': 'dev.caelis.dtw.openpanel.' + uuid.uuid4().hex,
            'CFBundleExecutable': 'DocumentProbe', 'CFBundleName': 'Owned Open Panel Probe',
            'CFBundlePackageType': 'APPL', 'NSPrincipalClass': 'NSApplication',
            'CFBundleDocumentTypes': [{'CFBundleTypeName': 'Owned probe document',
                'CFBundleTypeExtensions': ['dtwprobe'], 'CFBundleTypeRole': 'Editor',
                'NSDocumentClass': 'DTWProbeDocument'}]}, stream)
    cover = _fixture_class()(root / 'cover', 'open-panel-witness')
    proof = {'identity': artifact_identity(args.dtw), 'case': args.case, 'functional_pass': False,
             'fixture_sha256': hashlib.sha256(args.document_binary.read_bytes()).hexdigest()}
    child = client = observer = log = observer_log = None

    def state():
        path = root / 'state.json'
        return json.loads(path.read_text()) if path.exists() else {}

    def foreground():
        return int(subprocess.check_output([str(args.observer), '--current'], text=True).strip())

    try:
        log = (root / 'app.log').open('wb')
        child = subprocess.Popen(['/usr/bin/open', '-W', '-n', '-a', str(bundle)], stdout=log, stderr=log)
        wait_for(lambda: state().get('panel_ready') and bool(state().get('windows')), 15)
        time.sleep(.3)
        cover.prepare(activate=True, role='witness', compiled_native=args.fixture_binary)
        cover.start()
        wait_for(lambda: foreground() == cover.child.pid)
        proof['before_client'] = state()
        require(len(state()['windows']) == 1 and state()['windows'][0]['open_panel'], 'Expected only the Open panel')
        observer_log = (root / 'observer-stderr.log').open('wb')
        observer = subprocess.Popen([str(args.observer), str(root / 'focus.jsonl')],
                                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=observer_log)
        _ready(observer)
        client = AppClient(args.dtw, root / 'mcp', mcp_args=
                           ('--input-policy', 'background-only') if args.case == 'background-only' else ())
        start = time.time() * 1000
        view = client.capture(bundle, 'open-panel')
        proof['capture_interval'] = [start, time.time() * 1000]
        proof['before'] = state()
        proof['pointer_before'] = cover.state()['environment']['pointer_screen']
        require(state()['created'] == proof['before_client']['created'] and len(state()['windows']) == 1
                and state()['windows'][0]['open_panel'], 'Capture changed document state')
        digest = hashlib.sha256((root / 'mcp/open-panel.png').read_bytes()).hexdigest()
        handoff = {'png': str(root / 'mcp/open-panel.png'), 'capture_sha256': digest, 'view': view['view']}
        (root / 'handoff.json').write_text(json.dumps(handoff, indent=2))
        print(json.dumps({key: handoff[key] for key in ('png', 'capture_sha256')}), flush=True)
        wait_for(lambda: (root / 'point.json').exists(), 180)
        point = json.loads((root / 'point.json').read_text())
        require(point.get('capture_sha256') == digest, 'Point belongs to another capture')
        # Each exec borrows its actual current owner. A user choice between
        # the read-only capture and this action is legitimate; it is not an
        # external choice within an active input scope.
        proof['action_foreground'] = foreground()
        proof['pointer_before'] = cover.state()['environment']['pointer_screen']
        coordinates = {key: point[key] for key in ('x', 'y')}
        source = ('await app.focus();\n' if args.case == 'focus-first' else '')
        source += 'await app.click(' + json.dumps(coordinates) + ');'
        proof.update(point=point, source=source)
        start = time.time() * 1000
        data, frame = client.execute(bundle, source, allow_error=True)
        proof.update(reply=data, is_error=frame.get('isError', False), action_interval=[start, time.time() * 1000])
        effects = data.get('effects', {})
        require(not effects.get('dispatch', {}).get('unknown') and not effects.get('interrupted'), data)
        if args.case == 'background-only':
            require(data.get('error', {}).get('code') == 'foreground_required' and frame.get('isError'), data)
            require(effects.get('dispatch', {}).get('not_dispatched') == 1
                    and not effects.get('dispatch', {}).get('dispatched'), data)
            require(state()['created'] == proof['before']['created'] and len(state()['windows']) == 1, state())
        else:
            require(not frame.get('isError') and not data.get('error'), data)
            wait_for(lambda: state().get('created') == proof['before']['created'] + 1 and len(state().get('windows', [])) == 1)
            require(state()['windows'][0]['id'] != proof['before']['windows'][0]['id'], state())
        wait_for(lambda: foreground() == proof['action_foreground'])
        proof['after'] = state()
        proof['pointer_after'] = cover.state()['environment']['pointer_screen']
        require(sum((a-b)**2 for a,b in zip(proof['pointer_before'], proof['pointer_after'])) <= 4, 'Borrowed pointer was not restored')
        proof['functional_pass'] = True
    except BaseException as error:
        proof['failure'] = {'message': str(error), 'traceback': traceback.format_exc()}
    finally:
        cleanup = proof['cleanup'] = {}
        if client:
            cleanup['client'] = client.close()
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
        if 'capture_interval' in proof:
            proof['capture_focus'] = evaluate_focus(samples, *proof['capture_interval'], cover.child.pid)
        if 'action_interval' in proof:
            interval = proof['action_interval']
            proof['action_focus'] = (evaluate_focus(samples, *interval, proof['action_foreground'])
                if args.case == 'background-only' or proof['action_foreground'] == proof['before']['pid'] else
                evaluate_scope_focus(samples, *interval, proof['before']['pid'], proof['action_foreground']))
        (root / 'stop').touch()
        if child:
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                cleanup['app_forced'] = True
                pid = state().get('pid')
                command = process_table().get(pid, (None, ''))[1]
                if command == str(executable) or command.startswith(str(executable) + ' '):
                    subprocess.run(['/bin/kill', '-TERM', str(pid)], check=False)
                    child.wait(timeout=3)
            cleanup['app_returncode'] = child.poll()
        if log:
            log.close()
        cleanup['cover'] = cover.close()
        cleanup['passed'] = (not cleanup.get('app_forced') and cleanup.get('app_returncode') == 0
            and not cleanup.get('observer_forced') and cleanup.get('observer_returncode') == 0
            and cleanup['cover'].get('passed') and cleanup.get('client', {}).get('returncode') == 0
            and not cleanup.get('client', {}).get('forced') and cleanup.get('client', {}).get('native_workers_reaped'))
        focus = [proof.get(name, {}) for name in ('capture_focus', 'action_focus')]
        proof['functional_pass'] = proof['functional_pass'] and cleanup['passed'] and all(r.get('behavior_passed') is not False for r in focus)
        proof['sampling_pass'] = all(r.get('sampling_quality', {}).get('passed') for r in focus)
        proof['passed'] = proof['functional_pass'] and proof['sampling_pass']
        proof['status'] = 'failed' if not proof['functional_pass'] else 'passed' if proof['passed'] else 'inconclusive'
        (root / 'report.json').write_text(json.dumps(proof, ensure_ascii=False, indent=2))
    return proof


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('dtw', 'document-binary', 'fixture-binary', 'observer', 'output'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--case', choices=['auto', 'focus-first', 'background-only'], default='auto')
    args = parser.parse_args()
    proof = run(args)
    print(json.dumps({k: proof.get(k) for k in ('status', 'functional_pass', 'sampling_pass', 'failure')}))
    raise SystemExit(0 if proof['passed'] else 1)


if __name__ == '__main__':
    main()
