# Desktop World TypeScript SDK

Source-package installation: `npm install /absolute/path/clients/typescript` after `npm ci && npm run build` in that directory. The RC archive contains compiled `dist` and type declarations; no runtime npm dependencies. Node 20+ and a trusted matching `dtw` executable are required. No registry publication is implied.

```ts
import {HostSession, value, one} from '@caelis-labs/desktop-world';
const host = await HostSession.start({helper: '/trusted/bin/dtw', writeApps: ['Your APP']});
try {
  const dw = host.desktop; // Expose only this facade to the agent.
  const inventory = await dw.observe();
  const app = one(inventory, 'Your APP');
  await host.grant(app.ref); // Trusted owner approval, never a model-selected permission.
  const windows = await dw.observe(app.ref);
  const window = one(windows, 'Exact Window Title');
  const found = await dw.find(window.ref, {role: 'text_field', name_equals: 'Name'});
  const field = one(found, 'Name');
  const receipt = await dw.set(field.ref, 'Hello 中文', 'task-write-1');
  console.log(receipt.run_id, value((await dw.read(field.ref)).text));
} finally { await host.close(); }
```

For shortcuts, use `dw.transaction(tx => { tx.focus(windowRef); tx.press(tx.bindFocus('input',windowRef),'O',['primary']); })`. Builders gather locally and submit one native plan. `HostSession.declare/grant/revoke/grants/endTurn` dynamically manage APP authority. `DesktopError.reply/receipt` retains failures; `dw.reconcile(id)` waits for the original response, never replays. Explicitly end the turn to cancel; abandoning a Promise alone does not cancel input.

See [shared integration contract](../../docs/agent-integration.md), including Windows, incomplete coverage, audit rotation and receipt recovery. Run the real subprocess contract with `python scripts/check-sdks.py` from the repository root.
