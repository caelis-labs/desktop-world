#!/usr/bin/env python3
"""Opt-in real TextEdit control-character input with another owned app in front.

Uses ordinary stdio MCP and disposable plain-text documents. Each input script
is dispatched once; exact saved bytes and fresh window inventories verify its
effects. Foreground samples are coverage, not a desktop-idle requirement.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time
import uuid

from background_input_probe import front, fresh_end
from browser_workbench_probe import checked_windows, close_owned_window, initialize
from standard_agent_probe import Fixture, require, successful, wait_until, write_json

APP = 'com.apple.TextEdit'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = args.output.absolute(); root.mkdir(mode=0o700)
    repo = Path(__file__).resolve().parents[2]
    binary = args.dtw.resolve(strict=True)
    evidence = {'kind':'real-TextEdit-control-input-standard-MCP', 'passed':False,
                'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
                'global_desktop_idle_required':False, 'input_retries':0, 'cases':[]}
    observer = client = None
    try:
        observer = Fixture.prepare(root/'observer', 'dev.dtw.text-control.'+uuid.uuid4().hex,
                                   repo/'tests/fixtures/native-input/main.swift')
        write_json(observer.root/'fixture.json', {'background_probe':True,'x':700})
        observer.start()
        client = initialize(binary, root/'mcp')
        cases = [
            ('whole-text', 'Header\n中文 🐦\tend\r\nLast', 'Header\n中文 🐦\tend\nLast'),
            ('named-return', None, 'Alpha\nBeta'),
        ]
        for name, text, expected in cases:
            document = root/('DTW-Control-'+uuid.uuid4().hex+'.txt')
            document.write_text('Owned seed\n')
            subprocess.run(['open','-a','TextEdit',str(document)],check=True,timeout=10)
            row = {'case':name}; evidence['cases'].append(row)
            def opened():
                record,_ = client.execute(APP,'select-owned-document','return await app.windows();')
                matches = [w for w in checked_windows(record,APP) if w['title']==document.name]
                require(len(matches)<=1,'Owned title is ambiguous')
                return matches[0] if matches else None
            window = wait_until(opened,timeout=10)
            row['before'] = front(observer)
            source = 'const window_ref='+json.dumps(window['window_ref'])+';'
            source += 'await app.keypress({window_ref,keys:["META","A"]});'
            if text is not None:
                source += 'await app.typeText({window_ref,text:'+json.dumps(text)+'});'
            else:
                source += 'await app.typeText({window_ref,text:"Alpha"});await app.keypress({window_ref,keys:["RETURN"]});await app.typeText({window_ref,text:"Beta"});'
            source += 'await app.keypress({window_ref,keys:["META","S"]});return true;'
            row['input'],_ = client.execute(APP,name,source)
            successful(row['input'])
            row['after'] = fresh_end(observer)
            row['foreground_return_observed'] = row['after']['frontmost_pid']==observer.child.pid
            wait_until(lambda:document.read_bytes()==expected.encode('utf-8'),timeout=5)
            saved = document.read_bytes()
            require(saved == expected.encode('utf-8'), 'Saved bytes changed after readback')
            row['saved_text'] = saved.decode('utf-8')
            row['saved_sha256'] = hashlib.sha256(saved).hexdigest()
            row['close'] = {}
            close_owned_window(client,APP,window['window_ref'],row['close'],
                               epoch=client.context_epochs.get(APP,0))
            row['passed'] = True
        evidence['passed'] = True
    except BaseException as error:
        evidence['failure'] = {'type':type(error).__name__,'message':str(error)}
    finally:
        evidence['cleanup'] = []
        for name,resource in [('mcp',client),('observer',observer)]:
            if resource is None: continue
            row = {'resource':name}
            try:
                row['result'] = resource.close()
                row['passed'] = row['result'].get('returncode')==0 and not row['result'].get('forced')
            except BaseException as error:
                row.update(passed=False,error=str(error))
            evidence['cleanup'].append(row)
            evidence['passed'] = evidence['passed'] and row['passed']
        evidence['finished_unix_ms'] = int(time.time()*1000)
        write_json(root/'evidence.json',evidence)
    print(json.dumps({'passed':evidence['passed'],'cases':[
        {k:r.get(k) for k in ['case','passed','foreground_return_observed']} for r in evidence['cases']],
        'failure':evidence.get('failure')},ensure_ascii=False))
    return 0 if evidence['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
