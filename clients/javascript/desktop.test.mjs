import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createSession } from './desktop.mjs';

const observed = { objects: [{ ref: 'r1', role: 'button', name: { known: 'Save' }, value_preview: { known: '' }, states: { enabled: { known: false }, focused: { status: 'unknown' } } }], coverage: { complete: true, truncated: false }, seat: { focused_object: { known: 'r1' } } };
const completed = { run_id: 'run1', outcome: 'completed', state: 'terminal', steps: [{ id: 's1', delivery: 'complete', verification: 'not_requested' }] };

test('one script composes observations and effects; only selected output leaves local memory', async () => {
  const requests = [];
  const session = createSession(async request => {
    requests.push(request);
    return { result: request.op === 'act' ? completed : observed };
  }, { epoch: 'epoch1' });
  const result = await session.execute(`
    const ob = await dw.observe();
    state.button = dw.one(ob, {name:'Save'}).ref;
    await dw.invoke(state.button);
    print(dw.rows(await dw.outline(state.button), ['name','states.enabled','states.focused']));
  `);
  assert.equal(result.error, undefined);
  assert.equal(requests.length, 3);
  assert.equal(result.metrics.calls, 3);
  assert.deepEqual(result.outputs[0], [{ ref: 'r1', name: 'Save', 'states.enabled': false, 'states.focused': { status: 'unknown' } }]);
  assert.equal(result.actions[0].delivery_verification['complete/not_requested'], 1);
  assert.equal(result.observations.length, 2);
  const next = await session.execute('print(state.button)');
  assert.equal(next.outputs[0], 'r1');
});

test('partial receipt is always surfaced and cannot be caught to continue input', async () => {
  let calls = 0;
  const session = createSession(async () => {
    calls++;
    return { result: { ...completed, outcome: 'partial', steps: [{ id: 's1', delivery: 'unknown', verification: 'unknown', fault: { code: 'lost_focus' } }] } };
  });
  const result = await session.execute(`
    try { await dw.press('r1','Enter'); } catch {}
    try { await dw.type('r1','must not be sent'); } catch {}
  `);
  assert.equal(calls, 1);
  assert.equal(result.error.code, 'action_not_completed');
  assert.equal(result.actions[0].problems[0].delivery, 'unknown');
});

test('top-level faults stop the chain and no automatic retries occur', async () => {
  let calls = 0;
  const session = createSession(async () => { calls++; return { error: { code: 'expired_reference', message: 'Observe again' } }; });
  const result = await session.execute(`await dw.observe(); await dw.observe();`);
  assert.equal(calls, 1);
  assert.equal(result.error.code, 'expired_reference');
});

test('bounded output and incomplete coverage are never silent successful discovery', async () => {
  const session = createSession(async () => ({ result: { ...observed, coverage: { complete: false, truncated: true, continuation: 'page2' } } }), { printBytes: 100 });
  const incomplete = await session.execute(`dw.one(await dw.observe(), {name:'Save'});`);
  assert.match(incomplete.error.message, /incomplete/);
  assert.equal(incomplete.observations[0].continuation, 'page2');
  const large = await session.execute(`print('x'.repeat(101));`);
  assert.equal(large.error.code, 'print_budget_exceeded');
  assert.deepEqual(large.outputs, []);
});

test('concurrent calls serialize and calls after an error are not dispatched', async () => {
  const order = [];
  let active = 0;
  const session = createSession(async request => {
    assert.equal(active++, 0);
    order.push(request.id);
    await new Promise(resolve => setTimeout(resolve, 5));
    active--;
    return { result: completed };
  });
  const result = await session.execute(`await Promise.all([dw.invoke('r1'), dw.invoke('r1')]);`);
  assert.equal(result.error, undefined);
  assert.equal(order.length, 2);
  assert.notEqual(order[0], order[1]);
});

