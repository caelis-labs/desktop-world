// No GUI: exercise DTW's adapter against the exact pinned upstream JS prelude.
// The host bridge records arguments; it never starts a driver or sends input.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import {execFileSync} from 'node:child_process';

const root = path.resolve(import.meta.dirname, '../..');
const source = path.resolve(process.argv[2]);
const pin = '2d2f99228d4d2508dc6fcc84c176b296e146714a';
assert.equal(execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], {encoding:'utf8'}).trim(), pin);
assert.equal(execFileSync('git', ['-C', source, 'status', '--porcelain'], {encoding:'utf8'}).trim(), '');
assert.ok(fs.readFileSync(path.join(root, 'cmd/dtw/Cargo.toml'), 'utf8').includes(pin));
const prelude = fs.readFileSync(path.join(source,
  'libs/cua-driver/rust/crates/cua-driver-core/src/script_tool/prelude.js'), 'utf8');
const adapter = fs.readFileSync(path.join(root, 'cmd/dtw/src/click_contract.js'), 'utf8');
let passed = 0;

for (const platform of ['mac', 'windows']) {
  const calls = [];
  const context = vm.createContext({
    __host(op, data) {
      const args = JSON.parse(data);
      calls.push({op, args});
      let result = {pid: 123, window_id: 456};
      if (op === 'platform') result = platform;
      return JSON.stringify({ok:true, result});
    },
    __done() {}, __log() {},
  });
  vm.runInContext(prelude + '\n' + adapter, context);
  const run = (body) => vm.runInContext(
    `(async (cua) => {${body}})(__dtw_click_api(cua))`, context);
  for (const binding of ['getApp({pid:123,windowId:456})', 'launch("fixture")']) {
    for (const alias of ['modifier', 'modifiers']) {
      for (const method of ['click', 'rightClick']) {
        const options = `{${alias}:["ctrl"],delivery_mode:"foreground"}`;
        await run(`const app = await cua.${binding}; await app.${method}([10,20],${options});`);
        const last = calls.at(-1);
        assert.equal(last.op, method === 'click' ? 'click' : 'right_click');
        const window = binding.startsWith('getApp') ? {window_id:456} : {};
        assert.deepEqual(last.args, {pid:123, ...window, x:10, y:20,
          modifier:['ctrl'], delivery_mode:'foreground'});
        passed++;
      }
    }
  }
  await run(`const app = await cua.getApp({pid:123,windowId:456});
    await app.click([10,20],{button:"right",capture_id:"fresh"});`);
  assert.deepEqual(calls.at(-1).args,
    {pid:123,window_id:456,x:10,y:20,button:'right',capture_id:'fresh'});
  passed++;
  await run('await cua.call("click",{pid:123,x:10,y:20,modifiers:["shift"]});');
  assert.deepEqual(calls.at(-1).args, {pid:123,x:10,y:20,modifier:['shift']});
  passed++;
  await run('const app = await cua.getApp({pid:123}); await app.doubleClick([10,20]);');
  assert.deepEqual(calls.at(-1), {op:'double_click',args:{pid:123,x:10,y:20}});
  passed++;

  const invalid = [
    ['click','[10,20]','{modifier:"ctrl"}'],
    ['click','[10,20]','{modifiers:"ctrl"}'],
    ['click','[10,20]','{modifiers:["ctrl",42]}'],
    ['click','[10,20]','{modifiers:["typo"]}'],
    ['click','[10,20]','{modifiers:["ctrl"],modifier:["shift"]}'],
    ['click','[10,20]','{capture_id:""}'],
    ['click','{name:"fixture"}','{capture_id:"fresh"}'],
    ['click','[10,20]','{capture_id:"fresh",from_zoom:true}'],
    ['rightClick','[10,20]','{capture_id:"fresh"}'],
    ['rightClick','[10,20]','{from_zoom:true}'],
    ['rightClick','[10,20]','{button:"right"}'],
    ['rightClick','{name:"fixture"}','{modifiers:["ctrl"]}'],
    ['doubleClick','[10,20]','{modifiers:["ctrl"]}'],
    ['doubleClick','[10,20]','{modifier:[]}'],
    ['doubleClick','[10,20]','{capture_id:"fresh"}'],
    ['doubleClick','[10,20]','{from_zoom:true}'],
    ['doubleClick','[10,20]','{button:"right"}'],
    ['click','[10,20]','null'],
    ['click','[10,20]','[]'],
  ];
  for (const [method,target,options] of invalid) {
    const before = calls.length;
    await assert.rejects(run(`const app = await cua.getApp({pid:123});
      await app.${method}(${target},${options});`), /DTW /);
    assert.deepEqual(calls.slice(before).map(c=>c.op), ['get_app']);
    passed++;
  }
  for (const [tool,args] of [
    ['click','{x:10,y:20,modifier:"ctrl"}'],
    ['right_click','{x:10,y:20,capture_id:"fresh"}'],
    ['double_click','{x:10,y:20,modifiers:["ctrl"]}'],
  ]) {
    const before = calls.length;
    await assert.rejects(run(`await cua.call(${JSON.stringify(tool)},${args});`), /DTW /);
    assert.equal(calls.length, before);
    passed++;
  }
}
console.log(JSON.stringify({pin, passed, evidence:'pinned prelude + recording host; no native input'}));
