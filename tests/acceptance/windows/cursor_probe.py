#!/usr/bin/env python3
"""Opt-in Windows cursor GUI acceptance through ordinary stdio MCP.

Only DTW drives app input. An independent owned observer samples Win32 window
ownership/styles, physical screen pixels and process cleanup; it emits no cursor
events. A same-class foreign window is an explicit capture-exclusion control.
Run with python -X utf8. Pillow is used only for PNG comparison.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import threading
import time
import traceback


def require(value, message):
    if not value:
        raise AssertionError(message)


def write(path, value):
    path=Path(path)
    temp=path.with_name(path.name+'.next')
    temp.write_text(json.dumps(value, indent=2, ensure_ascii=True) + '\n', encoding='utf-8')
    temp.replace(path)


def wait(check, timeout=5):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        value = check()
        if value:
            return value
        time.sleep(.025)  # Independent readiness/effect reads, never action retries.
    raise TimeoutError('Independent readiness/effect condition did not settle')


def read(path):
    try:
        if os.name=='nt':
            # CRT open() omits FILE_SHARE_DELETE and can make the independent
            # fixture's File.Replace fail. Observe an immutable publication
            # without blocking its next atomic replacement.
            import ctypes
            import msvcrt
            from ctypes import wintypes as w
            k=ctypes.WinDLL('kernel32',use_last_error=True)
            k.CreateFileW.argtypes=[w.LPCWSTR,w.DWORD,w.DWORD,w.LPVOID,w.DWORD,w.DWORD,w.HANDLE]
            k.CreateFileW.restype=w.HANDLE
            k.CloseHandle.argtypes=[w.HANDLE]
            handle=k.CreateFileW(str(path),0x80000000,7,None,3,0,None)
            if handle==w.HANDLE(-1).value:
                return None
            try:
                fd=msvcrt.open_osfhandle(handle,os.O_RDONLY|os.O_BINARY)
            except BaseException:
                k.CloseHandle(handle)
                raise
            with os.fdopen(fd,'rb') as stream:
                data=stream.read().decode('utf-8-sig')
        else:
            data=Path(path).read_text(encoding='utf-8-sig')
        return json.loads(data)
    except (OSError, ValueError):
        return None


class Mcp:
    def __init__(self, binary, output):
        self.output = output
        output.mkdir()
        self.serial = 0
        self.messages = queue.Queue()
        self.pending = {}
        self.images = []
        self.err = (output/'stderr.log').open('wb')
        self.proc = subprocess.Popen([str(binary), 'mcp'], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=self.err)
        def receive():
            try:
                with (output/'stdout.jsonl').open('wb') as log:
                    for line in self.proc.stdout:
                        log.write(line)
                        log.flush()
                        self.messages.put(json.loads(line))
            except BaseException as error:
                self.messages.put(error)
            finally:
                self.messages.put(EOFError('MCP stdout closed'))
        self.reader = threading.Thread(target=receive, daemon=True)
        self.reader.start()
        self.rpc('initialize', {'protocolVersion':'2025-06-18', 'capabilities':{},
                                'clientInfo':{'name':'dtw-windows-cursor-acceptance','version':'1'}})
        self.send({'jsonrpc':'2.0', 'method':'notifications/initialized'})

    def send(self, message):
        with (self.output/'requests.jsonl').open('a', encoding='utf-8') as log:
            log.write(json.dumps(message) + '\n')
        self.proc.stdin.write((json.dumps(message) + '\n').encode('utf-8'))
        self.proc.stdin.flush()

    def begin(self, method, params):
        self.serial += 1
        self.send({'jsonrpc':'2.0', 'id':self.serial, 'method':method, 'params':params})
        return self.serial

    def receive(self, key, timeout=40):
        if key in self.pending:
            return self.pending.pop(key)
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            message = self.messages.get(timeout=max(.01, until-time.monotonic()))
            if isinstance(message, BaseException):
                raise message
            if message.get('id') == key:
                return message
            if 'id' in message:
                self.pending[message['id']] = message
        raise TimeoutError('MCP response')

    def rpc(self, method, params):
        response = self.receive(self.begin(method, params))
        require('error' not in response, response)
        return response['result']

    def tool(self, name, arguments):
        response = self.rpc('tools/call', {'name':name, 'arguments':arguments})
        value = response.get('structuredContent')
        texts = [c['text'] for c in response.get('content', []) if c.get('type') == 'text']
        require(isinstance(value, dict) and len(texts) == 1 and json.loads(texts[0]) == value,
                'MCP text/structured projections disagree')
        for index, content in enumerate(response.get('content', [])):
            if content.get('type') == 'image':
                data = base64.b64decode(content['data'])
                require(data.startswith(b'\x89PNG\r\n\x1a\n'), 'Malformed PNG')
                image = self.output/f'{self.serial}-image-{index}.png'
                image.write_bytes(data)
                self.images.append(image)
        return value

    def execute(self, app, case, source, error=None):
        compact = self.tool('exec', {'app':app, 'source':source})
        require(compact.get('app') == app and compact.get('execution_id'), compact)
        full = self.tool('result', {'app':app, 'execution_id':compact['execution_id']})
        write(self.output/(case+'-ledger.json'), full)
        require(full['execution_id'] == compact['execution_id'], 'Execution identity changed')
        require(full.get('status') == 'completed', full)
        if error is None:
            require(full.get('error') is None, full.get('error'))
        else:
            require(full.get('error') is not None and error in json.dumps(full), full)
        return full.get('value', {}).get('result'), full

    def close(self):
        if self.proc.poll() is None:
            self.proc.stdin.close()
        forced=False
        try:
            code = self.proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            self.proc.terminate()  # Popen retains this exact owned process handle.
            code = self.proc.wait(timeout=5)
            forced=True
        self.reader.join(timeout=2)
        self.err.close()
        require(not forced and not self.reader.is_alive() and code == 0, f'MCP cleanup failed: exit={code}, forced={forced}')
        return {'pid':self.proc.pid, 'exit_code':code, 'forced':False}


class ProcessHandle:
    """Read-only supervision, plus explicit termination of a proven owned helper.

    Holding the kernel handle prevents a recycled PID from becoming our target.
    No application UI is observed or driven through this helper.
    """
    def __init__(self, pid, binary, birth=None, terminate=False):
        import ctypes
        from ctypes import wintypes as w
        self.ctypes, self.w = ctypes, w
        k = self.k = ctypes.WinDLL('kernel32', use_last_error=True)
        k.OpenProcess.argtypes = [w.DWORD,w.BOOL,w.DWORD]
        k.OpenProcess.restype = w.HANDLE
        k.GetProcessTimes.argtypes = [w.HANDLE,*([ctypes.POINTER(w.FILETIME)]*4)]
        k.QueryFullProcessImageNameW.argtypes = [w.HANDLE,w.DWORD,w.LPWSTR,ctypes.POINTER(w.DWORD)]
        k.WaitForSingleObject.argtypes = [w.HANDLE,w.DWORD]
        k.GetExitCodeProcess.argtypes = [w.HANDLE,ctypes.POINTER(w.DWORD)]
        k.TerminateProcess.argtypes = [w.HANDLE,w.UINT]
        k.CloseHandle.argtypes = [w.HANDLE]
        self.handle = k.OpenProcess(0x100000|0x1000|(1 if terminate else 0),False,pid)
        require(self.handle, 'Cannot bind owned process handle')
        self.pid, self.allow_terminate = pid, terminate
        created, exited, kernel, user = (w.FILETIME() for _ in range(4))
        require(k.GetProcessTimes(self.handle,*(ctypes.byref(v) for v in [created,exited,kernel,user])), 'Process birth unavailable')
        self.birth = created.dwHighDateTime*2**32+created.dwLowDateTime
        if birth is not None:
            require(self.birth == birth, 'Owned process birth changed before binding')
        path = ctypes.create_unicode_buffer(32768)
        length = w.DWORD(len(path))
        require(k.QueryFullProcessImageNameW(self.handle,0,path,ctypes.byref(length)) and path.value.casefold() == str(binary).casefold(),
                'Owned process executable path changed before binding')

    def wait(self, timeout=5):
        require(self.k.WaitForSingleObject(self.handle,int(timeout*1000)) == 0, 'Owned process did not exit in time')
        code = self.w.DWORD()
        require(self.k.GetExitCodeProcess(self.handle,self.ctypes.byref(code)), 'Original process exit unavailable')
        return code.value

    def terminate(self):
        require(self.allow_terminate and self.k.TerminateProcess(self.handle,1), 'Intentional owned-helper termination failed')

    def alive(self):
        return self.k.WaitForSingleObject(self.handle,0) == 258

    def close(self):
        if self.handle:
            self.k.CloseHandle(self.handle)
            self.handle = None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(os.name == 'nt' and sys.flags.utf8_mode, 'Use Windows: python -X utf8 cursor_probe.py ...')
    from PIL import Image, ImageChops
    binary = args.dtw.resolve(strict=True)
    output = args.output.resolve()
    require(not output.exists(), 'Output must be a new evidence directory')
    output.mkdir(parents=True)
    here = Path(__file__).parent
    summary = {'passed':False, 'kind':'owned Windows fixtures via standard MCP; independent Win32/pixel observer',
               'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(), 'cases':[], 'cleanup':[]}
    fixtures, clients = [], []
    blocker = None
    snapshots = 0
    providers = {}

    def snapshot(target):
        nonlocal snapshots
        snapshots += 1
        path = output/f'observer-{snapshots}.json'
        subprocess.run([str(output/'build/CursorObserver.exe'), '--snapshot', str(path), str(target)],
                       check=True, creationflags=subprocess.CREATE_NO_WINDOW)
        return read(path)

    def state(f):
        value=wait(lambda:read(f['directory']/'state.json'),timeout=1)
        require(value['pid']==f['proc'].pid,'Fixture publication belongs to a different process')
        return value

    def helpers(s, client):
        return [w for w in s['windows'] if w['parent'] == client.proc.pid]

    def marker(client, fixture, colored=True):
        def settled():
            s = snapshot(int(state(fixture)['hwnd']))
            rows = helpers(s, client)
            if len(rows) != 1 or not rows[0]['visible']:
                return None
            if colored and max(rows[0]['palette_pixels']) < 20:
                return None
            return s, rows[0]
        return wait(settled)

    def select(client, fixture):
        catalog = client.tool('apps', {})
        found = [a for a in catalog['apps'] if a['app'].casefold() == str(fixture['exe']).casefold()]
        require(len(found) == 1, 'Owned fixture missing/ambiguous in app catalog')
        app = found[0]['app']
        before={p['pid'] for p in snapshot(int(state(fixture)['hwnd']))['processes'] if p['parent']==client.proc.pid}
        value, _ = client.execute(app, 'select-'+fixture['label'],
            "const ws=await app.windows();const w=ws.windows.filter(w=>w.title==="+json.dumps('DTW Windows Smoke '+fixture['label'])+
            ");if(w.length!==1)throw Error('Owned window not unique');state.window=w[0].window_ref;return w[0];")
        after={p['pid'] for p in snapshot(int(state(fixture)['hwnd']))['processes'] if p['parent']==client.proc.pid}
        new=after-before
        if new:
            require(len(new)==1,'App provider creation did not have a unique independent process delta')
            providers[(client.proc.pid,app)]=next(iter(new))
        return app, value['window_ref']

    def click(client, fixture, app, window, name, dx=0, dy=0):
        s = state(fixture)
        x, y = s['pad_point']['x']+dx-s['bounds']['x'], s['pad_point']['y']+dy-s['bounds']['y']
        before = s['left_up']
        _, full = client.execute(app, name,
            f"const window_ref={json.dumps(window)};await app.focus({{window_ref}});const c=await app.capture({{window_ref}});"
            f"return await app.click({{window_ref,observation_ref:c.observation_ref,x:{x},y:{y}}});")
        wait(lambda: state(fixture)['left_up'] == before+1)
        receipts = [step['receipt'] for step in full['script']['steps'] if step['operation'] == 'click']
        require(len(receipts) == 1 and receipts[0]['outcome']['dispatch'] == 'dispatched', 'Missing original click receipt')
        return marker(client, fixture)

    def children(client, fixture):
        scene = snapshot(int(state(fixture)['hwnd']))
        rows = [p for p in scene['processes'] if p['parent'] == client.proc.pid]
        handles = [ProcessHandle(p['pid'],binary) for p in rows]
        fresh = snapshot(int(state(fixture)['hwnd']))
        current = {p['pid'] for p in fresh['processes'] if p['parent'] == client.proc.pid}
        require({p.pid for p in handles}.issubset(current), 'Native owner changed during supervision binding')
        return handles

    def released(fixture):
        last = wait(lambda: (s if not (s:=state(fixture))['keys_down_global'] else None))
        time.sleep(.15)
        after = state(fixture)
        require(after['sequence']>last['sequence'] and not after['keys_down_global'], 'Input remained held after teardown')
        return after

    def inflight(client, fixture, app, window, case):
        client.execute(app,case+'-focus-editor',
            f"await app.focus({{window_ref:{json.dumps(window)}}});"
            f"await app.press({{window_ref:{json.dumps(window)},selector:{{role:'button',text:'Increment'}}}});"
            f"return await app.keypress({{window_ref:{json.dumps(window)},keys:['CTRL','A']}});")
        before=state(fixture)
        payload='DTW-'+case+'-'+''.join(f'{i:04d}:中文😀|' for i in range(180))
        journal=fixture['directory']/'events.jsonl'
        offset=journal.stat().st_size
        rpc=client.begin('tools/call',{'name':'exec','arguments':{'app':app,'source':
            f"await app.typeText({{window_ref:{json.dumps(window)},text:{json.dumps(payload)}}});throw Error('unexpected suffix');"}})
        def entered_event():
            # WM_TIMER publications can be starved by a real stream of input.
            # The app's synchronous TextChanged journal proves delivery without
            # altering application timing or retrying the input operation.
            with journal.open('rb') as stream:
                stream.seek(offset)
                lines=stream.read().splitlines()
            for line in lines:
                try:
                    event=json.loads(line)
                except ValueError:
                    continue
                if event['kind']=='text_changed' and 20<event['data']['length']<len(payload.encode('utf-16-le'))//2:
                    return event
            return None
        entered=wait(entered_event)
        running=client.tool('result',{'app':app})
        require(running['status']=='running','Interruption missed actual input; never replay')
        write(output/(case+'-running.json'),{'record':running,'rpc_id':rpc,'entered_length':entered['data']['length'],'entered_length_unit':'utf16','event':entered})
        return rpc,running['execution_id'],payload,before

    def stopped_effect(fixture,payload,before):
        after=released(fixture)
        require(0<len(after['text'])<len(payload) and payload.startswith(after['text']), 'Interrupted input is not the original partial prefix')
        require(after['saves']==before['saves'],'Unexecuted suffix saved the document')
        time.sleep(.15)
        require(state(fixture)['text']==after['text'],'Late input continued after interruption')
        return {'partial_prefix_length':len(after['text']),'payload_length':len(payload),'held_keys':after['keys_down_global'],'saves_unchanged':True}

    try:
        compile_result = subprocess.run(['powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',str(here/'compile-cursor.ps1'),
                        '-OutputDirectory',str(output/'build')], capture_output=True)
        (output/'compile.stdout').write_bytes(compile_result.stdout)
        (output/'compile.stderr').write_bytes(compile_result.stderr)
        require(compile_result.returncode == 0, 'Owned fixture compilation failed; inspect compile.stderr')
        for label in ['A','B']:
            directory = output/label
            directory.mkdir()
            exe = directory/'Fixture.exe'
            shutil.copy2(output/'build/Fixture.exe', exe)
            proc = subprocess.Popen([str(exe), str(directory), label])
            fixture = {'directory':directory, 'exe':exe, 'proc':proc, 'label':label}
            fixtures.append(fixture)
            wait(lambda: (s := state(fixture)) and s['ready'] and s['bounds_ok'])
        a, b = fixtures
        first = Mcp(binary, output/'session-A')
        clients.append(first)
        app_a, window_a = select(first, a)
        app_b, window_b = select(first, b)
        first.execute(app_a, 'semantic-no-marker',
            f"return await app.press({{window_ref:{json.dumps(window_a)},selector:{{role:'button',text:'Increment'}}}});")
        require(not helpers(snapshot(int(state(a)['hwnd'])), first), 'Semantic action started a cursor helper')
        summary['cases'].append('lazy-semantic-no-marker')

        shot, original = click(first, a, app_a, window_a, 'first-pointer')
        required = 0x080800A0
        require(original['ex_style'] & required == required and original['ex_style'] & 8 == 0 and not original['enabled'],
                'Cursor is interactive, activating, or topmost')
        require(shot['foreground_before'] and shot['foreground_after'], 'Marker stole foreground')
        summary['cases'].append('visible-colored-nonactivating-clickthrough-marker')

        _, cross = click(first, b, app_b, window_b, 'cross-app-pointer')
        clean_b_image = first.images[-1]
        require((cross['pid'],cross['hwnd'],cross['title']) == (original['pid'],original['hwnd'],original['title']),
                'One MCP session created per-app cursors or changed identity')
        summary['cases'].append('same-session-cross-app-continuity')

        second = Mcp(binary, output/'session-B')
        clients.append(second)
        second_app, second_window = select(second, b)
        click(second, b, second_app, second_window, 'second-session-pointer', -150, -50)
        s = snapshot(int(state(b)['hwnd']))
        rows1, rows2 = helpers(s, first), helpers(s, second)
        require(len(rows1) == len(rows2) == 1 and rows1[0]['pid'] != rows2[0]['pid'], 'Sessions share a visual process')
        require(rows1[0]['visible'] and rows2[0]['visible'] and max(rows1[0]['palette_pixels']) >= 20 and max(rows2[0]['palette_pixels']) >= 20,
                'Both independent markers must actually render')
        r1,r2=rows1[0]['bounds'],rows2[0]['bounds']
        require(r2['x']+r2['width']<=r1['x'] or r1['x']+r1['width']<=r2['x'],
                'Pixel proofs require nonoverlapping marker rectangles')
        summary['cases'].append('two-live-session-markers')

        # Static owned pad pixels are compared before and after both marker
        # windows are visible. Screen evidence separately proves rendering.
        first.execute(app_b, 'capture-with-two-markers', f"return await app.capture({{window_ref:{json.dumps(window_b)}}});")
        native_image = first.images[-1]
        desktop = Image.open(s['desktop_image']).convert('RGB')
        native_image = Image.open(native_image).convert('RGB')
        require(desktop.size == native_image.size, 'Physical capture geometry differs')
        cursor_rect = rows1[0]['bounds']
        scene = s['target_bounds']
        crop = (cursor_rect['x']-scene['x'],cursor_rect['y']-scene['y'],cursor_rect['x']-scene['x']+cursor_rect['width'],cursor_rect['y']-scene['y']+cursor_rect['height'])
        require(ImageChops.difference(desktop.crop(crop),native_image.crop(crop)).getbbox() is not None,
                'Visible marker was not independently distinguished from target capture')
        baseline = Image.open(clean_b_image).convert('RGB')
        # The pre-second-session capture was already overlay-free; its static
        # pad crop must remain identical after the peer overlay appeared.
        require(baseline.size == native_image.size and ImageChops.difference(baseline.crop(crop),native_image.crop(crop)).getbbox() is None,
                'Session overlays contaminated the independent target PNG')
        summary['cases'].append('exact-peer-exclusion-and-static-pixels')

        fake = output/'fake'
        fake.mkdir()
        shutil.copy2(output/'build/CursorObserver.exe', fake/'dtw.exe')
        fake_state = output/'spoof.json'
        blocker = subprocess.Popen([str(fake/'dtw.exe'),'--spoof',str(fake_state),state(b)['hwnd']],creationflags=subprocess.CREATE_NO_WINDOW)
        wait(lambda: read(fake_state))
        covered=snapshot(int(state(b)['hwnd']))
        require(covered['desktop_image'] is not None,'Foreign-window stacking needs independent physical pixels')
        frame=Image.open(covered['desktop_image']).convert('RGB');point=state(b)['pad_point'];bounds=covered['target_bounds']
        require(frame.getpixel((point['x']-bounds['x'],point['y']-bounds['y']))==(255,0,255),
                'A session marker rose above the unrelated owned cover')
        first.execute(app_b,'foreign-same-class-refused',f"return await app.capture({{window_ref:{json.dumps(window_b)}}});",error='occluded')
        (output/'spoof.json.stop').write_text('stop',encoding='utf-8')
        require(blocker.wait(timeout=5) == 0, 'Owned negative-control window did not exit')
        blocker = None
        summary['cases'].append('same-class-same-basename-foreign-window-not-exempt')
        summary['cases'].append('foreign-cover-remains-above-session-markers')

        # Move only our fixture via its independent supervisor control. The old
        # operation must hide rather than follow a changed target geometry.
        write(b['directory']/'layout.json',{'sequence':1,'window_offset':12,'control_offset':0})
        wait(lambda: state(b)['layout_sequence'] == 1)
        wait(lambda: all(not w['visible'] for w in snapshot(int(state(b)['hwnd']))['windows'] if w['parent'] in [first.proc.pid,second.proc.pid]))
        summary['cases'].append('geometry-change-retires-both-markers')
        click(first,b,app_b,window_b,'new-independent-action')
        _, recovered = marker(first,b)
        require(recovered['pid'] == original['pid'] and recovered['title'] == original['title'], 'Geometry change lost stable session identity')

        modal,_=first.execute(app_b,'native-modal-wait',
            f"await app.press({{window_ref:{json.dumps(window_b)},selector:{{role:'button',text:'Show owned dialog'}}}});"
            f"const modal=await app.waitForWindow({{window_ref:{json.dumps(window_b)},state:'dialog',timeout_ms:2000}});state.modal=modal.window_ref;return modal;")
        require(modal['kind']=='dialog' and modal['relation']=='child_modal','Native modal relation was not proven')
        closed,_=first.execute(app_b,'native-modal-close-and-unblocked',
            "await app.press({window_ref:state.modal,selector:{role:'button',text:'Close dialog'}});"
            "const closed=await app.waitForWindow({window_ref:state.modal,state:'closed',timeout_ms:2000});"
            f"const ready=await app.waitForWindow({{window_ref:{json.dumps(window_b)},state:'unblocked',timeout_ms:2000}});return {{closed,ready}};")
        require(closed['closed']['state']=='closed' and closed['ready']['state']=='unblocked','Native close/unblocked proof missing')
        summary['cases'].append('native-modal-owned-relation-closed-unblocked')

        # The point and element forms use the same retained contract as macOS.
        click(first,b,app_b,window_b,'scroll-focus-pad')
        current=state(b);before_wheel=current['wheel_delta'];x=current['pad_point']['x']-current['bounds']['x'];y=current['pad_point']['y']-current['bounds']['y']
        first.execute(app_b,'pixel-scroll-anchor',
            f"const window_ref={json.dumps(window_b)};const c=await app.capture({{window_ref}});return await app.scroll({{window_ref,observation_ref:c.observation_ref,x:{x},y:{y},delta_y:2}});")
        wait(lambda:state(b)['wheel_delta']==before_wheel-240)
        first.execute(app_b,'semantic-scroll-anchor',
            f"const window_ref={json.dumps(window_b)};const a=await app.find({{window_ref,selector:{{text:'Owned pointer pad'}}}});return await app.scroll({{window_ref,observation_ref:a.observation_ref,element_ref:a.element_ref,delta_y:1}});")
        wait(lambda:state(b)['wheel_delta']==before_wheel-360)
        summary['cases'].append('retained-pixel-and-clipped-semantic-wheel-anchors')

        # Confirm the kernel handles before intentionally killing only our
        # presentation child. A later independent action must recover identity.
        _,old=marker(first,b)
        killed=ProcessHandle(old['pid'],binary,old['birth'],terminate=True)
        try:
            killed.terminate();require(killed.wait()==1,'Owned helper termination exit changed')
        finally:
            killed.close()
        wait(lambda:not helpers(snapshot(int(state(b)['hwnd'])),first))
        _,replacement=click(first,b,app_b,window_b,'new-action-after-helper-exit')
        require(replacement['pid']!=old['pid'] and replacement['title']==old['title'],'Helper replacement lost session identity')
        summary['cases'].append('helper-exit-recovers-only-on-new-independent-action')

        # One owner types while another waits for the shared input lease. A
        # canceled owner must release the transaction, not abandon its mutex.
        queued_app,queued_window=select(second,a)
        click(first,b,app_b,window_b,'before-cancel-marker')
        _,cursor_before=marker(first,b)
        provider=ProcessHandle(providers[(first.proc.pid,app_b)],binary)
        original_helper=ProcessHandle(cursor_before['pid'],binary,cursor_before['birth'])
        rpc,execution_id,payload,before=inflight(first,b,app_b,window_b,'cancel')
        pa=state(a);px=pa['pad_point']['x']-pa['bounds']['x'];py=pa['pad_point']['y']-pa['bounds']['y'];before_click=pa['left_up']
        queued_rpc=second.begin('tools/call',{'name':'exec','arguments':{'app':queued_app,'source':
            f"const window_ref={json.dumps(queued_window)};await app.focus({{window_ref}});const c=await app.capture({{window_ref}});return await app.click({{window_ref,observation_ref:c.observation_ref,x:{px},y:{py}}});"}})
        def parked():
            record=second.tool('result',{'app':queued_app})
            return record if record['status']=='running' and any(s['operation']=='focus' for s in record.get('script',{}).get('steps',[])) else None
        parked_record=wait(parked)
        time.sleep(.05)
        first.send({'jsonrpc':'2.0','method':'notifications/cancelled','params':{'requestId':rpc,'reason':'owned Windows cancellation control'}})
        def terminal():
            record=first.tool('result',{'app':app_b,'execution_id':execution_id})
            return record if record['status']=='completed' else None
        cancelled=wait(terminal,8)
        write(output/'cancel-original-ledger.json',cancelled)
        script=cancelled['script']
        require(cancelled['error'] and script['failure']['kind']=='cancelled' and not script['state_committed'] and script['shutdown_error'] is None,
                'Cancellation lost its primary error/cleanup separation')
        typed=[s for s in script['steps'] if s['operation']=='typeText']
        require(len(typed)==1 and typed[0]['receipt']['outcome']['dispatch']=='unknown' and typed[0]['receipt']['provider_outcome'] is not None,
                'Original partial-input receipt was replaced')
        queue_response=second.receive(queued_rpc)
        write(output/'queued-input-response.json',queue_response)
        queued=second.tool('result',{'app':queued_app})
        write(output/'queued-input-ledger.json',queued)
        require(queued['status']=='completed' and queued['error'] is None,queued.get('error'))
        wait(lambda:state(a)['left_up']==before_click+1)
        require(provider.wait()==0,'Canceled native worker did not report clean exit')
        provider.close()
        require(original_helper.alive(),'Cancel killed the shared session helper')
        original_helper.close()
        wait(lambda:all(not w['visible'] for w in helpers(snapshot(int(state(b)['hwnd'])),first)))
        summary['cancel_effect']=stopped_effect(b,payload,before)
        require(first.tool('result',{'app':app_b,'execution_id':execution_id})==cancelled,'Original cancellation ledger changed')
        summary['cases'].append('inflight-cancel-and-concurrent-owner-transaction-handoff')
        app_b,window_b=select(first,b)
        click(first,b,app_b,window_b,'new-action-after-cancel')
        _,continued=marker(first,b)
        require(continued['pid']==cursor_before['pid'] and continued['title']==cursor_before['title'],'Cancel replaced session identity')

        # In-flight stdin EOF and abrupt parent exit retain exact child handles
        # so actual native/helper exit codes are evidence, not process absence.
        for mode in ['eof','parent-exit']:
            client=Mcp(binary,output/('session-'+mode));clients.append(client)
            app,window=select(client,b);click(client,b,app,window,mode+'-marker')
            owned=children(client,b)
            rpc,original,payload,before=inflight(client,b,app,window,mode)
            if mode=='eof':client.proc.stdin.close()
            else:client.proc.terminate()
            code=client.proc.wait(timeout=8);client.reader.join(timeout=2);client.err.close();clients.remove(client)
            exits=[]
            try:
                for handle in owned:exits.append({'pid':handle.pid,'birth':handle.birth,'exit_code':handle.wait()})
            finally:
                for handle in owned:handle.close()
            proof={'mcp_exit':code,'children':exits,'effect':stopped_effect(b,payload,before),
                'execution_id':original,'final_receipt':'unavailable after transport closure; original running ledger retained'}
            write(output/(mode+'-proof.json'),proof)
            require(code==(0 if mode=='eof' else 1) and all(p['exit_code']==0 for p in exits),'Native/helper shutdown was not clean')
            wait(lambda:not helpers(snapshot(int(state(b)['hwnd'])),client))
            summary['cases'].append('inflight-'+mode+'-native-and-helper-reclamation')

        # EOF closes one session without closing the other session's helper.
        closed = first.close()
        clients.remove(first)
        summary['cleanup'].append(closed)
        wait(lambda: not helpers(snapshot(int(state(b)['hwnd'])),first))
        click(second,b,second_app,second_window,'surviving-session-pointer')
        summary['cases'].append('session-eof-isolation-and-helper-reclamation')
        summary['passed'] = True
    except BaseException:
        summary['failure'] = traceback.format_exc()
        (output/'failure.txt').write_text(summary['failure'],encoding='utf-8')
    finally:
        if blocker is not None:
            (output/'spoof.json.stop').write_text('stop',encoding='utf-8')
            try:
                blocker.wait(timeout=5)
            except subprocess.TimeoutExpired:
                blocker.kill()
                summary['cleanup'].append({'spoof_forced':True})
                summary['passed'] = False
        for client in clients:
            try:
                summary['cleanup'].append(client.close())
            except BaseException:
                summary['passed'] = False
                summary['cleanup'].append({'mcp_error':traceback.format_exc()})
        for f in fixtures:
            (f['directory']/'stop').write_text('stop',encoding='utf-8')
            try:
                code = f['proc'].wait(timeout=5)
                summary['cleanup'].append({'fixture':f['label'],'exit_code':code,'forced':False})
                summary['passed'] &= code == 0
            except subprocess.TimeoutExpired:
                f['proc'].kill()
                summary['cleanup'].append({'fixture':f['label'],'forced':True})
                summary['passed'] = False
        write(output/'summary.json',summary)
        print(json.dumps(summary))
    return 0 if summary['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
