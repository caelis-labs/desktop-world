#!/usr/bin/env python3
"""Opt-in current two-tool Chromium audit with a disposable profile and local page.

Only DTW supplies desktop input. Independent HTTP events establish page effects.
Protects existing browser windows by checking the owned title before every script.
"""
import argparse
import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import subprocess
import sys
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import AppClient, artifact_identity, require
from windows.transport import ProcessHandle


def owned_dialogs(pid,executable,birth,diagnostics=False):
    """Prove the retained owner process and same-executable native dialog helper."""
    import ctypes
    from ctypes import wintypes as w
    user=ctypes.WinDLL('user32',use_last_error=True)
    callback=ctypes.WINFUNCTYPE(w.BOOL,w.HWND,w.LPARAM)
    user.EnumWindows.argtypes=[callback,w.LPARAM]
    user.GetWindowThreadProcessId.argtypes=[w.HWND,ctypes.POINTER(w.DWORD)]
    user.GetWindowTextW.argtypes=[w.HWND,w.LPWSTR,ctypes.c_int]
    user.GetClassNameW.argtypes=[w.HWND,w.LPWSTR,ctypes.c_int]
    user.GetWindow.argtypes=[w.HWND,w.UINT];user.GetWindow.restype=w.HWND
    user.GetAncestor.argtypes=[w.HWND,w.UINT];user.GetAncestor.restype=w.HWND
    user.IsWindowVisible.argtypes=[w.HWND];user.IsWindowEnabled.argtypes=[w.HWND]
    user.GetForegroundWindow.restype=w.HWND
    foreground=user.GetForegroundWindow()
    root_process=ProcessHandle(pid,executable,birth=birth)
    result=[]
    candidates=[]
    @callback
    def observe(hwnd,_):
        owner_pid=w.DWORD()
        user.GetWindowThreadProcessId(hwnd,ctypes.byref(owner_pid))
        actual_pid=owner_pid.value
        if not user.IsWindowVisible(hwnd):return True
        kind=ctypes.create_unicode_buffer(256);user.GetClassNameW(hwnd,kind,256)
        owner=user.GetWindow(hwnd,4)
        root=user.GetAncestor(owner,3) if owner else None
        root_pid=w.DWORD()
        if root:user.GetWindowThreadProcessId(root,ctypes.byref(root_pid))
        candidates.append({'hwnd':int(hwnd),'pid':actual_pid,'class':kind.value,'owner':int(owner or 0),'root_owner':int(root or 0),'root_owner_pid':root_pid.value,'foreground':hwnd==foreground})
        if kind.value=='#32770' and root_pid.value==pid:
            try: helper=ProcessHandle(actual_pid,executable)
            except (AssertionError,OSError):return True
            helper.close()
            title=ctypes.create_unicode_buffer(512);user.GetWindowTextW(hwnd,title,512)
            result.append({'hwnd':int(hwnd),'pid':actual_pid,'owner':int(root),'title':title.value,'owner_disabled':not bool(user.IsWindowEnabled(root))})
        return True
    try: require(user.EnumWindows(observe,0),'Native dialog inventory failed')
    finally: root_process.close()
    proven_pids={pid, *(row['pid'] for row in result)}
    candidates=[row for row in candidates if row['pid'] in proven_pids or row['root_owner_pid']==pid or row['foreground']]
    return {'proven':result,'candidates':candidates} if diagnostics else result


