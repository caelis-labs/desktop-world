package main

// POC presentation helpers. Native observations, full Refs, times and receipts
// stay in the script/native record; these helpers only shape what is printed.
// Their cache belongs to one QuickJS Session, never to the MCP server globally.
const disclosureJS = `(function () {
const disclosureCache = epoch => {
  let cache = state._dtwDisclosure;
  if (!cache || cache.epoch !== epoch) {
    cache = {epoch, refs: new Map(), aliasToRef: new Map(), refToAlias: new Map(), actionableAliases: new Set(),
      windowIds: new Map(), windowBounds: new Map(), counters: new Map(),
      nextWindow: cache?.nextWindow ?? 0, nextApp: cache?.nextApp ?? 0,
      nextUnbound: cache?.nextUnbound ?? 0};
    Object.defineProperty(state, '_dtwDisclosure', {value: cache, writable: true, configurable: true});
  }
  return cache;
};
const rememberAlias = (cache, alias, ref) => {
  cache.aliasToRef.set(alias, ref);
  cache.refToAlias.set(ref, alias);
  while (cache.aliasToRef.size > 512) {
    const oldest = cache.aliasToRef.keys().next().value;
    const oldRef = cache.aliasToRef.get(oldest);
    cache.aliasToRef.delete(oldest);
    cache.actionableAliases.delete(oldest);
    if (cache.refToAlias.get(oldRef) === oldest) cache.refToAlias.delete(oldRef);
  }
  return alias;
};
const windowAlias = (cache, ref) => {
  let alias = cache.windowIds.get(ref);
  if (!alias) {
    alias = 'W' + (++cache.nextWindow);
    cache.windowIds.set(ref, alias);
  }
  if (!cache.aliasToRef.has(alias)) rememberAlias(cache, alias, ref);
  return alias;
};
const objectAlias = (cache, ob, source) => {
  const known = cache.refToAlias.get(source.ref);
  if (known) return known;
  if (source.kind === 'window') return windowAlias(cache, source.ref);
  if (source.kind === 'application')
    return rememberAlias(cache, 'A' + (++cache.nextApp), source.ref);
  let owner = source.window;
  if (!owner && ob.coverage?.scope?.refs?.length === 1 &&
      cache.windowIds.has(ob.coverage.scope.refs[0])) owner = ob.coverage.scope.refs[0];
  let prefix;
  if (owner) prefix = windowAlias(cache, owner);
  else if (source.app) {
    prefix = cache.refToAlias.get(source.app);
    if (!prefix) prefix = rememberAlias(cache, 'A' + (++cache.nextApp), source.app);
  } else prefix = 'U' + (++cache.nextUnbound);
  const role = source.role ?? '';
  const family = role === 'text' || role === 'text_field' ? 'T' :
    role === 'button' ? 'B' : role === 'container' ? 'R' : 'N';
  const counterKey = prefix + '/' + family;
  const number = (cache.counters.get(counterKey) ?? 0) + 1;
  cache.counters.set(counterKey, number);
  return rememberAlias(cache, prefix + '/' + family + number, source.ref);
};
const indexObservation = ob => {
  if (!ob || !Array.isArray(ob.objects) || !ob.coverage)
    throw new TypeError('index requires one native observation');
  const cache = disclosureCache(ob.epoch);
  for (const source of ob.objects) {
    if (typeof source?.ref !== 'string' || !source.ref) continue;
    objectAlias(cache, ob, source);
    if (source.kind === 'window') {
      const rect = source.bounds?.value?.rect;
      if (rect && rect.width > 0 && rect.height > 0)
        cache.windowBounds.set(source.ref, rect);
    }
  }
  return cache;
};
dtw.index = ob => { indexObservation(ob); return ob.objects.length; };
dtw.ref = alias => {
  const ref = state._dtwDisclosure?.aliasToRef.get(alias);
  if (!ref) throw new Error('unknown or expired display alias');
  return ref;
};
const rawObserve = dtw.observe, rawRead = dtw.read, rawCapture = dtw.capture, rawAct = dtw.act;
const resolveTarget = target => {
  if (!target || !Object.prototype.hasOwnProperty.call(target, 'id')) return target;
  if (typeof target.id !== 'string' || Object.keys(target).length !== 1)
    throw new TypeError('display target must contain only one id');
  if (!state._dtwDisclosure?.actionableAliases.has(target.id))
    throw new Error('display alias needs one clean complete observation before an action');
  return {ref: dtw.ref(target.id)};
};
dtw.observe = args => {
  if (!args || typeof args !== 'object') return rawObserve(args);
  let scope = args.scope, match = args.match;
  if (scope?.ids !== undefined) {
    if (scope.refs !== undefined || !Array.isArray(scope.ids))
      throw new TypeError('scope.ids cannot mix with scope.refs');
    scope = {...scope, refs: scope.ids.map(dtw.ref)};
    delete scope.ids;
  }
  if (match?.within_id !== undefined) {
    if (match.within !== undefined) throw new TypeError('within_id cannot mix with within');
    match = {...match, within: dtw.ref(match.within_id)};
    delete match.within_id;
  }
  return rawObserve({...args, scope, match});
};
dtw.read = args => {
  if (args?.target_id === undefined) return rawRead(args);
  if (args.target !== undefined) throw new TypeError('target_id cannot mix with target');
  const mapped = {...args, target: dtw.ref(args.target_id)};
  delete mapped.target_id;
  return rawRead(mapped);
};
dtw.capture = args => {
  if (args?.target_id === undefined) return rawCapture(args);
  if (args.target !== undefined) throw new TypeError('target_id cannot mix with target');
  const mapped = {...args, target: dtw.ref(args.target_id)};
  delete mapped.target_id;
  return rawCapture(mapped);
};
dtw.act = args => {
  if (!Array.isArray(args?.steps)) return rawAct(args);
  const steps = args.steps.map(step => ({...step,
    target: resolveTarget(step.target),
    drag: step.drag ? {...step.drag, to: resolveTarget(step.drag.to)} : step.drag,
    before: step.before?.map(p => ({...p, target: resolveTarget(p.target)})),
    after: step.after?.map(p => ({...p, target: resolveTarget(p.target)}))}));
  return rawAct({...args, steps});
};
const areaHint = (cache, ob, source) => {
  const bounds = source.bounds?.value?.rect;
  let owner = source.window;
  if (!owner && ob.coverage?.scope?.refs?.length === 1 &&
      cache.windowIds.has(ob.coverage.scope.refs[0])) owner = ob.coverage.scope.refs[0];
  const window = cache.windowBounds.get(owner);
  if (!bounds || !window || window.width <= 0 || window.height <= 0) return undefined;
  const x = (bounds.x + bounds.width / 2 - window.x) / window.width;
  const y = (bounds.y + bounds.height / 2 - window.y) / window.height;
  if (x < 0 || x > 1 || y < 0 || y > 1) return undefined;
  if (y < 0.16) return 'top';
  if (y > 0.88) return 'bottom';
  if (x < 0.26) return 'left';
  if (x > 0.78) return 'right';
  return 'main';
};
dtw.disclose = (ob, options = {}) => {
  const cache = indexObservation(ob);
  const allowed = new Set(['kind','role','name','value_preview','uri','states','bounds','capabilities','lifecycle']);
  const fields = options.fields ?? ['kind','role','name'];
  if (!Array.isArray(fields) || fields.length === 0 || fields.some(f => !allowed.has(f)))
    throw new TypeError('disclose fields must be an explicit supported list');
  const maxItems = options.max_items ?? 16;
  if (!Number.isInteger(maxItems) || maxItems < 1 || maxItems > 32)
    throw new RangeError('disclose max_items must be 1..32');
  if (options.refresh !== undefined && typeof options.refresh !== 'boolean')
    throw new TypeError('disclose refresh must be boolean');
  const stable = value => {
    if (Array.isArray(value)) return value.map(stable);
    if (value && typeof value === 'object') {
      const copy = {};
      for (const [key, part] of Object.entries(value)) {
        if (key === 'sampled_at' || key === 'sample_start' || key === 'sample_end' || key === 'observed_at') continue;
        copy[key] = stable(part);
      }
      return copy;
    }
    return value;
  };
  const items = [];
  let unchanged = 0, omitted = 0;
  const clean = ob.coverage.complete === true && ob.coverage.dirty !== true &&
    ob.coverage.truncated !== true && !ob.coverage.continuation;
  const visited = new Set();
  for (const source of ob.objects) {
    if (typeof source?.ref !== 'string' || !source.ref || visited.has(source.ref)) continue;
    visited.add(source.ref);
    const id = cache.refToAlias.get(source.ref) ?? objectAlias(cache, ob, source);
    if (!clean) cache.actionableAliases.delete(id);
    const old = cache.refs.get(source.ref) ?? {};
    const changed = {};
    const signatures = {};
    for (const field of fields) {
      if (!Object.prototype.hasOwnProperty.call(source, field)) continue;
      const normalized = stable(source[field]);
      const signature = JSON.stringify(normalized);
      if (options.refresh || old[field] !== signature) {
        changed[field] = normalized;
        signatures[field] = signature;
      }
    }
    if (Object.keys(changed).length === 0) {
      if (clean && !cache.actionableAliases.has(id)) {
        if (items.length >= maxItems) { omitted++; continue; }
        items.push({id});
        cache.actionableAliases.add(id);
        continue;
      }
      unchanged++;
      continue;
    }
    if (items.length >= maxItems) { omitted++; continue; }
    const item = {id, ...changed};
    const hint = areaHint(cache, ob, source);
    if (hint) item.area_hint = hint;
    items.push(item);
    if (clean) cache.actionableAliases.add(id);
    cache.refs.delete(source.ref);
    cache.refs.set(source.ref, {...old, ...signatures});
    while (cache.refs.size > 512) cache.refs.delete(cache.refs.keys().next().value);
  }
  const c = ob.coverage;
  return {
    items, matched: ob.objects.length, unchanged, omitted,
    coverage: {complete: c.complete, dirty: c.dirty, truncated: c.truncated,
      more: !!c.continuation, unavailable_sources: c.unavailable_sources ?? []}
  };
};
})();
`
