#!/usr/bin/env python3
"""Current-protocol Windows parity acceptance using only owned desktop fixtures.

DTW supplies input. Independent fixture state, files, physical foreground flags
and PNG pixels establish effects. No failed input is replayed. Run GUI probes
sequentially on an unlocked ordinary desktop, with a fresh evidence directory.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import AppClient, METHODS, artifact_identity, require
from windows.app_protocol_probe import wait, wait_state
from windows.transport import ProcessHandle, read


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', required=True, type=Path)
    parser.add_argument('--installed-package', type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    require(os.name == 'nt', 'Windows interactive desktop required')
    from PIL import Image
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    identity = artifact_identity(args.dtw, args.installed_package)
    report = {'artifact': identity, 'passed': False, 'cases': [], 'cleanup': []}
    client, children, handles = None, [], []
    root = out / 'A'
    root.mkdir()
    stage = 'setup'

    def state(directory=root):
        return wait_state(directory, lambda s: True)

    def execute(source, target=None, allow_error=False):
        return client.execute(target or app, source, allow_error=allow_error)[0]

    def record(name, result=None):
        report['cases'].append({'name': name, 'passed': True, 'evidence': result})
        (out / 'report.json').write_text(json.dumps(report, ensure_ascii=True, indent=2), encoding='utf-8')
        print(name + ': passed', flush=True)

    def spawn(executable, directory, label='A'):
        directory.mkdir(exist_ok=True)
        child = subprocess.Popen([str(executable), str(directory), label])
        children.append((child, directory))
        snapshot = wait_state(directory, lambda s: s['ready'])
        return str(executable).lower(), snapshot

    def capture(name, target=None):
        return client.capture(target or app, name)

    def point(view, desktop):
        image = view['view']['image']
        b = image['bounds']
        return {'x': (desktop['x'] - b[0]) * image['width'] / b[2],
                'y': (desktop['y'] - b[1]) * image['height'] / b[3]}

    def pad(name, target=None, directory=root):
        return point(capture(name, target), state(directory)['pad_point'])

    def effect(source, predicate, directory=root, target=None, allow_error=False):
        response = execute(source, target, allow_error)
        snapshot = wait_state(directory, predicate)
        return {'response': response, 'effect': snapshot}

    try:
        compiled = subprocess.run(['powershell.exe', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
            str(Path(__file__).with_name('compile-cursor.ps1')), '-OutputDirectory', str(out / 'build')], capture_output=True)
        (out / 'compile.stdout').write_bytes(compiled.stdout)
        (out / 'compile.stderr').write_bytes(compiled.stderr)
        require(compiled.returncode == 0, 'fixture compilation failed')
        exe = out / 'Fixture.exe'
        shutil.copy2(out / 'build/Fixture.exe', exe)
        app, _ = spawn(exe, root)
        client = AppClient(args.dtw.resolve(), out / 'protocol')
        stage = 'installed_discovery_and_observation'
        catalog, _ = client.tool('apps', {'limit': 100})
        require(catalog['coverage']['installed'] is True, catalog)
        require(catalog.get('provider', {}).get('supported_capabilities'), catalog)
        stopped, _ = client.tool('apps', {'state': 'stopped', 'limit': 100})
        owned, _ = client.tool('apps', {'query': str(exe).upper(), 'state': 'running', 'limit': 1})
        require(len(owned['apps']) == 1, owned)
        record(stage, {'coverage': catalog['coverage'], 'provider': catalog['provider'], 'stopped_count': len(stopped['apps'])})
        full = execute('const v=await app.look({full:true});return {methods:Object.keys(app),windows:v.windows};')
        require(full['result']['methods'] == METHODS, full)
        record('public_method_surface', full)
        unchanged = execute('return await app.look();')
        require(unchanged['view'] == {'changed': False}, unchanged)
        record('unchanged_snapshot_delta', unchanged)

        stage = 'semantic_unicode_and_save'
        execute('await app.click({selector:{role:"button",text:"Increment"}});')
        wait_state(root, lambda s: s['increments'] == 1)
        payload = 'Windows 中文 😀\r\nowned parity'
        result = effect('await app.keypress("CONTROL+A");await app.typeText(' + json.dumps(payload) + ');await app.keypress({keys:["CTRL","S"]});', lambda s: s['saved'] == payload)
        require((root / 'saved.txt').read_bytes().decode('utf-8') == payload, 'saved bytes differ')
        record(stage, result)
        before = state()['increments']
        execute('const v=await app.look();await app.click(v.elements.find(e=>e.name==="Increment").id);')
        wait_state(root, lambda s: s['increments'] == before + 1)
        record('semantic_id_exact_effect')

        stage = 'coordinate_input'
        for name, extra, field in [('left', {}, 'left_up'), ('right', {'button':'right'}, 'right_up'), ('double', {'count':2}, 'double_clicks')]:
            p = pad(name)
            before = state()[field]
            record(name + '_click', effect('await app.click(' + json.dumps({**p, **extra}) + ');', lambda s, field=field, before=before: s[field] == before + 1))
        p = pad('hover')
        before = state()['moves']
        record('hover', effect('await app.move(' + json.dumps({**p, 'x': p['x'] + 30}) + ');', lambda s: s['moves'] > before))
        p = pad('drag')
        before = state()
        path = [p, {'x':p['x'] + 70, 'y':p['y'] + 20}]
        record('content_drag', effect('await app.drag(' + json.dumps({'path':path}) + ');', lambda s: s['left_up'] == before['left_up'] + 1 and s['drag_moves'] > before['drag_moves']))
        for name, delta, field, change in [('vertical', {'delta_y':3}, 'wheel_delta', -360), ('horizontal', {'delta_y':0,'delta_x':2}, 'horizontal_wheel_delta', 240)]:
            p = pad(name)
            before = state()[field]
            record(name + '_wheel', effect('await app.scroll(' + json.dumps({**p, **delta}) + ');', lambda s, field=field, before=before, change=change: s[field] == before + change))
        before = state()['wheel_delta']
        record('default_center_wheel', effect('await app.scroll({delta_y:3});', lambda s: s['wheel_delta'] == before - 360))

        stage = 'native_title_bar_drag'
        view = capture('before-title-drag')
        bounds = view['view']['windows'][0]['bounds']
        start = point(view, {'x':bounds[0] + bounds[2] / 2, 'y':bounds[1] + 12})
        end = point(view, {'x':bounds[0] + bounds[2] / 2 + 24, 'y':bounds[1] + 24})
        before = state()['bounds']
        result = effect('await app.drag(' + json.dumps({'path':[start,end]}) + ');', lambda s: s['bounds']['x'] == before['x'] + 24 and s['bounds']['y'] == before['y'] + 12)
        record(stage, result)
        stage = 'stale_geometry'
        old = pad('before-stale')
        (root / 'layout.json').write_text(json.dumps({'sequence':1,'window_offset':12,'control_offset':0}))
        wait_state(root, lambda s: s['layout_sequence'] == 1)
        rejected = execute('await app.click(' + json.dumps(old) + ');', allow_error=True)
        require(rejected.get('error', {}).get('code') == 'stale_scene', rejected)
        record(stage, rejected)

        stage = 'native_menu'
        before = state()['saves']
        # Foreground return at the exec boundary normally dismisses a Win32
        # native menu. Select its item within that same owned execution.
        record(stage, effect('await app.look();await app.click({selector:{scope:"menu",text:"Audit File"}});'
            'await app.click({selector:{scope:"menu",text:"Commit audit document"}});', lambda s: s['saves'] == before + 1))
        stage = 'modal_composition'
        effect('await app.look();await app.click({selector:{role:"button",text:"Show owned dialog"}});', lambda s: s['modal_open'])
        modal = capture('modal-stack')
        require(modal['view']['windows'][0]['title'] == 'Owned native modal', modal)
        record(stage, modal)
        record('modal_escape', effect('await app.keypress("ESC");', lambda s: not s['modal_open'], allow_error=True))

        stage = 'rejected_menu_does_not_focus'
        blocker_root = out / 'blocker'
        blocker_exe = out / 'FixtureBlocker.exe'
        shutil.copy2(exe, blocker_exe)
        blocker_app, _ = spawn(blocker_exe, blocker_root, 'B')
        actor = AppClient(args.dtw.resolve(), out / 'rejected-menu-foreground-actor')
        actor_request = actor.begin_execute(blocker_app,
            'await app.focus();const end=Date.now()+10000;while(Date.now()<end){};')
        try:
            wait_state(blocker_root, lambda s: s['foreground'])
            require(not state()['foreground'], 'owned target was not background')
            rejected = execute('await app.click({selector:{scope:"menu",text:"ABSENT-OWNED-CONTROL"}});', allow_error=True)
            require(rejected.get('error', {}).get('code') == 'selector_not_found', rejected)
            require(not state()['foreground'] and state(blocker_root)['foreground'], 'rejected menu changed foreground')
            record(stage, rejected)
            execute('await app.look();await app.quit();', blocker_app)
            children[-1][0].wait(timeout=8)
        finally:
            actor.cancel(actor_request)
            report['rejected_menu_actor_cleanup'] = actor.close()

        stage = 'multi_instance_continuity'
        other = out / 'B'
        spawn(exe, other, 'B')
        attempts = []
        for index in range(10):
            looked = execute('await app.look({full:true});')
            before_a, before_b = state()['increments'], state(other)['increments']
            clicked = execute('await app.click({selector:{role:"button",text:"Increment"}});')
            wait(lambda: state()['increments'] + state(other)['increments'] == before_a + before_b + 1)
            attempts.append({'look_top':looked['view']['windows'][0]['title'], 'click':clicked})
        record(stage, attempts)
        stage = 'multi_instance_global_order_and_pixels'
        for directory, sequence, offset, expected in [(other,1,-246,'B'), (root,2,12,'A')]:
            (directory / 'layout.json').write_text(json.dumps({'sequence':sequence,'window_offset':offset,'control_offset':0,'activate':1}))
            # This is an owned stacking setup, not business keyboard input.
            # Form.Activate may be refused by foreground locking; the actual
            # global order and physical/composite pixels below are the oracle.
            setup = wait_state(directory, lambda s, sequence=sequence: s['layout_sequence'] == sequence)
            view = capture('front-' + expected)
            require(view['view']['windows'][0]['title'].endswith(expected), view)
            desktop = {'x':300, 'y':300}
            pixel = point(view, desktop)
            with Image.open(client.output / ('front-' + expected + '.png')) as image:
                rgba = image.convert('RGBA').getpixel((int(pixel['x']),int(pixel['y'])))
            require(rgba[:3] == ((255,182,193) if expected == 'B' else (173,216,230)), {'front':expected,'pixel':rgba})
            record('front_order_' + expected, {'view':view,'intersection_pixel':rgba,
                'setup_foreground_observed': setup['foreground']})

        stage = 'layered_global_alpha'
        layered_root = out / 'layered'
        layered_root.mkdir()
        (layered_root / 'layered-window').write_text('global alpha fixture')
        layered_exe = out / 'FixtureLayered.exe'
        shutil.copy2(exe, layered_exe)
        layered_app, _ = spawn(layered_exe, layered_root)
        execute('await app.look();', layered_app)
        view = capture('layered', layered_app)
        pixel = point(view, state(layered_root)['pad_point'])
        with Image.open(client.output / 'layered.png') as image:
            rgba = image.convert('RGBA').getpixel((int(pixel['x']), int(pixel['y'])))
        require(rgba[:3] == (173,216,230) and 190 <= rgba[3] <= 192, rgba)
        record(stage, {'view':view,'rgba':rgba})
        execute('await app.quit();', layered_app)
        children[-1][0].wait(timeout=8)

        stage = 'quit_preserves_save_prompt'
        quit_root = out / 'quit'
        quit_root.mkdir()
        (quit_root / 'require-save').write_text('owned unsaved document')
        quit_exe = out / 'FixtureQuit.exe'
        shutil.copy2(exe, quit_exe)
        quit_app, _ = spawn(quit_exe, quit_root)
        execute('await app.look();await app.typeText("unsaved owned document");', quit_app)
        response = execute('await app.quit();', quit_app)
        def save_dialog():
            data = execute('await app.look({full:true});', quit_app)
            return data if any(w['title'] == 'Unsaved audit document' for w in data['view']['windows']) else None
        dialog = wait(save_dialog)
        require(children[-1][0].poll() is None, 'quit discarded unsaved fixture')
        execute('await app.keypress("ESC");', quit_app, allow_error=True)
        wait(lambda: not any(w['title'] == 'Unsaved audit document' for w in execute('await app.look({full:true});', quit_app)['view']['windows']))
        require(state(quit_root)['text'] == 'unsaved owned document', 'cancelled quit changed text')
        execute('await app.keypress("CTRL+S");await app.quit();', quit_app)
        children[-1][0].wait(timeout=8)
        record(stage, {'quit':response,'dialog':dialog,'saved':(quit_root/'saved.txt').read_text()})

        stage = 'cancellation_and_retained_reason'
        request = client.begin_execute(app, 'while(true) {}')
        busy = execute('return 1;', allow_error=True)
        require(busy.get('error',{}).get('code') == 'busy', busy)
        client.cancel(request)
        try:
            client.transport.receive(request, timeout=.5)
        except queue.Empty:
            pass
        recovery = execute('await app.look();return "after-cancel";')
        require(recovery.get('result') == 'after-cancel', recovery)
        client.transport.send({'jsonrpc':'2.0','id':request,'method':'tools/call','params':{'name':'exec','arguments':{'app':app,'source':'while(true) {}'}}})
        retained = client.transport.receive(request, timeout=10)['result']['structuredContent']
        require(retained.get('error',{}).get('code') == 'cancelled', retained)
        record(stage, retained)
        for child, directory in children:
            if child.poll() is None:
                (directory/'stop').write_text('owned graceful stop')
                child.wait(timeout=8)
        stage = 'stopped_absolute_executable_launch_and_quit'
        launched = execute('await app.look({full:true});')
        restart = out / 'restart'
        snapshot = wait_state(restart, lambda s: s['ready'])
        handle = ProcessHandle(snapshot['pid'], exe, terminate=True)
        handles.append((handle,restart))
        execute('await app.quit();')
        require(handle.wait() == 0, 'restarted app exit failed')
        record(stage, launched)
        report['passed'] = True
    except BaseException as error:
        report.update(failed_stage=stage,error=type(error).__name__ + ': ' + str(error))
        raise
    finally:
        for child,directory in children:
            if child.poll() is None:
                guard=directory/'require-save'
                if guard.exists():
                    guard.unlink()
                    report['cleanup'].append({'removed_owned_save_guard':str(directory)})
                (directory/'stop').write_text('owned graceful cleanup')
                try:
                    child.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    child.terminate();child.wait(timeout=5)
                    report['cleanup'].append({'forced_fixture':child.pid})
                    report['passed']=False
            report['cleanup'].append({'fixture_pid':child.pid,'exit_code':child.returncode})
            if child.returncode != 0: report['passed']=False
        for handle,directory in handles:
            if handle.alive():
                (directory/'stop').write_text('owned restart cleanup')
                try:
                    handle.wait()
                except Exception:
                    handle.terminate();report['passed']=False
            handle.close()
        if client:
            try:
                report['cleanup'].append({'mcp':client.close()})
            except Exception as error:
                report['cleanup'].append({'mcp_error':str(error)});report['passed']=False
        report['artifact_unchanged'] = artifact_identity(args.dtw,args.installed_package) == identity
        if not report['artifact_unchanged']: report['passed']=False
        (out/'report.json').write_text(json.dumps(report,ensure_ascii=True,indent=2)+'\n',encoding='utf-8')
    require(report['passed'], 'Windows acceptance failed; inspect report and cleanup evidence')


if __name__ == '__main__':
    main()
