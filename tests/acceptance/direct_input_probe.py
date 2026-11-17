#!/usr/bin/env python3
"""Attach to an owned ComputerUse fixture and verify direct input through real MCP.

The fixture's Web workspace must load browser-workbench's inventory page. This
probe never launches a GUI through another API, retries input, or restores desktop
state. It records endpoint observations; it does not claim continuous isolation.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import time

from app_client import AppClient


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def require(condition, evidence):
    if not condition:
        raise AssertionError(evidence)


def verify_save(before, after, sku, quantity, note):
    old = {item['sku']: item for item in before['items']}
    new = {item['sku']: item for item in after['items']}
    require(old.keys() == new.keys(), 'inventory membership changed')
    require(all(old[key] == new[key] for key in old if key != sku), 'neighbor record changed')
    require(new[sku]['quantity'] == quantity and new[sku]['note'] == note, new[sku])
    require(len(after['saves']) == len(before['saves']) + 1, after['saves'])
    require(after['saves'][-1]['sku'] == sku and after['saves'][-1]['note'] == note, after['saves'][-1])
    return new[sku]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', required=True, type=Path)
    parser.add_argument('--app', required=True, type=Path)
    parser.add_argument('--fixture-output', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path, help='New evidence directory')
    parser.add_argument('--sku', default='DTW-0137')
    parser.add_argument('--text', default='DTW direct input 中文 😀')
    parser.add_argument('--witness-app', type=Path, help='Optional owned foreground witness')
    parser.add_argument('--window-order-observer', type=Path, help='Compiled read-only window-order-observer')
    parser.add_argument('--quit', action='store_true', help='Request normal quit after verification')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    root = args.fixture_output.resolve(strict=True)
    state_path = root / 'native-state.json'
    inventory_path = root / 'workbench/state.json'
    require(inventory_path.is_file(), 'owned workbench oracle is missing')
    before = copy.deepcopy(read(inventory_path))
    report = {'binary_sha256': hashlib.sha256(args.dtw.read_bytes()).hexdigest(),
              'app': str(args.app.resolve()), 'passed': False, 'clean_run': False,
              'limits': ['Endpoint snapshots do not prove continuous foreground, pointer or z-order isolation.',
                         'This is a macOS fixture regression, not Windows or arbitrary-app acceptance.']}
    client = None
    started = False
    try:
        client = AppClient(args.dtw.resolve(), args.output / 'mcp')
        client.execute(args.app, 'await app.look({});')
        started = True
        if args.witness_app:
            client.execute(args.witness_app, 'await app.focus({});')
        time.sleep(.15)  # Read the fixture's next independent 100ms sample.
        report['before_native'] = read(state_path)
        def order(sample):
            if not args.window_order_observer:
                return None
            owners = [sample['pid']]
            if args.witness_app:
                owners.append(report['before_native']['environment']['frontmost_pid'])
            result = subprocess.run([str(args.window_order_observer.resolve()), '--current',
                ','.join(str(pid) for pid in sorted(set(owners)))], check=True,
                capture_output=True, text=True, timeout=3)
            return json.loads(result.stdout)
        report['window_order_before_native'] = order(report['before_native'])
        native_text = 'Native direct 中文 😀'
        source = """
await app.click({selector:{role:'button',text:'Native controls'}});
await app.setValue({selector:{role:'text_field',text:'Native note'},value:TEXT});
await app.click({selector:{role:'button',text:'Commit note'}});
const v=await app.look({role:'text_field',text:'Native note'});
return {value:v.elements[0].value};
""".replace('TEXT', json.dumps(native_text))
        native, _ = client.execute(args.app, source)
        require(native['result']['value'] == native_text, native)
        time.sleep(.15)
        report['after_native'] = read(state_path)
        report['window_order_after_native'] = order(report['after_native'])
        require(report['after_native']['native']['saved_note'] == native_text, report['after_native']['native'])
        if args.witness_app:
            require(report['after_native']['environment']['frontmost_pid'] ==
                    report['before_native']['environment']['frontmost_pid'],
                    'native semantic input changed the foreground app')
        source = """
