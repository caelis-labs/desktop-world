# Desktop World Python SDK

Python 3.11+, `asyncio`, no third-party runtime dependencies and no Node requirement. Install the SDK directory from the RC/source archive with `python -m pip install /trusted/path/clients/python`. Use a matching trusted native `dtw` executable. No PyPI publication is implied.

```python
import asyncio
from desktop_world import HostSession, known

async def main():
    async with await HostSession.start('/trusted/bin/dtw', write_apps=['Your APP']) as host:
        dw = host.desktop  # Agent-facing API; keep host authority private.
        ob = await dw.observe()
        if not ob['coverage']['complete']:
            raise RuntimeError('Narrow scope or continue discovery before selection')
        apps = [o for o in ob['objects'] if o.get('kind') == 'application'
                and o.get('name', {}).get('value') == 'Your APP']
        if len(apps) != 1:
            raise RuntimeError('Need one observed APP instance')
        await host.grant(apps[0]['ref'])  # Trusted owner approval.
        print(await host.grants())
        # Discover window/field Refs with dw.observe(appRef) and dw.find(windowRef,...).
        # receipt = await dw.set(field_ref, 'Hello 中文', request_id='task-write-1')
        # print(known((await dw.read(field_ref))['text']))
asyncio.run(main())
```

For a shortcut: `plan=dw.plan().focus(window_ref); plan.press(plan.bind_focus('input',window_ref),'O',['primary']); await dw.act(plan,request_id='open-1')`. Build without desktop effects, then submit once. `host.declare/grant/revoke/grants/end_turn` dynamically manages APPs. `DesktopError.reply/receipt` and `dw.reconcile(request_id)` retain original results. A cancelled desktop task ends the current turn; it does not erase prior effects.

Windows uses the default Proactor event loop. Do not replace it with SelectorEventLoop, which cannot run asyncio subprocesses. Use an absolute `dtw.exe` path; spawning does not invoke a shell. [Shared contract](../../docs/agent-integration.md) covers uncertainty, paging and owner boundaries. Run `python scripts/check-sdks.py` from the repository root.
