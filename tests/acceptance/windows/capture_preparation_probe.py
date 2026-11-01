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

from cursor_probe import Mcp, read, require, wait, write


def unchanged(state):
    return {key:state[key] for key in ('text','saved','saves','increments','left_up',
                                       'right_up','double_clicks','wheel_delta')}


def preparation(ledger, status):
    steps = ledger['script']['steps']
    require(len(steps)==1 and steps[0]['operation']=='capture', 'Capture script issued another public operation')
    resolution = steps[0]['resolution']
    require(resolution['status']==status, resolution)
    focus = resolution['focus_preparation']
    require(focus['attempted'] is True and focus['trigger']['code']=='windows_capture_occluded', focus)
    receipt = focus['receipt']
    require(receipt['outcome']['dispatch']=='dispatched' and receipt['outcome']['error'] is None
            and receipt['interruption'] is None, receipt)
    require(receipt['outcome']['delivery']['route']=='foreground', receipt)
    return focus


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
                              'One recorded ordinary focus preparation; capture script contains no focus/input call']}
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
        client = Mcp(binary, output/'mcp')
        catalogue = client.tool('apps',{})['apps']
        for label, fixture in zip(('A','B'), fixtures):
            fixture['app'] = next(row['app'] for row in catalogue
                                   if row['app'].casefold()==str(fixture['executable']).casefold())
            rows,_ = client.execute(fixture['app'],'windows-'+label,'return (await app.windows()).windows;')
            require(len(rows)==1, 'Owned app must have exactly one test window')
            fixture['window'] = rows[0]['window_ref']
        before_a,before_b = state(a),state(b)
        require(before_a['bounds']==before_b['bounds'], 'Fixtures do not overlap exactly')
        client.execute(b['app'],'focus-occluder',f'return await app.focus({{window_ref:{json.dumps(b["window"])}}});')
        wait(lambda:state(b)['foreground'] and not state(a)['foreground'])
        write(output/'occluded-before.json',{'A':state(a),'B':state(b)})
        source = f'return await app.capture({{window_ref:{json.dumps(a["window"])}}});'
        image_count = len(client.images)
        capture,ledger = client.execute(a['app'],'automatic-capture',source)
        focus = preparation(ledger,'captured')
        returned_images = client.images[image_count:]
        require(capture['window_ref']==a['window'] and returned_images, 'Fresh target image missing')
        # exec and retained result can both carry the same captured image.
        # They must agree; transport copies do not mean capture was replayed.
        image_hashes = {hashlib.sha256(path.read_bytes()).hexdigest() for path in returned_images}
        require(len(image_hashes)==1, 'Execution and retained-result PNG bytes differ')
        image = Image.open(client.images[-1]).convert('RGB')
        require(image.size==(capture['image']['width'],capture['image']['height']), 'PNG metadata dimensions differ')
        require(sum(count for count,color in image.getcolors(image.width*image.height) if color==(173,216,230))>1000,
                'PNG does not contain the owned fixture pointer pad')
        wait(lambda:state(a)['foreground'] and not state(b)['foreground'])
        require(unchanged(state(a))==unchanged(before_a) and unchanged(state(b))==unchanged(before_b), 'Capture/focus changed a document or control')
        write(output/'automatic-after.json',{'A':state(a),'B':state(b)})
        summary['cases'].append({'name':'overlapped-window-automatic-preparation','focus_preparations':1,
                                 'focus':focus,'fresh_png':True,'image_sha256':next(iter(image_hashes)),
                                 'image_transport_copies':len(returned_images),
                                 'foreground_target':True,'business_effects_unchanged':True})

        cover_state = output/'cover.json'
        cover = subprocess.Popen([str(output/'build/CursorObserver.exe'),'--spoof',str(cover_state),state(a)['hwnd']],
                                 creationflags=subprocess.CREATE_NO_WINDOW)
        cover_proof = wait(lambda:read(cover_state))
        require(cover_proof['pid']==cover.pid and cover_proof['topmost'] and cover_proof['visible'],
                'Owned persistent topmost cover was not established')
        image_count = len(client.images)
        _,ledger = client.execute(a['app'],'persistent-cover-refused',source,error='windows_capture_occluded')
        focus = preparation(ledger,'capture_failed')
        require(len(client.images)==image_count, 'Covered capture unexpectedly returned pixels')
        require(unchanged(state(a))==unchanged(before_a) and unchanged(state(b))==unchanged(before_b), 'Failed capture changed business state')
        summary['cases'].append({'name':'persistent-topmost-cover-refused','focus_preparations':1,
                                 'focus':focus,'cover':cover_proof,'no_image_returned':True,'business_effects_unchanged':True})
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
