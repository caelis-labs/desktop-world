// Only script execution lives here. Native authority and the MCP connection
// stay in the supervisor, so a blocked V8 worker cannot block status/EndTurn.
import { createSession } from '../clients/javascript/desktop.mjs';

let session;
let pending = new Map();
let sequence = 0;
const send = message => process.send?.(message);
process.on('message', async message => {
  if (message.type === 'init') {
    session = createSession(request => new Promise((resolve, reject) => {
      const id = ++sequence;
      pending.set(id, { resolve, reject });
      send({ type: 'desktop', id, request });
    }), { epoch: message.epoch });
    send({ type: 'ready' });
  } else if (message.type === 'desktop_result') {
    const entry = pending.get(message.id);
    if (entry) {
      pending.delete(message.id);
      if (message.error) entry.reject(Object.assign(new Error(message.error.message), message.error));
      else entry.resolve(message.response);
    }
  } else if (message.type === 'exec') {
    if (!session) return send({ type: 'result', id: message.id, result: { error: { code: 'worker_not_ready' } } });
    try { send({ type: 'result', id: message.id, result: await session.execute(message.code) }); }
    catch (error) { send({ type: 'result', id: message.id, result: { error: { code: 'worker_failed', message: String(error.message).slice(0, 1000) } } }); }
  }
});
process.on('disconnect', () => process.exit(1));
