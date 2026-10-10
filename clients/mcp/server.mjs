#!/usr/bin/env node
// Agent Plugins stdio server. stdout belongs exclusively to the MCP SDK.
import { McpServer } from '@modelcontextprotocol/server';
import { StdioServerTransport } from '@modelcontextprotocol/server/stdio';
import { z } from 'zod';
import { fork, spawn } from 'node:child_process';
import { realpathSync } from 'node:fs';
import { randomUUID } from 'node:crypto';
import { mkdir, readFile, realpath, stat, writeFile } from 'node:fs/promises';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { SessionTransport } from '../typescript/runtime/transport.mjs';

const PLUGIN_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const WORKER = join(PLUGIN_ROOT, 'mcp', 'worker.mjs');
const HELPER = join(PLUGIN_ROOT, 'bin', process.platform === 'win32' ? 'dtw.exe' : 'dtw');
const MAX_RECORDS = 128;
const MAX_IMAGES = 2;
const MAX_IMAGE_BYTES = 4 * 1024 * 1024;
const SCRIPT_TIMEOUT_MS = 60000;
const error = (code, message) => ({ code, message });
const toolResult = (value, images = [], text = JSON.stringify(value)) => ({
  isError: Boolean(value.error),
  structuredContent: value,
  content: [{ type: 'text', text }, ...images],
});
// Keep the machine-readable result and original receipts intact. Most agents
// only need printed values, coverage, and delivery state on the first pass.
// desktop_status(execution_id) exposes the full original result when needed.
function execText(value) {
  const brief = { execution_id: value.execution_id, state: value.state };
  if (value.outputs?.length) brief.outputs = value.outputs;
  if (value.observations?.length) brief.observations = value.observations.map(({ id, complete, truncated, dirty, continuation, unavailable_sources }) =>
    ({ id, complete, truncated, dirty, ...(continuation ? { more: true } : {}), ...(unavailable_sources?.length ? { unavailable_sources } : {}) }));
  if (value.actions?.length) brief.actions = value.actions.map(({ run_id, outcome, seat_health, delivery_verification, problems }) =>
    ({ run_id, outcome, seat_health, delivery_verification, ...(problems?.length ? { problems } : {}) }));
  if (value.native_request_ids?.length) brief.native_request_ids = value.native_request_ids;
  if (value.captures?.length) brief.captures = value.captures;
  if (value.error) brief.error = value.error;
  if (value.cleanup) brief.cleanup = value.cleanup;
  if (Object.keys(value.native_receipts ?? {}).length) brief.receipt_detail = 'desktop_status with this execution_id';
  return JSON.stringify(brief);
}
const bounded = (promise, ms, label) => Promise.race([
  promise,
  new Promise((_, reject) => { const timer = setTimeout(() => reject(new Error(`${label} exceeded ${ms} ms`)), ms); timer.unref(); }),
]);

function options(argv) {
  const out = { writeApps: [] };
  for (let i = 0; i < argv.length; i++) {
    const key = argv[i], value = argv[++i];
    if (!value || !['--data-dir', '--write-app', '--input-mode', '--input-policy'].includes(key)) throw new Error(`Unknown or incomplete server option: ${key}`);
    if (key === '--write-app') out.writeApps.push(value);
    else out[key.slice(2).replace(/-([a-z])/g, (_, ch) => ch.toUpperCase())] = value;
  }
  out.dataDir ??= process.env.PLUGIN_DATA;
  if (!out.dataDir || !isAbsolute(out.dataDir)) throw new Error('Supply an absolute --data-dir or client-managed PLUGIN_DATA.');
  if (out.inputMode && !['shared', 'cooperative'].includes(out.inputMode)) throw new Error('Invalid --input-mode.');
  if (out.inputPolicy && !['shared_input', 'no_shared_input'].includes(out.inputPolicy)) throw new Error('Invalid --input-policy.');
  return out;
}

class Supervisor {
  constructor(config) {
    this.config = config;
    this.records = new Map();
    this.active = null;
    this.stateLost = false;
    this.fenced = false;
    this.native = null;
    this.worker = null;
    this.sessionDir = join(config.dataDir, 'sessions', randomUUID());
    this.assetsDir = join(this.sessionDir, 'assets');
    this.ownerFile = join(this.sessionDir, 'owner.json');
    this.auditPath = join(this.sessionDir, 'mcp-audit.jsonl');
    this.ready = this.start().catch(e => { this.startError = error('startup_failed', String(e.message).slice(0, 1000)); });
  }

