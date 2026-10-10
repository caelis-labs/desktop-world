package main

// worldJS is the small Agent-facing object layer of the isolated POC. It
// compiles methods to the existing native requests; native identity, grants,
// scheduling and receipts remain authoritative. The address book lives only
// in this QuickJS Session.
const worldJS = `(function () {
const book = () => {
  const epoch = state._dtwDisclosure?.epoch;
  let b = state._dtwWorld;
  if (!b || b.epoch !== epoch) {
    b = {epoch, items: new Map()};
    Object.defineProperty(state, '_dtwWorld', {value:b, writable:true, configurable:true});
  }
  return b;
};
const nameOf = o => o?.name?.status === 'known' ? o.name.value : undefined;
const clean = ob => ob?.coverage?.complete === true && ob.coverage.dirty !== true &&
  ob.coverage.truncated !== true && !ob.coverage.continuation &&
  !(ob.coverage.unavailable_sources?.length);
const requireClean = (ob, label) => {
  if (!clean(ob)) throw Error(label + ' unresolved: observation incomplete; narrow or refresh');
};
const budget = {max_results:16,max_visited_nodes:1200,max_depth:14,read_deadline_ms:5000};
const list = items => {
  Object.defineProperty(items, 'toString', {value:() => items.length ?
    items.map(item => String(item)).join('\n') : '(none)'});
  return items;
};
const idFor = (ob, source) => {
  dtw.disclose(ob, {fields:['kind','role','name','capabilities'],max_items:16});
  const id = state._dtwDisclosure?.refToAlias.get(source.ref);
  if (!id || !state._dtwDisclosure.actionableAliases.has(id))
    throw Error('target not available in a clean disclosure');
  return id;
};
const remember = (id, source) => { book().items.set(id, source); return id; };
const available = o => (o.capabilities ?? []).filter(c =>
  c.support === 'supported' && c.availability === 'available').map(c => c.name);
const words = {setValue:'set_value',setChecked:'set_checked',setSelected:'set_selected',
  setExpanded:'set_expanded',scrollIntoView:'scroll_into_view',move:'pointer.move',
  click:'pointer.click',dragTo:'pointer.drag',scroll:'pointer.scroll',
  press:'keyboard.press',typeText:'keyboard.type_text',focus:'focus',invoke:'invoke'};
const argsFor = (method, args) => {
  switch (method) {
    case 'setValue': case 'typeText':
      if (typeof args[0] !== 'string') throw TypeError(method + ' needs text');
      return {[method === 'setValue' ? 'set_value' : 'type_text']:{text:args[0]}};
    case 'setChecked': case 'setSelected': case 'setExpanded':
      if (typeof args[0] !== 'boolean') throw TypeError(method + ' needs a boolean');
      return {[words[method]]:{[method === 'setChecked' ? 'checked' :
        method === 'setSelected' ? 'selected' : 'expanded']:args[0]}};
    case 'click':
      return {click:{button:args[0]?.button ?? 'left',count:args[0]?.count ?? 1}};
    case 'dragTo':
      if (!(args[0] instanceof Element)) throw TypeError('dragTo needs an Element');
      return {drag:{to:{id:args[0].id},duration_ms:args[1]?.durationMs ?? 250}};
    case 'scroll':
      return {scroll:{dx:args[0]?.dx ?? 0,dy:args[0]?.dy ?? 1,unit:'wheel_step'}};
    case 'press': {
      if (typeof args[0] !== 'string' || !args[0]) throw TypeError('press needs a key');
      return {press:{key:args[0],modifiers:args[1] ?? []}};
    }
    default: return {};
  }
};
const stepFor = (target, method, args, number) => {
  if (!words[method]) throw TypeError('unknown DTW behavior: ' + method);
  const source = book().items.get(target.id);
  if (!source || dtw.ref(target.id) !== source.ref)
    throw Error('target address expired; observe again');
  if (['invoke','setValue','setChecked','setSelected','setExpanded','scrollIntoView'].includes(method) &&
      !available(source).includes(words[method]))
    throw Error(target.id + '.' + method + ' unavailable; observe capabilities again');
  return {id:target.id + '.' + method + (number ? '#' + number : ''),
    op:words[method],target:{id:target.id},...argsFor(method,args)};
};
class Element {
  constructor(id) { this.id = id; }
  get name() { return nameOf(book().items.get(this.id)); }
  get role() { return book().items.get(this.id)?.role; }
  get behaviors() { return available(book().items.get(this.id) ?? {}).map(native =>
    Object.keys(words).find(k => words[k] === native)).filter(Boolean); }
  toString() {
    const source = book().items.get(this.id);
    if (!source || dtw.ref(this.id) !== source.ref) throw Error('target address expired');
    const label = nameOf(source) ?? '(unnamed)';
    const methods = this.behaviors;
    return this.id + ' ' + label + ' · ' + (source.role ?? source.kind) +
      (methods.length ? ' · ' + methods.join(', ') : '');
  }
  async read(property='value') {
    const source = book().items.get(this.id);
    if (!source || dtw.ref(this.id) !== source.ref) throw Error('target address expired');
    if (property === 'value') return dtw.read({target_id:this.id,limit_runes:2048});
    if (!['name','role','states','checked','selected','expanded'].includes(property))
      throw TypeError('unsupported read property');
    const ob = await dtw.observe({scope:{ids:[this.id]},projection:'detail',
      fields:['name','role','states'],budget:{max_results:1,read_deadline_ms:3000}});
    requireClean(ob, 'read');
    const found = ob.objects.find(o => o.ref === source.ref);
    if (!found) throw Error('target no longer exists');
    if (['checked','selected','expanded'].includes(property)) {
      if (!Object.prototype.hasOwnProperty.call(found.states ?? {},property))
        throw Error(property + ' is not exposed by this control');
      return found.states[property];
    }
    return found[property];
  }
  async _do(method, args) { return dtw.act({steps:[stepFor(this,method,args,0)]}); }
}
for (const method of Object.keys(words)) Element.prototype[method] = function (...args) {
  return this._do(method,args);
};
class Window extends Element {
  async find(query={}) {
    if (query.name !== undefined && query.nameContains !== undefined)
      throw TypeError('choose exact name or nameContains');
    if (query.name !== undefined && (typeof query.name !== 'string' || !query.name))
      throw TypeError('name must be nonempty text');
    if (query.nameContains !== undefined && (typeof query.nameContains !== 'string' || !query.nameContains))
      throw TypeError('nameContains must be nonempty text');
    const ob = await dtw.observe({scope:{ids:[this.id]},projection:'outline',
      fields:['name','role','capabilities','value_preview'],
      match:{within_id:this.id,...(query.name ? {name_equals:query.name} : {}),
        ...(query.nameContains ? {name_contains:query.nameContains} : {}),
        ...(query.role ? {role:query.role} : {})},budget});
    requireClean(ob, 'control');
    const hits = ob.objects.filter(o => o.kind === 'ui' &&
      (!query.name || nameOf(o) === query.name) &&
      (!query.nameContains || nameOf(o)?.includes(query.nameContains)) &&
      (!query.role || o.role === query.role));
    return list(hits.map(o => new Element(remember(idFor(ob,o),o))));
  }
  async one(query) {
    if (!query || typeof query.name !== 'string' || !query.name)
      throw TypeError('one needs an exact name');
    const hits = await this.find(query);
    if (hits.length !== 1) throw Error('control match count ' + hits.length + '; specify an exact unique target');
    return hits[0];
  }
  async capture() { return dtw.capture({kind:'window_content',target_id:this.id}); }
}
class App extends Element {
  async windows() {
    const ob = await dtw.observe({scope:{ids:[this.id]},projection:'summary',
      fields:['name','role','app','capabilities'],
      budget:{max_results:16,max_visited_nodes:512,max_depth:3,read_deadline_ms:4000}});
    requireClean(ob, 'windows');
    const own = dtw.ref(this.id);
    return list(ob.objects.filter(o => o.kind === 'window' && o.app === own)
      .map(o => new Window(remember(idFor(ob,o),o))));
  }
  async window(title) {
    if (typeof title !== 'string' || !title) throw TypeError('window needs an exact title');
    const ob = await dtw.observe({scope:{ids:[this.id]},projection:'outline',
      fields:['name','role','app','capabilities'],
      match:{within_id:this.id,name_equals:title},budget});
    requireClean(ob, 'window');
    const hits = ob.objects.filter(o => o.kind === 'window' && o.app === dtw.ref(this.id) && nameOf(o) === title);
    if (hits.length !== 1) throw Error('window match count ' + hits.length + '; select one exact title');
    return new Window(remember(idFor(ob,hits[0]),hits[0]));
  }
}
dtw.at = id => {
  if (typeof id !== 'string' || !book().items.has(id)) throw Error('unknown or expired DTW address');
  const source = book().items.get(id);
  if (dtw.ref(id) !== source.ref) throw Error('DTW address expired');
  return source.kind === 'application' ? new App(id) : source.kind === 'window' ? new Window(id) : new Element(id);
};
dtw.app = async name => {
  if (typeof name !== 'string' || !name) throw TypeError('app needs an exact name');
  const grants = await dtw.grants();
  const refs = [...new Set((grants.grants ?? []).filter(g => g.state === 'active' && g.application &&
    (!g.name || g.name === name)).map(g => g.application))];
  let found = [];
  for (const ref of refs) {
    const ob = await dtw.observe({scope:{refs:[ref]},projection:'detail',
      fields:['name','role','app'],budget:{max_results:1,read_deadline_ms:3000}});
    if (clean(ob)) found.push(...ob.objects.filter(o => o.kind === 'application' && o.ref === ref && nameOf(o) === name).map(o => ({ob,o})));
  }
  if (found.length === 0) {
    const ob = await dtw.observe({scope:{desktop:true},projection:'summary',fields:['name','role','app'],
      budget:{max_results:32,max_visited_nodes:10000,max_depth:3,read_deadline_ms:4000}});
    requireClean(ob, 'app');
    found = ob.objects.filter(o => o.kind === 'application' && nameOf(o) === name).map(o => ({ob,o}));
  }
  if (found.length !== 1) throw Error('app match count ' + found.length + '; select one exact instance');
  return new App(remember(idFor(found[0].ob,found[0].o),found[0].o));
};
dtw.transaction = async build => {
  if (typeof build !== 'function') throw TypeError('transaction needs a synchronous builder');
  const steps = [];
  const tx = {};
  for (const method of Object.keys(words)) tx[method] = (target,...args) => {
    if (!(target instanceof Element)) throw TypeError(method + ' needs an Element');
    if (steps.length >= 16) throw Error('transaction step limit exceeded');
    steps.push(stepFor(target,method,args,steps.length + 1));
  };
  const returned = build(tx);
  if (returned && typeof returned.then === 'function') throw TypeError('transaction builder must be synchronous');
  if (!steps.length) throw Error('empty transaction');
  return dtw.act({steps});
};
})();
`
