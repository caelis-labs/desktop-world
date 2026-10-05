import {test} from 'node:test';
import assert from 'node:assert/strict';
import {HostSession,DesktopError,PlanBuilder} from '../dist/index.js';
import {mkdtemp,readFile,rm} from 'node:fs/promises';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {execFileSync} from 'node:child_process';
const helper=process.env.DTW_SDK_TEST_HELPER;
test('SDK fixture path is required',()=>assert.ok(helper,'run scripts/check-sdks.py'));
test('typed client uses native owner channel, retains receipts and denies forged authority',async()=>{
 const directory=await mkdtemp(join(tmpdir(),'dtw-ts-'));const ownerFile=join(directory,'owner.json');
 const host=await HostSession.start({helper,inputMode:'shared',ownerFile,writeApps:['Later']});
 try{
 const dw=host.desktop;const ob=await dw.observe({projection:'outline',fields:['name','role','app','window','states']});
 const named=name=>ob.objects.find(o=>o.name?.value===name);assert.ok(ob.coverage.complete);
 assert.equal(named('内容').states.checked.value,false);assert.equal(typeof named('内容').version,'string');
 assert.equal((await host.grants()).grants[0].state,'pending');
 await assert.rejects(dw.call('grant',{application:named('Fixture').ref}));
 await assert.rejects(dw.set(named('内容').ref,'denied','denied'));
 await host.grant(named('Fixture').ref,'grant-main');
 const plan=dw.plan().set(named('内容').ref,'SDK 中文\n🙂');
 const receipt=await dw.act(plan,{},'original');assert.equal(receipt.outcome,'completed');
 const same=await dw.act(plan,{},'original');assert.equal(same.run_id,receipt.run_id);
 await assert.rejects(dw.set(named('内容').ref,'different','original'),e=>e.code==='request_conflict');
 assert.equal((await dw.read(named('内容').ref)).text.value,'SDK 中文\n🙂');
 assert.equal((await dw.reconcile('original')).result.run_id,receipt.run_id);
 // One plan gathers focus and execution-time focused target; two distinct actions are not used.
 await dw.transaction(tx=>{tx.focus(named('内容').ref);tx.press(tx.bindFocus('input',named('Desktop World Fixture').ref),'A',['primary']);});
 const cli=JSON.parse(execFileSync(helper,['auth','add','--session',ownerFile,'--app-ref',named('Other').ref,'--id','cli-other'],{encoding:'utf8'}));assert.ok(!cli.error);
 await host.revoke(named('Fixture').ref);
 await assert.rejects(dw.set(named('内容').ref,'blocked','revoked'),e=>e instanceof DesktopError);
 await dw.set(named('Other Field').ref,'other','other');
 assert.equal((await dw.read(named('内容').ref)).text.value,'SDK 中文\n🙂');
 await host.grant(named('Fixture').ref);const pending=dw.invoke(named('提交').ref,'cancel-original').catch(e=>e);await new Promise(r=>setTimeout(r,50));await host.endTurn();const stopped=await pending;assert.ok(stopped.receipt?.run_id);assert.equal((await dw.reconcile('cancel-original')).result.run_id,stopped.receipt.run_id);assert.equal((await dw.get(receipt.run_id)).run_id,receipt.run_id);await assert.rejects(dw.observe(),e=>e.code==='turn_expired');
 await host.beginTurn('fresh');assert.equal((await host.grants()).grants.length,0);
 }finally{await host.close();await rm(directory,{recursive:true,force:true});}
});
test('builder preserves explicit false and empty text; local build does not submit',()=>{
 const p=new PlanBuilder().set('ref','').add({op:'set_checked',target:{ref:'ref'},set_checked:{checked:false}});
 assert.equal(p.build().steps[0].set_value.text,'');assert.equal(p.build().steps[1].set_checked.checked,false);
});

test('JS runner exposes current owner state and rotates metadata without content logs',async()=>{
 const {spawn}=await import('node:child_process');const {writeFile}=await import('node:fs/promises');
 const directory=await mkdtemp(join(tmpdir(),'dtw-js-owner-'));const hostFile=join(directory,'host.json');const descriptor=join(directory,'session.json');
 const runnerPath=(await import('node:url')).fileURLToPath(new URL('../../javascript/desktop.mjs',import.meta.url));await writeFile(hostFile,JSON.stringify({helper,args:['serve','--input-mode','shared']}));
 let child;let auditDir;
 try{
 for(let round=0;round<2;round++){
  child=spawn(process.execPath,[runnerPath,'serve','--host',hostFile,'--session',descriptor],{stdio:['ignore','pipe','inherit']});
  const ready=await new Promise((resolve,reject)=>{let data='';const timer=setTimeout(()=>reject(new Error('runner startup timeout')),10000);child.once('exit',()=>reject(new Error('runner exited')));child.stdout.on('data',chunk=>{data+=chunk;if(data.includes('\n')){clearTimeout(timer);resolve(JSON.parse(data.split('\n')[0]));}});});
  assert.notEqual(ready.audit_dir,auditDir);auditDir=ready.audit_dir;
  const exec=(op,code='')=>JSON.parse(execFileSync(process.execPath,[runnerPath,op,'--session',descriptor],{encoding:'utf8',input:code,timeout:10000}));
  const inventory=exec('exec',"state.ob=await dw.observe({projection:'outline'});print(dw.rows(state.ob,['kind','name','app']));");assert.ok(!inventory.error);
  const status=exec('status');assert.equal(status.authorization.turn,'session');assert.equal(status.authorization.grants.length,0);assert.equal(status.owner_file,ready.owner_file);
  const app=inventory.outputs[0].find(o=>o.kind==='application'&&o.name==='Fixture');assert.ok(app);
  execFileSync(helper,['auth','add','--session',ready.owner_file,'--app-ref',app.ref,'--id','runner-grant'],{timeout:10000});
  assert.equal(exec('status').authorization.grants[0].state,'active');
  const plan=exec('exec',"const field=dw.one(state.ob,{name:'内容'});await dw.transaction(tx=>{tx.set(field.ref,'safe');});print('done');");assert.ok(!plan.error);assert.equal(plan.metrics.calls,1);
  await assert.rejects(readFile(join(auditDir,'wire.jsonl')));await assert.rejects(readFile(join(auditDir,'script-code.jsonl')));
  const exited=new Promise(resolve=>child.once('exit',resolve));assert.equal(exec('stop').stopped,true);await exited;child=undefined;
 }
 }finally{child?.kill();await rm(directory,{recursive:true,force:true});}
});
