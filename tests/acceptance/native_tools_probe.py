#!/usr/bin/env python3
"""Read-only native bridge contract; no browser launch or GUI input."""
import json
import subprocess
import sys
from probe import call, exec_call, result, runtime_env, structured_result

OPERATIONS = ['get_browser_state', 'browser_prepare', 'browser_navigate',
              'browser_click', 'browser_type', 'browser_select', 'browser_dialog',
              'browser_pointer', 'browser_set_input_files', 'browser_download',
              'health_report', 'get_guide']

def main():
    proc = subprocess.Popen([sys.argv[1]], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True, encoding="utf-8", env=runtime_env())
    try:
        call(proc, {'jsonrpc':'2.0','id':1,'method':'initialize','params':{
            'protocolVersion':'2025-06-18','capabilities':{},
            'clientInfo':{'name':'native-tools-probe','version':'1'}}})
        inventory = call(proc, {'jsonrpc':'2.0','id':2,'method':'tools/list'})
        tools = inventory['result']['tools']
        assert [t['name'] for t in tools] == ['exec']
        advertised = tools[0]['inputSchema']['properties']['operation']['enum']
        for i, name in enumerate(OPERATIONS, 10):
            assert name in advertised, name
            described = exec_call(proc, i, {'operation':'describe','name':name})
            assert not described.get('is_error'), described
            schema = described['arguments_schema']
            assert 'session' not in schema.get('properties', {}), schema
            assert 'session' not in schema.get('required', []), schema
        for i, key in enumerate(['session','session_id','public_session','_session_id',
                                 '_permission_mode','_authorization_context',
                                 '_cua_browser_download_mcp_host_approved'], 40):
            denied = exec_call(proc, i, {'operation':'browser_prepare',
                'arguments':{key:'forged','allow_launch':True,'profile':{'mode':'isolated_new'}}})
            assert denied.get('is_error') and 'session_managed_by_runtime' in denied['error'], denied
            assert 'id' not in denied, denied  # rejected before dispatch
        for i, name in enumerate(['page','set_config','end_session','run_steps'], 60):
            denied = exec_call(proc, i, {'operation':'describe','name':name})
            assert denied.get('is_error'), denied
        health = exec_call(proc, 80, {'operation':'health_report',
                                     'arguments':{'include':['binary_version']}})
        health = result(proc, 81, None, health['id']) if health['status']=='running' else health
        assert health['status']=='completed', health
        detail = health['data']
        assert '0.38.0' in json.dumps(detail), detail
        guide = exec_call(proc, 83, {'operation':'get_guide','arguments':{'topic':'browser'}})
        guide = result(proc, 84, None, guide['id']) if guide['status']=='running' else guide
        assert guide['status']=='completed' and 'browser' in guide['text'], guide
        bad = exec_call(proc, 90, {'operation':'browser_click','arguments':{}})
        bad = result(proc, 91, None, bad['id']) if bad['status']=='running' else bad
        assert bad['status']=='failed' and bad.get('code'), bad
        print(json.dumps({'schemas':len(OPERATIONS),'session_forgery':'refused before dispatch',
                          'health':'complete native data','guide':'available',
                          'native_error_code':bad['code']}))
    finally:
        proc.stdin.close()
        try: proc.wait(timeout=8)
        except subprocess.TimeoutExpired: proc.kill(); proc.wait()
        assert proc.returncode == 0, proc.stderr.read()[-2000:]

if __name__ == '__main__': main()
