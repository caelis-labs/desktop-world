#!/usr/bin/env python3
"""Measure desktop-capture coordinate mapping on an isolated Chrome canvas."""

import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
import uuid

from accept_chrome import exact_chrome_pid, run_script
from probe import call, exec_call, result


def image_call(proc, ident, session, args, path):
    response = call(proc, {'jsonrpc':'2.0','id':ident,'method':'tools/call',
                           'params':{'name':'exec','arguments':args}}, timeout=30)
    content = response['result']['content']
    summary = json.loads(content[0]['text'])
    if summary['status'] == 'running':
        response = call(proc, {'jsonrpc':'2.0','id':ident+1,'method':'tools/call',
                               'params':{'name':'exec','arguments':{
                                   'session':session,'operation':'result',
                                   'execution_id':summary['execution_id'],'part':'image'}}},timeout=30)
        content = response['result']['content']
        summary = json.loads(content[0]['text'])
    assert summary['status'] == 'completed', summary
    image = next(item for item in content if item['type'] == 'image')
    path.write_bytes(base64.b64decode(image['data']))
    return summary['capture']


def main():
    package = Path(sys.argv[1]).resolve()
    root = Path('/private/tmp') / f'dtw-cua-coordinate-{uuid.uuid4().hex[:8]}'
    root.mkdir(mode=0o700)
    html = root / 'coordinate_probe.html'
    shutil.copy2(Path(__file__).parent/'fixtures/coordinate_probe.html',html)
    profile = root/'chrome-profile'
    opened = subprocess.run(['open','-g','-na','/Applications/Google Chrome.app','--args',
                             f'--user-data-dir={profile}','--no-first-run',
                             '--no-default-browser-check','--disable-background-mode',
                             '--force-renderer-accessibility','--disable-translate',
                             f'--app={html.as_uri()}'],capture_output=True,text=True)
    if opened.returncode:
        raise RuntimeError(opened.stderr)
    pid = None
    try:
        for _ in range(100):
            try:
                pid = exact_chrome_pid(profile)
                break
            except (subprocess.CalledProcessError,RuntimeError):
                time.sleep(.1)
        if pid is None:
            raise RuntimeError('temporary Chrome PID not found')
        proc = subprocess.Popen([str(package/'bin/dtw')],stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,
                                env={'PATH':'/usr/bin:/bin','TMPDIR':'/private/tmp'})
        try:
            call(proc,{'jsonrpc':'2.0','id':1,'method':'initialize','params':{
                'protocolVersion':'2025-06-18','capabilities':{},
                'clientInfo':{'name':'coordinate-probe','version':'1'}}})
            session = 'coordinate-probe'
            app_script = (f'const a=await cua.getApp({{pid:{pid},title:"DTW Coordinate Probe"}});'
                          'const q=await a.query({name:"Coordinate grid"});'
                          'return {window_id:a.windowId,items:q};')
            for attempt in range(30):
                _, target = run_script(proc,2+attempt*2,session,app_script)
                if target['status']=='completed':
                    break
                time.sleep(.5)
            assert target['status']=='completed',target
            window_id = target['value']['window_id']
            items = target['value']['items']
            assert len(items)==1 and items[0].get('frame'),items
            frame = items[0]['frame']
            focused = exec_call(proc,100,{'session':session,'operation':'focus',
                                        'pid':pid,'window_id':window_id})
            if focused['status']=='running':
                focused = result(proc,101,session,focused['execution_id'])
            assert focused['status']=='completed' and focused['focus']['verified'],focused
            points=[]
            for index,(cx,cy) in enumerate([(100,100),(400,250),(700,400)]):
                cap = image_call(proc,10+index*10,session,{
                    'session':session,'operation':'desktop','include_image':True,
                    'max_image_dimension':1600,'wait_ms':4000},root/f'before-{index}.png')
                x=(frame['x']+1+cx)*cap['screenshot_width']/cap['screen_width']
                y=(frame['y']+1+cy)*cap['screenshot_height']/cap['screen_height']
                script=(f'return await cua.call("click",{{target:{{kind:"desktop",display_id:"primary"}},'
                        f'x:{x:.3f},y:{y:.3f},capture_id:{json.dumps(cap["capture_id"])}}});')
                _,click=run_script(proc,12+index*10,session,script)
                assert click['status']=='completed',click
                _,after=run_script(proc,14+index*10,session,
                    f'const a=await cua.getApp({{pid:{pid},windowId:{window_id}}});'
                    'const s=await a.getState({query:"CLICK",max_elements:30,include_screenshot:false});'
                    'return s.tree_markdown;')
                assert after['status']=='completed',after
                tree=after['value']
                import re
                match=re.search(r'CLICK (\d+),(\d+)',tree)
                actual=tuple(map(int,match.groups())) if match else None
                points.append({'intended_canvas':[cx,cy], 'screenshot_xy':[round(x,3),round(y,3)],
                               'actual_canvas':actual,
                               'error_canvas_px':[actual[0]-cx,actual[1]-cy] if actual else None,
                               'capture_geometry':{k:cap[k] for k in ('screenshot_width','screenshot_height','screen_width','screen_height')},
                               'click_summary':click.get('value',{}).get('summary')})
            report={'package':str(package),'pid':pid,'frame':frame,'points':points,
                    'html_sha256':hashlib.sha256(html.read_bytes()).hexdigest()}
            (root/'record.json').write_text(json.dumps(report,indent=2))
            print(json.dumps({'record':str(root/'record.json'),'points':points}))
        finally:
            proc.stdin.close()
            proc.wait(timeout=8)
    finally:
        if pid is not None:
            try:
                if exact_chrome_pid(profile)==pid:
                    os.kill(pid,signal.SIGTERM)
            except (subprocess.CalledProcessError,RuntimeError,ProcessLookupError):
                pass


if __name__=='__main__':
    main()
