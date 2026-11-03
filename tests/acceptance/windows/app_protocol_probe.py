#!/usr/bin/env python3
"""Current apps/exec Windows baseline and opaque modal composition acceptance.

Uses one owned WinForms process and independent file/control state. No model,
legacy result query, private DTW method, or replay of failed input.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import AppClient, METHODS, artifact_identity, require
from windows.transport import read


def state(root):
    # The fixture atomically replaces this publication. Share deletion while
    # reading so an independent observation cannot crash its publisher.
    return read(root / 'state.json')


def wait(predicate, timeout=8):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        result = predicate()
        if result:
            return result
        time.sleep(.03)
    raise AssertionError('owned state did not reach its expected condition')


def wait_state(root, predicate, timeout=8):
    def matched():
        snapshot = state(root)
        return snapshot if snapshot is not None and predicate(snapshot) else None
    return wait(matched, timeout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--installed-package', type=Path)
    args = parser.parse_args()
    require(os.name == 'nt', 'Windows desktop required; cross compilation is not GUI acceptance')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    identity = artifact_identity(args.dtw, args.installed_package)
    report = {'artifact': identity, 'passed': False, 'cases': [], 'binary_sha256': identity['binary_sha256'],
        'scope': 'Owned WinForms, ordinary user desktop; no Windows app-family/DPI completeness claim'}
    client, child = None, None
    root = output / 'fixture'
    root.mkdir()

    def record(name, **details):
        report['cases'].append({'name': name, 'passed': True, **details})

    try:
        compilation = subprocess.run(['powershell.exe', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
            str(Path(__file__).with_name('compile-cursor.ps1')), '-OutputDirectory', str(output / 'build')], capture_output=True)
        (output / 'compile.stdout').write_bytes(compilation.stdout)
        (output / 'compile.stderr').write_bytes(compilation.stderr)
        require(compilation.returncode == 0, 'fixture compilation failed; inspect original logs')
        exe = root / 'Fixture.exe'
        shutil.copy2(output / 'build/Fixture.exe', exe)
        child = subprocess.Popen([str(exe), str(root), 'A'])
        wait_state(root, lambda snapshot: snapshot['ready'] and snapshot['bounds_ok'])
        client = AppClient(args.dtw.resolve(), output / 'protocol')
        apps, response = client.tool('apps', {'query': str(exe), 'state': 'running'})
        require(not response.get('isError') and len(apps['apps']) == 1, apps)
        app = apps['apps'][0]['app']
        invalid, response = client.tool('exec', {'source': 'return 1;'})
        require(response.get('isError') and invalid['error']['code'] == 'invalid_arguments', invalid)
        initial, _ = client.execute(app, "await app.look();return {methods:Object.keys(app),session:typeof session,state:typeof state};")
        require(initial['result'] == {'methods': METHODS, 'session': 'undefined', 'state': 'undefined'}, initial)
        require(initial['view']['semantics']['complete'] and initial['view']['elements'], initial)
        record('discovery_required_app_and_public_surface')
        returned, _ = client.execute(app, 'return await app.look({full:true});')
        require(returned['view']['reset'] and 'result' not in returned, returned)
        unchanged, _ = client.execute(app, 'return await app.look();')
        require(unchanged['view'] == {'changed': False} and 'result' not in unchanged, unchanged)
        require(client.metrics[-1]['json_bytes'] <= 512, client.metrics[-1])
        record('returned_view_dedup_and_bounded_unchanged_delta')
        client.execute(app, "await app.click({selector:{role:'button',text:'Increment'}});")
        wait_state(root, lambda snapshot: snapshot['increments'] == 1 and snapshot['editor_focused'])
        payload = 'DTW 中文 😀\r\nowned protocol'
        client.execute(app, "await app.keypress(['CTRL','A']);await app.typeText(" + json.dumps(payload) + ');')
        wait_state(root, lambda snapshot: snapshot['text'] == payload)
        client.execute(app, "await app.keypress(['CTRL','S']);")
        wait_state(root, lambda snapshot: snapshot['saves'] == 1)
        require((root / 'saved.txt').read_bytes().decode('utf-8') == payload, 'saved bytes differ')
        client.capture(app, 'single-window')
        record('semantic_click_unicode_keyboard_and_file_readback')
        client.execute(app, "await app.click({selector:{role:'button',text:'Show owned dialog'}});")
        wait_state(root, lambda snapshot: snapshot['modal_open'])
        def modal_visible():
            view, _ = client.execute(app, 'await app.look({full:true});')
            return view if any(w['title'] == 'Owned native modal' and w['visible'] for w in view['view']['windows']) else None
        wait(modal_visible)
        modal = client.capture(app, 'opaque-modal-stack')
        windows = [w for w in modal['view']['windows'] if w['visible']]
        require(len(windows) >= 2 and windows[0]['title'] == 'Owned native modal', modal)
        require(all(isinstance(w.get('bounds'), list) and len(w['bounds']) == 4 for w in windows), modal)
        record('physical_bounds_order_and_covered_window_composition')
        closed, response = client.execute(app, "await app.click({selector:{role:'button',text:'Close dialog'}});", allow_error=True)
        if response.get('isError'):
            require(closed.get('effects', {}).get('dispatch', {}).get('unknown') == 1, closed)
        wait_state(root, lambda snapshot: not snapshot['modal_open'])
        client.execute(app, 'await app.look();')
        final = wait_state(root, lambda snapshot: True)
        require(final['text'] == payload and final['saves'] == 1, final)
        record('modal_close_fresh_read_without_input_replay')
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        cleanup_errors = []
        try:
            require(artifact_identity(args.dtw, args.installed_package) == identity, 'binary or installed package changed during acceptance')
        except BaseException as error:
            cleanup_errors.append(str(error))
        if client:
            try:
                report['mcp_cleanup'] = client.close()
                report['protocol_metrics'] = client.metrics
            except BaseException as error:
                cleanup_errors.append(str(error))
        if child:
            if child.poll() is None:
                (root / 'stop').write_text('owned graceful close')
            try:
                child.wait(timeout=8)
            except subprocess.TimeoutExpired:
                child.terminate()  # This run's exact owned process handle.
                child.wait(timeout=5)
                cleanup_errors.append('owned fixture required forced termination')
            report['fixture_exit'] = child.returncode
            if child.returncode != 0:
                cleanup_errors.append('owned fixture exited nonzero')
        if cleanup_errors:
            report['passed'] = False
            report['cleanup_errors'] = cleanup_errors
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        if cleanup_errors:
            raise AssertionError(cleanup_errors)


if __name__ == '__main__':
    main()
