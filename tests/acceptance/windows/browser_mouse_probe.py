#!/usr/bin/env python3
"""Chrome content-area click with an independent app in front and cursor fixed."""
import argparse, ctypes, hashlib, json, os, shutil, subprocess, sys, time, traceback, uuid
from ctypes import wintypes as w
from pathlib import Path
from cursor_probe import Mcp, read, require, wait, write
from browser_reliability_probe import window_diagnostics


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dtw',type=Path,required=True);p.add_argument('--chrome',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();binary=a.dtw.resolve(strict=True);out=a.output.resolve();require(os.name=='nt' and sys.flags.utf8_mode and not out.exists(),'Use Windows, utf8 and new output');out.mkdir(parents=True)
    summary={'passed':False,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'cases':[],'cleanup':[]};client=browser=server=fixture=None
    retain_scene=False;close_attempted=False;phase='setup';title=None
    user=ctypes.WinDLL('user32',use_last_error=True);user.GetForegroundWindow.restype=w.HWND
    def desktop():
        point=w.POINT();require(user.GetPhysicalCursorPos(ctypes.byref(point)),'cursor read');return [point.x,point.y,user.GetForegroundWindow()]
    try:
        from PIL import Image, ImageChops
        build=subprocess.run(['powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',str(Path(__file__).with_name('compile-cursor.ps1')),'-OutputDirectory',str(out/'build')],capture_output=True)
        (out/'compile.stdout').write_bytes(build.stdout);(out/'compile.stderr').write_bytes(build.stderr);require(build.returncode==0,'compile failed')
        web=out/'web';web.mkdir();src=Path(__file__).resolve().parents[2]/'fixtures/browser-workbench'
        shutil.copyfile(src/'server.py',web/'server.py')
        # A visible color oracle in this owned local fixture only. No DOM/CDP.
        html=(src/'index.html').read_text(encoding='utf-8').replace('</style>','tbody button{background:rgb(83,165,131)}</style>');(web/'index.html').write_text(html,encoding='utf-8')
        title='mouse-'+uuid.uuid4().hex[:12]
        with (out/'server.stdout').open('wb') as so,(out/'server.stderr').open('wb') as se:server=subprocess.Popen([sys.executable,str(web/'server.py'),'--output',str(web),'--case',title],stdout=so,stderr=se,creationflags=subprocess.CREATE_NO_WINDOW)
        def address():
            require(server.poll() is None,'server exited')
            try:return json.loads((out/'server.stdout').read_text())['url']
            except (ValueError,KeyError):return None
        url=wait(address)
        browser=subprocess.Popen([str(a.chrome.resolve()),'--user-data-dir='+str(out/'profile'),'--no-first-run','--no-default-browser-check','--force-renderer-accessibility','--window-position=570,0','--window-size=1100,900','--new-window',url])
        client=Mcp(binary,out/'mcp')
        def ref():
            catalog=client.tool('apps',{});app=next((x['app'] for x in catalog['apps'] if x['app'].casefold()==str(a.chrome.resolve()).casefold()),None)
            if not app:return None
            rows,_=client.execute(app,'owned-windows','return (await app.windows()).windows;');window=next((x['window_ref'] for x in rows if x['title'].startswith('DTW Workbench '+title)),None)
            return (app,window) if window else None
        app,window=wait(ref,20);wr=json.dumps(window)
        client.execute(app,'filter',f'await app.focus({{window_ref:{wr}}});await app.keypress({{window_ref:{wr},keys:["ESC"]}});return await app.setValue({{window_ref:{wr},selector:{{role:"text_field",text:"Search inventory"}},value:"DTW-0088"}});')
        directory=out/'fixture';directory.mkdir();fixture=subprocess.Popen([str(out/'build/Fixture.exe'),str(directory),'A']);wait(lambda:(s if (s:=read(directory/'state.json')) and s['ready'] else None))
        fa=next(x['app'] for x in client.tool('apps',{})['apps'] if x['app'].casefold()==str(out/'build/Fixture.exe').casefold())
        client.execute(fa,'foreground-other','const w=(await app.windows()).windows[0];return await app.focus({window_ref:w.window_ref});');wait(lambda:(s if (s:=read(directory/'state.json')) and s['foreground'] else None));time.sleep(.1)
        phase='content-capture'
        capture,_=client.execute(app,'content-capture',f'return await app.capture({{window_ref:{wr}}});')
        image=Image.open(client.images[-1]).convert('RGB');mask=Image.new('L',image.size,255)
        for channel,value in zip(image.split(),(83,165,131)):mask=ImageChops.multiply(mask,channel.point([255 if i==value else 0 for i in range(256)]))
        box=mask.getbbox();require(box,'No visible edit button');x=(box[0]+box[2])//2;y=(box[1]+box[3])//2
        phase='content-click'
        before=desktop();_,ledger=client.execute(app,'content-click',f'return await app.click({{window_ref:{wr},observation_ref:{json.dumps(capture["observation_ref"])},x:{x},y:{y}}});')
        state=wait(lambda:(s if (s:=read(web/'state.json')) and len([e for e in s['events'] if e.get('type')=='dialog_opened'])==1 else None));time.sleep(.15);after=desktop()
        click=next(s for s in ledger['script']['steps'] if s['operation']=='click');require(click['receipt']['outcome']['delivery']['route']=='background',click);require(before[:2]==after[:2],{'before':before,'after':after})
        foreground_pid=w.DWORD();user.GetWindowThreadProcessId(w.HWND(after[2]),ctypes.byref(foreground_pid))
        require(after[2]==before[2] or foreground_pid.value==browser.pid,'Unexpected foreground owner')
        require(not state['saves'],'Unexpected save');summary['cases'].append({'name':'chrome-single-left-background','dialog_opened_exactly_once':True,'physical_cursor_unchanged':True,'application_activated_itself':before[2]!=after[2]})
        # The following foreground keyboard action belongs in the same normal API
        # and must raise Chrome automatically, without asking the Agent for a mode.
        phase='automatic-foreground-followup';close_attempted=True
        client.execute(app,'automatic-foreground-followup',f'await app.keypress({{window_ref:{wr},keys:["ESC"]}});return await app.keypress({{window_ref:{wr},keys:["ALT","F4"]}});');require(browser.wait(10)==0,'Chrome exit');summary['cases'].append({'name':'chrome-foreground-keyboard-continuation','normal_exit':0})
        summary['passed']=True
    except BaseException:
        summary['failure']=traceback.format_exc();summary['failure_phase']=phase
        if browser and title:
            try:
                path=out/(phase+'-windows.json');write(path,window_diagnostics(browser.pid,'DTW Workbench '+title));summary['failure_time_window_diagnostics']=str(path)
            except BaseException:summary['window_diagnostic_failure']=traceback.format_exc()
        if phase=='content-capture' and client:
            capture_ledger=read(client.output/'content-capture-ledger.json') or {}
            capture_failure=(capture_ledger.get('script') or {}).get('failure') or {}
            retain_scene=capture_failure.get('code')=='windows_capture_occluded'
    finally:
        if close_attempted and browser:
            # Preserve an uncertain keypress even if the window naturally closed.
            # Observe once; never resend Escape or Alt+F4 during cleanup.
            started=time.monotonic();observation={'close_replayed':False,'timeout_seconds':10}
            try:observation['returncode']=browser.wait(10)
            except subprocess.TimeoutExpired:observation['timed_out']=True
            observation['elapsed_seconds']=time.monotonic()-started;summary['cleanup'].append({'owned_browser_exit_observation':observation})
        if client:
            try:summary['cleanup'].append(client.close())
            except BaseException:summary['passed']=False;summary['cleanup'].append({'error':traceback.format_exc()})
        if retain_scene:
            # Keep the actual failure scene for the supervisor, including the
            # foreground fixture if it is itself the occluder. MCP is closed.
            summary['retained_for_supervisor_inspection']={'browser_pid':browser.pid,'browser_exit_if_exited':browser.poll(),
                'profile':str(out/'profile'),'url':url,'server_pid':server.pid,'server_exit_if_exited':server.poll(),
                'server_stop_file':str(web/'stop'),'fixture_pid':fixture.pid if fixture else None,
                'fixture_stop_file':str(directory/'stop') if fixture else None,
                'reason':'Persistent capture occlusion; no input or automatic dismissal replayed'}
        if fixture and not retain_scene:
            (directory/'stop').write_text('stop');summary['cleanup'].append({'fixture_exit':fixture.wait(5)})
        if browser and not retain_scene and browser.poll() is None:browser.terminate();summary['passed']=False;summary['cleanup'].append({'forced_browser_exit':browser.wait(5)})
        if server and not retain_scene:
            if server.poll() is None:
                (web/'stop').touch()
                try:server.wait(5)
                except subprocess.TimeoutExpired:server.terminate();summary['passed']=False;summary['cleanup'].append({'forced_server_exit':server.wait(5)})
            summary['cleanup'].append({'server_exit':server.wait(5)})
        write(out/'summary.json',summary);print(json.dumps(summary))
    return 0 if summary['passed'] else 1

if __name__=='__main__':raise SystemExit(main())
