#!/usr/bin/env python3
"""Measure real WKWebView AX discovery and report unsupported boundaries explicitly."""
import argparse
import hashlib
import json
from pathlib import Path
import time
import traceback
import uuid

from standard_agent_probe import Fixture, Mcp, require, save_png, successful, wait_until, write_json

FIND = """
const ws=(await app.windows()).windows.filter(w=>w.title==='DTW Discovery Browser');
if(ws.length!==1) throw new Error('Expected one owned web window');
const window_ref=ws[0].window_ref;
let page=await app.observe({window_ref,role:'button',text:'Discovery Tail',limit:5});
let pages=1, matches=page.elements;
while(page.next){page=await app.observe({cursor:page.next});pages++;matches.push(...page.elements);}
state.target={window_ref,observation_ref:page.observation_ref,element_ref:matches.length===1?matches[0].element_ref:null};
return {pages,coverage:page.coverage,matches};
"""
ACT = """
const receipt=await app.press(state.target);
let page=await app.observe({window_ref:state.target.window_ref,text:'Count:',limit:5});
const counts=page.elements.map(e=>e.label);
while(page.next){page=await app.observe({cursor:page.next});counts.push(...page.elements.map(e=>e.label));}
const screenshot=await app.capture({window_ref:state.target.window_ref});
return {receipt,counts,coverage:page.coverage,screenshot};
"""

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dtw',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--warmup',action='store_true',help='Explicitly read and capture before discovery; reported separately from cold-start acceptance')
    args=parser.parse_args()
    output=args.output.absolute();output.mkdir(mode=0o700)
    binary=args.dtw.resolve(strict=True)
    source=Path(__file__).resolve().parents[1]/'fixtures/native-discovery-web/main.swift'
    evidence={'kind':'owned-wkwebview-native-ax','binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
              'fixture_source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
              'explicit_read_capture_warmup':args.warmup,
              'started_unix_ms':int(time.time()*1000),'accepted':False}
    fixture=mcp=None;failure=None
    try:
        fixture=Fixture.prepare(output/'fixture','dev.dtw.web-discovery.'+uuid.uuid4().hex,source)
        fixture.start()
        evidence['fixture_before']=wait_until(lambda:fixture.state() if fixture.state().get('ready') else None,timeout=30)
        mcp=Mcp(binary,output)
        mcp.rpc('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'web-discovery-probe','version':'1'}})
        mcp.notify('notifications/initialized')
        evidence['app']=fixture.app_id
        if args.warmup:
            warmup,_=mcp.execute(fixture.app_id,'warmup',"const w=(await app.windows()).windows.find(w=>w.title==='DTW Discovery Browser').window_ref;const sample=await app.observe({window_ref:w,limit:5});await app.capture({window_ref:w});return {coverage:sample.coverage,next:sample.next};")
            evidence['warmup']=successful(warmup)
        record,_=mcp.execute(fixture.app_id,'find',FIND)
        found=successful(record);evidence['discovery']=found
        require(fixture.counters()=={'clicks':0,'actions_started':0},'Read changed the page')
        if len(found['matches'])==1 and found['matches'][0]['can_press']:
            record,response=mcp.execute(fixture.app_id,'press',ACT)
            result=successful(record);evidence['action']=result
            require(result['receipt']['outcome']['dispatch']=='dispatched',result)
            wait_until(lambda:fixture.counters()=={'clicks':1,'actions_started':1})
            require(any('Count: 1' in value for value in result['counts']),result)
            require(fixture.state()['last_button']=='Discovery Tail',fixture.state())
            evidence['capture']=save_png(output,response,result['screenshot'])
            evidence['accepted']=True
        else:
            evidence['boundary']='Tail target not uniquely actionable through the current exact-process AX route; no input attempted'
            # A second independent read records the exposed role/label boundary;
            # it does not retry input or turn this failed acceptance into a pass.
            diagnostic,response=mcp.execute(fixture.app_id,'boundary',"const sample=await app.observe({window_ref:state.target.window_ref,limit:12});const screenshot=await app.capture({window_ref:state.target.window_ref});return {sample,screenshot};")
            detail=successful(diagnostic);evidence['boundary_sample']=detail['sample']
            evidence['capture']=save_png(output,response,detail['screenshot'])
            require(fixture.counters()=={'clicks':0,'actions_started':0},'Boundary inspection changed the page')
        evidence['fixture_after']=fixture.state()
    except BaseException as error:
        failure={'type':type(error).__name__,'message':str(error)}
        (output/'failure.txt').write_text(traceback.format_exc())
    finally:
        evidence['cleanup']=[]
        for name,resource in [('dtw',mcp),('fixture',fixture)]:
            if resource is None:continue
            try:
                closed=resource.close();evidence['cleanup'].append({'resource':name,**closed})
                require(not closed.get('forced') and closed.get('returncode',0)==0,closed)
            except BaseException as error:
                failure=failure or {'type':'CleanupFailure','message':str(error)}
        evidence['exec_projection']=mcp.projection_checks if mcp else []
        evidence['failure']=failure;evidence['passed']=failure is None and evidence['accepted']
        evidence['finished_unix_ms']=int(time.time()*1000)
        write_json(output/'evidence.json',evidence)
    print(json.dumps({'passed':evidence['passed'],'evidence':str(output/'evidence.json'),'boundary':evidence.get('boundary'),'failure':failure},ensure_ascii=False))
    return 0 if evidence['passed'] else 1


if __name__=='__main__':raise SystemExit(main())
