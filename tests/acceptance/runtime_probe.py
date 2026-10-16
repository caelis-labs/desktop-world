#!/usr/bin/env python3
"""Runtime context isolation and legacy-session rejection, without desktop input."""
import json, os, subprocess, sys, tempfile
from probe import call, exec_call, result

def start(binary):
 p=subprocess.Popen([binary],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=tempfile.TemporaryFile(mode='w+'),text=True)
 call(p,{'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'implicit-runtime-probe','version':'1'}}})
 return p

def run(p,n,script):
 v=exec_call(p,n,{'script':script});return result(p,n+1,None,v['id']) if v.get('status')=='running' else v

def main():
 a=start(sys.argv[1]); b=start(sys.argv[1])
 try:
  schema=call(a,{'jsonrpc':'2.0','id':2,'method':'tools/list','params':{}})['result']['tools'][0]['inputSchema']
  assert 'session' not in schema['properties']
  first=run(a,3,'state.n=(state.n||0)+1; return state.n;')
  second=run(a,5,'state.n=(state.n||0)+1; return state.n;')
  independent=run(b,3,'state.n=(state.n||0)+1; return state.n;')
  assert [first['value'],second['value'],independent['value']]==[1,2,1]
  foreign=exec_call(b,6,{'operation':'result','id':first['id']});assert foreign.get('error')=='execution_not_found',foreign
  legacy=exec_call(a,7,{'session':'other','script':'state.n=999;'});assert 'session_managed_by_runtime' in legacy.get('error',''),legacy
  assert run(a,8,'return state.n;')['value']==2
  print(json.dumps({'implicit_state':'passed','separate_process_state':'passed','foreign_receipt':'refused','legacy_named_session':'refused_without_execution'}))
 finally:
  for p in (a,b):
   p.stdin.close()
   try:p.wait(timeout=8)
   except subprocess.TimeoutExpired:p.kill();p.wait()
   assert p.returncode==0,p.returncode
if __name__=='__main__':main()
