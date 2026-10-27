#!/usr/bin/env python3
"""Desktop-independent validation of explicit observation schemas and errors."""
import subprocess
import sys
from probe import call, exec_call, runtime_env

proc = subprocess.Popen([sys.argv[1]], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE, text=True, env=runtime_env())
try:
    call(proc, {'jsonrpc':'2.0','id':1,'method':'initialize','params':{
        'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'observation-contract','version':'1'}}})
    discovery = exec_call(proc, 2, {'operation':'describe'})
    assert discovery['next_call'] == {'operation':'apps'}, discovery
    for i, name in enumerate(['apps','windows','observe'], 10):
        assert name in discovery['operations']
        schema = exec_call(proc, i, {'operation':'describe','name':name})
        assert schema['available'] and schema['arguments_schema']['additionalProperties'] is False, schema
    for i, args in enumerate([
        {'operation':'windows'}, {'operation':'windows','arguments':{'pid':1,'app':'Ambiguous'}},
        {'operation':'observe','arguments':{'pid':1}},
        {'operation':'observe','arguments':{'pid':1,'window_id':2,'ready_timeout_ms':1}},
        {'operation':'observe','arguments':{'pid':1,'window_id':2,'ready_when':'x','ready_timeout_ms':5001}},
        {'operation':'observe','arguments':{'pid':1,'window_id':2,'ready_when':' '}},
        {'operation':'apps','arguments':{'running':'false'}},
        {'operation':'apps','arguments':{'session':'forged'}},
        {'operation':'apps','arguments':{'limit':0}},
        {'operation':'app','pid':1,'window_id':0},
    ], 30):
        refused = exec_call(proc, i, args)
        assert refused['is_error'] and 'id' not in refused, refused
    print('observation schemas and pre-dispatch validation passed')
finally:
    proc.stdin.close()
    proc.wait(timeout=10)
    assert proc.returncode == 0, proc.stderr.read()
