#!/usr/bin/env python3
"""Open and dismiss a native chooser in a temporary Obsidian profile only."""

import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid

from accept_chrome import run_script
from probe import call
from probe_modal_focus import focus, obsidian_pid


def main():
    package = Path(sys.argv[1]).resolve()
    root = Path('/private/tmp') / f'dtw-cua-obsidian-open-{uuid.uuid4().hex[:8]}'
    root.mkdir(mode=0o700)
    profile = root / 'profile'
    record = {'root': str(root)}
    proc = None
    pid = None
    prior_pid = None
    try:
        opened = subprocess.run(['open', '-g', '-na', '/Applications/Obsidian.app',
                                 '--args', f'--user-data-dir={profile}'],
                                capture_output=True, text=True)
        if opened.returncode:
            raise RuntimeError(opened.stderr)
        for _ in range(100):
            try:
                pid = obsidian_pid(profile)
                break
            except (subprocess.CalledProcessError, RuntimeError):
                time.sleep(0.1)
        if pid is None:
            raise RuntimeError('temporary Obsidian process not found')
        record['pid'] = pid
        proc = subprocess.Popen([str(package / 'bin/dtw')], stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                env={'PATH': '/usr/bin:/bin', 'TMPDIR': '/private/tmp'})
        call(proc, {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
            'protocolVersion': '2025-06-18', 'capabilities': {},
            'clientInfo': {'name': 'dtw-open-panel-probe', 'version': '1'}}})
        session = 'obsidian-open-panel'
        _, active = run_script(proc, 2, session,
                               'return (await cua.listApps()).filter(a=>a.active).map(a=>a.pid);')
        if active.get('status') == 'completed' and len(active.get('value', [])) == 1:
            prior_pid = active['value'][0]
        for _ in range(30):
            _, main = run_script(proc, 4, session,
                                 f'const w=(await cua.listWindows({{pid:{pid}}})).filter(x=>x.pid==={pid});'
                                 'if(w.length!==1)return {windows:w};'
                                 f'const a=await cua.getApp({{pid:{pid},windowId:w[0].window_id}});'
                                 'const s=await a.getState({max_elements:80,include_screenshot:false});'
                                 'return {windows:w,tree:s.tree_markdown};')
            if main.get('status') == 'completed' and '打开本地仓库' in main.get('value', {}).get('tree', ''):
                break
            time.sleep(0.5)
        windows = main.get('value', {}).get('windows', [])
        if len(windows) != 1 or '打开本地仓库' not in main.get('value', {}).get('tree', ''):
            raise RuntimeError('temporary welcome view not uniquely identified')
        main_id = windows[0]['window_id']
        record['main_window_id'] = main_id
        record['main_focus'] = focus(proc, 20, session, pid, main_id)
        _, opened = run_script(proc, 30, session,
                               f'const a=await cua.getApp({{pid:{pid},windowId:{main_id}}});'
                               "const q=await a.query({role:'button',name:'打开'});"
                               "if(q.length!==1)return {found:q.length};"
                               "const action=await a.click({role:'button',name:'打开'},"
                               "{delivery_mode:'foreground'});"
                               f'const windows=(await cua.listWindows({{pid:{pid}}}))'
                               f'.filter(x=>x.pid==={pid}&&x.window_id!=={main_id});'
                               'const found=windows.filter(x=>x.bounds.width>100&&x.bounds.height>100);'
                               'let panel=null;'
                               'if(found.length===1){'
                               f'const p=await cua.getApp({{pid:{pid},windowId:found[0].window_id}});'
                               'const s=await p.getState({max_elements:100,include_screenshot:false});'
                               "const c=await p.query({role:'button',name:'取消'});"
                               'panel={window_id:found[0].window_id,element_count:s.element_count,'
                               'truncated:s.truncated,tree:s.tree_markdown.slice(0,1500),'
                               'cancel_count:c.length};'
                               'if(c.length===1){panel.cancel_action=await p.click('
                               "{role:'button',name:'取消'},{delivery_mode:'foreground'});}}"
                               'return {found:q.length,action,windows:windows.map(x=>'
                               '({window_id:x.window_id,title:x.title,layer:x.layer,'
                               'is_on_screen:x.is_on_screen,bounds:x.bounds})),panel};')
        record['open_action'] = opened
        if opened.get('status') != 'completed' or opened.get('value', {}).get('found') != 1:
            raise RuntimeError('Open button did not resolve uniquely')
        for delay in (0, 0.2, 0.7, 1.5):
            if delay:
                time.sleep(delay)
            _, discovery = run_script(proc, 40, session,
                                      f'const w=(await cua.listWindows()).filter(x=>'
                                      f'(x.pid==={pid}&&x.window_id!=={main_id})||'
                                      "(x.app_name==='Open and Save Panel Service'&&x.title==='打开'));"
                                      'return w.map(x=>({pid:x.pid,window_id:x.window_id,'
                                      'app_name:x.app_name,title:x.title,layer:x.layer,'
                                      'is_on_screen:x.is_on_screen,bounds:x.bounds}));')
            record.setdefault('window_samples', []).append(discovery.get('value'))
        candidates = [w for w in record['window_samples'][-1] or []
                      if w['pid'] == pid and w['is_on_screen']
                      and w['bounds']['width'] > 100 and w['bounds']['height'] > 100]
        record['proxy_candidates'] = candidates
        if len(candidates) == 1:
            panel_id = candidates[0]['window_id']
            _, state = run_script(proc, 50, session,
                                  f'const a=await cua.getApp({{pid:{pid},windowId:{panel_id}}});'
                                  'const s=await a.getState({max_elements:180,include_screenshot:false});'
                                  "const q=await a.query({role:'button',name:'取消'});"
                                  'return {tree:s.tree_markdown,element_count:s.element_count,'
                                  'truncated:s.truncated,cancel:q.map(x=>({label:x.label,role:x.role,frame:x.frame}))};')
            record['panel_state'] = state
            record['panel_focus'] = focus(proc, 60, session, pid, panel_id)
            if state.get('status') == 'completed' and len(state.get('value', {}).get('cancel', [])) == 1:
                _, cancelled = run_script(proc, 70, session,
                                          f'const a=await cua.getApp({{pid:{pid},windowId:{panel_id}}});'
                                          "const action=await a.click({role:'button',name:'取消'},"
                                          "{delivery_mode:'foreground'});"
                                          f'const windows=await cua.listWindows({{pid:{pid}}});'
                                          'return {action,windows:windows.map(x=>'
                                          '({window_id:x.window_id,title:x.title,is_on_screen:x.is_on_screen}))};')
                record['cancel_action'] = cancelled
    finally:
        if proc is not None:
            if prior_pid is not None and prior_pid != pid:
                try:
                    _, active = run_script(proc, 90, 'obsidian-open-panel',
                                           'return (await cua.listApps()).filter(a=>a.active).map(a=>a.pid);')
                    if active.get('value') == [pid]:
                        record['restore_prior'] = focus(proc, 92, 'obsidian-open-panel', prior_pid)
                except Exception as exc:
                    record['restore_error'] = str(exc)
            proc.stdin.close()
            try:
                proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        (root / 'record.json').write_text(json.dumps(record, ensure_ascii=False, indent=2))
        print(json.dumps({'record': str(root / 'record.json'), 'pid': pid,
                          'candidates': record.get('proxy_candidates'),
                          'panel_focus': record.get('panel_focus', {}).get('native'),
                          'cancel': record.get('cancel_action', {}).get('status')},
                         ensure_ascii=False)[:3000], flush=True)
        if pid is not None:
            try:
                if obsidian_pid(profile) == pid:
                    os.kill(pid, signal.SIGTERM)
                    for _ in range(20):
                        try:
                            os.kill(pid, 0)
                        except ProcessLookupError:
                            break
                        time.sleep(0.1)
                    else:
                        if obsidian_pid(profile) == pid:
                            os.kill(pid, signal.SIGKILL)
            except (subprocess.CalledProcessError, RuntimeError, ProcessLookupError):
                pass


if __name__ == '__main__':
    main()
