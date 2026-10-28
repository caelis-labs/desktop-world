#!/usr/bin/env python3
"""Opt-in real AX pagination through ordinary stdio MCP, using an owned large AppKit tree.

This proves native traversal and reference composition, not browser-specific AX
enablement, lock-screen operation, or an AX-independent window identity path.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import time
import traceback
import uuid

from standard_agent_probe import Fixture, Mcp, require, save_png, successful, write_json


FIND_TAIL = """
const ws=(await app.windows()).windows;
const chosen=ws.filter(w=>w.title==='DTW Native Target');
if(chosen.length!==1) throw new Error('Ambiguous fixture window');
state.window=chosen[0].window_ref;
let page=await app.observe({window_ref:state.window,role:'button',text:'discovery tail',limit:5});
let pages=1, visited=[page.coverage.visited], matches=page.elements.map(e=>({name:e.name,role:e.role,can_press:e.can_press}));
while(page.next){
  page=await app.observe({cursor:page.next}); pages++; visited.push(page.coverage.visited);
  matches.push(...page.elements.map(e=>({name:e.name,role:e.role,can_press:e.can_press})));
}
return {pages,visited,matches,coverage:page.coverage};
"""

COMPOSE = """
const ws=(await app.windows()).windows;
const w=ws.find(w=>w.title==='DTW Native Target').window_ref;
const peer=ws.find(w=>w.title==='DTW Native Peer').window_ref;
const first=await app.observe({window_ref:w,role:'button',text:'Discovery',limit:25});
if(first.elements.length!==25 || !first.next) throw new Error('Expected a bounded first page');
let page=first,pages=1,total=first.elements.length,visited=[first.coverage.visited];
const firstElement=first.elements[0];
while(page.next){
  page=await app.observe({cursor:page.next});pages++;total+=page.elements.length;visited.push(page.coverage.visited);
  if(page.observation_ref!==first.observation_ref) throw new Error('Query identity changed');
  if(page.elements.length>25) throw new Error('Page limit exceeded');
}
const other=await app.observe({window_ref:peer,role:'button',text:'Increment',limit:10});
const screenshot=await app.capture({window_ref:w});
const again=await app.windows();
if(again.windows.find(x=>x.title==='DTW Native Target').window_ref!==w) throw new Error('Inventory replaced a live window ref');
const action=await app.press({window_ref:w,observation_ref:first.observation_ref,element_ref:firstElement.element_ref});
// The earlier page remains actionable after paging, capture, another window's
// semantic read, and an inventory refresh. The peer query also remains alive.
const peerAction=await app.press({window_ref:peer,observation_ref:other.observation_ref,element_ref:other.elements.find(e=>e.can_press).element_ref});
const count=await app.observe({window_ref:w,text:'Count:',limit:5});
return {pages,total,visited,coverage:page.coverage,firstElement:{name:firstElement.name,role:firstElement.role},action,peerAction,count:count.elements.map(e=>e.label),screenshot};
"""

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    require(sys.platform=='darwin','Requires real macOS GUI')
    output=args.output.absolute(); output.mkdir(mode=0o700)
    binary=args.dtw.resolve(strict=True)
    source=Path(__file__).resolve().parents[1]/'fixtures/native-core/main.swift'
    evidence={'kind':'native-paged-discovery-and-standard-mcp','live_llm_host':False,
              'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
              'fixture_source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
              'started_unix_ms':int(time.time()*1000)}
    fixture=mcp=None
    failure=None
    try:
        fixture=Fixture.prepare(output/'fixture','dev.dtw.discovery.'+uuid.uuid4().hex,source)
        evidence['fixture_start']=fixture.start()
        evidence['large_tree']=fixture.command('big_tree')
        require(evidence['large_tree']['large_tree'],'Large fixture tree not installed')
        mcp=Mcp(binary,output)
        evidence['app']=fixture.app_id
        mcp.rpc('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'paged-discovery-probe','version':'1'}})
        mcp.notify('notifications/initialized')
        before=fixture.counters()
        found,_=mcp.execute(fixture.app_id,'tail',FIND_TAIL)
        result=successful(found); evidence['tail']=result
        require(result['matches']==[{'name':'Discovery Tail','role':'button','can_press':True}],result)
        require(result['coverage']['complete'] and result['coverage']['visited']>420,result)
        require(all(a<=b for a,b in zip(result['visited'],result['visited'][1:])),result)
        require(fixture.counters()==before,'Discovery changed fixture counters')
        write_json(output/'evidence.json',evidence)
        composed,response=mcp.execute(fixture.app_id,'compose',COMPOSE)
        result=successful(composed); evidence['composition']=result
        require(result['total']==420 and result['pages']>=17 and result['coverage']['complete'],result)
        require(all(a<b for a,b in zip(result['visited'],result['visited'][1:])),result)
        for name in ['action','peerAction']:
            require(result[name]['outcome']['dispatch']=='dispatched',result[name])
        require('Count: 2' in result['count'],result['count'])
        require(fixture.counters()=={'clicks':2,'actions_started':2},fixture.state())
        evidence['capture']=save_png(output,response,result['screenshot'])
        evidence['fixture_after']=fixture.state()
        require([e['title'] for e in evidence['fixture_after']['action_events']]==[result['firstElement']['name'],'Increment'],'Unexpected native action sequence')
    except BaseException as error:
        failure={'type':type(error).__name__,'message':str(error)}
        (output/'failure.txt').write_text(traceback.format_exc())
    finally:
        evidence['cleanup']=[]
        for name,resource in [('dtw',mcp),('fixture',fixture)]:
            if resource is None: continue
            try:
                closed=resource.close(); evidence['cleanup'].append({'resource':name,**closed})
                require(not closed.get('forced') and closed.get('returncode',0)==0,closed)
            except BaseException as error:
                failure=failure or {'type':'CleanupFailure','message':str(error)}
        evidence['exec_projection']=mcp.projection_checks if mcp else []
        evidence['failure']=failure; evidence['passed']=failure is None
        evidence['finished_unix_ms']=int(time.time()*1000)
        write_json(output/'evidence.json',evidence)
    print(json.dumps({'passed':evidence['passed'],'evidence':str(output/'evidence.json'),'failure':failure},ensure_ascii=False))
    return 0 if evidence['passed'] else 1


if __name__=='__main__':
    sys.exit(main())