  async audit(row) {
    // A private per-connection summary; scripts and image bytes are not logged.
    try { await writeFile(this.auditPath, JSON.stringify({ at: new Date().toISOString(), ...row }) + '\n', { flag: 'a', mode: 0o600 }); }
    catch (e) { console.error(`Desktop World audit write failed: ${e.message}`); }
  }

  async start() {
    if (!['darwin', 'win32'].includes(process.platform) || (process.platform === 'darwin' ? process.arch !== 'arm64' : process.arch !== 'x64')) throw new Error(`Unsupported platform ${process.platform}/${process.arch}`);
    await mkdir(this.assetsDir, { recursive: true, mode: 0o700 });
    const args = ['--session', this.ownerFile, '--assets-dir', this.assetsDir];
    for (const app of this.config.writeApps) args.push('--write-app', app);
    if (this.config.inputMode) args.push('--input-mode', this.config.inputMode);
    if (this.config.inputPolicy) args.push('--input-policy', this.config.inputPolicy);
    this.native = await SessionTransport.start(HELPER, args);
    this.epoch = this.native.hello.environment?.epoch;
    this.protocol = this.native.hello.protocol;
    this.inputMode = this.native.hello.input_mode;
    this.inputPolicy = this.native.hello.input_policy;
    console.error(`Desktop World owner descriptor: ${this.ownerFile}`);
    await this.audit({ event: 'start', epoch: this.epoch, owner_file: this.ownerFile });
    this.overlay = spawn(HELPER, ['cursor-overlay'], { stdio: ['pipe', 'ignore', 'inherit'], windowsHide: true });
    this.overlayReady = true;
    this.overlay.on('error', e => { this.overlayReady = false; console.error(`Desktop World cursor overlay unavailable: ${e.message}`); });
    this.overlay.on('exit', () => { this.overlayReady = false; });
    this.spawnWorker();
  }

  spawnWorker() {
    const worker = this.worker = fork(WORKER, [], { execPath: process.execPath, execArgv: ['--max-old-space-size=128'], stdio: ['ignore', 'ignore', 'inherit', 'ipc'], windowsHide: true });
    worker.on('message', message => this.onWorkerMessage(message));
    worker.once('exit', (code, signal) => {
      if (this.worker !== worker) return;
      this.worker = null;
      this.stateLost = true;
      if (!this.fenced) this.fence('worker_exited', `Worker exited (${code ?? signal}); persistent state is lost. Native receipts remain in the original session.`);
    });
    worker.send({ type: 'init', epoch: this.epoch });
  }

  async onWorkerMessage(message) {
    if (message.type === 'ready') { this.workerReady = true; return; }
    if (message.type === 'result') {
      const record = this.active;
      if (record?.id === message.id && !record.done) await this.finish(record, message.result);
      return;
    }
    if (message.type !== 'desktop') return;
    const record = this.active;
    if (!record || record.done || this.fenced) {
      this.worker?.send({ type: 'desktop_result', id: message.id, error: error('session_fenced', 'Native input is stopped; inspect original receipts.') });
      return;
    }
    const request = message.request;
    if (!request || typeof request.id !== 'string') return;
    record.nativeIds.push(request.id);
    record.nativeOps[request.id] = request.op;
    try {
      const response = await this.native.desktop(request);
      record.receipts[request.id] = response;
      if (request.op === 'capture' && response.result && !response.error) record.captures.push(response.result);
      if (request.op === 'act' && response.result?.steps && this.overlayReady) {
        const delivered = response.result.steps.filter(step => step.delivery === 'complete' && step.pointer?.frame === 'desktop');
        const point = delivered.at(-1)?.pointer;
        if (point) this.overlay.stdin.write(JSON.stringify({ op: 'show', point }) + '\n');
      }
      this.worker?.send({ type: 'desktop_result', id: message.id, response });
    } catch (e) {
      record.receipts[request.id] = { error: error(e.code ?? 'native_unknown', String(e.message).slice(0, 1000)) };
      this.worker?.send({ type: 'desktop_result', id: message.id, error: record.receipts[request.id].error });
    }
  }

