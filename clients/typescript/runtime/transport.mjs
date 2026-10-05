import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';

const verbs = new Set(['observe','read','sync','act','capture','get','cancel']);
export class DesktopError extends Error {
  constructor(fault, reply) {super(fault.message ?? fault.code);Object.assign(this,fault);this.reply=reply;this.receipt=reply?.result;}
}
// Only trusted host code owns this object. DesktopClient never accepts a channel.
export class SessionTransport {
  constructor(child) {
    this.child=child;this.records=new Map();this.sequence=0;this.closed=false;
    this.ready=new Promise((resolve,reject)=>{this.resolveReady=resolve;this.rejectReady=reject;});
    const fail=error=>{this.closed=true;this.rejectReady(error);for(const record of this.records.values())if(!record.reply)record.reject(error);};
    child.once('error',fail);child.stdin.on('error',fail);
    this.exited=new Promise(resolve=>child.once('exit',(code,signal)=>{fail(new DesktopError({code:'session_disconnected',message:`Session exited (${code??signal}); preserve original receipts; never replay.`}));resolve();}));
    createInterface({input:child.stdout}).on('line',line=>{
      try {
        if(Buffer.byteLength(line)>2*1024*1024)throw new Error('Session frame too large.');
        const reply=JSON.parse(line);
        if(reply.type==='hello') {
          if(reply.protocol!=='desktop-world/session-v0.1'||!reply.features?.includes('dynamic_app_grants'))throw new Error('Incompatible dtw session; use the matching RC helper.');
          this.hello=reply;this.resolveReady(reply);return;
        }
        const record=this.records.get(reply.id);
        if(!record)throw new Error('Unexpected session reply; effects may be unknown.');
        record.reply=reply;record.resolve(reply);
      }catch(error){fail(error);child.stdin.end();}
    });
  }
  static async start(helper,args=[]) {
    if(typeof helper!=='string'||!helper)throw new Error('Supply a trusted dtw executable path.');
    const session=new SessionTransport(spawn(helper,['session',...args],{stdio:['pipe','pipe','inherit'],windowsHide:true}));
    const timer=setTimeout(()=>{session.rejectReady(new Error('Session startup exceeded 15 seconds.'));session.child.stdin.end();},15000);
    try{await session.ready;return session;}catch(error){await session.close().catch(()=>{});throw error;}finally{clearTimeout(timer);}
  }
  async request(channel,op,args={},id) {
    if(channel==='desktop'&&!verbs.has(op))throw new DesktopError({code:'invalid_argument',message:'Unsupported desktop operation; authorization belongs to HostSession.'});
    if(this.closed)throw new DesktopError({code:'session_closed',message:'Do not restart to recover an uncertain effect.'});
    id??=`sdk-${++this.sequence}`;
    const body=JSON.stringify({id,channel,op,args});let record=this.records.get(id);
    if(record&&record.body!==body)throw new DesktopError({code:'request_conflict',message:'Reuse ID only with identical operation and arguments.'});
    if(!record){
      if(this.records.size>=4096)throw new DesktopError({code:'resource_exhausted',message:'Session request limit reached; retain receipts before closing.'});
      record={body};record.promise=new Promise((resolve,reject)=>{record.resolve=resolve;record.reject=reject;});record.promise.catch(()=>{});this.records.set(id,record);
      this.child.stdin.write(body+'\n',error=>{if(error){record.reject(error);this.child.stdin.end();}});
    }
    return record.promise;
  }
  desktop(request){return this.request('desktop',request.op,request.args??{},request.id);}
  async owner(op,args={},id){const reply=await this.request('host',op,args,id);if(reply.error)throw new DesktopError(reply.error,reply);return reply.result;}
  async reconcile(id){const record=this.records.get(id);if(!record)throw new Error('No original request with this ID.');return record.promise;}
  async close(){if(this.closing)return this.closing;this.closed=true;this.child.stdin.end();let forced=false;const timer=setTimeout(()=>{forced=true;this.child.kill();},2500);this.closing=this.exited.then(()=>{if(forced||this.child.exitCode!==0)throw new DesktopError({code:'close_incomplete',message:'Forced termination does not prove native cleanup.'});}).finally(()=>clearTimeout(timer));return this.closing;}
}
