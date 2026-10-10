import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { chmod, cp, mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Supervisor } from './server.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const PNG = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScL/nwAAAABJRU5ErkJggg==';
const helper = `#!/usr/bin/env node
import { createInterface } from 'node:readline';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
if (process.argv[2] === 'cursor-overlay') { process.stdin.resume(); }
else if (process.argv[2] === 'session') {
  const option = name => { const i=process.argv.indexOf(name); return i<0?undefined:process.argv[i+1]; };
  const assets = option('--assets-dir'); mkdirSync(assets, { recursive: true });
  const owner = option('--session'); writeFileSync(owner, '{}');
  process.stdout.write(JSON.stringify({type:'hello',protocol:'desktop-world/session-v0.1',environment:{epoch:'fixture-epoch'},input_mode:'shared',input_policy:option('--input-policy')??'shared_input',features:['dynamic_app_grants']})+'\\n');
  createInterface({input:process.stdin}).on('line', line => {
    const r=JSON.parse(line); let result, error;
    if (r.channel === 'host') result = r.op === 'grants' ? {turn:'session',grants:[]} : {acknowledged:true};
    else if (r.op === 'observe') result = {objects:[{ref:'app-1',kind:'application',name:{known:'Fixture'}}],coverage:{complete:true,truncated:false}};
    else if (r.op === 'act') {
      if (process.env.DTW_MCP_TEST_DELAY_ACT === '1') {
        setTimeout(() => process.stdout.write(JSON.stringify({id:r.id,protocol:'desktop-world/session-v0.1',world:'fixture-epoch',result:{run_id:'run-delayed',outcome:'completed',steps:[{id:'s1',delivery:'complete',verification:'not_requested'}]}})+'\\n'), 1800);
        return;
      }
      result={run_id:'run-1',outcome:'stopped',state:'terminal',steps:[{id:'s1',delivery:'none',verification:'not_requested'}]}; error={code:'permission_denied',message:'No native APP grant'};
    }
    else if (r.op === 'capture') { const path=join(assets,'capture-fixture.png');writeFileSync(path,Buffer.from('${PNG}','base64'));result={Capture:{Tiles:[{PixelWidth:1,PixelHeight:1,ImageToDesktop:{A:2,D:2,TX:10,TY:20}}]},Files:[{Path:path,Bytes:68}]}; }
    else result = {};
    process.stdout.write(JSON.stringify({id:r.id,protocol:'desktop-world/session-v0.1',world:'fixture-epoch',result,error})+'\\n');
  });
} else process.exit(2);
`;

async function fixture(t, { delayedAct = false } = {}) {
  const root = await mkdtemp(join(tmpdir(), 'DTW 插件 test '));
  const plugin = join(root, 'plugin with 空格');
  await cp(join(here, 'dist'), plugin, { recursive: true });
  await mkdir(join(plugin, 'bin'));
  await writeFile(join(plugin, 'bin', 'dtw'), helper);
  await chmod(join(plugin, 'bin', 'dtw'), 0o755);
  const child = spawn(process.execPath, [join(plugin, 'mcp', 'server.mjs'), '--data-dir', join(root, 'data 资料')], { stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, DTW_MCP_TEST_DELAY_ACT: delayedAct ? '1' : '0' } });
  let sequence = 0;
  const pending = new Map();
  const diagnostics = [];
  child.stderr.setEncoding('utf8').on('data', data => diagnostics.push(data));
  createInterface({ input: child.stdout }).on('line', line => {
    const reply = JSON.parse(line);
    const waiter = pending.get(reply.id);
    if (waiter) { pending.delete(reply.id); waiter(reply); }
  });
  const send = (method, params, id = ++sequence) => {
    const result = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`MCP ${method} timed out: ${diagnostics.join('')}`)), 5000);
      pending.set(id, reply => { clearTimeout(timer); resolve(reply); });
    });
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
    return { id, result };
  };
  const notify = (method, params) => child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method, params }) + '\n');
  t.after(async () => { child.stdin.end(); await new Promise(resolve => { if (child.exitCode !== null) return resolve(); const timer = setTimeout(() => { child.kill('SIGKILL'); resolve(); }, 3000); child.once('exit', () => { clearTimeout(timer); resolve(); }); }); await rm(root, { recursive: true, force: true }); });
  const initialized = await send('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'fixture', version: '1' } }).result;
  assert.equal(initialized.result.serverInfo.name, 'desktop-world');
  notify('notifications/initialized', {});
  return { send, notify, child };
}

