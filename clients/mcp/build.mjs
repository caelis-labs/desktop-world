import { build } from 'esbuild';
import { cp, mkdir, rm } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, 'dist');
await rm(out, { recursive: true, force: true });
await mkdir(join(out, 'mcp'), { recursive: true });
await build({ entryPoints: [join(here, 'server.mjs')], outfile: join(out, 'mcp', 'server.mjs'), bundle: true, platform: 'node', format: 'esm', target: 'node24', packages: 'bundle', legalComments: 'external' });
await cp(join(here, 'worker.mjs'), join(out, 'mcp', 'worker.mjs'));
await cp(resolve(here, '../javascript'), join(out, 'clients', 'javascript'), { recursive: true });
await cp(resolve(here, '../typescript/runtime'), join(out, 'clients', 'typescript', 'runtime'), { recursive: true });
