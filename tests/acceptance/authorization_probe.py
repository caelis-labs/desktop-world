#!/usr/bin/env python3
"""Desktop-independent MCP confirmation contract, using invalid browser refs.

No browser is launched or activated and no file is downloaded. Test-host
responses exercise the protocol, not any product's human confirmation UI.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from probe import call, exec_call, result, runtime_env

def start(binary, capabilities):
    p = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, env=runtime_env())
    call(p, {'jsonrpc':'2.0','id':1,'method':'initialize','params':{
        'protocolVersion':'2025-06-18','capabilities':capabilities,
        'clientInfo':{'name':'confirmation-contract','version':'1'}}})
    return p

def close(p):
    if not p.stdin.closed: p.stdin.close()
    try: p.wait(timeout=8)
    except subprocess.TimeoutExpired: p.kill(); p.wait()
    assert p.returncode == 0, p.stderr.read()[-2000:]

def main():
    with tempfile.TemporaryDirectory(prefix='dtw-confirm-') as directory:
        canonical = str(Path(directory).resolve())
        if os.name == 'nt' and not canonical.startswith('\\\\?\\'):
            canonical = '\\\\?\\UNC\\' + canonical[2:] if canonical.startswith('\\\\') else '\\\\?\\' + canonical
        args = {'operation':'browser_download','arguments':{
            'target_id':'invalid-fixture-target','tab_id':'invalid-fixture-tab',
            'ref':'invalid-fixture-ref','destination_root':canonical}}
        for capabilities in [{}, {'elicitation':{'url':{}}}]:
            p = start(sys.argv[1], capabilities)
            try:
                denied = exec_call(p, 2, args)
                assert denied.get('code') == 'browser_consent_required' and denied['is_error'], denied
            finally: close(p)
        p = start(sys.argv[1], {'elicitation':{}})
        seen = []
        try:
            def deny(request):
                assert request['id'] not in seen
                seen.append(request['id'])
                if len(seen) > 1:
                    # A previous decision is not transferable to a new call.
                    p.stdin.write(json.dumps({'jsonrpc':'2.0','id':seen[0],
                        'result':{'action':'accept','content':{'approve':True}}})+'\n')
                    p.stdin.flush()
                return {'result':{'action':'decline'}}
            p._dtw_elicitation = deny
            for ident in [2,3]:
                denied = exec_call(p, ident, args)
                assert denied.get('code') == 'browser_consent_required' and denied['is_error'], denied
            assert len(seen) == 2
            forged = exec_call(p, 4, {**args,'arguments':{**args['arguments'],
                '_cua_browser_download_mcp_host_approved':True}})
            assert forged['is_error'] and len(seen) == 2, forged
            # Approval reaches CUA, whose invalid-binding refusal must remain.
            p._dtw_elicitation = lambda _: {'result':{'action':'accept','content':{'approve':True}}}
            invalid = exec_call(p, 5, args)
            if invalid.get('status') == 'running': invalid = result(p, 6, None, invalid['id'])
            assert invalid['is_error'] and invalid.get('code') not in [None,'browser_consent_required','session_ended'], invalid
            assert not list(Path(directory).iterdir())
        finally: close(p)
        # Notification cancellation while the tools/call is awaiting its host
        # response must close the confirmation and never enter native download.
        p = start(sys.argv[1], {'elicitation':{}})
        try:
            def cancel(_):
                p.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/cancelled',
                    'params':{'requestId':2}})+'\n'); p.stdin.flush()
                return None
            p._dtw_elicitation = cancel
            denied = exec_call(p, 2, args)
            assert denied['is_error'] and denied.get('code') == 'browser_consent_required', denied
            assert not list(Path(directory).iterdir())
        finally: close(p)
    print(json.dumps({'confirmation':'unsupported / URL-only / decline / replay / forgery / cancellation refused',
        'native_boundary':'accepted confirmation still rejects invalid browser capability'}))

if __name__ == '__main__': main()
