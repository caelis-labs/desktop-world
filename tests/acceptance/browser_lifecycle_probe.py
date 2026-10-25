#!/usr/bin/env python3
"""Opt-in macOS GUI probe: idle EOF releases session-owned isolated Chromium.
No personal-profile attachment. Uses native prepare receipts to identify owned PIDs.
"""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
from probe import call, exec_call, result, structured_result

assert sys.platform=='darwin', 'This real process-lifecycle probe currently targets macOS'
root=Path(sys.argv[2]);root.mkdir(parents=True,exist_ok=False)
report=[]
for trial in range(3):
    p=subprocess.Popen([sys.argv[1]],stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                       stderr=(root/f'{trial}-stderr.log').open('w'),text=True)
    pid=None;command=None
    try:
        call(p,{'jsonrpc':'2.0','id':1,'method':'initialize','params':{
            'protocolVersion':'2025-06-18','capabilities':{},
            'clientInfo':{'name':'browser-lifecycle','version':'1'}}})
        started=exec_call(p,2,{'operation':'browser_prepare','arguments':{
            'allow_launch':True,'profile':{'mode':'isolated_new'}}})
        done=result(p,3,None,started['id']) if started['status']=='running' else started
        assert done['status']=='completed',done
        native=structured_result(p,4,None,done['id'])
        (root/f'{trial}-prepare.json').write_text(json.dumps(native,indent=2))
        assert native['action']=='launched_isolated_browser',native
        pid=native['prepared_pid']
        command=subprocess.run(['ps','-o','command=','-p',str(pid)],capture_output=True,text=True).stdout.strip()
        assert '/CuaDriver/BrowserProfiles/isolated-' in command and '--user-data-dir=' in command,command
        before=time.monotonic();p.stdin.close();p.wait(timeout=8)
        assert p.returncode==0,p.returncode
        deadline=time.monotonic()+5
        while time.monotonic()<deadline:
            state=subprocess.run(['ps','-o','stat=','-p',str(pid)],capture_output=True,text=True).stdout.strip()
            if not state or state.startswith('Z'):break
            time.sleep(.05)
        else:raise AssertionError(f'Owned browser PID {pid} remained live after idle EOF')
        report.append({'trial':trial,'pid':pid,'idle_eof':'browser exited','seconds':round(time.monotonic()-before,3)})
    finally:
        if p.poll() is None:
            p.kill();p.wait()
        if pid and command:
            current=subprocess.run(['ps','-o','command=','-p',str(pid)],capture_output=True,text=True).stdout.strip()
            if current==command:os.kill(pid,signal.SIGTERM) # only a proven fixture-owned residual
        (root/'report.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))
