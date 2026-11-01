"""Plugin installation contracts; no host configuration or GUI is changed."""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).parents[1]
SPEC = importlib.util.spec_from_file_location("stage_agent_plugin", ROOT / "scripts/stage-agent-plugin.py")
stager = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stager)


class PluginStageTests(unittest.TestCase):
    def test_both_targets_keep_binary_identity_and_all_hosts_use_the_same_skill(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "original executable"
            binary.write_bytes(b"existing DTW binary bytes\x00\xff")
            binary.chmod(0o755)
            for target, name in stager.plugin_package.EXECUTABLES.items():
                output = root / (target + " 中文 path")
                _, metadata = stager.stage(binary, output, ROOT, target)
                self.assertEqual(list((output / "bin").iterdir()), [output / "bin" / name])
                self.assertEqual((output / "bin" / name).read_bytes(), binary.read_bytes())
                self.assertEqual(metadata["binary_sha256"], hashlib.sha256(binary.read_bytes()).hexdigest())
                self.assertEqual(metadata["binary"], "bin/" + name)
                self.assertEqual(metadata["distribution"], "local-development-stage")
                self.assertEqual((output / "skills/desktop-world/SKILL.md").read_bytes(),
                                 (ROOT / "skills/desktop-world/SKILL.md").read_bytes())
                self.assertEqual((output / "skills/desktop-world/PATTERNS.md").read_bytes(),
                                 (ROOT / "skills/desktop-world/PATTERNS.md").read_bytes())
                manifests = [json.loads((output / path).read_text(encoding="utf-8")) for path in
                             ("plugin.json", ".claude-plugin/plugin.json", ".antigravity-plugin/plugin.json")]
                self.assertEqual({manifest["name"] for manifest in manifests}, {"desktop-world"})
                self.assertEqual({manifest["version"] for manifest in manifests}, {metadata["version"]})
                self.assertNotEqual(manifests[0]["$schema"], manifests[2]["$schema"])
                self.assertEqual(json.loads((output / "mcp_config.json").read_text())["mcpServers"], {
                    "desktop-world": {"command":"./bin/" + name, "args":["mcp"], "cwd":"${PLUGIN_ROOT}"}})
                for guide in ("install.md", "docs/install-caelis.md", "docs/install-dsh.md", "docs/install-agy.md"):
                    self.assertEqual((output / guide).read_bytes(), (ROOT / guide).read_bytes())
                before = (output / "plugin.json").read_bytes()
                with self.assertRaises(FileExistsError):
                    stager.stage(binary, output, ROOT, target)
                self.assertEqual((output / "plugin.json").read_bytes(), before)

    @unittest.skipUnless(shutil.which("node"), "Cordis adapter runs inside the DSH Node host")
    def test_cordis_resolves_installed_components_and_paths_without_installing_dependencies(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "existing executable"
            binary.write_bytes(b"already built")
            binary.chmod(0o755)
            output = root / "中文 plugin with spaces"
            target = stager.plugin_package.host_target("Windows" if os.name == "nt" else "Darwin")
            stager.stage(binary, output, ROOT, target)
            script = r"""
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import path from 'node:path';
const root = process.argv[1];
const adapter = await import(pathToFileURL(path.join(root, 'cordis/index.mjs')).href);
const imports = [], children = [];
const context = {
  fiber: { uid: 1 },
  on: () => {},
  loader: {
    import: async name => { imports.push(name); return { name }; },
    unwrapExports: module => module,
  },
  plugin: (module, config) => {
    children.push({ name: module.name, config });
    return Object.assign(Promise.resolve(), { dispose: async () => {} });
  },
};
await adapter.apply(context);
assert.equal(children.length, 2);
const skill = children.find(x => x.name === '@deepseek-ai/dsh-skill-filesystem').config;
assert.equal(skill.providerName, 'desktop-world');
assert.equal(skill.includeDefaultRoots, false);
assert.deepEqual(skill.customSkillDirs.map(x => path.resolve(x)), [path.join(root, 'skills')]);
const mcp = children.find(x => x.name === '@deepseek-ai/dsh-mcp-client').config;
assert.equal(mcp.command, path.join(root, 'bin', process.platform === 'win32' ? 'dtw.exe' : 'dtw'));
assert.equal(path.resolve(mcp.cwd), root);
assert.deepEqual(mcp.args, ['mcp']);
assert.equal(mcp.serverName, 'desktop-world');
assert.equal(mcp.failOnStartupError, true);
assert.deepEqual(imports.sort(), ['@deepseek-ai/dsh-mcp-client', '@deepseek-ai/dsh-skill-filesystem']);
const failedChildren = [];
await assert.rejects(adapter.apply({
  fiber: { uid: 1 }, on: () => {},
  loader: { import: async () => { throw new Error('Host lacks component'); }, unwrapExports: x => x },
  plugin: (...args) => failedChildren.push(args),
}), /Host lacks component/);
assert.equal(failedChildren.length, 0);
console.log('Cordis package contracts passed');
"""
            result = subprocess.run([shutil.which("node"), "--input-type=module", "-e", script, str(output)],
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, encoding="utf-8", timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("Cordis package contracts passed", result.stdout)
            self.assertFalse((output / "node_modules").exists())

    @unittest.skipUnless(shutil.which("node"), "Cordis adapter runs inside the DSH Node host")
    def test_cordis_awaits_startup_rolls_back_failure_and_stops_pending_negotiation(self):
        script = r"""
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
const adapter = await import(pathToFileURL(process.argv[1]).href);
const tick = () => new Promise(resolve => setImmediate(resolve));
function fixture() {
  const owner = { uid: 1 }, children = [], listeners = [];
  const context = {
    fiber: owner,
    on: (name, callback) => { if (name === 'internal/plugin') listeners.push(callback); },
    loader: { import: async name => ({ name }), unwrapExports: module => module },
    plugin: module => {
      let resolve, reject;
      const ready = new Promise((yes, no) => { resolve = yes; reject = no; });
      const child = { name: module.name, live: true, stops: 0, resolve, reject,
        then: ready.then.bind(ready),
        dispose: async () => { child.stops++; child.live = false; reject(new Error('negotiation stopped')); },
      };
      children.push(child);
      return child;
    },
  };
  return { context, owner, children, stop: async () => {
    owner.uid = null;
    await Promise.all(listeners.map(callback => callback(owner)));
  } };
}
{
  const f = fixture(); let settled = false;
  const pending = adapter.apply(f.context).finally(() => { settled = true; });
  await tick(); assert.equal(f.children.length, 2); assert.equal(settled, false);
  f.children[0].resolve(); await tick(); assert.equal(settled, false);
  f.children[1].resolve(); await pending; assert.equal(settled, true);
  await f.stop(); assert.ok(f.children.every(child => !child.live && child.stops === 1));
}
{
  const f = fixture(); const pending = adapter.apply(f.context);
  const rejected = assert.rejects(pending, /MCP failed to negotiate/);
  await tick(); f.children[0].resolve(); f.children[1].reject(new Error('MCP failed to negotiate'));
  await rejected; assert.ok(f.children.every(child => !child.live && child.stops === 1));
}
{
  const f = fixture(); const pending = adapter.apply(f.context);
  await tick(); f.children[0].resolve();
  await f.stop(); await pending;
  assert.ok(f.children.every(child => !child.live && child.stops === 1));
}
console.log('Cordis startup and disposal contracts passed');
"""
        result = subprocess.run([shutil.which("node"), "--input-type=module", "-e", script,
                                 str(ROOT / "plugins/cordis/index.mjs")], stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, encoding="utf-8", timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Cordis startup and disposal contracts passed", result.stdout)


if __name__ == "__main__":
    unittest.main()