test('script call budget and deadline do not continue to later effects', async () => {
  let calls = 0;
  const budgeted = createSession(async () => { calls++; return { result: observed }; }, { maxCalls: 2 });
  const budget = await budgeted.execute(`for (let i=0;i<5;i++) await dw.observe();`);
  assert.equal(budget.error.code, 'call_budget_exceeded');
  assert.equal(calls, 2);
  const timed = createSession(async () => { await new Promise(resolve => setTimeout(resolve, 30)); return { result: observed }; }, { timeoutMs: 5 });
  const timeout = await timed.execute(`await dw.observe(); await dw.invoke('r1');`);
  assert.equal(timeout.error.code, 'script_timeout');
  assert.equal(timeout.actions, undefined);
  timed.close();
  assert.equal((await timed.execute('print(1)')).error.code, 'session_closed');
});

test('read polling handles transitioning dialogs without replaying effects', async () => {
  let reads = 0;
  const session = createSession(async request => {
    assert.equal(request.op, 'observe');
    return { result: ++reads === 1 ? { ...observed, objects: [] } : observed };
  });
  const result = await session.execute(`const ob=await dw.waitFor({}, ob=>ob.objects.length===1, {interval_ms:50}); print(dw.rows(ob));`);
  assert.equal(result.error, undefined);
  assert.equal(reads, 2);
  assert.equal(result.outputs[0][0].name, 'Save');
});

test('disconnect stops later effects and retains in-flight outcome', async () => {
  const controller = new AbortController();
  let calls = 0;
  const session = createSession(async () => { calls++; controller.abort(); return { result: completed }; });
  const result = await session.execute(`await dw.invoke('r1'); await dw.invoke('r1');`, { signal: controller.signal });
  assert.equal(calls, 1);
  assert.equal(result.error.code, 'caller_disconnected');
  assert.equal(result.actions[0].outcome, 'completed');
});


test('presentation pages preserve every ref without native re-observation or silent clipping', async () => {
  const objects = Array.from({length:100}, (_,i)=>({ref:`r${i}`, role:'window', name:{known:'Window '+i}, value_preview:{known:'x'.repeat(150)}}));
  let calls = 0;
  const session = createSession(async()=>{calls++;return {result:{objects,coverage:{complete:true}}}});
  const first = await session.execute('state.ob=await dw.observe(); print(dw.list(state.ob));');
  assert.equal(first.error, undefined);
  let refs = first.outputs[0].rows.map(o=>o.ref);
  let offset = first.outputs[0].next_offset;
  assert.ok(offset > 0 && offset < 100);
  while (offset !== null) {
    const result = await session.execute(`print(dw.list(state.ob, undefined, {offset:${offset}}));`);
    assert.equal(result.error, undefined);
    refs.push(...result.outputs[0].rows.map(o=>o.ref));
    offset = result.outputs[0].next_offset;
  }
  assert.deepEqual(refs, objects.map(o=>o.ref));
  assert.equal(calls,1);
  const oversize = await session.execute("print(dw.list({objects:[{ref:'x',name:{known:'z'.repeat(5000)}}]}));");
  assert.equal(oversize.error.code,'row_budget_exceeded');
});

test('value accepts primitive metadata but unknown and redacted facts still fail closed', async () => {
  const session = createSession(async()=>({result:observed}));
  const result = await session.execute("const ob=await dw.observe();print([dw.value(ob.objects[0].role),dw.value({known:false}),dw.value({known:''}),dw.value(0)]);");
  assert.deepEqual(result.outputs[0],['button',false,'',0]);
  for (const status of ['unknown','redacted','unsupported']) {
    assert.equal((await session.execute(`print(dw.value({status:'${status}'}));`)).error.code,'fact_not_known');
  }
});


test('summary shorthand scopes the observed Ref without guessing an app field', async () => {
  const session = createSession(async request => {
    assert.deepEqual(request.args.scope,{refs:['app-ref']});
    assert.equal(request.args.projection,'summary');
    return {result:observed};
  });
  assert.equal((await session.execute("print(dw.list(await dw.observe('app-ref')));")).error,undefined);
});


test('focused convenience never retargets keyboard into another authorized window or app', async () => {
  for (const kind of ['application','window']) {
    const session = createSession(async()=>({result:{objects:[{ref:'scope',kind}],seat:{focused_object:{known:'other-field'},foreground_application:{known:'other-app'},foreground_window:{known:'other-window'}}}}));
    const result=await session.execute("await dw.press(await dw.focused('scope'),'A',['primary']);");
    assert.equal(result.error.code,'focus_outside_scope');
    assert.equal(result.metrics.calls,1);
    assert.equal(result.actions,undefined);
  }
  const session=createSession(async()=>({result:{objects:[{ref:'scope',kind:'window'}],seat:{focused_object:{known:'field'},foreground_window:{known:'scope'}}}}));
  assert.deepEqual((await session.execute("print(await dw.focused('scope'));")).outputs,['field']);
});


