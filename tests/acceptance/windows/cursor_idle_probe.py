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

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import AppClient, artifact_identity, require
from windows.app_protocol_probe import wait, wait_state
from windows.transport import ProcessHandle, read


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
        return 'await app.capture();await app.' + method + '(' + json.dumps(options) + ');'

    def trace(client, app, script, helper):
        request = client.begin_execute(app, script)
        started = time.monotonic()
        samples = []
        while time.monotonic() - started < 1.25:
            samples.append({'seconds': time.monotonic() - started, **window_sample(helper)})
            time.sleep(.008)
        response = client.transport.receive(request, timeout=8)['result']
        result = response['structuredContent']
        require(not response.get('isError') and result['status'] == 'completed', result)
        (out / (stage + '-trace.json')).write_text(json.dumps(samples, indent=2), encoding='utf-8')
        require(all(row['visible'] for row in samples), 'Marker was hidden between accepted operations')
        return samples

    def at(point, helper):
        row = window_sample(helper)
        return row['visible'] and abs(row['x'] - point['x']) <= 2 and abs(row['y'] - point['y']) <= 2

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
        image = first.capture(app, 'target')['view']['image']
        pad = initial['pad_point']
        first_pad = {'x': pad['x'] + 90, 'y': pad['y'] + 30}
        original_pointer = position()

        stage = 'physical_hover_and_coordinate_click'
        first.execute(app, source('move', pixel(image, first_pad)))
        state(lambda s: s['moves'] > initial['moves'])
        helper = marker(first, 'hover')
        handles.append(ProcessHandle(helper['pid'], binary, birth=helper['birth']))
        require(at(first_pad, helper), window_sample(helper))
        first.execute(app, source('click', pixel(image, first_pad)))
        state(lambda s: s['left_up'] == initial['left_up'] + 1)
        require(marker(first, 'coordinate-click')['hwnd'] == helper['hwnd'], 'Marker changed HWND')
        require(position() == original_pointer, 'Visual feedback displaced the physical pointer')
        record(helper=helper)

        stage = 'semantic_click_continuity'
        samples = trace(first, app, "await app.click({selector:{role:'button',text:'Increment'}});", helper)
        current = state(lambda s: s['increments'] == 1)
        visible = marker(first, 'semantic-click')
        require(visible['hwnd'] == helper['hwnd'], 'Semantic click changed helper HWND')
        bounds = current['increment_bounds']
        end = window_sample(helper)
        require(bounds['x'] <= end['x'] < bounds['x'] + bounds['width']
                and bounds['y'] <= end['y'] < bounds['y'] + bounds['height'], {'end': end, 'button': bounds})
        intermediate = {(row['x'], row['y']) for row in samples if end['x'] + 8 < row['x'] < first_pad['x'] - 8}
        require(len(intermediate) >= 4, 'Expected several visible positions between the two endpoints')
        require(position() == original_pointer, 'UIA click moved the physical pointer')
        record(intermediate_positions=len(intermediate), end=end)

        stage = 'scroll_and_content_drag'
        scroll = {'x': pad['x'] - 95, 'y': pad['y'] + 25}
        first.execute(app, source('scroll', {**pixel(image, scroll), 'delta_y': 2}))
        current = state(lambda s: s['wheel_delta'] == initial['wheel_delta'] - 240)
        wait(lambda: at(scroll, helper), timeout=2)
        marker(first, 'scroll')
        corner = {'x': scroll['x'], 'y': scroll['y'] - 72}
        end = {'x': scroll['x'] + 190, 'y': corner['y']}
        samples = trace(first, app, source('drag', {'path': [pixel(image, p) for p in (scroll, corner, end)]}), helper)
        state(lambda s: s['drag_moves'] > current['drag_moves'] and s['left_up'] == current['left_up'] + 1)
        marker(first, 'drag')
        require(at(end, helper), window_sample(helper))
        require(any(abs(row['x'] - corner['x']) <= 2 and corner['y'] + 8 < row['y'] < scroll['y'] - 8
                    for row in samples), 'Marker did not follow the vertical leg of the accepted drag')
        require(any(abs(row['y'] - end['y']) <= 2 and corner['x'] + 8 < row['x'] < end['x'] - 8
                    for row in samples), 'Marker did not follow the horizontal leg of the accepted drag')
        record(end=end)

        stage = 'native_title_bar_drag'
        view = first.capture(app, 'before-title-drag')['view']
        x, y, width, _ = view['windows'][0]['bounds']
        start = {'x': x + width // 2, 'y': y + 12}
        end = {'x': start['x'] + 24, 'y': start['y'] + 12}
        before = state()['bounds']
        first.execute(app, source('drag', {'path': [pixel(view['image'], p) for p in (start, end)]}))
        started = time.monotonic()
        moved = state(lambda s: s['bounds']['x'] == before['x'] + 24 and s['bounds']['y'] == before['y'] + 12)
        wait(lambda: at(end, helper), timeout=2)
        marker(first, 'title-bar')
        record(end=end)

        stage = 'one_minute_idle_and_independent_connection'
        first.execute(app, 'await app.look();')
        _, rejected = first.execute(app, "throw Error('owned idle exception');", allow_error=True)
        require(rejected.get('isError'), 'Expected owned script rejection')
        time.sleep(max(0, 3 - (time.monotonic() - started)))
        require(window_sample(helper)['visible'], 'Marker hid at the previous two-second timeout')
        second, second_app = connect('second')
        moved_pad = moved['pad_point']
        moved_image = second.capture(second_app, 'target')['view']['image']
        second.execute(second_app, source('move', pixel(moved_image, moved_pad)))
        second_helper = marker(second, 'second-hover')
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
        recovery = {'x': moved_pad['x'] - 115, 'y': moved_pad['y'] - 25}
        first.execute(app, source('move', pixel(moved_image, recovery)))
        recovered = marker(first, 'after-idle')
        require((helper['pid'], helper['hwnd'], helper['birth']) ==
                (recovered['pid'], recovered['hwnd'], recovered['birth']), recovered)
        require(at(recovery, helper), window_sample(helper))
        report['cleanup'].append(first.close())
        clients.remove(first)
        require(handles[0].wait() == 0, 'First helper failed to exit after EOF')
        second.execute(second_app, source('move', pixel(moved_image, moved_pad)))
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
