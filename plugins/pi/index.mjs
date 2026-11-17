import { fileURLToPath } from 'node:url';

export default function (pi) {
  const root = fileURLToPath(new URL('../', import.meta.url));
  const binary = fileURLToPath(new URL(
    process.platform === 'win32' ? '../bin/dtw.exe' : '../bin/dtw', import.meta.url));

  pi.registerMcpServer('dtw', {
    command: binary,
    args: ['mcp'],
    cwd: root,
    exposure: 'direct',
  });
}
