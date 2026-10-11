#!/usr/bin/env python3
"""Verify image-region -> AX candidate -> click on an isolated Chrome window."""

import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid

from accept_chrome import exact_chrome_pid, run_script
from probe import call


def main():
    package = Path(sys.argv[1]).resolve()
    root = Path('/private/tmp') / f'dtw-cua-region-{uuid.uuid4().hex[:8]}'
    root.mkdir(mode=0o700)
    profile = root / 'chrome-profile'
    html = root / 'region.html'
    html.write_text('<!doctype html><meta charset="utf-8"><title>DTW Region Probe</title>'
                    '<h1 id="status">READY-REGION</h1>'
                    '<button onclick="document.getElementById(\'status\').textContent=\'CLICKED-REGION\'">'
                    'Region target</button>')
    opened = subprocess.run(['open', '-g', '-na', '/Applications/Google Chrome.app', '--args',
                             f'--user-data-dir={profile}', '--no-first-run',
                             '--no-default-browser-check', '--disable-background-mode',
                             '--force-renderer-accessibility', '--disable-translate',
                             f'--app={html.as_uri()}'], capture_output=True, text=True)
    if opened.returncode:
        raise RuntimeError(opened.stderr)
    pid = None
    try:
        for _ in range(100):
            try:
                pid = exact_chrome_pid(profile)
                break
            except (subprocess.CalledProcessError, RuntimeError):
                time.sleep(.1)
        if pid is None:
            raise RuntimeError('isolated Chrome PID not found')
        proc = subprocess.Popen([str(package / 'bin/dtw')], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                env={'PATH': '/usr/bin:/bin', 'TMPDIR': '/private/tmp'})
        try:
            call(proc, {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
                'protocolVersion': '2025-06-18', 'capabilities': {},
                'clientInfo': {'name': 'region-probe', 'version': '1'}}})
            session = 'region-probe'
            inspect = (f'const a=await cua.getApp({{pid:{pid},title:"DTW Region Probe"}});'
                       'const s=await a.getState({tree_format:"elements",max_elements:100,'
                       'include_screenshot:true,max_image_dimension:1600});'
                       'return {window_id:a.windowId,valid:s.screenshot_frame_valid,'
                       'size:[s.screenshot_width,s.screenshot_height],truncated:s.truncated,'
                       'button:(s.elements||[]).filter(e=>e.label==="Region target")};')
            for attempt in range(20):
                _, before = run_script(proc, 2 + attempt * 2, session, inspect)
                if before['status'] == 'completed' and before['value']['button']:
                    break
                time.sleep(.5)
            assert before['status'] == 'completed', before
            value = before['value']
            assert value['valid'] and len(value['button']) == 1, value
            frame = value['button'][0]['screenshot_frame']
            assert all(frame[k] > 0 for k in ('w', 'h')), frame
            roi = {'x': max(0, frame['x'] - 3), 'y': max(0, frame['y'] - 3),
                   'w': frame['w'] + 6, 'h': frame['h'] + 6}
            script = (f'const roi={json.dumps(roi)};'
                      f'const app=await cua.getApp({{pid:{pid},windowId:{value["window_id"]}}});'
                      'const s=await app.getState({tree_format:"elements",max_elements:100,'
                      'include_screenshot:true,max_image_dimension:1600});'
                      'const overlaps=f=>f&&f.w>0&&f.h>0&&f.x<roi.x+roi.w&&f.x+f.w>roi.x&&'
                      'f.y<roi.y+roi.h&&f.y+f.h>roi.y;'
                      'const candidates=(s.elements||[]).filter(e=>e.enabled!==false&&'
                      'e.element_token&&(e.actions||[]).includes("AXPress")&&'
                      'overlaps(e.screenshot_frame)).slice(0,20).map(e=>({label:e.label,'
                      'role:e.role,token:e.element_token,center:[e.screenshot_frame.x+'
                      'e.screenshot_frame.w/2,e.screenshot_frame.y+e.screenshot_frame.h/2],'
                      'frame:e.screenshot_frame}));'
                      'return {valid:s.screenshot_frame_valid,capture_id:s.capture_id,'
                      'size:[s.screenshot_width,s.screenshot_height],truncated:s.truncated,'
                      'candidates};')
            _, candidates = run_script(proc, 100, session, script)
            assert candidates['status'] == 'completed', candidates
            result = candidates['value']
            assert result['valid'] and result['capture_id'], result
            exact = [c for c in result['candidates'] if c['label'] == 'Region target']
            assert len(exact) == 1 and exact[0]['center'] == [
                frame['x'] + frame['w'] / 2, frame['y'] + frame['h'] / 2], result
            # Act by the returned native element token, never by an unrelated window.
            _, click = run_script(proc, 102, session,
                                  f'const a=await cua.getApp({{pid:{pid},windowId:{value["window_id"]}}});'
                                  f'return await a.click({json.dumps(exact[0]["token"])});')
            assert click['status'] == 'completed', click
            _, after = run_script(proc, 104, session,
                                  f'const a=await cua.getApp({{pid:{pid},windowId:{value["window_id"]}}});'
                                  'const s=await a.getState({query:"CLICKED-REGION",'
                                  'max_elements:50,include_screenshot:false});'
                                  'return {clicked:String(s.tree_markdown).includes("CLICKED-REGION")};')
            assert after['status'] == 'completed' and after['value']['clicked'], after
            record = {'package': str(package), 'pid': pid, 'window_id': value['window_id'],
                      'roi': roi, 'candidate_count': len(result['candidates']),
                      'target': {k: exact[0][k] for k in ('label', 'role', 'center', 'frame')},
                      'image_size': result['size'], 'truncated': result['truncated'],
                      'click_status': click['status'], 'readback': after['value']}
            path = root / 'record.json'
            path.write_text(json.dumps(record, indent=2))
            print(json.dumps({'record': str(path), **record}))
        finally:
            proc.stdin.close()
            proc.wait(timeout=8)
    finally:
        if pid is not None:
            try:
                if exact_chrome_pid(profile) == pid:
                    os.kill(pid, signal.SIGTERM)
            except (subprocess.CalledProcessError, RuntimeError, ProcessLookupError):
                pass


if __name__ == '__main__':
    main()
