// Build the same portable payload on native macOS arm64 and Windows amd64.
// Called by platform packaging scripts after verified official Node download.
import { cp, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const [version, platform, flavor, helper, nodeBinary, nodeLicense, nodeArchiveSHA, output] = process.argv.slice(2);
if (!/^v\d+\.\d+\.\d+$/.test(version) || !['darwin-arm64', 'windows-amd64'].includes(platform) || !['full', 'lite'].includes(flavor) || !helper || !nodeBinary || !nodeLicense || !/^[a-f0-9]{64}$/i.test(nodeArchiveSHA ?? '') || !output) throw new Error('usage: build-plugin-package.mjs vX.Y.Z PLATFORM full|lite HELPER NODE NODE_LICENSE NODE_ARCHIVE_SHA256 OUTPUT');
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = resolve(output);
const revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repo, encoding: 'utf8' }).trim();
const helperVersion = JSON.parse(execFileSync(helper, ['version'], { encoding: 'utf8' }));
if (helperVersion.version !== version || helperVersion['vcs.revision'] !== revision || helperVersion['vcs.modified'] !== 'false') throw new Error('Native helper version, revision or clean source does not match this package.');
if (`${helperVersion.os}-${helperVersion.arch}` !== platform) throw new Error('Native helper platform does not match package.');
const nodeVersion = execFileSync(nodeBinary, ['--version'], { encoding: 'utf8' }).trim();
if (nodeVersion !== 'v24.21.0') throw new Error(`Expected pinned official Node v24.21.0, got ${nodeVersion}`);

const copy = (from, to) => cp(from, join(out, to), { recursive: true });
await mkdir(out, { recursive: true });
await copy(helper, join('bin', platform === 'windows-amd64' ? 'dtw.exe' : 'dtw'));
if (flavor === 'full') {
  await copy(nodeBinary, join('runtime', platform === 'windows-amd64' ? 'node.exe' : 'node'));
  await copy(nodeLicense, join('runtime', 'NODE-LICENSE'));
}
await copy(join(repo, 'clients/mcp/dist/mcp'), 'mcp');
await copy(join(repo, 'clients/mcp/dist/clients'), 'clients');
await copy(join(repo, 'packaging/plugin/skills'), 'skills');
await copy(join(repo, 'docs/scripting.md'), 'skills/desktop-world/references/scripting.md');
for (const path of ['skills/desktop-world/SKILL.md', 'skills/desktop-world/references/scripting.md']) {
  const file = join(out, path);
  await writeFile(file, (await readFile(file, 'utf8')).replaceAll('\r\n', '\n'));
}
await copy(join(repo, 'packaging/plugin/README.md'), 'README.md');
await copy(join(repo, 'docs/scripting.md'), 'docs/scripting.md');
for (const name of ['LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES.md']) await copy(join(repo, name), name);
for (const [name, path] of [
  ['MCP-SERVER-LICENSE', 'clients/mcp/node_modules/@modelcontextprotocol/server/LICENSE'],
  ['MCP-CORE-LICENSE', 'clients/mcp/node_modules/@modelcontextprotocol/core/LICENSE'],
  ['ZOD-LICENSE', 'clients/mcp/node_modules/zod/LICENSE'],
]) await copy(join(repo, path), join('licenses', name));

const plugin = JSON.parse(await readFile(join(repo, 'packaging/plugin/plugin.json'), 'utf8'));
plugin.version = version.slice(1);
const mcp = JSON.parse(await readFile(join(repo, 'packaging/plugin/mcp.json'), 'utf8'));
const server = mcp.mcpServers['desktop-world'];
if (flavor === 'full') server.command = platform === 'windows-amd64' ? './runtime/node.exe' : './runtime/node';
else { server.command = platform === 'windows-amd64' ? './bin/dtw.exe' : './bin/dtw'; server.args = ['plugin-node', '--data-dir', '${PLUGIN_DATA}']; }
await writeFile(join(out, 'plugin.json'), JSON.stringify(plugin, null, 2) + '\n');
await writeFile(join(out, 'mcp.json'), JSON.stringify(mcp, null, 2) + '\n');
const hashFile = async path => createHash('sha256').update(await readFile(path)).digest('hex');
const manifest = {
  version, source_revision: revision, platform, flavor, helper: helperVersion,
  node: flavor === 'full'
    ? { mode: 'bundled', version: nodeVersion, archive_sha256: nodeArchiveSHA.toLowerCase(), binary_sha256: await hashFile(nodeBinary) }
    : { mode: 'external', required: 'Node 24.x', path_environment: 'DTW_NODE_PATH' },
  mcp_sdk: '@modelcontextprotocol/server@2.3.1',
  agent_plugins_spec: '1.0.0',
  signing: platform === 'windows-amd64' ? 'unsigned' : 'check codesign and notarization separately',
  native_gui_acceptance: 'requires exact-archive interactive validation on this platform',
};
await writeFile(join(out, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
await writeFile(join(out, 'SOURCE.txt'), `Source: https://github.com/caelis-labs/desktop-world\nRevision: ${revision}\nRelease: ${version}\nGitHub provides source archives for the matching release tag.\n`);
async function files(dir) {
  const result = [];
  for (const item of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, item.name);
    if (item.isDirectory()) result.push(...await files(path));
    else if (item.isFile()) result.push(path);
    else throw new Error(`Package contains unsupported filesystem entry: ${path}`);
  }
  return result;
}
const lines = [];
for (const path of (await files(out)).sort()) lines.push(`${await hashFile(path)}  ${relative(out, path).replaceAll('\\', '/')}`);
await writeFile(join(out, 'SHA256SUMS'), lines.join('\n') + '\n');
console.log(JSON.stringify({ package: out, version, platform, flavor, revision, files: lines.length }));
