import {SessionTransport,DesktopError,type Reply} from '../runtime/transport.mjs';
import {PlanBuilder} from '../runtime/plan.mjs';
import type {ActOptions,GrantStatus,Observation,ObserveArgs,ReadResult,Receipt,Ref,SessionOptions,Step} from './types.js';
export * from './types.js';
export {PlanBuilder,DesktopError};
export type {Reply};

export class DesktopClient {
  private queries=new WeakMap<Observation,ObserveArgs>();
  private building=false;
  constructor(private transport:SessionTransport){}
  async call<T=unknown>(op:string,args:unknown={},id?:string):Promise<T>{
    if(this.building)throw new Error('Build a plan locally; desktop calls occur after transaction submission.');
    const reply=await this.transport.request<T>('desktop',op,args,id);
    if(reply.error)throw new DesktopError(reply.error,reply);
    return reply.result as T;
  }
  async observe(args:ObserveArgs|Ref={}):Promise<Observation>{
    const request:ObserveArgs=structuredClone({scope:{desktop:true},projection:'summary',fields:['name','role','app','window'],budget:{max_results:32,max_output_bytes:8192},...(typeof args==='string'?{scope:{refs:[args]}}:args)});
    const result=await this.call<Observation>('observe',request);this.queries.set(result,request);return result;
  }
  outline(ref:Ref,options:ObserveArgs={}){return this.observe({projection:'outline',fields:['name','role'],...options,scope:{refs:[ref]},budget:{max_depth:4,max_results:32,max_output_bytes:8192,...options.budget}});}
  find(within:Ref,locator:Omit<NonNullable<ObserveArgs['match']>,'within'>,options:ObserveArgs={}){return this.outline(within,{...options,budget:{max_depth:12,...options.budget},match:{...locator,within}});}
  next(ob:Observation){const request=this.queries.get(ob);if(!request||!ob.coverage.continuation)throw new Error('Use an original observation with native continuation.');return this.observe({...request,continuation:ob.coverage.continuation});}
  plan(){return new PlanBuilder();}
  async act(steps:Step[]|PlanBuilder,options:ActOptions={},id?:string):Promise<Receipt>{
    const args=steps instanceof PlanBuilder?steps.build(options):{...options,steps:steps.map((s,i)=>({...s,id:s.id??`s${i+1}`}))};
    if(this.building)throw new Error('Submit only after building the local plan.');
 const reply=await this.transport.request<Receipt>('desktop','act',args,id);if(reply.error)throw new DesktopError(reply.error,reply);const receipt=reply.result!;
    if(receipt.outcome!=='completed')throw new DesktopError({code:'action_not_completed',message:'Inspect original receipt; do not replay.'},reply);
    return receipt;
  }
  async transaction(build:(tx:PlanBuilder)=>void|Promise<void>,options:ActOptions={},id?:string){
    if(this.building)throw new Error('Nested transactions are unsupported.');const plan=this.plan();this.building=true;
    try{await build(plan);}finally{this.building=false;}return this.act(plan,options,id);
  }
  read(ref:Ref,options:Record<string,unknown>={}){return this.call<ReadResult>('read',{...options,target:ref});}
  sync(cursor:string,options:Record<string,unknown>={}){return this.call('sync',{...options,cursor});}
  capture(args:Record<string,unknown>){return this.call('capture',args);}
  get(runId:string){return this.call<Receipt>('get',{run_id:runId});}
  cancel(runId:string){return this.call<Receipt>('cancel',{run_id:runId});}
  reconcile(id:string){return this.transport.reconcile<Receipt>(id);}
  set(ref:Ref,text:string,id?:string){return this.act(this.plan().set(ref,text),{},id);}
  invoke(ref:Ref,id?:string){return this.act(this.plan().invoke(ref),{},id);}
}
export class HostSession {
  readonly desktop:DesktopClient;
  private constructor(private transport:SessionTransport){this.desktop=new DesktopClient(transport);}
  static async start(options:SessionOptions){
    const args:string[]=[];
    const flags:{[key:string]:string|undefined}={'--input-mode':options.inputMode??'cooperative','--input-policy':options.inputPolicy,'--assets-dir':options.assetsDir,'--audit':options.audit,'--audit-mode':options.auditMode,'--session':options.ownerFile};
    for(const [flag,value]of Object.entries(flags))if(value!==undefined)args.push(flag,value);
    for(const name of options.writeApps??[])args.push('--write-app',name);
    for(const title of options.writeAppWindows??[])args.push('--write-app-window',title);
    return new HostSession(await SessionTransport.start(options.helper,args));
  }
  get hello(){return this.transport.hello;}
  grant(application:Ref,id?:string){return this.transport.owner('grant',{application},id);}
  declare(name:string,id?:string){return this.transport.owner('declare',{name},id);}
  declareWindow(windowTitle:string,id?:string){return this.transport.owner('declare',{window_title:windowTitle},id);}
  revoke(application:Ref,id?:string){return this.transport.owner('revoke',{application},id);}
  revokeGrant(grantId:string,id?:string){return this.transport.owner('revoke',{grant_id:grantId},id);}
  grants(){return this.transport.owner<GrantStatus>('grants');}
  beginTurn(turn:string){return this.transport.owner('begin_turn',{turn});}
  endTurn(){return this.transport.owner('end_turn');}
  close(){return this.transport.close();}
}

export function value<T>(fact:import('./types.js').Fact<T>):T {if('known' in fact)return fact.known;if(fact.status==='known')return fact.value;throw new DesktopError({code:'fact_unavailable',message:'Fact is unknown, unsupported, redacted or unrequested.'});}
export function one(ob:Observation,name:string){if(!ob.coverage.complete||ob.coverage.dirty||ob.coverage.truncated||ob.coverage.unavailable_sources?.length)throw new Error('Cannot prove uniqueness from incomplete coverage.');const matches=ob.objects.filter(o=>o.name&&value(o.name)===name);if(matches.length!==1)throw new Error(`Expected exactly one named object; found ${matches.length}.`);return matches[0];}
