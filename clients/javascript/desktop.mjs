#!/usr/bin/env node
// A local scripting adapter over the existing helper, not another desktop engine.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { createServer, createConnection } from 'node:net';
import { mkdtemp, mkdir, readFile, writeFile, chmod, unlink, rmdir } from 'node:fs/promises';
import { openSync, writeSync, closeSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import { randomUUID } from 'node:crypto';

const HELP = `Desktop World JavaScript — Node 20+, no packages, persistent native session.
Trusted host: node desktop.mjs serve --host host.json [--session harness/session.json]
Agent:        node desktop.mjs exec [--session harness/session.json] <<'JS'
state.inventory = await dw.observe();
print(dw.list(state.inventory));
JS
Stop:         node desktop.mjs stop [--session harness/session.json]

Each exec is an async JavaScript body. Use await, variables, loops and conditionals.
state survives exec calls; local const/let do not. Only print(...) enters the model
response, together with mandatory coverage, action outcomes, failures and metrics.
Errors/partial/unknown effects stop further calls in that script; never auto-retry.

dw.observe(args={})                 desktop summary by default
dw.outline(ref, options={})         bounded subtree; options override fields/budget
dw.find(within, locator, options)   bounded exact/substring discovery, returns observation
dw.waitFor(observeArgs, predicate, options={})  poll reads until predicate(ob)===true
dw.one(ob, {role, name, kind})      require one match AND complete, untruncated coverage
dw.list(ob, fields=['role','name','value_preview'], options={})  bounded rows + next_offset
dw.rows(ob, fields)                full row array for local filtering, can exceed print limit
dw.value(field)                    unwrap known facts or primitive fields; throws for unknown/redacted
dw.focused(scopeRef)               fresh focused UI Ref, scoped observation
dw.focus(windowOrFocusableUIRef) / dw.invoke(ref) / dw.set(ref, text)
dw.press(ref, key, modifiers=[]) / dw.type(ref, text)
dw.click(ref, options={})           left single click by default
dw.act(steps, options={})           one ordered native plan, max 16 steps
dw.read(ref, options={}) / dw.sync(cursor, options={})
dw.capture(args) / dw.get(runId) / dw.cancel(runId)
dw.call(op,args,id?)                underlying seven verbs; stable id for receipt recovery

Keys: A-Z,0-9,Enter,Tab,Escape,Backspace,Delete,Space,Left,Right,Up,Down,
Home,End,PageUp,PageDown. Modifiers: primary,meta,control,alt,shift (lowercase).
primary = Command on macOS, Control on Windows. Target is always an observed Ref.
Roles are normalized: application,window,button,text_field,text,container (not AX names).
Focus a WINDOW before input; application Refs are discovery scopes, not focus targets.
list defaults to 20 rows/4 KiB; use next_offset on the SAME saved observation for more.
Use rows for local filtering before print; value works on role/name/state alike.
After opening a dialog, observe its actual objects/focus before the next input.
An action completed with verification=not_requested proves dispatch, not task success.
read uses limit_runes<=4096 and continuation=result.next for more text.

Limits: 32 calls, 60 seconds, 8 KiB printed JSON per exec; 64 KiB source.
Full transport evidence stays in harness/wire.jsonl; script metrics in scripts.jsonl.
No TCP listener. This is a trusted local code runner, NOT a security sandbox.
Do not read task files or use other automation when an evaluation requires UI only.
`;

const known = fact => {
  if (['string', 'number', 'boolean'].includes(typeof fact)) return fact;
  if (fact && Object.hasOwn(fact, 'known')) return fact.known;
  if (fact?.status === 'known' && Object.hasOwn(fact, 'value')) return fact.value;
  throw Object.assign(new Error(`Expected a known fact, got ${JSON.stringify(fact)}`), { code: 'fact_not_known' });
};
const errorInfo = error => ({ code: error?.code ?? 'script_failed', message: String(error?.message ?? error).slice(0, 2000) });
const bytes = value => Buffer.byteLength(JSON.stringify(value));
const pick = (value, keys) => Object.fromEntries(keys.filter(k => value?.[k] !== undefined).map(k => [k, value[k]]));
const smallCoverage = value => pick(value, ['scope', 'fields', 'max_depth', 'complete', 'truncated', 'dirty', 'continuation', 'unavailable_sources']);
const rows = (ob, fields = ['role', 'name', 'value_preview']) => (ob.objects ?? []).map(object => {
  const row = { ref: object.ref };
  for (const field of fields) {
    let value = field.split('.').reduce((v, key) => v?.[key], object);
    if (value && Object.hasOwn(value, 'known')) value = value.known;
    else if (value?.status === 'known') value = value.value;
    if (value !== undefined) row[field] = value;
  }
  return row;
});


function receiptSummary(id, op, receipt) {
  const counts = {};
  const problems = [];
  for (const step of receipt?.steps ?? []) {
    const key = `${step.delivery ?? 'unknown'}/${step.verification ?? 'unknown'}`;
    counts[key] = (counts[key] ?? 0) + 1;
    if (step.fault || step.delivery === 'unknown') problems.push(step);
  }
  return { id, op, ...pick(receipt, ['run_id', 'state', 'outcome', 'fault', 'seat_health']), delivery_verification: counts, ...(problems.length ? { problems } : {}) };
}

// transport receives a helper Request and returns its full helper Response.
export function createSession(transport, { epoch = '', maxCalls = 32, timeoutMs = 60000, printBytes = 8192 } = {}) {
  const state = Object.create(null);
  let sequence = 0;
  let busy = false;
  let closed = false;
  let current;
  const close = () => { closed = true; if (current) current.active = false; };
  async function execute(code, { signal } = {}) {
    if (busy) return { error: { code: 'script_busy', message: 'One script at a time; do not overlap desktop tasks.' } };
    if (closed) return { error: { code: 'session_closed', message: 'Do not reuse old references in a new session.' } };
    if (typeof code !== 'string' || Buffer.byteLength(code) > 65536) return { error: { code: 'invalid_script', message: 'Source must be at most 64 KiB.' } };
    busy = true;
    const run = current = { active: true, failed: null, queue: Promise.resolve(), calls: 0, outputs: [], observations: [], actions: [], wireBytes: 0, inputBytes: 0 };
    const started = Date.now();
    let printed = 0;
    let timer;
    const fail = error => { run.failed ??= errorInfo(error); return error; };
    const disconnected = () => { run.active = false; fail(Object.assign(new Error('Caller disconnected; subsequent calls stopped. Inspect original receipts before retry.'), { code: 'caller_disconnected' })); };
    signal?.addEventListener('abort', disconnected, { once: true });
    if (signal?.aborted) disconnected();
    function call(op, args = {}, id) {
      const invoke = async () => {
        if (!run.active || closed || run.failed) throw Object.assign(new Error('Script stopped; inspect its recorded error before a new script.'), { code: 'chain_stopped' });
        if (++run.calls > maxCalls) throw fail(Object.assign(new Error(`At most ${maxCalls} calls per script.`), { code: 'call_budget_exceeded' }));
        const request = { id: id ?? `js-${++sequence}`, op, args };
        run.inputBytes += bytes(request);
        let response;
        try { response = await transport(request); }
        catch (error) { throw fail(error); }
        run.wireBytes += bytes(response);
        const result = response.result;
        if (result?.coverage) run.observations.push({ id: request.id, op, ...smallCoverage(result.coverage), ...pick(result, ['next', 'truncated', 'reset_required', 'reset_reason', 'cursor']) });
        if (op === 'act' || op === 'get' || op === 'cancel') run.actions.push(receiptSummary(request.id, op, result));
        if (response.error) throw fail(Object.assign(new Error(response.error.message ?? response.error.code), response.error));
        if (op === 'act' && result?.outcome !== 'completed') throw fail(Object.assign(new Error(`Action outcome ${result?.outcome ?? 'missing'}; inspect receipt, do not replay.`), { code: 'action_not_completed' }));
        return result;
      };
      const result = run.queue.then(invoke);
      // Every request is serialized, including Promise.all and unawaited calls.
      run.queue = result.catch(error => { fail(error); });
      return result;
    }
    const target = ref => {
      if (typeof ref !== 'string' || !ref) throw new Error('Target must be an observed Ref string.');
      return { ref };
    };
    const act = (steps, options = {}) => call('act', { ...options, steps: steps.map((step, i) => ({ id: `s${i + 1}`, ...step })) });
    const observe = (args = {}) => call('observe', { scope: { desktop: true }, projection: 'summary', fields: ['name', 'role', 'app', 'window'], budget: { max_results: 256, max_output_bytes: 131072 }, ...args });
    const outline = (ref, options = {}) => observe({ scope: { refs: [ref] }, projection: 'outline', fields: ['name', 'role', 'value_preview', 'states', 'capabilities'], ...options, budget: { max_depth: 6, max_results: 60, max_text_runes: 400, max_output_bytes: 16000, ...options.budget } });
    const api = Object.freeze({
      call, observe, outline, act, value: known,
      async waitFor(args, predicate, { timeout_ms = 3000, interval_ms = 100 } = {}) {
        if (typeof predicate !== 'function' || !Number.isFinite(timeout_ms) || timeout_ms < 1 || timeout_ms > 10000 || !Number.isFinite(interval_ms) || interval_ms < 50 || interval_ms > 1000) throw new Error('waitFor requires a predicate, timeout_ms 1..10000 and interval_ms 50..1000.');
        const end = Date.now() + timeout_ms;
        do {
          const ob = await observe(args);
          if (predicate(ob) === true) return ob;
          if (Date.now() >= end) break;
          await new Promise(resolve => setTimeout(resolve, Math.min(interval_ms, end - Date.now())));
        } while (Date.now() < end);
        throw Object.assign(new Error('Observation condition did not become true; no action was retried.'), { code: 'condition_timeout' });
      },
      find: (within, locator, options = {}) => outline(within, { ...options, match: { ...locator, within } }),
      one(ob, filter = {}) {
        const coverage = ob?.coverage;
        if (!coverage?.complete || coverage.truncated || coverage.dirty || coverage.unavailable_sources?.length) throw new Error('Cannot prove uniqueness from incomplete/dirty coverage; narrow scope/filter or continue observation.');
        const matches = (ob.objects ?? []).filter(o => Object.entries(filter).every(([key, value]) => key === 'name' ? ((o.name && Object.hasOwn(o.name, 'known') ? o.name.known : o.name?.value) === value) : o[key] === value));
        if (matches.length !== 1) throw new Error(`Expected exactly one object; found ${matches.length}.`);
        return matches[0];
      },
      rows,
      list(ob, fields, { offset = 0, limit = 20, max_bytes = 4096 } = {}) {
        if (!Number.isInteger(offset) || offset < 0 || !Number.isInteger(limit) || limit < 1 || limit > 100 || !Number.isInteger(max_bytes) || max_bytes < 512 || max_bytes > 6144) throw new Error('list requires offset>=0, limit 1..100, max_bytes 512..6144.');
        const all = rows(ob, fields);
        if (offset > all.length) throw new Error('list offset exceeds observation length.');
        const page = { rows: [], total: all.length, next_offset: null };
        let i = offset;
        for (; i < all.length && page.rows.length < limit; i++) {
          page.rows.push(all[i]);
          page.next_offset = i + 1 < all.length ? i + 1 : null;
          if (bytes(page) > max_bytes) {
            page.rows.pop();
            if (!page.rows.length) throw Object.assign(new Error('One row exceeds list max_bytes; choose fewer fields or locally shorten text.'), { code: 'row_budget_exceeded' });
            break;
          }
        }
        page.next_offset = i < all.length ? i : null;
        return page;
      },
      async focused(scopeRef) {
        const ob = await observe({ scope: { refs: [scopeRef] }, projection: 'detail', fields: ['name', 'role', 'app', 'window'], budget: { max_results: 4 } });
        return known(ob.seat?.focused_object);
      },
      focus: ref => act([{ op: 'focus', target: target(ref) }]),
      invoke: ref => act([{ op: 'invoke', target: target(ref) }]),
      set: (ref, text) => act([{ op: 'set_value', target: target(ref), set_value: { text } }]),
      press: (ref, key, modifiers = []) => act([{ op: 'keyboard.press', target: target(ref), press: { key, modifiers } }]),
      type: (ref, text) => act([{ op: 'keyboard.type_text', target: target(ref), type_text: { text } }]),
      click: (ref, options = {}) => act([{ op: 'pointer.click', target: target(ref), click: { button: 'left', count: 1, ...options } }]),
      read: (ref, options = {}) => call('read', { target: ref, ...options }),
      sync: (cursor, options = {}) => call('sync', { cursor, ...options }),
      capture: args => call('capture', args),
      get: runId => call('get', { run_id: runId }),
      cancel: runId => call('cancel', { run_id: runId }),
    });
    const context = vm.createContext({ dw: api, state, print(value) {
      const encoded = JSON.stringify(value);
      if (encoded === undefined) throw new Error('print requires a JSON value.');
      if (printed + Buffer.byteLength(encoded) > printBytes) throw Object.assign(new Error(`Printed output exceeds ${printBytes} bytes; select fewer fields/objects and retain data in state.`), { code: 'print_budget_exceeded' });
      printed += Buffer.byteLength(encoded);
      run.outputs.push(JSON.parse(encoded));
    } }, { codeGeneration: { strings: false, wasm: false } });
    try {
      const script = new vm.Script(`(async () => {\n${code}\n})()`, { filename: 'desktop-script.js' });
      await Promise.race([
        script.runInContext(context, { timeout: 1000, breakOnSigint: true }),
        new Promise((_, reject) => { timer = setTimeout(() => reject(Object.assign(new Error('Script deadline reached; subsequent calls stopped.'), { code: 'script_timeout' })), timeoutMs); }),
      ]);
    } catch (error) { fail(error); }
    finally {
      // Calls still queued after the script returns must not create later effects.
      run.active = false;
      clearTimeout(timer);
      await run.queue;
      signal?.removeEventListener('abort', disconnected);
      busy = false;
      current = undefined;
    }
    return { epoch, outputs: run.outputs, ...(run.observations.length ? { observations: run.observations } : {}), ...(run.actions.length ? { actions: run.actions } : {}), ...(run.failed ? { error: run.failed } : {}), metrics: { calls: Math.min(run.calls, maxCalls), elapsed_ms: Date.now() - started, request_bytes: run.inputBytes, helper_response_bytes: run.wireBytes, printed_bytes: printed } };
  }
  return { execute, close };
}

async function connectHelper(host, directory) {
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const wire = openSync(join(directory, 'wire.jsonl'), 'wx', 0o600);
  const child = spawn(resolve(host.helper), host.args, { stdio: ['pipe', 'pipe', 'inherit'] });
  const pending = new Map();
  let broken;
  let readyResolve, readyReject;
  const ready = new Promise((yes, no) => { readyResolve = yes; readyReject = no; });
  const fail = error => {
    broken = error;
    readyReject(error);
    for (const entry of pending.values()) { clearTimeout(entry.timer); entry.reject(error); }
    pending.clear();
  };
  const writeLog = row => { writeSync(wire, JSON.stringify({ at: new Date().toISOString(), ...row }) + '\n'); };
  child.once('error', fail);
  child.stdin.on('error', fail);
  const exited = new Promise(resolveExit => child.once('exit', (code, signal) => {
    fail(Object.assign(new Error(`Helper exited (${code ?? signal}); effects may be unknown; never automatically restart/replay.`), { code: 'helper_exited' }));
    resolveExit();
  }));
  createInterface({ input: child.stdout }).on('line', line => {
    try {
      const response = JSON.parse(line);
      writeLog({ direction: 'response', data: response });
      if (response.type === 'hello') { readyResolve(response); return; }
      const entry = pending.get(response.id);
      if (entry) { pending.delete(response.id); clearTimeout(entry.timer); entry.resolve(response); }
    } catch (error) { fail(error); child.stdin.end(); }
  });
  let startup;
  try { startup = await Promise.race([ready, new Promise((_, reject) => { const t = setTimeout(() => reject(new Error('Helper startup timed out')), 15000); t.unref(); })]); }
  catch (error) { child.kill(); closeSync(wire); throw error; }
  return {
    hello: startup,
    call(request) {
      if (broken) return Promise.reject(broken);
      if (pending.has(request.id)) return Promise.reject(new Error('Request ID is already in flight.'));
      writeLog({ direction: 'request', data: request });
      return new Promise((resolveCall, reject) => {
        const timer = setTimeout(() => { fail(Object.assign(new Error(`Transport deadline for ${request.id}; outcome unknown. Session fenced; inspect wire log.`), { code: 'transport_unknown' })); child.stdin.end(); }, 15000);
        pending.set(request.id, { resolve: resolveCall, reject, timer });
        child.stdin.write(JSON.stringify(request) + '\n', error => { if (error) fail(error); });
      });
    },
    async close() {
      child.stdin.end();
      const timer = setTimeout(() => child.kill('SIGTERM'), 2500);
      await exited;
      clearTimeout(timer);
      closeSync(wire);
    },
  };
}

export async function main(argv) {
  const command = argv.shift();
  if (!command || command === 'help' || command === '--help') { console.log(HELP); return; }
  const options = {};
  while (argv.length) {
    const key = argv.shift();
    if (!['--host', '--session'].includes(key) || !argv.length) throw new Error('Use --help; unknown/missing option.');
    options[key.slice(2)] = argv.shift();
  }
  const sessionPath = resolve(options.session ?? 'harness/session.json');
  if (command === 'serve') {
    const host = JSON.parse(await readFile(options.host ?? 'host.json', 'utf8'));
    const logDir = dirname(sessionPath);
    await mkdir(logDir, { recursive: true, mode: 0o700 });
    const audit = openSync(join(logDir, 'scripts.jsonl'), 'wx', 0o600);
    let sources, helper;
    try {
      sources = openSync(join(logDir, 'script-code.jsonl'), 'wx', 0o600);
      helper = await connectHelper(host, logDir);
    } catch (error) {
      closeSync(audit);
      if (sources !== undefined) closeSync(sources);
      throw error;
    }
    const session = createSession(helper.call, { epoch: helper.hello.environment?.epoch });
    const socketDir = await mkdtemp(join(tmpdir(), 'dw-js-'));
    await chmod(socketDir, 0o700);
    const address = process.platform === 'win32' ? `\\\\.\\pipe\\desktop-world-${randomUUID()}` : join(socketDir, 's');
    let stopping;
    let ownsSession = false;
    const executions = new Set();
    const stop = () => stopping ??= (async () => {
      session.close();
      server.close();
      await helper.close();
      await Promise.allSettled([...executions]);
      closeSync(audit);
      closeSync(sources);
      if (ownsSession) await unlink(sessionPath).catch(() => {});
      if (process.platform !== 'win32') await unlink(address).catch(() => {});
      await rmdir(socketDir).catch(() => {});
    })();
    const server = createServer(socket => {
      let content = '';
      let received = false;
      const controller = new AbortController();
      socket.once('close', () => controller.abort());
      socket.setEncoding('utf8');
      socket.setTimeout(80000, () => socket.destroy());
      socket.on('error', () => {});
      socket.on('data', async chunk => {
        if (received) return;
        content += chunk.toString('utf8');
        if (Buffer.byteLength(content) > 270000) { received = true; socket.end(JSON.stringify({ error: { code: 'request_too_large' } }) + '\n'); return; }
        if (!content.includes('\n')) return;
        received = true;
        try {
          const request = JSON.parse(content.slice(0, content.indexOf('\n')));
          if (request.op === 'stop') { await stop(); socket.end(JSON.stringify({ stopped: true }) + '\n'); return; }
          if (request.op !== 'exec') throw new Error('Expected exec or stop');
          writeSync(sources, JSON.stringify({ at: new Date().toISOString(), code: request.code }) + '\n');
          const work = session.execute(request.code, { signal: controller.signal });
          executions.add(work);
          const response = await work;
          response.metrics ??= {};
          response.metrics.script_bytes = Buffer.byteLength(request.code ?? '');
          const output = JSON.stringify(response);
          writeSync(audit, JSON.stringify({ at: new Date().toISOString(), ...response.metrics, model_response_bytes: Buffer.byteLength(output), error: response.error?.code }) + '\n');
          socket.end(output + '\n');
          executions.delete(work);
        } catch (error) { socket.end(JSON.stringify({ error: errorInfo(error) }) + '\n'); }
      });
    });
    try {
      await new Promise((yes, no) => { server.once('error', no); server.listen(address, yes); });
      if (process.platform !== 'win32') await chmod(address, 0o600);
      await writeFile(sessionPath, JSON.stringify({ address, pid: process.pid, epoch: helper.hello.environment?.epoch }) + '\n', { mode: 0o600, flag: 'wx' });
      ownsSession = true;
      process.once('SIGINT', stop); process.once('SIGTERM', stop);
      console.log(JSON.stringify({ ready: true, session: sessionPath, epoch: helper.hello.environment?.epoch }));
    } catch (error) { await stop(); throw error; }
    return;
  }
  if (command !== 'exec' && command !== 'stop') throw new Error('Expected serve, exec, stop or help.');
  const connection = JSON.parse(await readFile(sessionPath, 'utf8'));
  let code = '';
  process.stdin.setEncoding('utf8');
  if (command === 'exec') for await (const chunk of process.stdin) { code += chunk.toString('utf8'); if (Buffer.byteLength(code) > 65536) throw new Error('Source exceeds 64 KiB.'); }
  await new Promise((yes, no) => {
    const socket = createConnection(connection.address);
    socket.setEncoding('utf8');
    let output = '';
    socket.setTimeout(80000, () => socket.destroy(new Error('Session response timed out; inspect original logs before retry.')));
    socket.on('connect', () => socket.write(JSON.stringify({ op: command, code }) + '\n'));
    socket.on('data', chunk => { output += chunk.toString('utf8'); });
    socket.on('error', no);
    socket.on('end', () => { try { const result = JSON.parse(output); console.log(JSON.stringify(result)); if (result.error) process.exitCode = 1; yes(); } catch (error) { no(error); } });
  });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(JSON.stringify({ error: errorInfo(error) })); process.exitCode = 1; });
}