def owned_browser_pid(executable,marker,not_before):
    import ctypes
    from ctypes import wintypes as w
    user=ctypes.WinDLL('user32',use_last_error=True)
    callback=ctypes.WINFUNCTYPE(w.BOOL,w.HWND,w.LPARAM)
    user.EnumWindows.argtypes=[callback,w.LPARAM]
    user.GetWindowThreadProcessId.argtypes=[w.HWND,ctypes.POINTER(w.DWORD)]
    user.GetWindowTextW.argtypes=[w.HWND,w.LPWSTR,ctypes.c_int]
    found=set()
    @callback
    def observe(hwnd,_):
        pid=w.DWORD();user.GetWindowThreadProcessId(hwnd,ctypes.byref(pid))
        try: handle=ProcessHandle(pid.value,executable)
        except (AssertionError,OSError):return True
        try:
            if handle.birth < not_before:return True
            title=ctypes.create_unicode_buffer(512);user.GetWindowTextW(hwnd,title,512)
            if title.value.startswith(marker):found.add(pid.value)
        finally:handle.close()
        return True
    require(user.EnumWindows(observe,0),'Owned browser window enumeration failed')
    require(len(found)<=1,{'owned_browser_pids':sorted(found)})
    return next(iter(found),None)


def close_owned_browser(pid, executable, birth, marker):
    """Normal WM_CLOSE cleanup of an exact owned neighbour, never other windows."""
    import ctypes
    from ctypes import wintypes as w
    retained = ProcessHandle(pid, executable, birth=birth)
    user = ctypes.WinDLL('user32', use_last_error=True)
    callback = ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
    user.EnumWindows.argtypes = [callback, w.LPARAM]
    user.GetWindowThreadProcessId.argtypes = [w.HWND, ctypes.POINTER(w.DWORD)]
    user.GetWindowTextW.argtypes = [w.HWND, w.LPWSTR, ctypes.c_int]
    user.PostMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
    windows = []
    @callback
    def collect(hwnd, unused):
        owner = w.DWORD()
        user.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid:
            title = ctypes.create_unicode_buffer(512)
            user.GetWindowTextW(hwnd, title, 512)
            if title.value.startswith(marker):
                windows.append(hwnd)
        return True
    try:
        require(user.EnumWindows(collect, 0) and len(windows) == 1, {'owned_neighbor_windows': len(windows)})
        require(retained.alive() and user.PostMessageW(windows[0], 0x10, 0, 0), 'normal neighbour close refused')
    finally:
        retained.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', required=True, type=Path)
    parser.add_argument('--chrome', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--review-chooser-hover', action='store_true',
                        help='Publish an actual chooser PNG and wait for a hash-bound pixel hover answer')
    parser.add_argument('--allow-decoration-image-limit', action='store_true',
                        help='Keep known native tooltip capture unsupported; verify semantic file flow and fresh post-tooltip PNG separately')
    parser.add_argument('--observer', type=Path, help='Compiled read-only native snapshot observer for the proven chooser')
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    marker = 'DTW Browser Audit ' + out.name
    destination_marker = marker + ' Destination ready'
    neighbour_marker = 'DTW Unchanged Neighbour ' + out.name
    observed = {'text': '', 'clicks': 0, 'scroll_x': 0, 'scroll_y': 0}
    neighbour_state = {'text': 'NEIGHBOUR_SENTINEL', 'clicks': 0, 'scroll_x': 0, 'scroll_y': 0, 'input_events': 0}
    lock = threading.Lock()
    html = '''<!doctype html><meta charset="utf-8"><title>__TITLE__</title>
<style>body{font:18px sans-serif;margin:20px}h1{font-size:20px;margin:0 0 8px}p{margin:8px 0}textarea{width:90%;height:70px}button{padding:8px}#pane{height:430px;width:90%;overflow:scroll;border:3px solid #579}#content{width:2400px;height:2400px;background:repeating-linear-gradient(45deg,#def,#def 60px,#cde 60px,#cde 120px)}</style>
<h1>__TITLE__</h1><textarea aria-label="Audit Unicode editor" autofocus></textarea><br>
<button id="increment">Audit increment</button><button id="save">Audit save</button>
<input id="choose" type="file" aria-label="Audit choose file">
<p id="status">Ready</p><div id="pane"><div id="content">Owned scroll area</div></div>
<script>const e=document.querySelector('textarea'),p=document.querySelector('#pane');
function publish(v){fetch('/event',{method:'POST',body:JSON.stringify(v)})}
e.oninput=()=>publish({text:e.value});
document.querySelector('#increment').onclick=()=>{publish({increment:true});e.focus()};
document.querySelector('#save').onclick=()=>{publish({saved:e.value});e.focus()};
document.querySelector('#choose').onclick=e=>publish({picker_requested:true,picker_trusted:e.isTrusted,picker_activation:navigator.userActivation.isActive});
document.querySelector('#choose').onchange=async event=>{const f=event.target.files[0];if(f)publish({selected_file:f.name,selected_text:await f.text()})};
p.onscroll=()=>publish({scroll_x:p.scrollLeft,scroll_y:p.scrollTop});
window.onload=()=>{e.focus();publish({ready:true,dpr:devicePixelRatio,text:e.value})};</script>'''.replace('__TITLE__', marker)
    neighbour_html = html.replace(marker, neighbour_marker).replace("fetch('/event'", "fetch('/neighbour-event'")
    neighbour_html = neighbour_html.replace('autofocus></textarea>', 'autofocus>NEIGHBOUR_SENTINEL</textarea>')
    neighbour_html += "<script>for(const kind of ['keydown','mousedown','wheel'])document.addEventListener(kind,()=>publish({neighbour_input:true}));</script>"

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path == '/delayed-destination':
                with lock:
                    observed['destination_requests'] = observed.get('destination_requests', 0) + 1
                time.sleep(.75)
                page = html.replace(marker, destination_marker).replace('ready:true', 'ready:true,destination_ready:true')
            else:
                page = neighbour_html if self.path.startswith('/neighbour') else html
            body = page.encode('utf-8')
            self.send_response(200)
            self.send_header('Content-Type', 'text/html; charset=utf-8')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            value = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            with lock:
                destination = neighbour_state if self.path == '/neighbour-event' else observed
                if value.pop('increment', False):
                    destination['clicks'] += 1
                if value.pop('neighbour_input', False):
                    destination['input_events'] += 1
                destination.update(value)
                journal = 'neighbour-events.jsonl' if destination is neighbour_state else 'events.jsonl'
                with (out / journal).open('a', encoding='utf-8') as log:
                    log.write(json.dumps(destination, ensure_ascii=True) + '\n')
            self.send_response(204)
            self.end_headers()

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    report = {'artifact': artifact_identity(args.dtw), 'cases': [], 'marker': marker,
        'browser':str(args.chrome.resolve()),'browser_sha256':hashlib.sha256(args.chrome.read_bytes()).hexdigest()}
    child = client = launcher = browser_handle = None
    neighbour = neighbour_launcher = neighbour_handle = None
    neighbour_pid = None
    browser_pid = None
    app = str(args.chrome.resolve()).lower()

    def execute(source):
        # Every operation verifies the selected process/window still belongs to this run.
        guard = 'const v=await app.look();if(!v.windows[0]?.title.startsWith(' + json.dumps(marker) + '))throw new Error("Owned Chrome is not the selected window");'
        data, response = client.execute(app, guard + source, allow_error=True)
        for i, content in enumerate(response.get('content', [])):
            if content.get('type') == 'image':
                (out / ('image-%03d-%d.png' % (len(client.metrics), i))).write_bytes(base64.b64decode(content['data']))
        return data

    def wait(predicate, timeout=8):
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            with lock:
                state = dict(observed)
            if predicate(state):
                return state
            time.sleep(.03)
        raise AssertionError('HTTP effect did not settle: ' + repr(state))

    def case(name, source, predicate=None, native_dialog=False):
        row = {'name': name}
        try:
            data = execute(source)
            row['response'] = data
            require(not data.get('error'), data)
            if predicate:
                row['effect'] = wait(predicate)
            if native_dialog:
                until=time.monotonic()+8
                while True:
                    dialogs=owned_dialogs(browser_pid,args.chrome.resolve(),browser_handle.birth)
                    if dialogs: break
                    if time.monotonic()>=until:
                        row['native_observation']=owned_dialogs(browser_pid,args.chrome.resolve(),browser_handle.birth,diagnostics=True)
                        client.capture(app,'unproven-native-dialog')
                        raise AssertionError('Owned native Open dialog did not appear')
                    time.sleep(.03)
                view,_=client.execute(app,'await app.look({full:true});')
                require(any(w['title']==dialogs[0]['title'] for w in view['view']['windows']),view)
                row['native_dialogs']=dialogs
                native_capture, _ = client.execute(app, 'await app.capture({full:true});')
                image_error = native_capture.get('view', {}).get('image', {}).get('error')
                if image_error:
                    # The ordinary pointer may already expose a tooltip at
                    # first open. Keep that refusal without moving the cursor.
                    proof = owned_dialogs(browser_pid, args.chrome.resolve(), browser_handle.birth, diagnostics=True)
                    require(args.allow_decoration_image_limit and image_error.get('code') == 'windows_surface_unsupported'
                            and any(w['class'] in ('tooltips_class32', 'SysShadow') for w in proof['candidates']), native_capture)
                    row['initial_decoration_image_limit'] = {'capture': native_capture, 'native': proof}
                else:
                    client.capture(app,'native-open-dialog')
            row['verified'] = True
        except Exception as error:
            row.update(verified=False, error=str(error))
        report['cases'].append(row)
        print(name + ': ' + str(row['verified']), flush=True)

    try:
        neighbour = subprocess.Popen([str(args.chrome), '--user-data-dir=' + str(out / 'neighbour-profile'),
            '--no-first-run', '--no-default-browser-check', '--disable-sync', '--disable-background-mode', '--guest',
            '--force-renderer-accessibility', '--new-window', '--window-position=1150,200', '--window-size=800,800',
            'http://127.0.0.1:%d/neighbour' % server.server_address[1]])
        neighbour_launcher = ProcessHandle(neighbour.pid, args.chrome.resolve())
        until = time.monotonic() + 10
        while neighbour_pid is None:
            neighbour_pid = owned_browser_pid(args.chrome.resolve(), neighbour_marker, neighbour_launcher.birth)
            require(time.monotonic() < until, 'owned same-executable neighbour did not launch')
            if neighbour_pid is None: time.sleep(.03)
        neighbour_handle = ProcessHandle(neighbour_pid, args.chrome.resolve(), terminate=True)
        while not neighbour_state.get('ready'):
            require(time.monotonic() < until, 'owned neighbour did not publish ready state')
            time.sleep(.03)
        report['neighbour_pid'] = neighbour_pid
        child = subprocess.Popen([str(args.chrome), '--user-data-dir=' + str(out / 'profile'),
            '--no-first-run', '--no-default-browser-check', '--disable-sync', '--disable-background-mode', '--guest',
            '--force-renderer-accessibility', '--new-window', '--window-position=300,200', '--window-size=1000,800',
            'http://127.0.0.1:%d/' % server.server_address[1]])
        launcher=ProcessHandle(child.pid,args.chrome.resolve())
        wait(lambda s: s.get('ready'))
        title_deadline=time.monotonic()+8
        while browser_pid is None:
            browser_pid=owned_browser_pid(args.chrome.resolve(),marker,launcher.birth)
            if browser_pid is not None:break
            require(time.monotonic()<title_deadline,'No newly launched browser process published the owned title')
            time.sleep(.03)
        browser_handle=ProcessHandle(browser_pid,args.chrome.resolve(),terminate=True)
        report['launcher_pid']=child.pid
        report['browser_pid']=browser_pid
        client = AppClient(args.dtw.resolve(), out / 'protocol')
        case('chrome_look_capture', 'await app.capture({full:true});')
        destination = f'http://127.0.0.1:{server.server_address[1]}/delayed-destination'
        case('chrome_same_script_navigation_readback',
             'await app.keypress("CTRL+L");await app.typeText(' + json.dumps(destination) + ');'
             'await app.keypress("ENTER");const after=await app.look();'
             'if(!after.windows.some(w=>w.title?.startsWith(' + json.dumps(destination_marker) + ')))'
             'throw Error("Readback preceded the destination page");',
             lambda s: s.get('destination_ready') and s['destination_requests'] == 1)
        case('chrome_element_id_across_filters',
             'const filter={role:"button",text:"Audit save"};'
             'const first=(await app.look(filter)).elements.find(e=>e.name==="Audit save")?.id;'
             'await app.look({text:"Audit Unicode editor"});'
             'const next=(await app.look(filter)).elements.find(e=>e.name==="Audit save")?.id;'
             'if(!first||first!==next)throw Error("Browser element ID changed across filters");')
        case('chrome_empty_unicode_typing', 'await app.typeText("Empty 中文 😀");', lambda s: s['text'] == 'Empty 中文 😀')
        case('chrome_guarded_ascii_typing',
             'await app.keypress("CTRL+A");'
             'const editor=(await app.look({text:"Audit Unicode editor"})).elements.find(e=>e.name==="Audit Unicode editor");'
             'if(!editor)throw Error("Owned editor was not observed");'
             'await app.typeText({target:editor.id,text:"owned ASCII"});',
             lambda s: s['text'] == 'owned ASCII')
        payload = 'Chrome 中文 😀\nowned page'
        case('chrome_unicode_typing', 'await app.keypress("CTRL+A");await app.typeText(' + json.dumps(payload) + ');', lambda s: s['text'] == payload)
        case('chrome_terminal_caret_emoji', 'await app.keypress("END");await app.typeText("🚀");', lambda s: s['text'] == payload + '🚀')
        case('chrome_literal_label', 'await app.keypress("CTRL+A");await app.typeText("Audit Unicode editor");await app.typeText("😀");', lambda s: s['text'] == 'Audit Unicode editor😀')
        case('chrome_literal_marker', 'await app.keypress("CTRL+A");await app.typeText("\\uFFFC");await app.typeText("😀");', lambda s: s['text'] == '\uFFFC😀')
        case('chrome_restore_multiline', 'await app.keypress("CTRL+A");await app.typeText(' + json.dumps(payload) + ');', lambda s: s['text'] == payload)
        case('chrome_semantic_increment', 'await app.click({selector:{role:"button",text:"Audit increment"}});', lambda s: s['clicks'] == 1)
        case('chrome_semantic_save', 'await app.click({selector:{role:"button",text:"Audit save"}});', lambda s: s.get('saved') == s['text'] and bool(s['text']))
        # Default anchor is the active surface center; this page places its scroll pane there.
        case('chrome_vertical_scroll', 'await app.scroll({delta_y:3});', lambda s: s['scroll_y'] > 0)
        case('chrome_horizontal_scroll', 'await app.scroll({delta_y:0,delta_x:3});', lambda s: s['scroll_x'] > 0)
        # Leave the real pointer in its ordinary position. No preparatory
        # parking: tooltip/shadow failures must remain visible to this probe.
        chooser = 'await app.click({selector:{role:"button",text:"Audit choose file"}});'
        case('chrome_native_open_dialog', chooser, native_dialog=True)
        require(report['cases'][-1]['verified'], report['cases'][-1])
        selected_directory = out / 'selection'
        selected_directory.mkdir()
        selected = selected_directory / ('DTW-owned-' + 'long-name-' * 12 + '中文-137.txt')
        selected_text = 'Exact browser file selection 中文 😀\r\nowned bytes 137'
        selected.write_bytes(selected_text.encode('utf-8'))
        navigation, _ = client.execute(app, 'await app.keypress("ALT+D");await app.typeText(' + json.dumps(str(selected_directory)) + ');await app.keypress("ENTER");')
        require(not navigation.get('error'), navigation)
        if args.review_chooser_hover:
            capture = client.capture(app, 'chooser-hover')
            png = client.output / 'chooser-hover.png'
            digest = hashlib.sha256(png.read_bytes()).hexdigest()
            (out / 'hover-request.json').write_text(json.dumps({'capture': str(png), 'sha256': digest,
                'image': capture['view']['image'], 'instruction': 'Hover the visible owned long-name file row; image pixels only.'}), encoding='utf-8')
            answer = out / 'hover-answer.json'
            until = time.monotonic() + 240
            while not answer.exists():
                require(time.monotonic() < until, 'chooser pixel review timed out')
                time.sleep(.1)
            point = json.loads(answer.read_text(encoding='utf-8'))
            require(point['sha256'] == digest, 'stale chooser hover answer')
            hovered, _ = client.execute(app, 'await app.move(' + json.dumps({'x': point['x'], 'y': point['y']}) + ');')
            require(not hovered.get('error'), hovered)
            time.sleep(2)
            decorations = owned_dialogs(browser_pid, args.chrome.resolve(), browser_handle.birth, diagnostics=True)
            report['tooltip_observation'] = decorations
            require(any(row['class'] in ('tooltips_class32', 'SysShadow') for row in decorations['candidates']),
                    {'missing_real_tooltip_or_shadow': decorations})
            # Read while the actual decoration remains present.
            tooltip_capture, _ = client.execute(app, 'await app.capture({full:true});')
            image_error = tooltip_capture.get('view', {}).get('image', {}).get('error')
            report['tooltip_capture'] = tooltip_capture
            report['strict_s03_capture_passed'] = image_error is None
            if args.observer:
                subprocess.run([str(args.observer.resolve()), '--snapshot', str(out / 'physical-tooltip.json'),
                                str(decorations['proven'][0]['hwnd'])], check=True, creationflags=subprocess.CREATE_NO_WINDOW)
                subprocess.run([str(args.observer.resolve()), '--surfaces', str(out / 'native-surfaces.json'),
                                str(decorations['proven'][0]['hwnd'])], check=True, creationflags=subprocess.CREATE_NO_WINDOW)
            if image_error:
                require(args.allow_decoration_image_limit and image_error.get('code') == 'windows_surface_unsupported', tooltip_capture)
                report['accepted_image_limit'] = image_error
                # This is a new semantic read, not coordinate authorization or
                # a retry of unknown input. Keyboard focus uses the real modal.
                current, _ = client.execute(app, 'await app.look({full:true});')
                modal_window = next((row for row in current['view']['windows']
                    if row['title'] == decorations['proven'][0]['title']), None)
                require(modal_window and current['view']['elements'] and all(
                    row['id'].startswith(modal_window['id'] + '.') for row in current['view']['elements']), current)
            else:
                client.capture(app, 'chooser-with-tooltip')
        selected_response, _ = client.execute(app, 'await app.keypress("ALT+N");await app.typeText(' + json.dumps(str(selected)) + ');await app.keypress("ENTER");')
        require(not selected_response.get('error'), selected_response)
        selected_state = wait(lambda s: s.get('selected_file') == selected.name and s.get('selected_text') == selected_text)
        report['exact_file_selection'] = {'response': selected_response, 'effect': selected_state,
                                         'sha256': hashlib.sha256(selected.read_bytes()).hexdigest()}
        # Open a fresh chooser for the separate Escape/cancellation contract.
        reopened, _ = client.execute(app, chooser)
        require(not reopened.get('error'), reopened)
        # Decorations retain their true z-order. Prove the semantic receiver
        # against our native-owned modal rather than assuming windows[0] is it.
        dialogs = owned_dialogs(browser_pid, args.chrome.resolve(), browser_handle.birth)
        require(len(dialogs) == 1, dialogs)
        data, response = client.execute(app, 'const v=await app.look();const m=v.windows.find(w=>w.title===' + json.dumps(dialogs[0]['title']) + ');if(!v.windows.some(w=>w.title.startsWith(' + json.dumps(marker) + '))||!m||!v.elements.length||!v.elements.every(e=>e.id.startsWith(m.id+".")))throw new Error("Owned Chrome modal is not selected");await app.keypress("ESC");', allow_error=True)
        report['dialog_escape'] = data
        require(not data.get('error'), data)
        until = time.monotonic() + 8
        while owned_dialogs(browser_pid, args.chrome.resolve(), browser_handle.birth):
            require(time.monotonic() < until, 'owned chooser did not close after Escape')
            time.sleep(.03)
        case('chrome_after_dialog', 'await app.capture({full:true});')
        with lock:
            report['neighbour_unchanged'] = dict(neighbour_state)
        require(neighbour_state['text'] == 'NEIGHBOUR_SENTINEL' and neighbour_state['clicks'] == 0
                and neighbour_state['scroll_x'] == 0 and neighbour_state['scroll_y'] == 0
                and neighbour_state['input_events'] == 0 and 'saved' not in neighbour_state, neighbour_state)
    except BaseException as error:
        report['error']=type(error).__name__+': '+str(error)
        raise
    finally:
        if client:
            report['close_window'] = execute('await app.quit();')
            report['mcp_cleanup'] = client.close()
        if browser_handle:
            try: report['ui_process_exit']=browser_handle.wait(timeout=8)
            except Exception:
                browser_handle.terminate()
                browser_handle.wait(timeout=5)
                report['forced_browser_cleanup']=True
            browser_handle.close()
        if child:
            try:
                child.wait(timeout=8)
            except subprocess.TimeoutExpired:
                child.terminate()  # Exact disposable-profile Popen, recorded as failure.
                child.wait(timeout=5)
                report['forced_browser_cleanup'] = True
            report['browser_exit'] = child.returncode
        if launcher:launcher.close()
        if neighbour_handle:
            try:
                if neighbour_handle.alive():
                    close_owned_browser(neighbour_pid, args.chrome.resolve(), neighbour_handle.birth, neighbour_marker)
                report['neighbour_exit'] = neighbour_handle.wait(timeout=8)
            except BaseException as error:
                report['neighbour_cleanup_error'] = str(error)
                if neighbour_handle.alive():
                    neighbour_handle.terminate()
                    neighbour_handle.wait(timeout=5)
                report['forced_neighbour_cleanup'] = True
            neighbour_handle.close()
        if neighbour:
            try:
                report['neighbour_launcher_exit'] = neighbour.wait(timeout=8)
            except subprocess.TimeoutExpired:
                neighbour.terminate()
                neighbour.wait(timeout=5)
                report['forced_neighbour_cleanup'] = True
        if neighbour_launcher:neighbour_launcher.close()
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
        report['http_state'] = observed
        report['passed'] = (bool(report['cases']) and all(c['verified'] for c in report['cases'])
            and not report.get('error') and bool(report.get('exact_file_selection'))
            and not report.get('dialog_escape',{}).get('error')
            and report.get('neighbour_exit') == 0 and report.get('neighbour_launcher_exit') == 0
            and bool(report.get('neighbour_unchanged')) and not report.get('forced_neighbour_cleanup')
            and not report.get('close_window',{}).get('error') and report.get('browser_exit') == 0
            and report.get('ui_process_exit') == 0 and not report.get('forced_browser_cleanup'))
        (out / 'report.json').write_text(json.dumps(report, ensure_ascii=True, indent=2) + '\n', encoding='utf-8')
        if not report.get('error'):
            require(report['passed'],'Browser acceptance failed; inspect original report/wire evidence')


if __name__ == '__main__':
    main()
