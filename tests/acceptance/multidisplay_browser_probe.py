#!/usr/bin/env python3
"""Opt-in macOS multi-display checks on owned Chrome windows via standard DTW MCP.

Same --dtw/--output arguments as background_browser_probe. Requires two real
screens with different backing scales and a negative-origin secondary display.
AX moves are fixture setup only; all tested capture/clicks use DTW. No display
configuration, user profiles, or unknown-effect input is changed/replayed.
"""
import json
from pathlib import Path
import subprocess
import time
import tempfile

import background_browser_probe as browser
from standard_agent_probe import require, successful, save_png, wait_until

HELPER = Path('/private/tmp/dtw-multidisplay-setup')
MARKER = '''
  const probeWindow = location.hash === '#guard' ? 'guard' : 'target';
  if(probeWindow === 'guard') document.title += ' guard';
  const marker = document.createElement('button');
  marker.id='multi-target'; marker.setAttribute('aria-label','Multi display target');
  marker.style.cssText='position:fixed;left:20px;top:160px;width:80px;height:60px;background:rgb(231,31,31);border:0;border-radius:0;padding:0;margin:0;z-index:99999;box-shadow:none';
  document.body.appendChild(marker);
'''
INSTRUMENTATION = browser.INSTRUMENTATION.replace('  let sequence = 0;', MARKER+'  let sequence = 0;')
INSTRUMENTATION = INSTRUMENTATION.replace('{probe_seq: ++sequence, ...details}', '{probe_seq: ++sequence, probe_window:probeWindow, ...details}')
INSTRUMENTATION = INSTRUMENTATION.replace("rects:{save:rect(", "rects:{marker:rect('#multi-target'),save:rect(")


def helper(*args):
    result=subprocess.run([str(HELPER), *map(str,args)],text=True,capture_output=True,timeout=10)
    require(result.returncode==0, {'setup_args':list(map(str,args)),'stdout':result.stdout,'stderr':result.stderr})
    return json.loads(result.stdout)


def state(path):
    return json.loads(path.read_text())


def snapshot(path, kind='target', predicate=lambda s:True):
    def ready():
        rows=[e for e in state(path)['events'] if e.get('type')=='probe_snapshot' and e.get('probe_window')==kind]
        current=max(rows,key=lambda e:e['probe_seq']) if rows else None
        return current if current and predicate(current) else None
    return wait_until(ready,timeout=8)


def point_for(snapshot, image):
    w,r=snapshot['window'],snapshot['rects']['marker']
    client=[r['x']+r['width']/2,r['y']+r['height']/2]
    local=[(w['width']-w['inner_width'])/2+client[0],w['height']-w['inner_height']+client[1]]
    return {'x':round(local[0]*image['width']/w['width']),
            'y':round(local[1]*image['height']/w['height']),
            'client':client,'screen':[w['x']+local[0],w['y']+local[1]],'page':snapshot}


def pointer_events(path, offset):
    return [e for e in state(path)['events'][offset:] if e.get('type') in ('probe_pointerdown','probe_pointerup','probe_click')]


def select_window(client,title):
    r,_=client.execute(browser.APP,'select-window','const w=(await app.windows()).windows.filter(w=>w.title==='+json.dumps(title)+');if(w.length!==1)throw Error("Owned window not unique");state.w=w[0].window_ref;return w[0];')
    return successful(r)


