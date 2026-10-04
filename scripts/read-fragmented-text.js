// Execute this body in the persistent desktop.mjs session. The trusted caller
// first stores an observed, authorized document/container Ref in state.document.
if (typeof state.document !== 'string' || !state.document) throw new Error('Set state.document to an observed document/container Ref.');
const root = state.document;
const objects = [];
const coverages = [];
let page = await dw.outline(root, {
  fields: ['role', 'parent', 'value_preview'],
  budget: {max_depth: 16, max_results: 256, max_visited_nodes: 256,
    max_text_runes: 384, max_output_bytes: 12000, read_deadline_ms: 5000}
});
let incomplete = false;
for (let pages = 0; ; pages++) {
  coverages.push(page.coverage);
  objects.push(...page.objects);
  if (!page.coverage.continuation) {
    incomplete ||= !page.coverage.complete;
    break;
  }
  if (pages >= 7) { incomplete = true; break; }
  page = await dw.next(page); // Same query; native scan can advance after result pages.
}
const byRef = new Map();
const children = new Map();
for (const object of objects) {
  if (byRef.has(object.ref)) { incomplete = true; continue; }
  byRef.set(object.ref, object);
  if (!children.has(object.parent)) children.set(object.parent, []);
  children.get(object.parent).push(object.ref); // AX sibling order, not string sort.
}
const blocks = [];
const redacted = [];
let block = {ref: root, fragments: []};
const seen = new Set();
function flush() { if (block.fragments.length) blocks.push(block); block = {ref: root, fragments: []}; }
function walk(ref) {
  if (seen.has(ref)) { incomplete = true; return; }
  seen.add(ref);
  const object = byRef.get(ref);
  if (!object) { incomplete = true; return; }
  const childRefs = children.get(ref) ?? [];
  // Preserve native blocks. Do not invent paragraph semantics when AX omits them.
  const boundary = object.role === 'paragraph' || object.role === 'heading' ||
    (object.parent === root && object.role === 'container');
  if (boundary) { flush(); block.ref = ref; }
  const value = object.value_preview;
  if (value?.status === 'redacted') redacted.push({ref, status: 'redacted'});
  if (object.role === 'text' && childRefs.length === 0) {
    if (value && Object.hasOwn(value, 'known')) {
      const text = value.known;
      // Native previews cap at 384 UTF-16 units; a surrogate boundary can cap
      // at 383. Conservatively retain a clipping marker, never claim full text.
      const clipped = text.length >= 383;
      incomplete ||= clipped;
      block.fragments.push({ref, text, ...(clipped ? {possibly_clipped: true} : {})});
    } else {
      incomplete = true;
      block.fragments.push({ref, status: value?.status ?? 'unknown'});
    }
  }
  for (const child of childRefs) walk(child);
  if (boundary) flush();
}
walk(root); flush();
state.fragmentedText = {target: root, blocks, redacted, incomplete, coverages};
print({target: root, blocks, redacted, incomplete});