test('MCP tools preserve state, IDs, native denial and PNG ImageContent', async t => {
  const mcp = await fixture(t);
  const listed = await mcp.send('tools/list', {}).result;
  assert.deepEqual(listed.result.tools.map(tool => tool.name), ['desktop_exec', 'desktop_status']);
  let status;
  for (let i = 0; i < 30; i++) {
    status = await mcp.send('tools/call', { name: 'desktop_status', arguments: {} }).result;
    if (status.result.structuredContent.worker_ready) break;
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.equal(status.result.structuredContent.epoch, 'fixture-epoch');
  assert.equal(status.result.structuredContent.input_policy, 'shared_input');
  assert.equal(status.result.structuredContent.physical_pointer_input, 'may_move_real_cursor');
  assert.equal(status.result.structuredContent.worker_ready, true, JSON.stringify(status.result.structuredContent));
  const first = await mcp.send('tools/call', { name: 'desktop_exec', arguments: { execution_id: 'observe-1', code: "state.saved = await dw.observe(); print(dw.list(state.saved, ['name']));" } }).result;
  assert.equal(first.result.structuredContent.metrics.calls, 1);
  assert.equal(first.result.structuredContent.outputs[0].rows[0].name, 'Fixture');
  const firstText = JSON.parse(first.result.content[0].text);
  assert.equal(firstText.outputs[0].rows[0].name, 'Fixture');
  assert.deepEqual(firstText.native_request_ids, ['js-1']);
  assert.equal(firstText.observations[0].complete, true);
  assert.equal('metrics' in firstText, false);
  assert.ok(Buffer.byteLength(first.result.content[0].text) < Buffer.byteLength(JSON.stringify(first.result.structuredContent)));
  const next = await mcp.send('tools/call', { name: 'desktop_exec', arguments: { execution_id: 'state-2', code: 'print(state.saved.objects[0].ref);' } }).result;
  assert.equal(next.result.structuredContent.outputs[0], 'app-1');
  const same = await mcp.send('tools/call', { name: 'desktop_exec', arguments: { execution_id: 'observe-1', code: "state.saved = await dw.observe(); print(dw.list(state.saved, ['name']));" } }).result;
  assert.deepEqual(same.result.structuredContent, first.result.structuredContent);
  const conflict = await mcp.send('tools/call', { name: 'desktop_exec', arguments: { execution_id: 'observe-1', code: 'print(1)' } }).result;
  assert.equal(conflict.result.structuredContent.error.code, 'execution_conflict');
  const denied = await mcp.send('tools/call', { name: 'desktop_exec', arguments: { execution_id: 'deny-3', code: "try { await dw.set('app-1','x'); } catch {} await dw.observe();" } }).result;
  assert.equal(denied.result.isError, true);
  assert.equal(denied.result.structuredContent.native_request_ids.length, 1);
  assert.equal(denied.result.structuredContent.actions[0].run_id, 'run-1');
  const deniedText = JSON.parse(denied.result.content[0].text);
  assert.equal(deniedText.error.code, 'permission_denied');
  assert.deepEqual(deniedText.native_request_ids, denied.result.structuredContent.native_request_ids);
  assert.equal(deniedText.receipt_detail, 'desktop_status with this execution_id');
  const deniedStatus = await mcp.send('tools/call', { name: 'desktop_status', arguments: { execution_id: 'deny-3' } }).result;
  assert.equal(deniedStatus.result.structuredContent.execution.result.native_receipts[deniedText.native_request_ids[0]].error.code, 'permission_denied');
  const capture = await mcp.send('tools/call', { name: 'desktop_exec', arguments: { execution_id: 'image-4', code: "await dw.capture({kind:'visible_region',max_pixel_width:1,max_pixel_height:1});" } }).result;
  assert.equal(capture.result.content[1].type, 'image');
  assert.equal(capture.result.content[1].mimeType, 'image/png');
  assert.equal(capture.result.structuredContent.captures[0].Tiles[0].ImageToDesktop.TX, 10);
  const syntax = await mcp.send('tools/call', { name: 'desktop_exec', arguments: { execution_id: 'syntax-5', code: 'const = ;' } }).result;
  assert.equal(syntax.result.structuredContent.error.line, 1);
});

test('synchronous loop after await leaves status responsive and cancellation fences native input', async t => {
  const mcp = await fixture(t);
  const running = mcp.send('tools/call', { name: 'desktop_exec', arguments: { execution_id: 'hang-1', code: 'await dw.observe(); while (true) {}' } });
  void running.result.catch(() => {}); // MCP may drop the cancelled request response.
  let query;
  for (let i = 0; i < 30; i++) {
    query = await mcp.send('tools/call', { name: 'desktop_status', arguments: { execution_id: 'hang-1' } }).result;
    if (query.result.structuredContent.execution?.state === 'running') break;
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.equal(query.result.structuredContent.execution.state, 'running');
  mcp.notify('notifications/cancelled', { requestId: running.id, reason: 'fixture cancellation' });
  let final;
  for (let i = 0; i < 50; i++) {
    final = await mcp.send('tools/call', { name: 'desktop_status', arguments: { execution_id: 'hang-1' } }).result;
    if (final.result.structuredContent.execution?.result) break;
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.equal(final.result.structuredContent.fenced, true, JSON.stringify(final.result.structuredContent));
  assert.equal(final.result.structuredContent.execution.result.error.code, 'cancelled');
});

test('late native act receipt stays queryable under its original ID after EndTurn', async t => {
  const mcp = await fixture(t, { delayedAct: true });
  const code = "await dw.call('act',{steps:[{id:'s1',op:'pointer.click'}]});";
  const running = mcp.send('tools/call', { name: 'desktop_exec', arguments: { execution_id: 'late-1', code } });
  void running.result.catch(() => {});
  let status;
  for (let i = 0; i < 40; i++) {
    status = (await mcp.send('tools/call', { name: 'desktop_status', arguments: { execution_id: 'late-1' } }).result).result.structuredContent;
    if (status.execution?.native_request_ids.length) break;
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.deepEqual(status.execution.native_request_ids, ['js-1']);
  mcp.notify('notifications/cancelled', { requestId: running.id, reason: 'late receipt regression' });
  for (let i = 0; i < 40; i++) {
    status = (await mcp.send('tools/call', { name: 'desktop_status', arguments: { execution_id: 'late-1' } }).result).result.structuredContent;
    if (status.execution?.result) break;
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.equal(status.execution.state, 'failed');
  assert.equal(status.execution.result.error.code, 'cancelled');
  assert.equal(status.cleanup, 'end_turn_acknowledged');
  assert.deepEqual(status.execution.result.native_receipts['js-1'], { pending: true, outcome: 'unknown' });
  for (let i = 0; i < 150; i++) {
    status = (await mcp.send('tools/call', { name: 'desktop_status', arguments: { execution_id: 'late-1' } }).result).result.structuredContent;
    if (status.execution.result.native_receipts['js-1']?.result) break;
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.equal(status.execution.state, 'failed');
  assert.equal(status.execution.result.error.code, 'cancelled');
  assert.deepEqual(status.execution.native_request_ids, ['js-1']);
  assert.equal(status.execution.result.native_receipts['js-1'].result.run_id, 'run-delayed');
  assert.equal(status.execution.result.native_receipts['js-1'].result.outcome, 'completed');
  assert.equal(status.execution.result.native_receipts['js-1'].result.steps[0].delivery, 'complete');
  const retry = (await mcp.send('tools/call', { name: 'desktop_exec', arguments: { execution_id: 'late-1', code } }).result).result.structuredContent;
  assert.equal(retry.error.code, 'cancelled');
  assert.deepEqual(retry.native_request_ids, ['js-1']);
});

test('already-aborted exec returns cancellation without worker dispatch', async () => {
  const s = Object.create(Supervisor.prototype);
  s.ready = Promise.resolve();
  s.records = new Map();
  s.active = null;
  s.fenced = false;
  s.stateLost = false;
  s.worker = { send() { throw Error('worker dispatch on aborted signal'); }, kill() {} };
  s.native = { closed: false, owner: async op => {
    assert.equal(op, 'end_turn');
    return { acknowledged: true };
  } };
  s.audit = async () => {};
  s.assetsDir = process.cwd();
  const controller = new AbortController();
  controller.abort();
  const result = await s.exec({ execution_id: 'pre-cancel-1', code: 'print(1)' }, controller.signal);
  assert.equal(result.structuredContent.error.code, 'cancelled');
  assert.equal(result.structuredContent.cleanup, 'end_turn_acknowledged');
  assert.deepEqual(result.structuredContent.native_request_ids, []);
  assert.equal(s.fenced, true);
});