  async imagesFor(record) {
    const images = [];
    let total = 0;
    const root = await realpath(this.assetsDir);
    for (const capture of record.captures) {
      const files = capture.Files ?? capture.files ?? [];
      for (const file of files) {
        if (images.length >= MAX_IMAGES) throw Object.assign(new Error('At most two capture images per exec.'), { code: 'image_budget_exceeded' });
        const path = file.Path ?? file.path;
        if (!path || !isAbsolute(path)) throw Object.assign(new Error('Invalid native capture path.'), { code: 'image_path_invalid' });
        const resolved = await realpath(path);
        const rel = relative(root, resolved);
        if (!rel || rel === '..' || rel.startsWith(`..${sep}`) || isAbsolute(rel)) throw Object.assign(new Error('Capture path escapes this session assets directory.'), { code: 'image_path_invalid' });
        const info = await stat(resolved);
        if (!info.isFile() || info.size > MAX_IMAGE_BYTES - total) throw Object.assign(new Error('Capture image exceeds 4 MiB exec budget.'), { code: 'image_budget_exceeded' });
        const bytes = await readFile(resolved);
        if (!bytes.subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10]))) throw Object.assign(new Error('Native capture is not PNG.'), { code: 'image_invalid' });
        total += bytes.length;
        images.push({ type: 'image', mimeType: 'image/png', data: bytes.toString('base64') });
      }
    }
    return images;
  }

  nativeReceipts(record) {
    return Object.fromEntries(record.nativeIds
      .filter(id => ['act', 'get', 'cancel'].includes(record.nativeOps[id]) || record.receipts[id]?.error)
      .map(id => [id, record.receipts[id] ?? { pending: true, outcome: 'unknown' }]));
  }

  async finish(record, result) {
    if (record.done) return;
    clearTimeout(record.timer);
    record.abortSignal?.removeEventListener('abort', record.abortHandler);
    record.done = true;
    record.state = result.error ? 'failed' : 'completed';
    const value = { execution_id: record.id, state: record.state, ...result, native_request_ids: record.nativeIds, captures: record.captures.map(c => c.Capture ?? c.capture), native_receipts: this.nativeReceipts(record) };
    let images = [];
    try { images = await this.imagesFor(record); }
    catch (e) { value.error ??= error(e.code ?? 'image_failed', String(e.message).slice(0, 1000)); value.state = record.state = 'failed'; }
    record.result = toolResult(value, images, execText(value));
    await this.audit({ event: 'execution_end', execution_id: record.id, state: record.state, native_request_ids: record.nativeIds, error: value.error?.code });
    if (this.active === record) this.active = null;
    record.resolve(record.result);
  }

  async fence(code, message) {
    if (this.fenced) return this.fencePromise;
    this.fenced = true; // Before any awaited cleanup, block further desktop requests.
    this.stateLost = true;
    if (this.overlay) { this.overlay.stdin.end(); this.overlay.kill('SIGTERM'); this.overlay = null; this.overlayReady = false; }
    this.worker?.kill('SIGKILL');
    this.worker = null;
    const record = this.active;
    this.fencePromise = (async () => {
      let cleanup = 'not_started';
      try { if (this.native && !this.native.closed) { await bounded(this.native.owner('end_turn', { turn: 'session' }, `mcp-end-${randomUUID()}`), 13000, 'EndTurn'); cleanup = 'end_turn_acknowledged'; } }
      catch (e) { cleanup = `close_incomplete: ${String(e.message).slice(0, 300)}`; }
      this.cleanup = cleanup;
      await this.audit({ event: 'fenced', reason: code, cleanup, native_request_ids: record?.nativeIds ?? [] });
      if (record && !record.done) await this.finish(record, { error: error(code, message), cleanup });
    })();
    return this.fencePromise;
  }

  async exec({ execution_id: id, code }, signal) {
    await this.ready;
    if (this.startError) return toolResult({ execution_id: id, error: this.startError });
    const previous = this.records.get(id);
    if (previous) {
      if (previous.code !== code) return toolResult({ execution_id: id, error: error('execution_conflict', 'This execution_id belongs to different code.') });
      return previous.promise;
    }
    if (this.records.size >= MAX_RECORDS) return toolResult({ execution_id: id, error: error('execution_capacity', 'Execution record capacity reached; open a new explicitly authorized connection after reconciliation.') });
    if (this.active) return toolResult({ execution_id: id, error: error('script_busy', 'One script at a time; inspect desktop_status.') });
    if (this.fenced || this.stateLost || !this.worker) return toolResult({ execution_id: id, error: error('state_lost', 'Worker state was lost or the native session was fenced; no automatic restart or replay.') });
    const record = { id, code, state: 'running', done: false, nativeIds: [], nativeOps: {}, receipts: {}, captures: [] };
    record.promise = new Promise(resolve => { record.resolve = resolve; });
    this.records.set(id, record);
    this.active = record;
    record.timer = setTimeout(() => this.fence('script_timeout', 'Script deadline reached; native input stopped and original receipts retained.'), SCRIPT_TIMEOUT_MS);
    if (signal) {
      record.abortSignal = signal;
      record.abortHandler = () => { void this.fence('cancelled', 'MCP request cancelled; native input stopped.'); };
      signal.addEventListener('abort', record.abortHandler, { once: true });
      if (signal.aborted) {
        record.abortHandler();
        return record.promise;
      }
    }
    this.worker.send({ type: 'exec', id, code });
    return record.promise;
  }

  async status(id) {
    await this.ready;
    const record = id ? this.records.get(id) : [...this.records.values()].at(-1);
    let grants;
    if (this.native && !this.native.closed) {
      try { grants = await bounded(this.native.owner('grants', {}, `mcp-grants-${randomUUID()}`), 1000, 'grant status'); }
      catch (e) { grants = { unavailable: String(e.message).slice(0, 200) }; }
    }
    const result = record?.result?.structuredContent;
    return toolResult({ version: '0.1.0', protocol: this.protocol, epoch: this.epoch, platform: `${process.platform}/${process.arch}`, native_ready: Boolean(this.native && !this.native.closed), worker_ready: Boolean(this.worker && this.workerReady), cursor_overlay: this.overlayReady ? 'ready' : 'unavailable', fenced: this.fenced, state_lost: this.stateLost, cleanup: this.cleanup, input_mode: this.inputMode, input_policy: this.inputPolicy, physical_pointer_input: this.inputPolicy === 'no_shared_input' ? 'blocked' : 'may_move_real_cursor', owner_file: this.native ? this.ownerFile : undefined, grants, startup_error: this.startError, execution: record ? { execution_id: record.id, state: record.state, native_request_ids: record.nativeIds, result: result ? { ...result, native_receipts: this.nativeReceipts(record) } : undefined } : undefined, ...(id && !record ? { error: error('execution_not_found', 'No execution with this ID in this connection.') } : {}) });
  }

  async close() {
    await this.ready;
    if (this.closing) return this.closing;
    this.closing = (async () => {
      if (!this.fenced) await this.fence('disconnected', 'MCP connection closed; no further native input.');
      if (this.native) {
        try { await this.native.close(); }
        catch (e) { await this.audit({ event: 'close_incomplete', message: String(e.message).slice(0, 300) }); }
      }
    })();
    return this.closing;
  }
}

