#!/usr/bin/env python3
"""Opt-in owned Chrome submit/traversal probe using only the DTW under test.

No browser driver, DOM input, direct SendInput or external website. Read-only
fixture journals/pixels establish effects independently from DTW's dispatch.
Run with python -X utf8; Pillow is used only for the owned button color oracle.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import traceback
import uuid

from cursor_probe import Mcp, read, require, wait, write
from probe_paths import browser_workbench


def window_diagnostics(owned_pid, owned_title_prefix):
    """Read-only, non-atomic desktop geometry; never records unrelated titles."""
    from ctypes import wintypes as w
    user = ctypes.WinDLL('user32', use_last_error=True)
    user.GetTopWindow.argtypes = [w.HWND]
    user.GetTopWindow.restype = w.HWND
    user.GetWindow.argtypes = [w.HWND, w.UINT]
    user.GetWindow.restype = w.HWND
    user.GetForegroundWindow.restype = w.HWND
    user.GetWindowRect.argtypes = [w.HWND, ctypes.POINTER(w.RECT)]
    user.GetWindowRect.restype = w.BOOL
    user.GetWindowThreadProcessId.argtypes = [w.HWND, ctypes.POINTER(w.DWORD)]
    user.GetWindowThreadProcessId.restype = w.DWORD
    user.GetClassNameW.argtypes = [w.HWND, w.LPWSTR, ctypes.c_int]
    user.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, ctypes.c_int]
    user.IsWindowVisible.argtypes = user.IsIconic.argtypes = [w.HWND]
    user.IsWindowVisible.restype = user.IsIconic.restype = w.BOOL
    user.GetDpiForWindow.argtypes = [w.HWND]
    user.GetDpiForWindow.restype = w.UINT
    user.GetThreadDpiAwarenessContext.restype = w.HANDLE
    user.GetAwarenessFromDpiAwarenessContext.argtypes = [w.HANDLE]
    user.GetAwarenessFromDpiAwarenessContext.restype = ctypes.c_int
    get_style = user.GetWindowLongPtrW if ctypes.sizeof(ctypes.c_void_p) == 8 else user.GetWindowLongW
    get_style.argtypes = [w.HWND, ctypes.c_int]
    get_style.restype = ctypes.c_ssize_t
    try:
        dwm = ctypes.WinDLL('dwmapi')
        dwm.DwmGetWindowAttribute.argtypes = [w.HWND, w.DWORD, w.LPVOID, w.DWORD]
        dwm.DwmGetWindowAttribute.restype = ctypes.c_long
    except OSError:
        dwm = None
    result = {'observed_at_unix_ns': time.time_ns(), 'atomic_snapshot': False,
              'owned_browser_pid': owned_pid, 'foreground_hwnd': user.GetForegroundWindow(),
              'z_order': 'GetTopWindow followed by GW_HWNDNEXT; visible windows only',
              'bounds_coordinates': 'GetWindowRect under the observer thread DPI context; may be virtualized',
              'observer_dpi_context': user.GetThreadDpiAwarenessContext(),
              'window_bound': 1024, 'windows': []}
    result['observer_dpi_awareness'] = user.GetAwarenessFromDpiAwarenessContext(result['observer_dpi_context'])
    result['dpi_awareness_values'] = {'0':'unaware', '1':'system-aware', '2':'per-monitor-aware', '-1':'invalid'}
    hwnd, visited = user.GetTopWindow(None), set()
    while hwnd and len(visited) < result['window_bound']:
        if hwnd in visited:
            result['incomplete_reason'] = 'window_chain_cycle_during_observation'
            break
        visited.add(hwnd)
        if user.IsWindowVisible(hwnd):
            pid, rect = w.DWORD(), w.RECT()
            tid = user.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            class_name, title = ctypes.create_unicode_buffer(256), ctypes.create_unicode_buffer(1024)
            user.GetClassNameW(hwnd, class_name, len(class_name))
            user.GetWindowTextW(hwnd, title, len(title))
            row = {'hwnd': hwnd, 'z_index': len(visited)-1, 'pid': pid.value,
                   'thread_id': tid, 'class': class_name.value,
                   'owned_browser_pid_match': pid.value == owned_pid,
                   'owned_title_match': title.value.startswith(owned_title_prefix),
                   'topmost': bool(get_style(hwnd, -20) & 8), 'minimized': bool(user.IsIconic(hwnd)),
                   'owner_hwnd': user.GetWindow(hwnd, 4), 'dpi': user.GetDpiForWindow(hwnd)}
            if user.GetWindowRect(hwnd, ctypes.byref(rect)):
                row['bounds'] = [rect.left, rect.top, rect.right, rect.bottom]
            else:
                row['bounds_error'] = ctypes.get_last_error()
            if dwm:
                cloaked = w.DWORD()
                status = dwm.DwmGetWindowAttribute(hwnd, 14, ctypes.byref(cloaked), ctypes.sizeof(cloaked))
                if status == 0:
                    row['cloaked'] = cloaked.value
                else:
                    row['cloaked_hresult'] = status
            result['windows'].append(row)
        hwnd = user.GetWindow(hwnd, 2)
    if hwnd and 'incomplete_reason' not in result:
        result['incomplete_reason'] = 'window_bound'
    result['visited_windows'] = len(visited)
    result['owned_title_match_count'] = sum(row['owned_title_match'] for row in result['windows'])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--chrome', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--skip-traversal', action='store_true', help='Collect input events only; keep traversal explicitly unverified')
    args = parser.parse_args()
    source = browser_workbench(__file__, ('server.py', 'index.html', 'reliability.html'))
    require(os.name == 'nt' and sys.flags.utf8_mode, 'Use Windows: python -X utf8')
    binary, chrome = args.dtw.resolve(strict=True), args.chrome.resolve(strict=True)
    output = args.output.resolve()
    require(not output.exists(), 'Use a new evidence directory')
    output.mkdir(parents=True)
    summary = {'passed': False, 'kind': 'owned Chrome reliability through standard DTW MCP',
               'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
               'binary': str(binary), 'chrome': str(chrome),
               'fixture_sha256': {name: hashlib.sha256((source/name).read_bytes()).hexdigest()
                                  for name in ('server.py', 'index.html', 'reliability.html')},
               'live_llm_host': False, 'release_artifact': False, 'cases': [], 'cleanup': [],
               'limitations': ['Owned fixture, not GitHub reproduction or arbitrary website certification',
                               'Pixel point uses an owned fixture color oracle, not model vision',
                               'Browser trust flags do not authenticate the OS input source',
                               'Background receipt route does not establish background desktop occupancy']}
    server = browser = client = None
    app = window_ref = None
    case = 'reliability-' + uuid.uuid4().hex[:12]

    def diagnose(label):
        # Observation failures must not hide the original dispatch/capture error.
        try:
            path = output/(label+'-windows.json')
            write(path, window_diagnostics(browser.pid, 'DTW Reliability '+case))
            return {'path': str(path)}
        except BaseException:
            return {'diagnostic_failure': traceback.format_exc()}

    try:
        from PIL import Image, ImageChops
        from ctypes import wintypes
        user = ctypes.WinDLL('user32', use_last_error=True)
        user.GetForegroundWindow.restype = wintypes.HWND
        def desktop():
            point = wintypes.POINT()
            require(user.GetPhysicalCursorPos(ctypes.byref(point)), 'Physical cursor observation failed')
            return {'foreground_hwnd':user.GetForegroundWindow(), 'physical_cursor':[point.x,point.y]}
        web = output / 'web'
        with (output/'server.stdout').open('wb') as stdout, (output/'server.stderr').open('wb') as stderr:
            server = subprocess.Popen([sys.executable, '-X', 'utf8', str(source/'server.py'),
                                       '--output', str(web), '--case', case], stdout=stdout, stderr=stderr,
                                      creationflags=subprocess.CREATE_NO_WINDOW)
        def address():
            require(server.poll() is None, 'Owned fixture server exited')
            try:
                return json.loads((output/'server.stdout').read_text(encoding='utf-8'))['url']
            except (ValueError, KeyError):
                return None
        url = wait(address) + 'reliability'
        summary['url'] = url
        browser = subprocess.Popen([str(chrome), '--user-data-dir='+str(output/'profile'),
                                    '--no-first-run', '--no-default-browser-check',
                                    '--force-renderer-accessibility', '--window-position=50,30',
                                    '--window-size=1150,1000', '--new-window', url])
        summary['owned_browser_pid'] = browser.pid
        client = Mcp(binary, output/'mcp')
        def owned_window():
            catalogue = client.tool('apps', {})
            identifier = next((row['app'] for row in catalogue['apps']
                               if row['app'].casefold() == str(chrome).casefold()), None)
            if not identifier:
                return None
            rows, _ = client.execute(identifier, 'owned-windows', 'return (await app.windows()).windows;')
            selected = next((row['window_ref'] for row in rows
                             if row['title'].startswith('DTW Reliability '+case)), None)
            return (identifier, selected) if selected else None
        app, window_ref = wait(owned_window, timeout=20)
        wr = json.dumps(window_ref)
        client.execute(app, 'prepare-window', f'await app.focus({{window_ref:{wr}}});return await app.keypress({{window_ref:{wr},keys:["ESC"]}});')
        wait(lambda: (state if (state := read(web/'state.json')) and
                      any(row['kind'] == 'ready' for row in state['reliability']['events']) else None))

        def snapshot(label):
            state = wait(lambda: read(web/'state.json'))
            write(output/(label+'-state.json'), state)
            return state['reliability']

        def labelled(label):
            client.execute(app, label+'-label', f'''return await app.setValue({{window_ref:{wr},
                selector:{{role:'text_field',text:'Run label'}},value:{json.dumps(label)}}});''')

        def button_point(capture, path):
            image = Image.open(path).convert('RGB')
            mask = Image.new('L', image.size, 255)
            for channel, value in zip(image.split(), (83,165,131)):
                mask = ImageChops.multiply(mask, channel.point([255 if i == value else 0 for i in range(256)]))
            box = mask.getbbox()
            require(box, 'Owned Create button color was not visible in this fresh capture')
            return (box[0]+box[2])//2, (box[1]+box[3])//2

        for trusted in (False, True):
            if trusted:
                client.execute(app, 'enable-trusted-pointer', f'''return await app.press({{window_ref:{wr},
                    selector:{{role:'checkbox',text:'Require trusted pointer sequence'}}}});''')
                wait(lambda: (state if (state := read(web/'state.json')) and
                              any(row['kind']=='mode_changed' and row.get('mode')=='trusted-pointer'
                                  for row in state['reliability']['events']) else None))
            for method in ('semantic-press', 'pixel-click'):
                label = ('trusted' if trusted else 'normal')+'-'+method
                row = {'name': label, 'method': method, 'trusted_pointer_mode': trusted}
                labelled(label)
                before = snapshot(label+'-before')
                try:
                    if method == 'semantic-press':
                        result, ledger = client.execute(app, label, f'''return await app.press({{window_ref:{wr},
                            selector:{{role:'button',text:'Create test record'}}}});''')
                    else:
                        # Diagnostic preflight for the baseline's capture path;
                        # focus does not force the following click's route.
                        capture, _ = client.execute(app, label+'-capture', f'await app.focus({{window_ref:{wr}}});return await app.capture({{window_ref:{wr}}});')
                        x, y = button_point(capture, client.images[-1])
                        row['pixel_point'] = {'x':x, 'y':y, 'observation_ref':capture['observation_ref']}
                        row['desktop_before_click'] = desktop()
                        result, ledger = client.execute(app, label, f'''return await app.click({{window_ref:{wr},
                            observation_ref:{json.dumps(capture['observation_ref'])},x:{x},y:{y}}});''')
                        row['desktop_after_click'] = desktop()
                    row['receipt'] = result
                except BaseException:
                    row['dispatch_failure'] = traceback.format_exc()
                    row['failure_time_window_diagnostics'] = diagnose(label)
                # Wait only for readback. Never repeat a submit after a failure,
                # no receipt, or an uncertain/late effect.
                try:
                    wait(lambda: (state if (state := read(web/'state.json')) and
                                  len(state['reliability']['attempts']) > len(before['attempts']) else None), timeout=3)
                except TimeoutError:
                    row['no_attempt_observed_within_seconds'] = 3
                after = snapshot(label+'-after')
                row['events'] = [event for event in after['events'][len(before['events']):] if event['run']==label]
                row['attempts'] = after['attempts'][len(before['attempts']):]
                row['submissions'] = after['submissions'][len(before['submissions']):]
                row['ordinary_submit_passed'] = (len(row['attempts'])==1 and len(row['submissions'])==1)
                row['observed_once'] = len(row['attempts']) <= 1 and len(row['submissions']) <= 1
                summary['cases'].append(row)
                write(output/'progress.json', summary)

        def traverse(filtered):
            name = 'filtered-links' if filtered else 'unfiltered-links'
            query = ",role:'link'" if filtered else ''
            source_text = f'''let page=await app.observe({{window_ref:{wr},limit:100{query}}});
const names=[], pages=[], fragments=[], unnamed=[];
for(let count=0;count<128;count++) {{
  pages.push({{coverage:page.coverage,elements:page.elements.length,has_more:!!page.next}});
  for(const element of page.elements) {{
    if(element.role==='link' && element.name && element.name.startsWith('Fixture link ')) names.push(element.name);
    if(element.name && element.name.includes('fragment')) fragments.push({{role:element.role,name:element.name}});
    if(element.role==='button' && !element.name) unnamed.push({{element_ref:element.element_ref,can_press:element.can_press}});
  }}
  if(!page.next) return {{complete:page.coverage.complete,terminal:page.coverage,names,pages,fragments,unnamed}};
  try {{page=await app.observe({{cursor:page.next}});}}
  catch(error) {{return {{complete:false,terminal:{{reason:'native_read_failed',code:error.code,message:error.message}},names,pages,fragments,unnamed}};}}
}}
return {{complete:false,terminal:{{reason:'probe_page_bound'}},names,pages,fragments,unnamed}};'''
            try:
                data, _ = client.execute(app, name, source_text)
                write(output/(name+'.json'), data)
                return data
            except BaseException:
                data = {'complete':False, 'probe_failure':traceback.format_exc()}
                ledger = read(client.output/(name+'-ledger.json'))
                if ledger:
                    data['partial_pages'] = [step['discovery'] for step in ledger.get('script',{}).get('steps',[])
                                             if step.get('discovery')]
                write(output/(name+'.json'), data)
                return data
        if args.skip_traversal:
            filtered = unfiltered = {'complete':False, 'skipped':True}
        else:
            client.execute(app, 'load-link-catalogue', f'''return await app.press({{window_ref:{wr},
                selector:{{role:'button',text:'Load 360-link catalogue'}}}});''')
            wait(lambda: (state if (state := read(web/'state.json')) and
                          any(row['kind']=='ready' and row.get('target')=='catalogue'
                              for row in state['reliability']['events']) else None))
            filtered, unfiltered = traverse(True), traverse(False)
        comparison = {'filtered_complete':filtered['complete'], 'unfiltered_complete':unfiltered['complete']}
        if filtered['complete'] and unfiltered['complete']:
            expected = {f'Fixture link {i:03}' for i in range(1,361)}
            left, right = set(filtered['names']), set(unfiltered['names'])
            comparison.update(filtered_count=len(left),unfiltered_count=len(right),equal=left==right,
                              expected_exact=left==right==expected)
        else:
            comparison.update(status='unproven',reason='Only fully paginated complete sets can be compared')
        summary['traversal'] = comparison
        # The browser's queued journal can settle after the immediate snapshot.
        # Classify final effects by the action's immutable label, preserving
        # early readback separately and never replaying a submission.
        final = snapshot('final')
        for row in summary['cases']:
            label = row['name']
            row['early_readback'] = {key:row[key] for key in ('events','attempts','submissions')}
            for key in ('events','attempts','submissions'):
                row[key] = [item for item in final[key] if item['run']==label]
            row['ordinary_submit_passed'] = len(row['attempts'])==1 and len(row['submissions'])==1
            row['observed_once'] = len(row['attempts'])<=1 and len(row['submissions'])<=1
        summary['input_passed'] = (all(row['observed_once'] and not row.get('dispatch_failure') for row in summary['cases'])
                             and all(row['ordinary_submit_passed'] for row in summary['cases'] if not row['trusted_pointer_mode'])
                             and all(len(row['submissions'])==1 for row in summary['cases'] if row['name']=='trusted-pixel-click'))
        summary['passed'] = summary['input_passed'] and (args.skip_traversal or comparison.get('expected_exact', False))
        summary['passed_scope'] = 'input only; traversal unverified' if args.skip_traversal else 'input and complete traversal comparison'
    except BaseException:
        summary['failure'] = traceback.format_exc()
    finally:
        if client and app and window_ref and browser and browser.poll() is None:
            close_error = False
            try:
                client.execute(app, 'close-owned-browser', f'return await app.keypress({{window_ref:{json.dumps(window_ref)},keys:["ALT","F4"]}});')
            except BaseException:
                close_error = True
                summary['passed'] = False
                summary['cleanup'].append({'browser_close_error':traceback.format_exc(),
                                           'failure_time_window_diagnostics':diagnose('close-owned-browser')})
            # A failed post-dispatch verification can follow an actual close.
            # Preserve that failure, observe process exit, and never replay close.
            started = time.monotonic()
            observation = {'after_close_error':close_error, 'timeout_seconds':10,
                           'close_replayed':False}
            try:
                observation['returncode'] = browser.wait(timeout=10)
                if observation['returncode'] != 0:
                    summary['passed'] = False
            except subprocess.TimeoutExpired:
                observation['timed_out'] = True
                summary['passed'] = False
            observation['elapsed_seconds'] = time.monotonic()-started
            summary['cleanup'].append({'owned_browser_exit_observation':observation})
        if client:
            try:
                summary['cleanup'].append(client.close())
            except BaseException:
                summary['passed'] = False
                summary['cleanup'].append({'mcp_error':traceback.format_exc()})
        if browser and browser.poll() is None:
            browser.terminate()
            summary['passed'] = False
            summary['cleanup'].append({'forced_owned_browser_exit':browser.wait(timeout=5)})
        if server:
            if server.poll() is None:
                (output/'web'/'stop').touch()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.terminate()
            summary['cleanup'].append({'fixture_server_exit':server.wait(timeout=5)})
        write(output/'summary.json', summary)
        print(json.dumps(summary, ensure_ascii=True))
    return 0 if summary['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
