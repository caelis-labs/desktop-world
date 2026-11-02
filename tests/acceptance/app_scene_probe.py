#!/usr/bin/env python3
"""Owned native scene acceptance over the two public MCP tools; no model A/B."""
import argparse,base64,hashlib,json,os,plistlib,subprocess,time,uuid
from pathlib import Path
from standard_agent_probe import Mcp,require,wait_until,write_json

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--dtw',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--display-setup',type=Path);args=parser.parse_args()
    out=args.output.resolve();out.mkdir(parents=True,exist_ok=False);out.chmod(0o700)
    bundle=out/'Scene.app';exe=bundle/'Contents/MacOS/Scene';exe.parent.mkdir(parents=True)
    identifier='dev.caelis.dtw.scene.'+uuid.uuid4().hex
    with (bundle/'Contents/Info.plist').open('wb') as f:plistlib.dump({'CFBundleExecutable':'Scene','CFBundleIdentifier':identifier,'CFBundleName':'DTW Scene','CFBundlePackageType':'APPL'},f)
    source=Path(__file__).resolve().parents[1]/'fixtures/app-scene/main.swift'
    subprocess.run(['xcrun','swiftc','-module-cache-path',str(out/'swift-cache'),str(source),'-o',str(exe)],check=True,capture_output=True,timeout=90)
    report={'binary_sha256':hashlib.sha256(args.dtw.read_bytes()).hexdigest(),'cases':[],'limits':['owned AppKit scene; no claim of app-family or cross-platform completeness']};m=Mcp(args.dtw.resolve(),out)
    def record(name,**details): report['cases'].append({'name':name,'passed':True,**details});write_json(out/'report.json',report)
    def state():
        try:return json.loads((out/'state.json').read_text())
        except (FileNotFoundError,json.JSONDecodeError):return None
    def command(text):
        (out/'command').write_text(text);wait_until(lambda:state() and state()['command']==text)
    def run(source):
        data,result=m.tool('exec',{'app':str(bundle),'source':source});require(not result.get('isError'),data);return data,result
    def capture(name):
        data,result=run('await app.capture({full:true});');require('error' not in data['view'].get('image',{}),data)
        images=[c for c in result['content'] if c['type']=='image'];require(len(images)==1,data)
        (out/(name+'.png')).write_bytes(base64.b64decode(images[0]['data']));return data
    def point(view,title):
        windows=[w for w in view['windows'] if w['visible']];xs=[w['bounds'][0] for w in windows];ys=[w['bounds'][1] for w in windows]
        left,top=min(xs),min(ys);right=max(w['bounds'][0]+w['bounds'][2] for w in windows);bottom=max(w['bounds'][1]+w['bounds'][3] for w in windows)
        p=state()['buttons'].get(title) or next(w['button'] for w in state()['windows'] if w['title'].endswith(title));image=view['image']
        return {'x':(p[0]-left)/(right-left)*image['width'],'y':(p[1]-top)/(bottom-top)*image['height']}
    try:
        m.rpc('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'scene-native-acceptance','version':'1'}});m.notify('notifications/initialized')
        require([t['name'] for t in m.rpc('tools/list',{})['tools']]==['apps','exec'],'two tools only');record('two_public_tools')
        for name,arguments in [('exec',{'source':'return 1'}),('api',{}),('result',{})]:
            data,result=m.tool(name,arguments);require(result.get('isError') is True,data)
        record('required_app_and_retired_routes')
        stopped,_=m.tool('apps',{'query':'TextEdit','state':'stopped'});require(stopped['apps'] and all(not r['running'] for r in stopped['apps']),stopped);record('installed_stopped_discovery',rows=stopped['apps'],coverage=stopped['coverage'])
        first,_=run('await app.look();');require(state(),'automatic startup');record('automatic_start_and_baseline',view=first['view'])
        image=capture('stack');require(len(image['view']['windows'])>=2,image);record('stacked_capture',image=image['view']['image'])
        before=len(state()['gestures']);a=point(image['view'],'gesture-start');b=point(image['view'],'peer')
        rejected,response=m.tool('exec',{'app':str(bundle),'source':'await app.drag({path:'+json.dumps([a,b])+'});'})
        require(response.get('isError') and rejected.get('failure',{}).get('code')=='cross_window_drag',rejected);require(len(state()['gestures'])==before,state());record('cross_window_drag_rejected_before_input')
        p=point(image['view'],'peer');run('await app.click('+json.dumps(p)+');')
        wait_until(lambda:sum(state()['counts'].values())==1);require(state()['counts']['peer']==1 and state()['counts']['main']==0,state());record('overlap_click_hits_top_surface',state=state())
        command('popup');image=capture('popup');require(len(image['view']['windows'])>=3,image)
        p=point(image['view'],'popup');run('await app.click('+json.dumps(p)+');');wait_until(lambda:state()['counts']['popup']==1);require(state()['counts']['peer']==1,state());record('floating_popup_capture_and_click',state=state())
        command('move_popup');data,result=m.tool('exec',{'app':str(bundle),'source':'await app.click('+json.dumps(p)+');'});require(result.get('isError') is True,data);record('stale_scene_rejected_without_repeat',failure=data)
        command('close_popup');image=capture('close-peer');p=point(image['view'],'Close DTW Scene peer');run('await app.click('+json.dumps(p)+');');wait_until(lambda:not any(w['title'].endswith('peer') for w in state()['windows']));record('close_window_through_normal_ui')
        run("await app.keypress(['META','O']);");wait_until(lambda:state()['panel']['visible'])
        image=capture('open-panel');record('system_open_panel_capture',coverage=image['view']['coverage'],windows=len(image['view']['windows']))
        run("await app.keypress(['ESC']);");wait_until(lambda:state()['panel']['result']=='cancel');record('system_open_panel_keyboard_cancel')
        owned=out/'owned.txt';owned.write_text('owned open panel input')
        run("await app.keypress(['META','O']);");wait_until(lambda:state()['panel']['visible'])
        run("await app.keypress(['META','SHIFT','G']);")
        run('await app.typeText('+json.dumps(str(owned))+');')
        run("await app.keypress(['ENTER']);")
        run("await app.keypress(['ENTER']);");wait_until(lambda:state()['panel']['result']=='ok')
        require(state()['panel']['opened']==str(owned),state());record('helper_dialog_path_input_and_open',state=state()['panel'])
        run("await app.keypress(['META','S']);");wait_until(lambda:state()['panel']['visible']);image=capture('system-save-panel')
        run("await app.keypress(['ENTER']);");wait_until(lambda:state()['panel']['result']=='ok' and (out/'saved.txt').exists())
        require((out/'saved.txt').read_text()=='saved by native panel','save readback');record('system_save_panel_independent_file_readback')
        run("await app.keypress(['META','S']);");wait_until(lambda:state()['panel']['visible']);run("await app.keypress(['ENTER']);")
        image=capture('overwrite-confirmation');require(any(e.get('name')=='Replace' for e in image['view']['elements']),image);record('system_overwrite_confirmation_is_observable')
        cancel=next(e['id'] for e in image['view']['elements'] if e.get('role')=='button' and e.get('name')=='Cancel')
        cancelled,response=m.tool('exec',{'app':str(bundle),'source':'await app.click({target:'+json.dumps(cancel)+'});'})
        if response.get('isError'):require(cancelled.get('effects',{}).get('dispatch',{}).get('unknown')==1,cancelled)
        def underlying_save():
            observed,result=m.tool('exec',{'app':str(bundle),'source':'await app.look({full:true,screenshot:true});'})
            view=observed.get('view',{});windows=view.get('windows',[])
            return observed if windows and windows[0].get('title')=='Save' and 'error' not in view.get('image',{}) else None
        wait_until(underlying_save);require(state()['panel']['result']=='pending',state());run("await app.keypress(['ESC']);")
        wait_until(lambda:state()['panel']['result']=='cancel');require((out/'saved.txt').read_text()=='saved by native panel','overwrite cancel readback');record('nested_overwrite_cancel_preserves_file')
        command('block_quit');pending,_=run('await app.quit();');require(pending['view'].get('running',True),pending)
        image=capture('save-sheet');require('Keep Open' in state()['buttons'],state());record('quit_save_sheet_is_observable',image=image['view']['image'])
        p=point(image['view'],'Keep Open');run('await app.click('+json.dumps(p)+');');wait_until(lambda:state()['cancelled']==1);record('save_sheet_coordinate_fallback',state=state())
        command('allow_quit')
        if args.display_setup:
            displays=json.loads(subprocess.check_output([str(args.display_setup),'layout'],text=True))['screens'];report['displays']=displays
            primary=next(d for d in displays if d['primary']);external=next((d for d in displays if not d['primary']),None)
            if external:
                placements=[('primary',primary['bounds'][0]+300,primary['bounds'][1]+200),('external',external['bounds'][0]+200,external['bounds'][1]+80)]
                if external['bounds'][1]==primary['bounds'][1]+primary['bounds'][3]:placements.append(('spanning',max(primary['bounds'][0],external['bounds'][0])+300,external['bounds'][1]-117))
                before=capture('before-displays');old=point(before['view'],'gesture-start')
                for name,x,y in placements:
                    command('move:'+str(x)+':'+str(y))
                    rejected,response=m.tool('exec',{'app':str(bundle),'source':'await app.move('+json.dumps(old)+');'})
                    require(response.get('isError') and rejected.get('failure',{}).get('code')=='stale_scene',rejected)
                    image=capture('display-'+name);main=next(w for w in image['view']['windows'] if w['title'].endswith('main'))
                    if name=='spanning' and (abs(main['bounds'][0]-x)>=3 or abs(main['bounds'][1]-y)>=3):
                        report.setdefault('skipped',[]).append({'name':'physical_spanning_window','reason':'OS relocated the requested spanning frame','requested':[x,y],'actual':main['bounds']});write_json(out/'report.json',report);continue
                    require(abs(main['bounds'][0]-x)<3 and abs(main['bounds'][1]-y)<3,main);start=point(image['view'],'gesture-upper');end=point(image['view'],'gesture-lower');offset=len(state()['gestures'])
                    dragged,response=m.tool('exec',{'app':str(bundle),'source':'await app.drag({path:'+json.dumps([start,end])+'});'})
                    if name=='spanning' and response.get('isError'):
                        require(dragged.get('failure',{}).get('code')=='display_proof_unavailable',dragged)
                        require(dragged.get('effects',{}).get('dispatch',{}).get('not_dispatched')==1,dragged)
                        require(not any(e['kind'] in ['down','up','drag'] for e in state()['gestures'][offset:]),state())
                        report.setdefault('boundaries',[]).append({'name':'cross_display_content_drag','status':'unsupported','failure':dragged['failure'],'dispatch':'not_dispatched'})
                        record('spanning_capture_and_physical_input_refusal',windows=image['view']['windows'])
                        continue
                    require(not response.get('isError'),dragged)
                    wait_until(lambda:any(e['kind']=='up' for e in state()['gestures'][offset:]))
                    up=next(e for e in state()['gestures'][offset:] if e['kind']=='up');require(abs(up['x']-340)<2 and abs(up['y']-10)<2,up)
                    record('display_'+name+'_capture_drag_and_stale_mapping',image=image['view']['image'],windows=image['view']['windows'])
                    image=capture('display-'+name+'-after');old=point(image['view'],'gesture-start')
                    if name=='spanning':
                        run("await app.keypress(['META','O']);");wait_until(lambda:state()['panel']['visible']);panel=capture('spanning-system-panel');record('system_panel_after_cross_display_parent_relocation',windows=panel['view']['windows'])
                        run("await app.keypress(['ESC']);");wait_until(lambda:state()['panel']['result']=='cancel')
                command('move:100:448')
            else:report['display_skip']='No second physical display'
        image=capture('gesture');start=point(image['view'],'gesture-start');end=point(image['view'],'gesture-end')
        offset=len(state()['gestures']);run('await app.move('+json.dumps(start)+');')
        wait_until(lambda:any(e['kind']=='move' for e in state()['gestures'][offset:]));record('hover_delivers_real_mouse_move')
        run('await app.drag({path:'+json.dumps([start,end])+'});')
        wait_until(lambda:any(e['kind']=='up' for e in state()['gestures'][offset:]))
        events=state()['gestures'][offset:];require(sum(e['kind']=='down' for e in events)==1 and sum(e['kind']=='up' for e in events)==1,events)
        require(any(e['kind']=='drag' and e['dx']!=0 for e in events) and abs(next(e for e in events if e['kind']=='up')['x']-340)<2,events);record('drag_has_one_paired_press_and_exact_endpoint',events=events)
        offset=len(state()['gestures']);rejected,response=m.tool('exec',{'app':str(bundle),'source':'await app.drag({path:[]});'})
        require(response.get('isError') and not any(e['kind']=='down' for e in state()['gestures'][offset:]),rejected);record('invalid_drag_has_no_down')
        command('interrupt_drag');image=capture('interrupt-drag');start=point(image['view'],'gesture-start');end=point(image['view'],'gesture-end');offset=len(state()['gestures'])
        rejected,response=m.tool('exec',{'app':str(bundle),'source':'await app.drag({path:'+json.dumps([start,end])+'});'})
        require(response.get('isError'),rejected);wait_until(lambda:any(e['kind']=='up' for e in state()['gestures'][offset:]))
        events=state()['gestures'][offset:];require(sum(e['kind']=='down' for e in events)==1 and sum(e['kind']=='up' for e in events)==1,events);record('occluded_drag_stops_and_releases_without_replay',failure=rejected,events=events)
        command('clear_obstacle')
        image=capture('slider');start=point(image['view'],'slider-start');end=point(image['view'],'slider-end')
        run('await app.drag({path:'+json.dumps([start,end])+'});');wait_until(lambda:state()['slider']>90);record('standard_appkit_slider_value_changes',value=state()['slider'])
        image=capture('cancel-drag');start=point(image['view'],'gesture-start');end=point(image['view'],'gesture-end');offset=len(state()['gestures'])
        call=m.begin('tools/call',{'name':'exec','arguments':{'app':str(bundle),'source':'await app.drag({path:'+json.dumps([start,end,start])+'});'}})
        wait_until(lambda:any(e['kind']=='down' for e in state()['gestures'][offset:]))
        m.notify('notifications/cancelled',{'requestId':call,'reason':'owned drag cancellation boundary'})
        wait_until(lambda:any(e['kind']=='up' for e in state()['gestures'][offset:]))
        events=state()['gestures'][offset:];require(sum(e['kind']=='down' for e in events)==1 and sum(e['kind']=='up' for e in events)==1,events)
        def read_after_cancel():
            observed,response=m.tool('exec',{'app':str(bundle),'source':'await app.capture({full:true});'})
            if observed.get('error',{}).get('code')=='busy':return None
            require(not response.get('isError'),observed);require('error' not in observed.get('view',{}).get('image',{}),observed);return observed
        recovered=wait_until(read_after_cancel);require(recovered['view'].get('reset') is True,recovered);record('cancelled_drag_releases_and_ordinary_read_recovers',events=events)
        strict_dir=out/'strict-pointer';strict_dir.mkdir();strict=Mcp(args.dtw.resolve(),strict_dir,mcp_args=('--input-policy','background-only'))
        try:
            strict.rpc('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'scene-strict-pointer','version':'1'}});strict.notify('notifications/initialized')
            shot,_=strict.tool('exec',{'app':str(bundle),'source':'await app.capture({full:true});'});p=point(shot['view'],'gesture-start');offset=len(state()['gestures'])
            rejected,response=strict.tool('exec',{'app':str(bundle),'source':'await app.move('+json.dumps(p)+');'})
            require(response.get('isError') and rejected.get('failure',{}).get('code')=='foreground_required',rejected);require(not any(e['kind']=='move' for e in state()['gestures'][offset:]),state());record('pointer_gesture_respects_background_only')
        finally:strict.close()
        image=capture('close-last');p=point(image['view'],'Close DTW Scene main');run('await app.click('+json.dumps(p)+');');wait_until(lambda:not state()['windows'])
        empty,_=run('await app.look({full:true});');require(empty['view']['running'] and not empty['view']['windows'],empty);record('running_app_without_windows')
        strict_dir=out/'strict';strict_dir.mkdir();strict=Mcp(args.dtw.resolve(),strict_dir,mcp_args=('--input-policy','background-only'))
        try:
            strict.rpc('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'scene-strict','version':'1'}});strict.notify('notifications/initialized')
            strict.tool('exec',{'app':str(bundle),'source':'await app.look();'})
            rejected,response=strict.tool('exec',{'app':str(bundle),'source':"await app.keypress(['META','N']);"})
            require(response.get('isError') and rejected.get('failure',{}).get('code')=='foreground_required',rejected);require(state()['new_requests']==0,state());record('windowless_shortcut_respects_background_only')
        finally:strict.close()
        run("await app.keypress(['META','N']);");wait_until(lambda:state()['new_requests']==1 and state()['windows']);record('windowless_app_shortcut_creates_window',state=state())
        before=state()['inputs']['DTW Scene main']
        rejected,response=m.tool('exec',{'app':str(bundle),'source':"await app.click({selector:{role:'text_field'}});"})
        require(response.get('isError') and rejected.get('failure',{}).get('code')=='element_not_pressable',rejected)
        require(any(c['type']=='image' for c in response['content']),rejected)
        require(state()['inputs']['DTW Scene main']==before,state());record('semantic_rejection_provides_image_without_input')
        image=capture('text-field');p=point(image['view'],'scene-input');text='DTW 中文🙂 café'
        run('await app.click('+json.dumps(p)+"); await app.keypress(['META','A']); await app.typeText("+json.dumps(text)+');')
        wait_until(lambda:state()['inputs']['DTW Scene main']==text);record('coordinate_focus_and_unicode_keyboard_readback',state=state())
        semantic,response=run('await app.look();');require(not any(c['type']=='image' for c in response['content']),semantic);record('explicit_semantic_look_stops_unnecessary_images')
        rejected,response=m.tool('exec',{'app':str(bundle),'source':'await app.click('+json.dumps(p)+');'})
        require(response.get('isError') and rejected.get('failure',{}).get('code')=='capture_required',rejected);require(state()['inputs']['DTW Scene main']==text,state());record('semantic_look_retires_old_image_coordinates')
        run('await app.quit();')
        pid=state()['pid'];wait_until(lambda:subprocess.run(['kill','-0',str(pid)],capture_output=True).returncode!=0)
        time.sleep(.2);require(subprocess.run(['kill','-0',str(pid)],capture_output=True).returncode!=0,'quit auto-relaunched');record('quit_does_not_auto_relaunch')
        restarted,_=run('await app.look();');require(state()['pid']!=pid,state());require(restarted['view'].get('reset') is True,restarted);require(restarted['view']['running'] is True,restarted);record('restart_resets_baseline',view=restarted['view'])
        run('await app.quit();');report['passed']=True
    except BaseException as error:
        report['passed']=False;report['failure']=str(error);raise
    finally:
        current=state()
        if current and subprocess.run(['kill','-0',str(current['pid'])],capture_output=True).returncode==0:
            (out/'command').write_text('quit')
            try:wait_until(lambda:subprocess.run(['kill','-0',str(current['pid'])],capture_output=True).returncode!=0)
            except AssertionError:
                owned=subprocess.run(['ps','-p',str(current['pid']),'-o','comm='],text=True,capture_output=True).stdout.strip()
                if owned==str(exe):os.kill(current['pid'],15)
        report['cleanup']=m.close();write_json(out/'report.json',report)
    print(json.dumps({'passed':report['passed'],'cases':len(report['cases']),'report':str(out/'report.json')}))
if __name__=='__main__':main()
