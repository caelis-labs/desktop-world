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
  const root = parse(globalThis.__initial_state);
  const methods = parse(globalThis.__methods);
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
  // Build data-only, null-prototype copies. Only host argument objects may
  // omit undefined data properties; all other JSON checks remain strict.
  const encode = (value, label, limit, omitUndefinedProperties = false) => {
    const ancestors = [];
    setPrototype(ancestors, null);
    let nodes = 0;
    const copy = (value, depth, objectField = false) => {
      if (++nodes > 32768 || depth > 64) fail(`${label} exceeds JSON traversal limit`, "invalid_json");
      if (omitUndefinedProperties && objectField && value === undefined) return undefined;
      if (value === null || typeof value === "string" || typeof value === "boolean") return value;
      if (typeof value === "number") {
        if (!finite(value)) fail(`${label} contains a non-finite number`, "invalid_json");
        return value;
      }
      if (typeof value !== "object") fail(`${label} contains a non-JSON value`, "invalid_json");
      const proto = prototype(value);
      const array = isArray(value);
      if (proto !== null && proto !== (array ? arrayPrototype : objectPrototype)) fail(`${label} contains a non-JSON object`, "invalid_json");
      for (let i = 0; i < ancestors.length; i++) if (ancestors[i] === value) fail(`${label} contains a cycle`, "invalid_json");
      ancestors[ancestors.length] = value;
      const out = array ? [] : create(null);
      if (array) setPrototype(out, null);
      const keys = ownKeys(value);
      let count = 0;
      for (let i = 0; i < keys.length; i++) {
        const key = keys[i];
        if (array && key === "length") continue;
        if (typeof key !== "string") fail(`${label} contains a symbol key`, "invalid_json");
        const field = descriptor(value, key);
        if (!field || !descriptor(field, "value") || !field.enumerable) fail(`${label} contains an accessor or hidden property`, "invalid_json");
        if (array && (StringClass(count) !== key)) fail(`${label} contains an array hole or extra property`, "invalid_json");
        const copied = copy(field.value, depth+1, !array);
        if (copied !== undefined)
          define(out, key, {value:copied, enumerable:true, writable:true, configurable:true});
        count++;
      }
      if (array && count !== value.length) fail(`${label} contains an array hole`, "invalid_json");
      ancestors.length--;
      return out;
    };
    const text = stringify(copy(value, 0));
    if (bytes(text) > limit) fail(`${label} exceeds ${limit} bytes`, "output_limit");
    return text;
  };
  const call = (op, args) => {
    if (!accepting) {
      phaseViolation = true;
      fail("app calls are unavailable during script settlement", "invalid_phase");
    }
    const line = lineOf(new ErrorClass().stack);
    const reply = parse(host(op, encode(args, "host arguments", 65536, true), line));
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
  for (const metadata of methods) {
    const op = metadata.name;
    const method = async (...args) => {
      if (metadata.arity === 0) {
        if (args.length !== 0) fail("app.windows takes no arguments", "invalid_args");
        return call(op, {});
      }
      if (args.length !== 1) fail(`app.${op} takes one exact argument object`, "invalid_args");
      return call(op, args[0]);
    };
    define(app, op, {value:freeze(method), enumerable:true});
  }
  define(globalThis, "app", {value:freeze(app), writable:false, configurable:false});
  define(globalThis, "state", {get:()=>root, set:()=>fail("state root cannot be replaced", "invalid_state"), configurable:false});
  const show = (value) => {
    if (typeof value === "string") return value;
    try { return encode(value, "console", 65536); }
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
  const reject = (error) => finish(rejectionEnvelope(error));
  // Captured and removed by Rust before user code can execute.
  globalThis.__reject_unhandled = rejectionEnvelope;
  globalThis.__settle = (promise) => apply(then, promise, [
    (value) => {
      accepting = false;
      try {
        const value_json = encode(value === undefined ? null : value, "result", 65536);
        const state_json = encode(root, "state", 65536);
        if (phaseViolation) fail("app calls are unavailable during script settlement", "invalid_phase");
        finish(envelope({ok:true, value_json, state_json}));
      } catch (error) { reject(error); }
    }, reject,
  ]);
})();
