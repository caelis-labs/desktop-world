#!/usr/bin/env python3
"""Opt-in Windows pointer feedback and idle acceptance through public apps/exec."""
import argparse
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

from PIL import ImageGrab

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import AppClient, artifact_identity, require
from windows.app_protocol_probe import wait, wait_state
from windows.transport import ProcessHandle, read

PALETTE = {(178, 132, 255), (247, 132, 170), (96, 218, 174), (244, 178, 66),
           (76, 204, 224), (221, 113, 236), (232, 82, 98), (184, 220, 54), (80, 126, 236)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(os.name == 'nt', 'Windows desktop required')
    binary, out = args.dtw.resolve(strict=True), args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    report = {'passed': False, 'artifact': artifact_identity(binary), 'cases': [], 'cleanup': []}
    root = out / 'fixture'
    root.mkdir()
    child = None
    clients, handles = [], []
    user = ctypes.WinDLL('user32', use_last_error=True)
    user.SetThreadDpiAwarenessContext.argtypes = [ctypes.c_void_p]
    user.SetThreadDpiAwarenessContext.restype = ctypes.c_void_p
    user.IsWindowVisible.argtypes = [wintypes.HWND]
    user.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user.GetCursorPos.argtypes = [ctypes.POINTER(wintypes.POINT)]
    require(user.SetThreadDpiAwarenessContext(ctypes.c_void_p(-4)), 'PMv2 coordinates unavailable')
    stage = 'setup'

    def record(**evidence):
        report['cases'].append({'name': stage, **evidence})
        print(stage + ': passed', flush=True)

    def state(predicate=lambda _: True):
        return wait_state(root, predicate)

    def position():
        point = wintypes.POINT()
        require(user.GetCursorPos(ctypes.byref(point)), 'Physical pointer unavailable')
        return [point.x, point.y]

    def window_sample(helper):
        hwnd = helper['hwnd']
        rect = wintypes.RECT()
        require(user.GetWindowRect(hwnd, ctypes.byref(rect)), 'Owned helper window disappeared')
        hotspot = int(14 * helper['dpi'] / 96 + .5)
        return {'visible': bool(user.IsWindowVisible(hwnd)), 'x': rect.left + hotspot, 'y': rect.top + hotspot}

    def marker(client, label):
        path = out / (label + '.json')
        subprocess.run([str(observer), '--snapshot', str(path), initial['hwnd']],
                       check=True, creationflags=subprocess.CREATE_NO_WINDOW)
        scene = read(path)
        rows = [row for row in scene['windows'] if row['parent'] == client.transport.proc.pid]
        require(len(rows) == 1, {'label': label, 'rows': rows})
        helper = rows[0]
        require(helper['visible'] and max(helper['palette_pixels']) >= 20,
                {'label': label, 'helper': helper})
        require(Path(helper['path']).resolve() == binary, helper)
        require(helper['ex_style'] & 0x08080020 == 0x08080020 and not helper['ex_style'] & 8, helper)
        require(scene['foreground_before'] and scene['foreground_after'], scene)
        return helper

    def connect(label):
        client = AppClient(binary, out / label)
        clients.append(client)
        apps, _ = client.tool('apps', {'query': str(exe), 'state': 'running'})
        require(len(apps['apps']) == 1, apps)
        return client, apps['apps'][0]['app']

    def pixel(image, point):
        left, top, width, height = image['bounds']
        return {'x': (point['x'] - left) * image['width'] / width,
                'y': (point['y'] - top) * image['height'] / height}

    def source(method, options):
        return 'await app.' + method + '(' + json.dumps(options) + ');'

    def ring_pixels(hotspot, dpi):
        x, y = hotspot['x'], hotspot['y']
        scale = dpi / 96
        inner = (7 * scale) ** 2
        outer = (16 * scale) ** 2
        inset = int(16 * scale + .5)
        image = ImageGrab.grab((x - inset, y - inset, x + 3 * inset, y + 3 * inset),
                               include_layered_windows=True, all_screens=True).convert('RGB')
        rgb = image.tobytes()
        colored = [(i % image.width - inset, i // image.width - inset)
                   for i, color in enumerate(zip(rgb[::3], rgb[1::3], rgb[2::3])) if color in PALETTE]
        distance = [dx * dx + dy * dy for dx, dy in colored]
        return ({'ring': sum(inner < d <= outer for d in distance),
                 'center': sum(d <= inner for d in distance),
                 'outside': sum(d > outer for d in distance)}, image)

    def trace(client, app, script, helper, pixel_at):
        request = client.begin_execute(app, script)
        started = time.monotonic()
        samples = []
        first_ring = None
        peak_count, peak_frame = 0, None
        while time.monotonic() - started < 5:
            row = {'seconds': time.monotonic() - started, **window_sample(helper), 'pointer': position()}
            if row['visible'] and abs(row['x'] - pixel_at['x']) <= 2 and abs(row['y'] - pixel_at['y']) <= 2:
                row['pixels'], frame = ring_pixels(row, helper['dpi'])
                if row['pixels']['ring'] > peak_count:
                    peak_count, peak_frame = row['pixels']['ring'], frame
                if row['pixels']['ring'] >= 20 and first_ring is None:
                    first_ring = row['seconds']
            samples.append(row)
            if first_ring is not None and (not row['visible'] or row['seconds'] - first_ring >= .9):
                break
            time.sleep(.008)
        response = client.transport.receive(request, timeout=8)['result']
        result = response['structuredContent']
        require(not response.get('isError') and result['status'] == 'completed', result)
        (out / (stage + '-trace.json')).write_text(json.dumps(samples, indent=2), encoding='utf-8')
        if peak_frame is not None:
            peak_frame.save(out / (stage + '.png'))
        return samples

    def at(point, helper):
        row = window_sample(helper)
        return row['visible'] and abs(row['x'] - point['x']) <= 2 and abs(row['y'] - point['y']) <= 2

    def check_ring(samples, point):
        frames = [row for row in samples if row.get('pixels', {}).get('ring', 0) >= 20]
        require(frames and all(row['pixels']['center'] == 0 and row['pixels']['outside'] == 0
                               for row in frames), {'point': point, 'frames': frames})
        require(any(abs(row['pointer'][0] - point['x']) <= 2 and abs(row['pointer'][1] - point['y']) <= 2
                    for row in frames), {'point': point, 'frames': frames})
        require(not samples[-1]['visible'], 'System cue did not hide promptly')
        return len(frames)

    try:
        build = subprocess.run(['powershell.exe', '-NoProfile', '-ExecutionPolicy', 'Bypass',
            '-File', str(Path(__file__).with_name('compile-cursor.ps1')), '-OutputDirectory', str(out / 'build')],
            capture_output=True)
        (out / 'compile.stdout').write_bytes(build.stdout)
        (out / 'compile.stderr').write_bytes(build.stderr)
        require(build.returncode == 0, 'Fixture compilation failed')
        observer = out / 'build/CursorObserver.exe'
        exe = root / 'Fixture.exe'
        shutil.copy2(out / 'build/Fixture.exe', exe)
        child = subprocess.Popen([str(exe), str(root), 'A'])
        initial = state(lambda s: s['ready'] and s['bounds_ok'] and s['foreground'])
        require(initial['pid'] == child.pid, initial)
        first, app = connect('first')
        pad = initial['pad_point']
        first_pad = {'x': pad['x'] + 90, 'y': pad['y'] + 30}
        original_pointer = position()

        stage = 'semantic_click_uses_virtual_cursor'
        semantic = "await app.click({selector:{role:'button',text:'Increment'}});"
        first.execute(app, semantic)
        state(lambda s: s['increments'] == initial['increments'] + 1)
        helper = marker(first, 'semantic-click')
        handles.append(ProcessHandle(helper['pid'], binary, birth=helper['birth']))
        require(position() == original_pointer, 'UIA click moved the system pointer')
        record(helper=helper)

        stage = 'physical_hover'
        image = first.capture(app, 'before-hover')['view']['image']
        samples = trace(first, app, source('move', pixel(image, first_pad)), helper, first_pad)
        state(lambda s: s['moves'] > initial['moves'])
        hover_frames = check_ring(samples, first_pad)
        hover_pixels = max(row.get('pixels', {}).get('ring', 0) for row in samples)
        record(frames=hover_frames, peak_ring_pixels=hover_pixels)

        stage = 'coordinate_click_pulse'
        image = first.capture(app, 'before-click')['view']['image']
        samples = trace(first, app, source('click', pixel(image, first_pad)), helper, first_pad)
        state(lambda s: s['left_up'] == initial['left_up'] + 1)
        click_frames = check_ring(samples, first_pad)
        click_pixels = max(row.get('pixels', {}).get('ring', 0) for row in samples)
        require(click_pixels > hover_pixels, 'Click pulse is missing')
        record(frames=click_frames, peak_ring_pixels=click_pixels)

        stage = 'physical_scroll'
        scroll = {'x': pad['x'] - 95, 'y': pad['y'] + 25}
        image = first.capture(app, 'before-scroll')['view']['image']
        samples = trace(first, app, source('scroll', {**pixel(image, scroll), 'delta_y': 2}), helper, scroll)
        current = state(lambda s: s['wheel_delta'] == initial['wheel_delta'] - 240)
        scroll_frames = check_ring(samples, scroll)
        record(frames=scroll_frames)

        stage = 'content_drag'
        corner = {'x': scroll['x'], 'y': scroll['y'] - 72}
        end = {'x': scroll['x'] + 190, 'y': corner['y']}
        image = first.capture(app, 'before-drag')['view']['image']
        samples = trace(first, app, source('drag', {'path': [pixel(image, p) for p in (scroll, corner, end)]}), helper, end)
        state(lambda s: s['drag_moves'] > current['drag_moves'] and s['left_up'] == current['left_up'] + 1)
        drag_frames = check_ring(samples, end)
        require(any(row['visible'] and abs(row['x'] - corner['x']) <= 2 and corner['y'] + 8 < row['y'] < scroll['y'] - 8
                    for row in samples), 'Marker did not follow the vertical leg of the accepted drag')
        require(any(row['visible'] and abs(row['y'] - end['y']) <= 2 and corner['x'] + 8 < row['x'] < end['x'] - 8
                    for row in samples), 'Marker did not follow the horizontal leg of the accepted drag')
        record(frames=drag_frames, end=end)

        stage = 'native_title_bar_drag'
        image = first.capture(app, 'before-title-drag')['view']['image']
        x, y, width, _ = image['bounds']
        start = {'x': x + width // 2, 'y': y + 12}
        end = {'x': start['x'] + 24, 'y': start['y'] + 12}
        before = state()['bounds']
        original_pointer = position()
        first.execute(app, source('drag', {'path': [pixel(image, p) for p in (start, end)]}))
        started = time.monotonic()
        state(lambda s: s['bounds']['x'] == before['x'] + 24 and s['bounds']['y'] == before['y'] + 12)
        wait(lambda: at(end, helper), timeout=2)
        marker(first, 'title-bar')
        require(position() == original_pointer, 'Window relocation moved the system pointer')
        record(end=end)

        stage = 'one_minute_idle_and_independent_connection'
        first.execute(app, 'await app.look();')
        _, rejected = first.execute(app, "throw Error('owned idle exception');", allow_error=True)
        require(rejected.get('isError'), 'Expected owned script rejection')
        time.sleep(max(0, 3 - (time.monotonic() - started)))
        require(window_sample(helper)['visible'], 'Marker hid at the previous two-second timeout')
        second, second_app = connect('second')
        second.execute(second_app, semantic)
        second_helper = marker(second, 'second-semantic')
        require(second_helper['pid'] != helper['pid'], 'Connections shared a helper')
        handles.append(ProcessHandle(second_helper['pid'], binary, birth=second_helper['birth']))
        while window_sample(helper)['visible'] and time.monotonic() - started < 64:
            time.sleep(.05)
        elapsed = time.monotonic() - started
        require(58 <= elapsed <= 63 and not window_sample(helper)['visible'], {'elapsed': elapsed})
        require(window_sample(second_helper)['visible'], 'One connection expired the other connection')
        require(handles[0].alive() and handles[1].alive(), 'Idle destroyed a live helper')
        require(position() == original_pointer, 'Visual feedback or idle changed the physical pointer')
        record(seconds=round(elapsed, 3))

        stage = 'same_helper_recovers_and_eof_is_isolated'
        first.execute(app, "await app.click({selector:{role:'button',text:'Save'}});")
        recovered = marker(first, 'after-idle')
        require((helper['pid'], helper['hwnd'], helper['birth']) ==
                (recovered['pid'], recovered['hwnd'], recovered['birth']), recovered)
        report['cleanup'].append(first.close())
        clients.remove(first)
        require(handles[0].wait() == 0, 'First helper failed to exit after EOF')
        second.execute(second_app, semantic)
        require(marker(second, 'after-first-eof')['hwnd'] == second_helper['hwnd'], 'EOF affected other helper')
        record()
        report['passed'] = True
    except BaseException as error:
        report.update(stage=stage, error=type(error).__name__ + ': ' + str(error))
        raise
    finally:
        for client in clients:
            try:
                report['cleanup'].append(client.close())
            except BaseException as error:
                report['cleanup'].append({'mcp_error': str(error)})
                report['passed'] = False
        for handle in handles:
            try:
                code = handle.wait()
                report['cleanup'].append({'helper_pid': handle.pid, 'exit_code': code})
                report['passed'] &= code == 0
            except BaseException as error:
                report['cleanup'].append({'helper_error': str(error)})
                report['passed'] = False
            finally:
                handle.close()
        if child:
            (root / 'stop').write_text('owned normal close', encoding='utf-8')
            try:
                report['fixture_exit'] = child.wait(timeout=8)
                report['passed'] &= child.returncode == 0
            except subprocess.TimeoutExpired:
                child.terminate()
                child.wait(timeout=5)
                report.update(passed=False, forced_fixture_cleanup=True)
        (out / 'summary.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
    require(report['passed'], report)


if __name__ == '__main__':
    main()
