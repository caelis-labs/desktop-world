#!/usr/bin/env python3
"""Opt-in Windows capture recovery on owned overlapping windows via DTW MCP.

No app input is driven outside the DTW under test. Fixture state and the owned
topmost negative-control helper independently establish foreground/effects.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import traceback

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from app_client import AppClient, require
from windows.transport import read
from cursor_probe import wait, write


def unchanged(state):
    return {key:state[key] for key in ('text','saved','saves','increments','left_up',
                                       'right_up','double_clicks','wheel_delta')}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    require(os.name=='nt' and sys.flags.utf8_mode, 'Use Windows: python -X utf8')
    binary = args.dtw.resolve(strict=True)
    output = args.output.resolve()
    require(not output.exists(), 'Use a new evidence directory')
    output.mkdir(parents=True)
    here = Path(__file__).parent
    summary = {'passed':False, 'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
               'kind':'owned Windows automatic capture preparation via standard MCP',
               'live_llm_host':False, 'release_artifact':False, 'cases':[], 'cleanup':[],
               'fixture_sha256':{name:hashlib.sha256((here/name).read_bytes()).hexdigest()
                                  for name in ('Fixture.cs','CursorObserver.cs')},
               'limitations':['Owned WinForms/GDI windows only, not WGC or arbitrary application capture',
                              'The capture that prepares focus contains only a capture call']}
    fixtures, client, cover = [], None, None
    try:
        from PIL import Image
        compiled = subprocess.run(['powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',
                                  str(here/'compile-cursor.ps1'),'-OutputDirectory',str(output/'build')], capture_output=True)
        (output/'compile.stdout').write_bytes(compiled.stdout)
        (output/'compile.stderr').write_bytes(compiled.stderr)
        require(compiled.returncode==0, 'Fixture compilation failed; inspect compile.stderr')
        for label in ('A','B'):
            directory = output/label
            directory.mkdir()
            executable = directory/'Fixture.exe'
            shutil.copy2(output/'build/Fixture.exe', executable)
            # Same fixture label gives identical origins; canonical executable
            # paths still provide independent app/PID/window bindings.
            process = subprocess.Popen([str(executable),str(directory),'A'])
            fixture = {'directory':directory,'executable':executable,'process':process}
            fixtures.append(fixture)
            wait(lambda: (state if (state:=read(directory/'state.json')) and
                          state['ready'] and state['bounds_ok'] and state['pid']==process.pid else None))
        a,b = fixtures
        def state(fixture):
            require(fixture['process'].poll() is None, 'Owned fixture exited before supervision read')
            result = wait(lambda:read(fixture['directory']/'state.json'))
            require(result['pid']==fixture['process'].pid, 'Fixture process identity changed')
            require(not result['closed'], 'Owned fixture window closed before supervision read')
            return result
        client = AppClient(binary, output/'protocol')
        for fixture in fixtures:
            apps, _ = client.tool('apps', {'query':str(fixture['executable']), 'state':'running'})
            fixture['app'] = next(row['app'] for row in apps['apps']
                                  if row['app'].casefold()==str(fixture['executable']).casefold())
        before_a,before_b = state(a),state(b)
        require(before_a['bounds']==before_b['bounds'], 'Fixtures do not overlap exactly')
        client.execute(b['app'], 'await app.focus();')
        wait(lambda:state(b)['foreground'] and not state(a)['foreground'])
        write(output/'occluded-before.json',{'A':state(a),'B':state(b)})
        capture = client.capture(a['app'], 'foreground-preparation')
        info = capture['view']['image']
        image_file = client.output/'foreground-preparation.png'
        image = Image.open(image_file).convert('RGB')
        require(image.size==(info['width'],info['height']), 'PNG metadata dimensions differ')
        require(sum(count for count,color in image.getcolors(image.width*image.height) if color==(173,216,230))>1000,
                'PNG does not contain the owned fixture pointer pad')
        wait(lambda:state(a)['foreground'] and not state(b)['foreground'])
        require(unchanged(state(a))==unchanged(before_a) and unchanged(state(b))==unchanged(before_b), 'Capture/focus changed a document or control')
        write(output/'automatic-after.json',{'A':state(a),'B':state(b)})
        summary['cases'].append({'name':'overlapped-window-automatic-preparation',
                                 'fresh_png':True,'image_sha256':hashlib.sha256(image_file.read_bytes()).hexdigest(),
                                 'foreground_target':True,'business_effects_unchanged':True})

        cover_state = output/'cover.json'
        cover = subprocess.Popen([str(output/'build/CursorObserver.exe'),'--spoof',str(cover_state),state(a)['hwnd']],
                                 creationflags=subprocess.CREATE_NO_WINDOW)
        cover_proof = wait(lambda:read(cover_state))
        require(cover_proof['pid']==cover.pid and cover_proof['topmost'] and cover_proof['visible'],
                'Owned persistent topmost cover was not established')
        capture = client.capture(a['app'], 'topmost-cover-masked')
        frame = capture['view']['image']
        image = Image.open(client.output/'topmost-cover-masked.png').convert('RGBA')
        bounds = cover_proof['bounds']
        click = {'x':(bounds['x'] + bounds['width']/2 - frame['bounds'][0])/frame['bounds'][2],
                 'y':(bounds['y'] + bounds['height']/2 - frame['bounds'][1])/frame['bounds'][3],
                 'unit':'fraction'}
        cover_point = (int(click['x']*image.width), int(click['y']*image.height))
        require(image.getpixel(cover_point)==(0,0,0,0), 'Foreign cover pixels were exposed')
        require(image.getchannel('A').getextrema()==(0,255), 'Uncovered pixels were lost')
        require(state(a)['foreground'], 'Capture changed the foreground owner')
        summary['cases'].append({'name':'persistent-topmost-cover-masked','cover':cover_proof,
                                 'covered_pixels_hidden':True,'uncovered_pixels_retained':True})
        rejected,response = client.execute(a['app'], f'await app.click({json.dumps(click)});', allow_error=True)
        require(response.get('isError') and rejected['error']['code']=='outside_app', rejected)
        client.capture(a['app'], 'reseed-scene')
        client.execute(b['app'], 'await app.focus();')
        wait(lambda:state(b)['foreground'] and not state(a)['foreground'])
        rejected,response = client.execute(a['app'], 'await app.click({x:0.2,y:0.2,unit:"fraction"});', allow_error=True)
        require(response.get('isError') and rejected['error']['code']=='windows_capture_occluded', rejected)
        require(state(b)['foreground'] and not state(a)['foreground'], 'Stale input stole foreground')
        require(unchanged(state(a))==unchanged(before_a) and unchanged(state(b))==unchanged(before_b), 'Rejected input changed business state')
        summary['cases'].append({'name':'masked-and-stale-points-denied','target_effects_unchanged':True})
        summary['passed'] = True
    except BaseException:
        summary['failure'] = traceback.format_exc()
    finally:
        if cover:
            (output/'cover.json.stop').touch()
            try:
                code=cover.wait(timeout=5)
                summary['cleanup'].append({'cover_exit':code})
                summary['passed'] &= code==0
            except subprocess.TimeoutExpired:
                cover.terminate(); cover.wait(timeout=5); summary['passed']=False
                summary['cleanup'].append({'forced_cover_exit':True})
        if client:
            try: summary['cleanup'].append(client.close())
            except BaseException:
                summary['passed']=False; summary['cleanup'].append({'mcp_error':traceback.format_exc()})
        for fixture in reversed(fixtures):
            (fixture['directory']/'stop').touch()
            try:
                code=fixture['process'].wait(timeout=5)
                summary['cleanup'].append({'fixture_pid':fixture['process'].pid,'exit':code})
                summary['passed'] &= code==0
            except subprocess.TimeoutExpired:
                fixture['process'].terminate(); fixture['process'].wait(timeout=5); summary['passed']=False
                summary['cleanup'].append({'forced_fixture_exit':fixture['process'].pid})
        write(output/'summary.json',summary)
        print(json.dumps(summary,ensure_ascii=True))
    return 0 if summary['passed'] else 1


if __name__=='__main__':
    raise SystemExit(main())
