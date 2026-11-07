// Narrow adaptation of CUA 0.38.0 script_tool/prelude.js (85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19).
// Copyright (c) 2025 Cua AI, Inc. MIT; see THIRD_PARTY_NOTICES.md.
// Only bridge hiding, call diagnostics, console and promise settlement are retained.
(() => {
  "use strict";
  const host = globalThis.__host;
  const finish = globalThis.__done;
  const emit = globalThis.__log;
  const parse = JSON.parse;
  const stringify = JSON.stringify;
  const create = Object.create;
  const define = Object.defineProperty;
  const freeze = Object.freeze;
  const prototype = Object.getPrototypeOf;
  const setPrototype = Object.setPrototypeOf;
  const descriptor = Object.getOwnPropertyDescriptor;
  const ownKeys = Reflect.ownKeys;
  const isArray = Array.isArray;
  const finite = Number.isFinite;
  const ErrorClass = Error;
  const StringClass = String;
  const objectPrototype = Object.prototype;
  const arrayPrototype = Array.prototype;
  const apply = Reflect.apply;
  const then = Promise.prototype.then;
  const indexOf = String.prototype.indexOf;
  const slice = String.prototype.slice;
  const charAt = String.prototype.charCodeAt;
  const split = String.prototype.split;
  const regexpExec = RegExp.prototype.exec;
  const targetId = /^w[0-9]+\.e[0-9]+$/;
  const root = parse(globalThis.__initial_state);
  const methods = parse(globalThis.__methods);
  const sessionMetadata = parse(globalThis.__session);
  delete globalThis.__session;
  delete globalThis.__methods;
  let accepting = true;
  let phaseViolation = false;
  delete globalThis.__host;
  delete globalThis.__done;
  delete globalThis.__log;
  delete globalThis.__initial_state;

  const fail = (message, code) => {
    const error = new ErrorClass(message);
    error.name = "DtwError"; error.code = code;
    throw error;
  };
  const lineOf = (stack) => {
    if (typeof stack !== "string") return 0;
    const start = apply(indexOf, stack, ["script.js:"]);
    if (start < 0) return 0;
    let n = 0;
    for (let i = start + 10; i < stack.length; ++i) {
      const c = apply(charAt, stack, [i]);
      if (c < 48 || c > 57) break;
      n = n * 10 + c - 48;
    }
    return n;
  };
  const bytes = (text) => {
    let size = 0;
    for (let i = 0; i < text.length; ++i) {
      const c = apply(charAt, text, [i]);
      if (c < 128) size++;
      else if (c < 2048) size += 2;
      else if (c >= 0xd800 && c <= 0xdbff && i + 1 < text.length) { size += 4; i++; }
      else size += 3;
    }
    return size;
  };
  // Build data-only, null-prototype copies. Host arguments and returned objects
  // omit undefined data properties; persistent state remains strict JSON.
  const encode = (value, label, limit, omitUndefinedProperties = false) => {
    const ancestors = [];
    const stack = [];
    setPrototype(ancestors, null);
    setPrototype(stack, null);
    const holder = create(null);
    let nodes = 0;
    const invalid = (reason, path) => fail(`${label} ${reason} at ${path}`, "invalid_json");
    const enter = (value, depth, objectField, path, parent, key) => {
      if (++nodes > 32768 || depth > 64) invalid("exceeds JSON traversal limit", path);
      if (omitUndefinedProperties && objectField && value === undefined) return;
      if (value === null || typeof value === "string" || typeof value === "boolean" || typeof value === "number") {
        if (typeof value === "number" && !finite(value)) invalid("contains a non-finite number", path);
        define(parent, key, {value, enumerable:true, writable:true, configurable:true});
        return;
      }
      if (typeof value !== "object") invalid("contains a non-JSON value", path);
      const proto = prototype(value);
      const array = isArray(value);
      if (proto !== null && proto !== (array ? arrayPrototype : objectPrototype)) invalid("contains a non-JSON object", path);
      for (let i = 0; i < ancestors.length; i++) if (ancestors[i] === value) invalid("contains a cycle", path);
      const out = array ? [] : create(null);
      if (array) setPrototype(out, null);
      define(parent, key, {value:out, enumerable:true, writable:true, configurable:true});
      ancestors[ancestors.length] = value;
      stack[stack.length] = {value, depth, path, out, array, keys:ownKeys(value), index:0, count:0};
    };
    enter(value, 0, false, "$", holder, "value");
    while (stack.length) {
      const frame = stack[stack.length - 1];
      if (frame.index === frame.keys.length) {
        if (frame.array && frame.count !== frame.value.length) invalid("contains an array hole", frame.path);
        stack.length--;
        ancestors.length--;
        continue;
      }
      const key = frame.keys[frame.index++];
      if (frame.array && key === "length") continue;
      if (typeof key !== "string") invalid("contains a symbol key", frame.path);
      const location = `${frame.path}[${stringify(apply(slice, key, [0, 128]))}]`;
      const field = descriptor(frame.value, key);
      if (!field || !descriptor(field, "value") || !field.enumerable) invalid("contains an accessor or hidden property", location);
      if (frame.array && StringClass(frame.count) !== key) invalid("contains an array hole or extra property", location);
      frame.count++;
      enter(field.value, frame.depth + 1, !frame.array, location, frame.out, key);
    }
    const text = stringify(holder.value);
    if (bytes(text) > limit) fail(`${label} exceeds ${limit} bytes`, "output_limit");
    return text;
  };
  const call = (op, args) => {
    if (!accepting) {
      phaseViolation = true;
      fail("app calls are unavailable during script settlement", "invalid_phase");
    }
    const line = lineOf(new ErrorClass().stack);
    const encoded = encode(args, "host arguments", 65536, true);
    // A locally rejected argument never starts a read and cannot settle a
    // preceding action's readback. Clear dirty only at the host handoff.
    if (agent && !(op === "scene.wait" && typeof args.ms === "number")) dirty = op !== "scene.look" && op !== "scene.capture" && op !== "scene.after" && op !== "scene.readText" && op !== "scene.waitForNewWindow" && op !== "scene.wait";
    const reply = parse(host(op, encoded, line));
    if (!reply.ok) {
      const error = new ErrorClass(`${op} failed: ${reply.error}`);
      error.name = "DtwError"; error.op = op; error.code = reply.code;
      error.details = reply.details ?? null;
      error.call = reply.call; error.line = line;
      throw error;
    }
    return reply.result;
  };
  const app = create(null);
  const bindWindow = (row) => {
    const bound = create(null);
    const window_ref = row.window_ref;
    define(bound, "window_ref", {value:window_ref, enumerable:true});
    define(bound, "title", {value:row.title ?? null, enumerable:true});
    for (const metadata of methods) {
      if (!metadata.window) continue;
      const op = metadata.name;
      const method = async (...args) => {
        if (args.length > 1) fail(`window.${op} takes one argument object`, "invalid_args");
        const options = args.length ? parse(encode(args[0], "window arguments", 65536, true)) : create(null);
        if (!options || isArray(options) || typeof options !== "object" || descriptor(options, "window_ref"))
          fail("Bound window arguments must omit window_ref", "invalid_args");
        if (op === "observe" && descriptor(options, "cursor"))
          fail("Continue pagination with app.observe({cursor})", "invalid_args");
        options.window_ref = window_ref;
        return call(op, options);
      };
      define(bound, op === "closeWindow" ? "close" : op, {value:freeze(method),enumerable:true});
    }
    return freeze(bound);
  };
  for (const metadata of methods) {
    const op = metadata.name;
    const method = async (...args) => {
      if (metadata.arity === 0) {
        if (args.length !== 0) fail("app.windows takes no arguments", "invalid_args");
        return call(op, {});
      }
      if (metadata.optional && args.length === 0) { const result = call(op, {}); return op === "window" ? bindWindow(result) : result; }
      if (args.length !== 1) fail(`app.${op} takes one exact argument object`, "invalid_args");
      const result = call(op, args[0]);
      return op === "window" ? bindWindow(result) : result;
    };
    define(app, op, {value:freeze(method), enumerable:true});
  }
  const agent = sessionMetadata.agent === true;
  let dirty = false;
  if (agent) {
    const surface = create(null);
    for (const op of ["look", "capture", "readText", "wait", "waitForNewWindow", "click", "setValue", "move", "drag", "typeText", "keypress", "scroll", "focus", "quit"]) {
      const method = async (...args) => {
        if (args.length > 1) fail(`app.${op} takes one argument`, "invalid_arguments");
        let options = args.length ? args[0] : {};
        if (op === "wait" && typeof options === "number") options = {ms:options};
        if (op === "typeText" && typeof options === "string") options = {text:options};
        if (op === "readText" && typeof options === "string") options = {target:options};
        if (op === "keypress" && typeof options === "string") options = {keys:apply(split, options, ["+"])};
        if (op === "keypress" && isArray(options)) options = {keys:options};
        if (op === "click" && typeof options === "string") options = apply(regexpExec, targetId, [options]) !== null ? {target:options} : {selector:{text:options}};
        const result = call(`scene.${op}`, options);
        return result;
      };
      define(surface, op, {value:freeze(method), enumerable:true});
    }
    define(globalThis, "app", {value:freeze(surface), writable:false, configurable:false});
  } else {
  define(globalThis, "app", {value:freeze(app), writable:false, configurable:false});
  define(globalThis, "state", {get:()=>root, set:()=>fail("state root cannot be replaced", "invalid_state"), configurable:false});
  const session = create(null);
  define(session, "id", {value:sessionMetadata.id, enumerable:true});
  define(session, "app", {value:sessionMetadata.app, enumerable:true});
  define(session, "state", {get:()=>root, enumerable:true});
  const result = async (...args) => {
    if (args.length > 1) fail("session.result takes one optional argument", "invalid_args");
    let query = args.length === 0 ? {} : args[0];
    if (typeof query === "string") query = {execution_id:query};
    return call("session.result", query);
  };
  define(session, "result", {value:freeze(result), enumerable:true});
  define(globalThis, "session", {value:freeze(session), writable:false, configurable:false});
  }
  const show = (value) => {
    if (typeof value === "string") return value;
    try { return encode(value, "console", 8 * 1024 * 1024); }
    catch (_) { return "[non-JSON value]"; }
  };
  const console = create(null);
  for (const level of ["log", "info", "warn", "error", "debug"]) {
    const log = (...values) => {
      let text = "";
      for (let i = 0; i < values.length; i++) text += (i ? " " : "") + show(values[i]);
      // Bound the Rust bridge copy; Rust applies exact UTF-8 aggregate limits.
      emit(level, apply(slice, text, [0, 8000]));
    };
    define(console, level, {value:freeze(log), enumerable:true});
  }
  define(globalThis, "print", {value:console.log, writable:false, configurable:false});
  define(globalThis, "console", {value:freeze(console), writable:false, configurable:false});
  const envelope = (fields) => {
    const out = create(null);
    const keys = ownKeys(fields);
    for (let i = 0; i < keys.length; i++) define(out, keys[i], {value:fields[keys[i]], enumerable:true});
    return stringify(out);
  };
  const clip = (text, limit) => text.length > limit ? apply(slice,text,[0,limit-15]) + "… [truncated]" : text;
  const rejectionEnvelope = (error) => {
    accepting = false;
    let name = null, message = "script error", stack = "", op = null, code = null, call = null, line = null;
    try {
      name = typeof error?.name === "string" ? error.name : null;
      message = typeof error?.message === "string" ? error.message : StringClass(error);
      stack = typeof error?.stack === "string" ? error.stack : "";
      op = typeof error?.op === "string" ? error.op : null;
      code = typeof error?.code === "string" ? error.code : null;
      call = typeof error?.call === "number" ? error.call : null;
      line = lineOf(stack) || (typeof error?.line === "number" ? error.line : null);
    } catch (_) { message = "exception could not be inspected"; }
    return envelope({ok:false, name:name === null ? null : apply(slice,name,[0,128]), message:clip(message,4000), line, op:op === null ? null : apply(slice,op,[0,128]), code:code === null ? null : apply(slice,code,[0,128]), call});
  };
  const readAfter = () => {
    call("scene.after", {});
  };
  const reject = (error) => {
    if (agent && dirty && accepting) { try { readAfter(); } catch (_) {} }
    finish(rejectionEnvelope(error));
  };
  // Captured and removed by Rust before user code can execute.
  globalThis.__reject_unhandled = rejectionEnvelope;
  globalThis.__settle = (promise) => apply(then, promise, [
    (value) => {
      try {
        if (agent && dirty) readAfter();
        accepting = false;
        const value_json = encode(value === undefined ? null : value, "result", 65536, true);
        const state_json = encode(root, "state", 65536);
        if (phaseViolation) fail("app calls are unavailable during script settlement", "invalid_phase");
        finish(envelope({ok:true, value_json, state_json}));
      } catch (error) { reject(error); }
    }, reject,
  ]);
})();
