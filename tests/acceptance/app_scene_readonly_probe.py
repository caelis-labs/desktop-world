#!/usr/bin/env python3
"""Read-only script corpus on already-running real apps; redact UI payloads."""
import argparse,copy,hashlib,json
from pathlib import Path
from standard_agent_probe import Mcp,write_json
class RedactedMcp(Mcp):
    def _record(self,direction,message):
        if direction=='response' and isinstance(message.get('result'),dict):
            result=message['result'];data=result.get('structuredContent')
            if isinstance(data,dict) and 'view' in data:
                view=data['view'];safe={'app':data.get('app'),'status':data.get('status'),'error':data.get('error'),'window_count':len(view.get('windows',[])),'element_count':len(view.get('elements',[])),'semantics':view.get('semantics'),'image':view.get('image')}
                message=copy.deepcopy(message);message['result']['structuredContent']=safe
                for item in message['result'].get('content',[]):
                    if item.get('type')=='text':item['text']=json.dumps(safe)
                    elif item.get('type')=='image':item['data']='<redacted UI image>'
        super()._record(direction,message)
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--dtw',required=True,type=Path);p.add_argument('--output',required=True,type=Path);a=p.parse_args()
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False);out.chmod(0o700)
    report={'binary_sha256':hashlib.sha256(a.dtw.read_bytes()).hexdigest(),'cases':[],'scope':'look/capture only on discovered running apps; no mouse, keyboard or quit scripts; UI payloads redacted'}
    m=RedactedMcp(a.dtw.resolve(),out)
    try:
        m.rpc('initialize',{'protocolVersion':'2025-06-18','capabilities':{},'clientInfo':{'name':'scene-readonly-corpus','version':'1'}});m.notify('notifications/initialized')
        for app in ['com.apple.finder','com.google.Chrome','com.kingsoft.wpsoffice.mac','dev.caelis.bot']:
            found,_=m.tool('apps',{'query':app,'state':'running'});matches=[r for r in found['apps'] if r['app']==app]
            if not matches:report['cases'].append({'app':app,'status':'not_running_skipped'});continue
            data,result=m.tool('exec',{'app':app,'source':'await app.look({full:true});'})
            view=data.get('view',{});image=view.get('image',{});elements=view.get('elements',[])
            usable=bool(elements) or any(c['type']=='image' for c in result.get('content',[]))
            status='usable_read' if usable and not result.get('isError') else 'unavailable'
            if view.get('running') and not view.get('windows') and not result.get('isError'):status='running_without_window'
            case={'app':app,'status':status,'window_count':len(view.get('windows',[])),'element_count':len(elements),'semantic_coverage':view.get('semantics'),'image':image,'error':data.get('error'),'startup':view.get('startup')}
            if view.get('windows'):
                captured,response=m.tool('exec',{'app':app,'source':'await app.capture({full:true});'})
                case['explicit_capture']={'image':captured.get('view',{}).get('image'),'has_image':any(c['type']=='image' for c in response.get('content',[])),'error':captured.get('error')}
            report['cases'].append(case)
            write_json(out/'report.json',report)
    finally:report['cleanup']=m.close();write_json(out/'report.json',report)
    print(json.dumps(report,ensure_ascii=False))
if __name__=='__main__':main()
