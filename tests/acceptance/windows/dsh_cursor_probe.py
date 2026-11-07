"""Read-only physical arrow/pulse observation around actual DSH model input."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import artifact_identity, require
from dsh_client import DshClient, MODEL_BOUNDARY
from windows.app_protocol_probe import wait_state
from windows.desktop_probe import children
from windows.transport import ProcessHandle, read


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('dsh', 'package', 'fixture-binary', 'observer', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    root = out / 'fixture'
    root.mkdir()
    package = args.package.resolve(strict=True)
    binary = package / 'bin/dtw.exe'
    exe = out / 'CursorFixture.exe'
    shutil.copy2(args.fixture_binary, exe)
    report = {'passed': False, 'artifact': artifact_identity(binary, package),
        'business_actor': 'actual DSH model through full Cordis plugin',
        'observer': 'read-only kernel/window identity and physical desktop pixels; no observer input'}
    child = client = None
    session = None
    handles = []
    stop = threading.Event()
    observed = {}
    observer_errors = []
    watcher = None
    patch = out / 'cordis.patch.yml'
    patch.write_text("- id: acp\n  name: '@deepseek-ai/dsh-acp'\n  config:\n"
        "    provider: deepseek-account\n    model: deepseek-flash\n- insert:\n"
        "    - id: desktop-world-plugin\n      name: '" + (package / 'cordis/index.mjs').as_uri() + "'\n", encoding='utf-8')
    try:
        child = subprocess.Popen([str(exe), str(root), 'A'])
        initial = wait_state(root, lambda value: value['ready'])
        client = DshClient(args.dsh, patch, out / 'dsh', Path.cwd())
        client.call('initialize', {'protocolVersion': 1, 'clientCapabilities': {},
            'clientInfo': {'name': 'dtw-physical-cursor-observer', 'version': '1'}})
        report['session'] = client.call('session/new', {'cwd': str(Path.cwd()), 'mcpServers': []})
        session = report['session']['sessionId']
        client.call('session/set_config_option', {'sessionId': session, 'configId': 'reasoning_effort', 'value': 'high'})
        pending = [client.process.pid]
        while pending:
            for row in children(pending.pop()):
                pending.append(row['pid'])
                if row['exe'].casefold() == 'dtw.exe':
                    try:
                        handles.append(ProcessHandle(row['pid'], binary))
                    except (AssertionError, OSError):
                        continue
        require(handles, 'Actual DSH-owned installed MCP identity absent')
        owners = {handle.pid for handle in handles}
        report['dsh_owned_dtw_processes'] = [{'pid': h.pid, 'birth': h.birth} for h in handles]

        def observe():
            count = 0
            pulse_at = None
            try:
                while not stop.is_set():
                    state = read(root / 'state.json')
                    if not state or state['moves'] <= initial['moves']:
                        time.sleep(.005)
                        continue
                    phase = 'arrow' if state['left_up'] == initial['left_up'] else 'pulse'
                    if phase == 'pulse' and pulse_at is None:
                        pulse_at = time.monotonic()
                    if pulse_at is not None and time.monotonic() - pulse_at > .35:
                        phase = 'after-pulse'
                    if phase in observed:
                        time.sleep(.005)
                        continue
                    count += 1
                    path = out / ('physical-%03d.json' % count)
                    subprocess.run([str(args.observer.resolve()), '--snapshot', str(path), initial['hwnd']],
                        check=True, creationflags=subprocess.CREATE_NO_WINDOW)
                    value = read(path)
                    candidates = [row for row in value['windows'] if row['parent'] in owners
                        and row['path'] and Path(row['path']).resolve() == binary
                        and row['visible'] and max(row['palette_pixels']) >= 20]
                    if value['foreground_before'] and value['foreground_after'] and value['desktop_image'] and len(candidates) == 1:
                        observed[phase] = {'snapshot': str(path), 'value': value, 'helper': candidates[0]}
                    if len(observed) == 3:
                        return
            except BaseException as error:
                observer_errors.append(str(error))
        watcher = threading.Thread(target=observe, daemon=True)
        watcher.start()
        prompt = f'''先加载本完整插件的 desktop-world Skill，读取安装包中的 API。只通过 desktop-world apps/exec 操作我拥有的 {exe}，标题 DTW Windows Smoke A。
禁止 shell、文件/HTTP/CDP 等旁路，文档之外不准读取文件；不要操作其他应用。
这是只读观察器采集视觉反馈的任务：先观察截图，在蓝色 Owned pointer / wheel pad 中央找一个点。
在同一条 exec 中：对这个点 move 悬停，停留约 900ms；再次 capture 后在同一点单击一次，再停留约 900ms。
停留可以用 Date.now 的短循环实现，整条脚本保持在 30 秒内。不要点 Increment、Save 或其他控件，不要拖动。
最后截图并报告；派发不确定不得重放。业务应只有这一次悬停和一次单击，观察器不会发出输入。'''
        prompt += MODEL_BOUNDARY
        (out / 'prompt.txt').write_text(prompt, encoding='utf-8')
        started = time.time() * 1000
        report['prompt_result'] = client.call('session/prompt', {'sessionId': session,
            'prompt': [{'type': 'text', 'text': prompt}]}, timeout=360)
        report['prompt_interval'] = {'start_ms': started, 'end_ms': time.time() * 1000}
        stop.set()
        watcher.join(timeout=5)
        require(not watcher.is_alive() and not observer_errors, observer_errors)
        require(set(observed) == {'arrow', 'pulse', 'after-pulse'}, {'observed_phases': list(observed)})
        state = wait_state(root, lambda value: value['left_up'] == initial['left_up'] + 1)
        require(state['increments'] == 0 and state['saves'] == 0, 'extra business effect')
        from PIL import Image
        palettes = [(178,132,255),(247,132,170),(96,218,174),(244,178,66),(76,204,224),
                    (221,113,236),(232,82,98),(184,220,54),(80,126,236)]
        background = (173,216,230)
        arrow, pulse, after = (observed[key] for key in ('arrow', 'pulse', 'after-pulse'))
        require(len({row['helper']['hwnd'] for row in (arrow, pulse, after)}) == 1, 'helper window changed')
        for row in (arrow, pulse, after):
            style = row['helper']['ex_style']
            require(style & 0x08080020 == 0x08080020 and not style & 8, 'cursor window styles changed')
        helper = pulse['helper']
        scale = helper['dpi'] / 96
        hotspot = round(14 * scale)
        color = palettes[max(range(9), key=lambda index: helper['palette_pixels'][index])]
        expected = tuple(round(c * .28 + b * .72) for c, b in zip(color, background))
        images = [Image.open(row['value']['desktop_image']).convert('RGB') for row in (arrow, pulse, after)]
        points = []
        import math
        for index in range(16):
            dx, dy = round(8 * scale * math.cos(index * math.pi / 8)), round(8 * scale * math.sin(index * math.pi / 8))
            values = []
            for row, image in zip((arrow, pulse, after), images):
                bounds, target = row['helper']['bounds'], row['value']['target_bounds']
                values.append(image.getpixel((bounds['x'] + hotspot + dx - target['x'], bounds['y'] + hotspot + dy - target['y'])))
            if values[0] == background and values[2] == background and all(abs(a-b) <= 2 for a,b in zip(values[1], expected)):
                points.append({'offset': [dx,dy], 'normal': values[0], 'pulse': values[1], 'after': values[2]})
        require(len(points) >= 6, {'pulse_ring_samples': points, 'expected_blend': expected})
        report['physical_phases'] = {key: {'snapshot': value['snapshot'], 'helper': value['helper'],
            'image': value['value']['desktop_image']} for key,value in observed.items()}
        report['pulse_ring_samples'] = points
        report['final_state'] = state
        report['passed'] = True
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error)
        raise
    finally:
        stop.set()
        if watcher:
            watcher.join(timeout=5)
        if client:
            if session:
                try:
                    client.call('session/close', {'sessionId': session}, timeout=30)
                except BaseException as error:
                    report.update(passed=False, session_close_error=str(error))
            report['dsh_cleanup'] = client.close()
            report['passed'] &= report['dsh_cleanup']['exit_code'] == 0 and not report['dsh_cleanup']['forced'] and report['dsh_cleanup']['reader_joined']
        report['dtw_cleanup'] = []
        for handle in handles:
            try:
                code = handle.wait(timeout=8)
                report['dtw_cleanup'].append({'pid': handle.pid, 'exit': code})
                report['passed'] &= code == 0
            except BaseException as error:
                report.update(passed=False, dtw_cleanup_error=str(error))
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
        (out / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        (out / 'rpc-intervals.json').write_text(json.dumps([
            {'name': 'actual-DSH-cursor-prompt', **report['prompt_interval']}
        ] if 'prompt_interval' in report else [], indent=2), encoding='utf-8')
        print(json.dumps({key: report.get(key) for key in ('passed', 'error')}), flush=True)
    require(report['passed'], report)


if __name__ == '__main__':
    main()