def run_cases(client,observer,path,output,evidence,pid):
    evidence['kind']='owned-Chrome-multidisplay-standard-MCP'
    evidence['setup_helper_sha256']=browser.file_sha256(HELPER)
    evidence['probe_source_sha256']=browser.file_sha256(Path(__file__))
    evidence['passed_scope']='screenshot pixel/DOM coordinate agreement, negative origins, mixed scale, cross-display stale references and auxiliary-event guards'
    layout=helper('layout')['screens'];evidence['display_layout']=layout
    primary=next(d for d in layout if d['primary'])
    external=next(d for d in layout if not d['primary'] and (d['bounds'][0]<0 or d['bounds'][1]<0) and d['scale']!=primary['scale'])
    title=evidence['owned_window']['title']
    guard_title=title+' guard'
    subprocess.run([evidence['chrome_path'],'--user-data-dir='+evidence['profile'],'--new-window',evidence['server']['url']+'#guard'],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15)
    snapshot(path,'guard')
    # Same Chrome process, a different window at the auxiliary global point.
    evidence['chrome_guard_move']=helper('move',pid,guard_title+' - Google Chrome',-120,-650)
    g=snapshot(path,'guard',lambda s: s['window']['x']<=-1<s['window']['x']+s['window']['width'] and s['window']['y']<=-1<s['window']['y']+s['window']['height'])
    evidence['guard_initial']=g
    # A different application's window also watches for real mouse events.
    evidence['observer_move']=helper('move',observer.child.pid,'DTW Browser Background Observer',-120,-452)
    browser.front(observer)
    guard = evidence['guard_positive_control'] = {}
    gw=g['window']; gx=(-1-gw['x']); gy=(-1-gw['y'])
    client_point=[gx-(gw['width']-gw['inner_width'])/2,gy-(gw['height']-gw['inner_height'])]
    guard['client_point']=client_point
    require(0<=client_point[0]<gw['inner_width'] and 0<=client_point[1]<gw['inner_height'],'Auxiliary global point is outside guard client')
    select_window(client,guard_title)
    r,response=client.execute(browser.APP,'guard-capture','state.guardshot=await app.capture({window_ref:state.w});return state.guardshot;')
    shot=successful(r);guard['capture']=r
    guard_dir=output/'guard';guard_dir.mkdir();guard['png']=save_png(guard_dir,response,shot)
    offset=len(state(path)['events'])
    r,_=client.execute(browser.APP,'guard-positive-click',f"return await app.click({{window_ref:state.w,observation_ref:state.guardshot.observation_ref,x:{gx*shot['image']['width']/gw['width']},y:{gy*shot['image']['height']/gw['height']}}});")
    guard['click']=r;successful(r);browser.require_delivery(r,['background'])
    wait_until(lambda:any(e.get('type')=='probe_click' for e in pointer_events(path,offset)),timeout=5)
    time.sleep(.5);guard['events']=pointer_events(path,offset)
    require(len(guard['events'])==3 and all(e.get('probe_window')=='guard' and e.get('trusted') for e in guard['events']),'Guard failed positive input control')
    require(all(abs(e['client_x']-client_point[0])<=1 and abs(e['client_y']-client_point[1])<=1 for e in guard['events']),'Guard coordinate control mismatch')
    ex,ey,ew,eh=external['bounds'];px,py,pw,ph=primary['bounds']
    placements=[('external',ex+20,ey+100),('primary',px+150,py+80),
                ('external-right',ex+ew-1020,ey+100),('external-return',ex+20,ey+100)]
    evidence['cases']=[]
    old=None
    for name,x,y in placements:
        proof={'name':name,'requested_position':[x,y]};evidence['cases'].append(proof)
        proof['move']=helper('move',pid,title+' - Google Chrome',x,y)
        page=snapshot(path,predicate=lambda s:abs(s['window']['x']-x)<2 and abs(s['window']['y']-y)<2)
        proof['native_after_move']=helper('inspect',pid,title+' - Google Chrome')
        require(abs(proof['native_after_move']['bounds'][0]-x)<1 and abs(proof['native_after_move']['bounds'][1]-y)<1,'Native position differs from page geometry')
        if old:
            offset=len(state(path)['events'])
            r,_=client.execute(browser.APP,name+'-stale','return await app.click({window_ref:state.w,observation_ref:state.old.observation_ref,x:'+str(old['x'])+',y:'+str(old['y'])+'});')
            proof['stale']=r
            failure=r.get('script',{}).get('failure',{})
            require(failure.get('operation')=='click' and failure.get('code') in ('stale_observation','target_changed'),'Expected stale click rejection')
            steps=r.get('script',{}).get('steps',[])
            require(len(steps)==1 and steps[0].get('operation')=='click' and steps[0].get('receipt',{}).get('outcome',{}).get('dispatch')=='not_dispatched','Missing authoritative non-dispatch receipt')
            proof['stale_age_seconds']=time.monotonic()-old['captured_at']
            require(proof['stale_age_seconds']<5,'Old capture may have expired before move check')
            time.sleep(.4);require(not pointer_events(path,offset),'Stale capture sent mouse events')
        proof['window']=select_window(client,title)
        r,response=client.execute(browser.APP,name+'-capture','state.shot=await app.capture({window_ref:state.w});return state.shot;')
        shot=successful(r);proof['capture']=r
        directory=output/name;directory.mkdir();proof['png']=save_png(directory,response,shot)
        page=snapshot(path,predicate=lambda s:abs(s['window']['x']-x)<2 and abs(s['window']['y']-y)<2)
        point=point_for(page,shot['image']);proof['point']=point
        color=helper('pixel',directory/'capture.png',point['x'],point['y']);proof['pixel']=color
        red,green,blue,alpha=color['rgba'];require(red>190 and green<65 and blue<65 and alpha>240,'Capture coordinate misses red marker')
        before=browser.fresh_end(observer);offset=len(state(path)['events'])
        r,_=client.execute(browser.APP,name+'-click',f'return await app.click({{window_ref:state.w,observation_ref:state.shot.observation_ref,x:{point["x"]},y:{point["y"]},button:"left",count:1}});')
        proof['click']=r;successful(r);browser.require_delivery(r,['background'])
        wait_until(lambda:any(e.get('type')=='probe_click' for e in pointer_events(path,offset)),timeout=5)
        time.sleep(.5)
        events=pointer_events(path,offset);proof['pointer_events']=events
        require(len(events)==3,'Extra or missing DOM mouse events (including guard window)')
        require(sorted(e['type'] for e in events)==['probe_click','probe_pointerdown','probe_pointerup'],'Unpaired DOM input')
        for event in events:
            require(event.get('trusted') and event.get('probe_window')=='target' and event.get('control')=='multi-target','Auxiliary event reached a different target')
            require(abs(event['client_x']-point['client'][0])<=1 and abs(event['client_y']-point['client'][1])<=1,'Screenshot and real DOM coordinates differ')
        after=browser.fresh_end(observer);proof['observer_before']=before;proof['observer_after']=after
        require(after['events_count']==before['events_count'],'Input leaked to observer')
        require(after['frontmost_pid']==observer.child.pid,'Background click changed foreground')
        r,_=client.execute(browser.APP,name+'-retain-shot','state.old=await app.capture({window_ref:state.w});return state.old;')
        old=point_for(snapshot(path),successful(r)['image']);old['captured_at']=time.monotonic()
        proof['passed']=True
    require({c['point']['page']['window']['device_pixel_ratio'] for c in evidence['cases']} >= {primary['scale'],external['scale']},'Mixed backing scales were not observed in Chrome')
    require(any(c['point']['screen'][0]<0 and c['point']['screen'][1]<0 for c in evidence['cases']),'Negative X/Y not exercised')
    evidence['all_cases_passed']=True


if __name__=='__main__':
    # Build from this checkout, never silently reuse another checkout's helper.
    with tempfile.TemporaryDirectory(prefix='dtw-multidisplay-setup-') as scratch:
        HELPER=Path(scratch)/'setup'
        source=Path(__file__).resolve().parents[1]/'fixtures/multidisplay_setup.swift'
        subprocess.run(['swiftc','-module-cache-path','/private/tmp/dtw-multidisplay-swift-cache',str(source),'-o',str(HELPER)],check=True)
        raise SystemExit(browser.main(case_runner=run_cases,instrumentation=INSTRUMENTATION,observer_options={'all_mouse_events':True}))