export { Supervisor };

export async function main(argv = process.argv.slice(2)) {
  const supervisor = new Supervisor(options(argv));
  const server = new McpServer({ name: 'desktop-world', version: '0.1.0' });
  server.registerTool('desktop_exec', {
    title: 'Execute Desktop World script',
    description: 'Run an approved local async JavaScript body with dw, state and print. Can change authorized applications. Review the entire code before approval. Read the installed desktop-world Skill.',
    inputSchema: z.object({ execution_id: z.string().min(1).max(128), code: z.string().max(65536) }).strict(),
    annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
  }, (args, ctx) => supervisor.exec(args, ctx.mcpReq.signal));
  server.registerTool('desktop_status', {
    title: 'Desktop World status and original execution receipt',
    description: 'Query the connection, authorization summary and a named original execution, including after cancellation. Does not restart or replay work.',
    inputSchema: z.object({ execution_id: z.string().min(1).max(128).optional() }).strict(),
    annotations: { readOnlyHint: true },
  }, ({ execution_id }) => supervisor.status(execution_id));
  const transport = new StdioServerTransport();
  server.server.onclose = () => { void supervisor.close(); };
  process.once('SIGTERM', () => { void supervisor.close().finally(() => process.exit()); });
  process.once('SIGINT', () => { void supervisor.close().finally(() => process.exit()); });
  await server.connect(transport);
  return { server, supervisor };
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) main().catch(e => { console.error(`Desktop World MCP startup failed: ${e.message}`); process.exitCode = 1; });
