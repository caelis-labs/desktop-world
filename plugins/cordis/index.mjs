/** Mount the host's existing MCP bridge and an isolated bundled Skill provider. */
import { fileURLToPath } from 'node:url';

export const name = 'desktop-world';
export const inject = ['loader', 'tools', 'skills'];

export async function apply(ctx) {
  const children = [];
  // The host MCP client can reject with the caller's raw abort object. Its
  // generic renderer turns that into "[object Object]", losing the only known
  // fact: this call was interrupted. Use the public presentation seam only;
  // neither this hook nor the host receipt proves native dispatch or cleanup.
  ctx.on('tools/post-execute', async (exec, result, next) => {
    const decision = await next();
    if (decision.kind !== 'accept' || decision.content !== undefined ||
        decision.value !== undefined || !exec.signal?.aborted ||
        !['mcp__dtw__apps', 'mcp__dtw__exec'].includes(exec.name) ||
        result.isError !== true || result.error?.message !== '[object Object]' ||
        result.content?.length !== 1 || result.content[0].type !== 'text' ||
        result.content[0].text !== 'Error: [object Object]') return decision;
    return {
      ...decision,
      content: [{ type: 'text', text: 'Error: ' + JSON.stringify({
        status: 'interrupted',
        error: {
          code: 'execution_interrupted',
          message: 'Tool execution was interrupted. This host receipt does not confirm native dispatch or cleanup. Do not replay the action; inspect target state before new input.',
        },
        original_host_error: result.content[0].text,
      }) }],
    };
  });
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
      serverName: 'dtw',
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