await app.click({selector:{role:'button',text:'Web workspace'}});
await app.wait({role:'text_field',text:'Search inventory',timeout_ms:5000});
await app.click({selector:{role:'text_field',text:'Search inventory'}});
await app.keypress({keys:['META','A']});
await app.typeText({text:sku});
await app.wait({text:'1 matching records',timeout_ms:5000});
await app.click({selector:{role:'button',text:'Edit '+sku}});
await app.setValue({selector:{role:'text_field',text:'Target quantity'},value:'37'});
const fields=await app.look({role:'text_area',text:'Handoff note'});
if(fields.elements.length!==1)throw new Error('note identity is not unique');
const note=fields.elements[0];
await app.click({target:note.id});
await app.keypress({keys:['META','A']});
await app.typeText({text:noteText});
const noteView=await app.look({role:'text_area',text:'Handoff note'});
const page=await app.readText({target:noteView.elements[0].id});
const qtyView=await app.look({role:'text_field',text:'Target quantity'});
return {note:page.text,quantity:qtyView.elements[0].value};
"""
        source = 'const sku=' + json.dumps(args.sku) + ';const noteText=' + json.dumps(args.text) + ';\n' + source
        typed, _ = client.execute(args.app, source)
        report['typed'] = typed
        client.capture(args.app, 'note-after-input')
        require(typed['result'] == {'note': args.text, 'quantity': '37'}, typed)
        client.execute(args.app, "await app.click({selector:{role:'button',text:'Save changes'}}); "
                       "await app.wait({text:'Saved '+" + json.dumps(args.sku) + ",timeout_ms:5000});")
        report['saved_record'] = verify_save(before, read(inventory_path), args.sku, 37, args.text)
        client.execute(args.app, "await app.click({selector:{role:'button',text:'Native controls'}});")
        if args.witness_app:
            client.execute(args.witness_app, 'await app.focus({});')
        canvas = client.capture(args.app, 'hover-target')
        sample = read(state_path)
        point = sample['anchors']['gesture-desk']
        bounds = canvas['view']['image']['bounds']
        x, y = (point[0] - bounds[0]) / bounds[2], (point[1] - bounds[1]) / bounds[3]
        report['before_hover'] = sample['environment']
        report['window_order_before_hover'] = order(sample)
        report['hover_coordinate_source'] = 'Owned fixture geometry mapped into the returned DTW canvas; not model vision.'
        client.execute(args.app, 'await app.move(' + json.dumps({'x': x, 'y': y, 'unit': 'fraction'}) + ');')
        time.sleep(.15)
        hovered = read(state_path)
        report['after_hover'] = hovered['environment']
        report['window_order_after_hover'] = order(hovered)
        require(hovered['environment']['frontmost_pid'] == hovered['pid'],
                'foreground hover was undone at execution settlement')
        actual = hovered['environment']['pointer_screen']
        require(sum((a-b)**2 for a, b in zip(actual, point)) <= 2,
                {'expected_hover_point': point, 'actual_pointer': actual})
        require(hovered['native']['gestures'].get('hover', 0) > sample['native']['gestures'].get('hover', 0),
                'hover callback was not observed')
        client.execute(args.app, 'await app.focus({});')
        time.sleep(.15)
        focused = read(state_path)
        report['after_explicit_focus'] = focused['environment']
        require(focused['environment']['frontmost_pid'] == focused['pid'],
                'explicit focus was undone at execution settlement')
        require(focused['pressed_mouse_buttons'] == 0, 'mouse button remains held')
        if args.quit:
            quit_data, _ = client.execute(args.app, 'await app.quit({});')
            started = False
            report['quit'] = quit_data
            require(quit_data['view']['running'] is False, quit_data)
        report['passed'] = True
        report['clean_run'] = not any(row['is_error'] for row in client.metrics)
    except Exception as error:
        report['error'] = str(error)
        if state_path.exists():
            report['last_native'] = read(state_path)
        if client and started:
            try:
                client.capture(args.app, 'failure-state')
            except Exception as capture_error:
                report['capture_error'] = str(capture_error)
    finally:
        if client:
            try:
                cleanup = client.close()
                report['transport_cleanup'] = cleanup
                require(cleanup.get('returncode') == 0 and not cleanup.get('forced')
                        and cleanup.get('native_workers_reaped') is True, cleanup)
            except Exception as cleanup_error:
                report['transport_cleanup_error'] = str(cleanup_error)
                report['clean_run'] = False
        (args.output / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'passed': report['passed'], 'clean_run': report['clean_run'],
                      'report': str(args.output / 'report.json'), 'error': report.get('error')}, ensure_ascii=False))
    raise SystemExit(0 if report['passed'] and report['clean_run'] else 1)


if __name__ == '__main__':
    main()