test('next retains native query across scripts and never guesses continuation arguments', async () => {
  const requests=[];
  const session=createSession(async req=>{
    requests.push(req.args);
    return {result:{...observed, coverage:{complete:requests.length>1,truncated:requests.length===1,...(requests.length===1?{continuation:'page2'}:{})}}};
  });
  await session.execute("state.ob=await dw.outline('win',{budget:{max_depth:9},fields:['name']});");
  const next=await session.execute("state.ob=await dw.next(state.ob);print(dw.list(state.ob));");
  assert.equal(next.error,undefined);
  assert.deepEqual(requests[1],{...requests[0],continuation:'page2'});
  assert.ok((await session.execute('await dw.next(state.ob);')).error);
  assert.equal(requests.length,2);
});

test('fragmented subtree recipe preserves Ref order, native blocks, redaction and clipping uncertainty', async () => {
  const {readFile}=await import('node:fs/promises');
  const recipe=await readFile(new URL('../../scripts/read-fragmented-text.js',import.meta.url),'utf8');
  const sample={sample_start:'2026-09-30T00:00:00Z',sample_end:'2026-09-30T00:00:00.1Z',visited_nodes:9};
  const pages=[
    {objects:[{ref:'doc',role:'document'},{ref:'p',parent:'doc',role:'container'},{ref:'q',parent:'doc',role:'container'},{ref:'a',parent:'p',role:'text',value_preview:{known:'Repeat'}}],coverage:{...sample,complete:false,continuation:'next'}},
    {objects:[{ref:'space',parent:'p',role:'text',value_preview:{known:' '}},{ref:'b',parent:'p',role:'text',value_preview:{known:'Repeat'}},{ref:'unicode',parent:'q',role:'text',value_preview:{known:'中文 🌍'}},{ref:'long',parent:'q',role:'text',value_preview:{known:'x'.repeat(383)}},{ref:'secret',parent:'doc',role:'text_field',value_preview:{status:'redacted'}}],coverage:{...sample,complete:true}}
  ];
  const session=createSession(async request=>{assert.equal(request.op,'observe');return {result:pages.shift()}});
  await session.execute("state.document='doc'");
  const result=await session.execute(recipe);
  assert.equal(result.error,undefined);
  assert.equal(result.metrics.calls,2);
  const output=result.outputs[0];
  assert.deepEqual(output.blocks.map(b=>b.fragments.map(f=>f.ref)),[['a','space','b'],['unicode','long']]);
  assert.equal(output.blocks[0].fragments.map(f=>f.text).join(''),'Repeat Repeat');
  assert.equal(output.blocks[1].fragments[0].text,'中文 🌍');
  assert.equal(output.blocks[1].fragments[1].possibly_clipped,true);
  assert.deepEqual(output.redacted,[{ref:'secret',status:'redacted'}]);
  assert.equal(output.incomplete,true);
  assert.equal(result.observations.length,2);
});

test('fragmented subtree recipe reports native incomplete sample without expanding scope', async () => {
  const {readFile}=await import('node:fs/promises');
  const recipe=await readFile(new URL('../../scripts/read-fragmented-text.js',import.meta.url),'utf8');
  const session=createSession(async request=>{assert.deepEqual(request.args.scope,{refs:['doc']});return {result:{objects:[{ref:'doc',role:'document'},{ref:'unknown',role:'text',parent:'doc',value_preview:{status:'unknown'}}],coverage:{complete:false,unavailable_sources:['ax_timeout']}}}});
  await session.execute("state.document='doc'");
  const result=await session.execute(recipe);
  assert.equal(result.metrics.calls,1);
  assert.equal(result.outputs[0].incomplete,true);
  assert.deepEqual(result.outputs[0].blocks[0].fragments,[{ref:'unknown',status:'unknown'}]);
  assert.deepEqual(result.observations[0].unavailable_sources,['ax_timeout']);
});
