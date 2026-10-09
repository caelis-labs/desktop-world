import Ajv2020 from 'ajv/dist/2020.js';
import { readFile, lstat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(process.argv[2] ?? '');
if (!process.argv[2]) throw new Error('usage: node verify-package.mjs PLUGIN_ROOT');
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const ajv = new Ajv2020({ strict: true, allErrors: true });
for (const component of ['plugin', 'mcp']) {
  const schema = JSON.parse(await readFile(join(repo, 'packaging', 'schema', `${component}.schema.json`), 'utf8'));
  const data = JSON.parse(await readFile(join(root, `${component}.json`), 'utf8'));
  const valid = ajv.compile(schema);
  if (!valid(data)) throw new Error(`${component}.json: ${ajv.errorsText(valid.errors)}`);
}
const mcp = JSON.parse(await readFile(join(root, 'mcp.json'), 'utf8'));
const entries = Object.entries(mcp.mcpServers);
if (entries.length !== 1 || entries[0][0] !== 'desktop-world') throw new Error('Expected exactly one desktop-world stdio server.');
const config = entries[0][1];
if (!config.command.startsWith('./') || config.command.includes('${') || config.args[0] !== '${PLUGIN_ROOT}/mcp/server.mjs' || config.cwd !== '${PLUGIN_DATA}') throw new Error('Invalid portable command, args or cwd.');
if (!(await lstat(join(root, config.command.slice(2)))).isFile()) throw new Error('Bundled Node executable missing.');
const skill = await readFile(join(root, 'skills/desktop-world/SKILL.md'), 'utf8');
if (!skill.startsWith('---\nname: desktop-world\n') || !skill.includes('\ndescription: ')) throw new Error('Plugin Skill is not discoverable.');
const lines = (await readFile(join(root, 'SHA256SUMS'), 'utf8')).trim().split('\n');
for (const line of lines) {
  const match = /^([a-f0-9]{64})  ([^\n]+)$/.exec(line);
  if (!match || match[2].startsWith('/') || match[2].includes('..')) throw new Error(`Invalid SHA256SUMS entry: ${line}`);
  const path = join(root, match[2]);
  const info = await lstat(path);
  if (!info.isFile()) throw new Error(`Not a regular file: ${match[2]}`);
  const hash = createHash('sha256').update(await readFile(path)).digest('hex');
  if (hash !== match[1]) throw new Error(`SHA256 mismatch: ${match[2]}`);
}
console.log(JSON.stringify({ valid: true, root, files: lines.length, schema: 'Agent Plugins 1.0.0' }));
