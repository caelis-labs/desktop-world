/** Mount the host's existing MCP bridge and an isolated bundled Skill provider. */
import { fileURLToPath } from 'node:url';

export const name = 'desktop-world';
export const inject = ['loader', 'tools', 'skills'];

export async function apply(ctx) {
  const children = [];
  let stopping;
  const stopChildren = () => stopping ??= Promise.allSettled(
    children.map(child => Promise.resolve().then(() => child.dispose())),
  ).then(results => {
    const failed = results.find(result => result.status === 'rejected');
    if (failed) throw failed.reason;
  });
  // A parent may be disabled while awaiting a child's MCP negotiation. Start
  // child disposal at that notification so negotiation can settle before the
  // parent awaits its normal Cordis-owned effect cleanup.
  ctx.on('internal/plugin', fiber => {
    if (fiber === ctx.fiber && fiber.uid === null) return stopChildren();
  }, { global: true });

  // Use the host loader, not a second npm copy: duplicate MCP namespace checks
  // and disposal must belong to the same implementation as the other servers.
  const [mcpClient, skillFilesystem] = await Promise.all([
    ctx.loader.import('@deepseek-ai/dsh-mcp-client'),
    ctx.loader.import('@deepseek-ai/dsh-skill-filesystem'),
  ]).then(modules => modules.map(module => ctx.loader.unwrapExports(module)));
  if (ctx.fiber.uid === null) return;
  const root = fileURLToPath(new URL('../', import.meta.url));
  const binary = fileURLToPath(new URL(
    process.platform === 'win32' ? '../bin/dtw.exe' : '../bin/dtw', import.meta.url));
  const skills = fileURLToPath(new URL('../skills/', import.meta.url));

  try {
    children.push(ctx.plugin(skillFilesystem, {
      providerName: 'desktop-world',
      includeDefaultRoots: false,
      customSkillDirs: [skills],
      watch: false,
    }));
    children.push(ctx.plugin(mcpClient, {
      serverName: 'desktop-world',
      transport: 'stdio',
      command: binary,
      args: ['mcp'],
      cwd: root,
      failOnStartupError: true,
    }));
    // ctx.plugin returns a thenable Fiber. Activation of this adapter includes
    // both child startups; a failed MCP must not leave a green parent/Skill.
    await Promise.all(children);
  } catch (error) {
    await stopChildren();
    if (ctx.fiber.uid !== null) throw error;
  }
}
