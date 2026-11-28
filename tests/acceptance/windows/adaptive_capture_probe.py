"""Verify adaptive PNGs and physical clicks in an owned Win32 fixture."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import AppClient, require
from windows.app_protocol_probe import wait_state


def compile_fixture(binary):
    env = os.environ | {
        'DTW_FIXTURE_SOURCE': str(Path(__file__).with_name('CaptureFixture.cs').resolve()),
        'DTW_FIXTURE_BINARY': str(binary),
    }
    subprocess.run(['powershell.exe', '-NoProfile', '-Command', r"""
        $ErrorActionPreference = 'Stop'
        Add-Type -Path $env:DTW_FIXTURE_SOURCE -ReferencedAssemblies @(
          'System.dll','System.Drawing.dll','System.Windows.Forms.dll','System.Web.Extensions.dll'
        ) -OutputAssembly $env:DTW_FIXTURE_BINARY -OutputType WindowsApplication
    """], env=env, check=True)


def sampled_point(image, x, y):
    left, top, width, height = image['bounds']
    return [int((left + (x + .5) * width / image['width']) // 1),
            int((top + (y + .5) * height / image['height']) // 1)]


def click_target(client, app, image, targets, target, region=False):
    left, top, width, height = targets[target]
    sx, sy, sw, sh = image['bounds']
    x = int((left + width / 2 - sx) * image['width'] / sw)
    y = int((top + height / 2 - sy) * image['height'] / sh)
    expected = sampled_point(image, x, y)
    point = {'x': x, 'y': y}
    if region:
        point = {'x': (x + .25) / image['width'], 'y': (y + .25) / image['height'], 'unit': 'fraction'}
    result, _ = client.execute(app, f'await app.click({json.dumps(point)});')
    require(result['effects']['dispatch'] == {'dispatched': 1}, result)
    return expected


def capture(client, app, options, png):
    reply, response = client.execute(app, f'await app.capture({json.dumps(options)});')
    image = reply['view']['image']
    require('error' not in image, image)
    parts = [part for part in response['content'] if part['type'] == 'image']
    require(len(parts) == 1, reply)
    payload = base64.b64decode(parts[0]['data'], validate=True)
    png.write_bytes(payload)
    return image, {'png_bytes': len(payload), 'capture_ms': round(client.metrics[-1]['seconds'] * 1000, 2)}


def run_case(binary, exe, root, bounds, style, test_move=False):
    root.mkdir()
    child = subprocess.Popen([str(exe), str(root), *map(str, bounds), style])
    client = None
    try:
        initial = wait_state(root, lambda value: 'targets' in value)
        app = str(exe).lower()
        client = AppClient(binary, root / 'protocol')
        full, full_metrics = capture(client, app, {}, root / 'full.png')
        require(full['bounds'] == initial['bounds'], {'image': full, 'fixture': initial['bounds']})
        expected = click_target(client, app, full, initial['targets'], 'blue')
        first = wait_state(root, lambda value: len(value['hits']) == 1)['hits'][0]
        require(first == {'target': 'blue', 'point': expected}, {'hit': first, 'expected': expected})

        left, top, width, height = initial['bounds']
        bx, by, bw, bh = initial['targets']['red']
        if style == 'dense':
            bx, by, _, bh = initial['targets']['blue']
            last_x, _, last_width, _ = initial['targets']['pink']
            bw = last_x + last_width - bx
        crop = {'x': max(0, bx - left - 10) / width, 'y': max(0, by - top - 10) / height,
                'width': (bw + 20) / width, 'height': (bh + 20) / height}
        region, region_metrics = capture(client, app, {'region': crop}, root / 'region.png')
        require(region['canvas_bounds'] == initial['bounds'], region)
        if style == 'dense':
            require(256 < region['width'] <= 800
                    and [region['width'], region['height']] == region['bounds'][2:], region)
        expected = click_target(client, app, region, initial['targets'], 'red', region=True)
        second = wait_state(root, lambda value: len(value['hits']) == 2)['hits'][1]
        require(second == {'target': 'red', 'point': expected}, {'hit': second, 'expected': expected})
        if test_move:
            (root / 'move.txt').write_text(f'{left+17},{top+12}', encoding='ascii')
            wait_state(root, lambda value: value['sequence'] == 1)
            failed, raw = client.execute(app, 'await app.click({x:0.5,y:0.5,unit:"fraction"});', allow_error=True)
            require(raw.get('isError') and failed['error']['code'] == 'stale_scene', failed)
            require(len(wait_state(root, lambda value: True)['hits']) == 2, failed)
        return {'bounds': initial['bounds'], 'dpi': initial['dpi'], 'displays': initial['displays'],
                'full': {**full_metrics, 'size': [full['width'], full['height']]},
                'region': {**region_metrics, 'size': [region['width'], region['height']], 'bounds': region['bounds']},
                'hits': [first, second], 'stale_after_move': test_move}
    finally:
        try:
            if client:
                client.close()
        finally:
            (root / 'stop').touch()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.terminate()
                child.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir()
    exe = root / 'CaptureFixture.exe'
    compile_fixture(exe)
    binary = args.dtw.resolve(strict=True)
    report = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'cases': {}, 'unavailable': []}
    try:
        cases = report['cases']
        cases['sparse'] = run_case(binary, exe, root / 'sparse', [80, 80, 1440, 900], 'sparse', True)
        cases['dense'] = run_case(binary, exe, root / 'dense', [80, 80, 1440, 900], 'dense')
        require(cases['dense']['full']['size'][0] > cases['sparse']['full']['size'][0], cases)
        displays = cases['dense']['displays']
        cases['4k_window'] = run_case(binary, exe, root / '4k-window', [0, 0, 3840, 2160], 'dense')
        require(cases['4k_window']['bounds'][2:] == [3840, 2160], cases['4k_window'])
        if not any(w >= 3840 and h >= 2160 for _, _, w, h in displays):
            report['unavailable'].append('4K display. A 3840x2160 native window was captured with part off-screen.')
        negative = next((d for d in displays if d[0] < 0 or d[1] < 0), None)
        if negative:
            x, y, w, h = negative
            cases['negative_display'] = run_case(binary, exe, root / 'negative', [x+15, y+15, min(w-30, 1440), min(h-30, 900)], 'dense')
        else:
            report['unavailable'].append('Negative-origin secondary display.')
        seam = next(((ax+aw, max(ay, by)) for ax, ay, aw, ah in displays for bx, by, _, bh in displays
                     if ax+aw == bx and min(ay+ah, by+bh) - max(ay, by) > 250), None)
        if seam:
            x, y = seam
            cases['straddling'] = run_case(binary, exe, root / 'straddling', [x-130, y+55, 720, 190], 'dense')
        else:
            report['unavailable'].append('Window spanning two physical displays with a horizontal seam.')
        if not {96, 144}.issubset({case['dpi'] for case in cases.values()}):
            report['unavailable'].append('100%/150% mixed physical DPI.')
        report['passed'] = True
    finally:
        (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(root / 'report.json')


if __name__ == '__main__':
    main()
