#!/usr/bin/env python3
"""Owned AX faults through apps/exec; performs physical pointer input, no model A/B."""
import argparse
import json
import plistlib
import shutil
import subprocess
import sys
import uuid
from pathlib import Path

from app_client import AppClient, artifact_identity, require
from standard_agent_probe import wait_until, write_json


AX_PROBE = r'''
import ApplicationServices
import Foundation
let app = AXUIElementCreateApplication(Int32(CommandLine.arguments[1])!)
AXUIElementSetMessagingTimeout(app, 0.2)
var value: CFTypeRef?
let status = AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &value)
var output: [String:Any] = ["trusted": AXIsProcessTrusted(), "windows_status": status.rawValue]
if status == .success, let windows = value as? [AXUIElement], let window = windows.first {
    AXUIElementSetMessagingTimeout(window, 0.2)
    var count = 0; var children: CFTypeRef?; var range: CFArray?
    let counted = AXUIElementGetAttributeValueCount(window, kAXChildrenAttribute as CFString, &count)
    let copied = AXUIElementCopyAttributeValue(window, kAXChildrenAttribute as CFString, &children)
    let ranged = AXUIElementCopyAttributeValues(window, kAXChildrenAttribute as CFString, 0, 1, &range)
    output["count_status"] = counted.rawValue; output["range_status"] = ranged.rawValue
    output["copy_status"] = copied.rawValue; output["count"] = count
    output["copy_count"] = (children as? [AXUIElement])?.count ?? 0
}
print(String(data: try JSONSerialization.data(withJSONObject: output), encoding: .utf8)!)
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--baseline', type=Path, help='Optional prior binary for the owned count-fault regression')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--profiles', nargs='+', choices=['count-range', 'range', 'opaque', 'no-windows'],
                        default=['count-range', 'range', 'opaque', 'no-windows'])
    parser.add_argument('--continue-effect-gaps', action='store_true',
                        help='Audit remaining cases after a missing hover effect; the run still fails')
    parser.add_argument('--only-hover',action='store_true',help='Final acceptance of the only changed fallback shape')
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    out.chmod(0o700)
    source = Path(__file__).resolve().parents[1] / 'fixtures/ax-compat/main.m'
    fixture = out / 'AXCompat'
    subprocess.run(['xcrun', 'clang', '-fobjc-arc', '-fmodules', '-fmodules-cache-path=' + str(out / 'clang-cache'),
                    '-Wall', '-Wextra', '-Werror', '-Wno-unused-parameter', str(source), '-framework', 'AppKit',
                    '-framework', 'ApplicationServices', '-o', str(fixture)], check=True, timeout=90)
    probe_source = out / 'ax-probe.swift'
    probe_source.write_text(AX_PROBE)
    probe = out / 'ax-probe'
    subprocess.run(['xcrun', 'swiftc', '-module-cache-path', str(out / 'swift-cache'), str(probe_source),
                    '-o', str(probe)], check=True, timeout=90)
    report = {'artifact': artifact_identity(args.dtw), 'cases': [],
              'scope': 'owned native AppKit provider faults on macOS; Windows/real third-party apps not implied'}
    owned = []
    client = AppClient(args.dtw, out / 'candidate')

    def record(name, *, passed=True, **details):
        report['cases'].append({'name': name, 'passed': passed, **details})
        write_json(out / 'report.json', report)

    def state(directory):
        try:
            return json.loads((directory / 'state.json').read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            return None

    def make(profile):
        directory = out / profile
        directory.mkdir()
        (directory / 'profile').write_text(profile)
        bundle = directory / 'AXCompat.app'
        executable = bundle / 'Contents/MacOS/AXCompat'
        executable.parent.mkdir(parents=True)
        shutil.copy2(fixture, executable)
        with (bundle / 'Contents/Info.plist').open('wb') as stream:
            plistlib.dump({'CFBundleExecutable': 'AXCompat', 'CFBundleIdentifier': 'dev.caelis.dtw.ax.' + uuid.uuid4().hex,
                          'CFBundleName': 'DTW AX ' + profile, 'CFBundlePackageType': 'APPL',
                          'NSPrincipalClass':'NSApplication'}, stream)
        log = (directory / 'fixture.log').open('w')
        child = subprocess.Popen([str(executable)], stdout=log, stderr=subprocess.STDOUT)
        owned.append((directory, child, log))
        wait_until(lambda: state(directory))
        require(state(directory)['pid'] == child.pid, 'fixture oracle does not belong to the spawned process')
        return directory, bundle

    def point(view, directory, name):
        x, y = state(directory)['points'][name]
        image = view['image']
        left, top, width, height = image['bounds']
        return {'x': (x - left) * image['width'] / width, 'y': (y - top) * image['height'] / height}

    def capture(bundle, name):
        return client.capture(bundle, name)['view']

    def click(bundle, point):
        return client.execute(bundle, 'await app.click(' + json.dumps(point) + ');')

    def stopped(directory):
        (directory / 'quit').touch()
        child = next(child for root, child, _ in owned if root == directory)
        require(child.wait(timeout=5) == 0, 'owned fixture did not quit normally')

    try:
        for profile in args.profiles:
            directory, bundle = make(profile)
            samples = []
            def provider_ready():
                sample = json.loads(subprocess.check_output([str(probe), str(state(directory)['pid'])], text=True, timeout=5))
                samples.append(sample)
                if not sample['trusted']:
                    return False
                if profile == 'no-windows':
                    return sample['windows_status'] == -25200
                if profile == 'opaque':
                    return sample.get('copy_status') == -25200 and sample.get('count_status') == -25200
                return (sample.get('copy_status') == 0 and sample.get('copy_count', 0) >= 2
                        and sample.get('range_status') == -25200
                        and sample.get('count_status') == (-25200 if profile == 'count-range' else 0))
            wait_until(provider_ready)
            raw = samples[-1]
            require(raw['trusted'], raw)
            if profile in ('count-range', 'range'):
                require(raw['copy_status'] == 0 and raw['copy_count'] >= 2 and raw['range_status'] != 0, raw)
                require((raw['count_status'] != 0) == (profile == 'count-range'), raw)
            elif profile == 'opaque':
                require(raw['copy_status'] != 0 and raw['count_status'] != 0, raw)
            else:
                require(raw['windows_status'] != 0, raw)
            record(profile + ':provider_fault_proven', ax=raw)

            if args.only_hover:
                view=capture(bundle,'hover')
                offset=len(state(directory)['gestures'])
                client.execute(bundle,'await app.move('+json.dumps(point(view,directory,'pad-start'))+');')
                wait_until(lambda:'move' in state(directory)['gestures'][offset:])
                record(profile+':final_hover_callback')
                client.execute(bundle,'await app.quit();')
                child=next(child for root,child,_ in owned if root==directory)
                require(child.wait(timeout=5)==0,'product quit did not close the owned fixture')
                record(profile+':normal_quit_after_hover')
                continue

            if profile == 'count-range' and args.baseline:
                old = AppClient(args.baseline, out / 'baseline')
                try:
                    view = old.capture(bundle, 'before')['view']
                    data, response = old.execute(bundle, 'await app.click(' + json.dumps(point(view, directory, 'button')) + ');', allow_error=True)
                    require(response.get('isError') and data['failure']['code'] == 'children_unavailable', data)
                    require(state(directory)['clicks'] == 0, state(directory))
                    record('baseline_count_failure_blocks_coordinate_click', failure=data['failure'])
                finally:
                    old.close()

            data, _ = client.execute(bundle, 'await app.look({full:true});')
            if profile in ('count-range', 'range'):
                button = next(row for row in data['view']['elements'] if row.get('name') == 'Next Tip')
                client.execute(bundle, 'await app.click({target:' + json.dumps(button['id']) + '});')
                wait_until(lambda: state(directory)['clicks'] == 1)
                record(profile + ':semantic_target_click_restored')
                client.execute(bundle, "await app.click({selector:{role:'button',text:'Next Tip'}});")
                wait_until(lambda: state(directory)['clicks'] == 2)
                record(profile + ':semantic_selector_click_restored')
            else:
                require(not data['view']['elements'] and not data['view']['semantics']['complete'], data)
                require('image' in data['view'], data)
                record(profile + ':automatic_visual_fallback')
                rejected, response = client.execute(bundle,
                    "await app.click({selector:{role:'button',text:'Next Tip'}});", allow_error=True)
                require(response.get('isError') and 'image' in rejected['view'], rejected)
                require(state(directory)['clicks'] == 0, state(directory))
                record(profile + ':unproven_semantic_action_keeps_visual_path_without_replay')

            before = state(directory)['clicks']
            view = capture(bundle, profile + '-click')
            click(bundle, point(view, directory, 'button'))
            wait_until(lambda: state(directory)['clicks'] == before + 1)
            record(profile + ':coordinate_click')

            if profile == 'count-range':
                view = capture(bundle, 'right-click')
                right = point(view, directory, 'pad-start')
                right['button'] = 'right'
                client.execute(bundle, 'await app.click(' + json.dumps(right) + ');')
                wait_until(lambda: 'right-up' in state(directory)['gestures'])
                require('right-down' in state(directory)['gestures'], state(directory))
                record('coordinate_right_click_pair')
                view = capture(bundle, 'double-click')
                before = state(directory)['clicks']
                double = point(view, directory, 'button')
                double['count'] = 2
                client.execute(bundle, 'await app.click(' + json.dumps(double) + ');')
                wait_until(lambda: state(directory)['clicks'] == before + 2)
                record('coordinate_double_click_two_effects')

            view = capture(bundle, profile + '-move')
            client.execute(bundle, 'await app.move(' + json.dumps(point(view, directory, 'pad-start')) + ');')
            try:
                wait_until(lambda: 'move' in state(directory)['gestures'])
            except AssertionError:
                if not args.continue_effect_gaps:
                    raise
                record(profile + ':pointer_move', passed=False,
                       failure='dispatched but no fixture hover callback; input was not replayed')
            else:
                record(profile + ':pointer_move')
            if profile=='count-range' and 'move' in state(directory)['gestures']:
                view=capture(bundle,'same-window-hover')
                before=len(state(directory)['gestures'])
                client.execute(bundle,'await app.move('+json.dumps(point(view,directory,'pad-end'))+');')
                wait_until(lambda:'move' in state(directory)['gestures'][before:])
                record('repeated_hover_effect_not_inferred_from_cursor_position')
            view = capture(bundle, profile + '-drag')
            pointer_before = state(directory)['pointer']
            samples_before = len(state(directory)['pointer_samples'])
            path = [point(view, directory, name) for name in ('pad-start', 'pad-end')]
            client.execute(bundle, 'await app.drag({path:' + json.dumps(path) + '});')
            wait_until(lambda: 'up' in state(directory)['gestures'])
            gestures = state(directory)['gestures']
            require(all(kind in gestures for kind in ['down', 'drag', 'up']), gestures)
            require(state(directory)['pressed_mouse_buttons'] == 0, state(directory))
            record(profile + ':drag_and_release', cursor_effect={
                'before': pointer_before, 'after': state(directory)['pointer'],
                'samples': state(directory)['pointer_samples'][samples_before:],
                'scope': 'diagnostic actual system pointer; movement is not a requirement for future background routes'})

            view = capture(bundle, profile + '-field')
            click(bundle, point(view, directory, 'field'))
            client.execute(bundle, "await app.keypress({keys:['CMD','A']});")
            client.execute(bundle, "await app.typeText({text:'AX fallback 😀 works'});")
            wait_until(lambda: state(directory)['text'] == 'AX fallback 😀 works')
            client.execute(bundle, "await app.keypress({keys:['CMD','K']});")
            wait_until(lambda: state(directory)['keys'] == 1)
            record(profile + ':keyboard_and_text')
            view = capture(bundle, profile + '-wheel')
            anchor = point(view, directory, 'pad-start')
            anchor['delta_y'] = 3
            before = state(directory)['wheels']
            client.execute(bundle, 'await app.scroll(' + json.dumps(anchor) + ');')
            wait_until(lambda: state(directory)['wheels'] > before)
            before = state(directory)['wheels']
            client.execute(bundle, 'await app.scroll({delta_y:3});')
            wait_until(lambda: state(directory)['wheels'] > before)
            record(profile + ':anchored_and_default_scroll')

            if profile == 'count-range':
                foreign, _ = make('occluder')
                view = capture(bundle, 'foreign-occluder')
                before = state(directory)['clicks']
                data, response = client.execute(bundle,
                    'await app.click(' + json.dumps(point(view, directory, 'button')) + ');', allow_error=True)
                require(not response.get('isError'), data)
                wait_until(lambda:state(directory)['clicks']==before+1)
                require(state(foreign)['clicks'] == 0,
                        {'target': state(directory), 'foreign': state(foreign)})
                record('foreign_occlusion_uses_addressed_input_without_wrong_app_input')
                stopped(foreign)

            view = capture(bundle, profile + '-stale')
            stale = point(view, directory, 'button')
            before = state(directory)
            (directory / 'move').touch()
            wait_until(lambda: state(directory)['bounds'] != before['bounds'])
            data, response = client.execute(bundle, 'await app.click(' + json.dumps(stale) + ');', allow_error=True)
            require(response.get('isError') and data['failure']['code'] == 'stale_scene', data)
            require(state(directory)['clicks'] == before['clicks'], state(directory))
            record(profile + ':stale_geometry_still_rejected', failure=data['failure'])
            client.execute(bundle, 'await app.quit();')
            record(profile + ':normal_quit_without_semantic_tree_requirement')
            stopped(directory)
        report['status'] = 'passed' if all(case['passed'] for case in report['cases']) else 'failed'
    except BaseException as error:
        report['status'] = 'failed'
        report['failure'] = repr(error)
        raise
    finally:
        for directory, child, log in owned:
            (directory / 'quit').touch()
            try:
                child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                child.terminate()  # Only the directly spawned disposable child.
                try:
                    child.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=2)
            log.close()
        client.close()
        write_json(out / 'report.json', report)
    print(json.dumps({'status': report['status'], 'cases': len(report['cases']), 'report': str(out / 'report.json')}))
    return 0 if report['status'] == 'passed' else 1


if __name__ == '__main__':
    sys.exit(main())
