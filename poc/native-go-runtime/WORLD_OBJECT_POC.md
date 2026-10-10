# DTW object input/output POC checkpoint

Date: 2026-10-10. Base HEAD: `d222f53b2939832f1c349106db026691465ec189`.
Scope: isolated native Go POC only. This checkpoint implements
the user's simple Agent boundary, **App → Window → Element**, while keeping
native grants, system permission, execution receipts and seat coordination
inside DTW. `dtw.app` does not ask the Agent to enumerate or create grants;
the trusted host still owns them. It does not bypass macOS Accessibility.

## Agent shape

The sole public MCP tool remains `exec`. With `DTW_POC_TEXT_OUTPUT=1`, the
official MCP client received one `TextContent` block and no
`structuredContent`. A caller-chosen `execution_id` starts every reply. A
display address can be copied unchanged into the next script:

```javascript
const app = await dtw.app('Fixture');
const win = await app.window('Fixture Window');
const button = await win.one({name:'Submit'});
print(button);
// Next exec: await dtw.at('W1/B1').invoke();
```

Actual official MCP client result text from the isolated synthetic native
boundary ([saved test log](evidence/world-object-contract-test.log)):

```text
e1 · W1/B1 Submit · button · invoke, setChecked, setSelected, setExpanded
e2 · W1/B1.invoke: completed, verified via semantic
```

Those content texts were 76 and 52 UTF-8 bytes. The old duplicate
`structuredContent` is absent in this mode. Byte counts are **not** model
Token counts. The result names the same `W1/B1` and `invoke` accepted by the
script. The native Ref and run/request IDs remain in the original result by
`exec(operation:"result",execution_id:"e2",detail:"full")`.

The JavaScript facade also supports bounded `app.windows()`, `win.find`,
`win.one`, `Element.read`, semantic setters and invocation, window capture,
directed mouse/keyboard methods, and one-plan `dtw.transaction`. It compiles
to existing native operations and never accepts a transport/mode selector.
No new product API or independent status tool was added.

## Evidence and limits

| Check | Result | Evidence / limit |
| --- | --- | --- |
| Official MCP one-tool text-only wire | Pass in isolated test | `TestWorldObjectScriptAndTextOnlyMCP`: `StructuredContent == nil`, two exact text lines above, same original ID for status/result. |
| Address reused across `exec`, persistent JS object, stale epoch | Pass in isolated test | Same test called `dtw.at('W1/B1').invoke()` and `state.button.invoke()` across calls; epoch change refused old object before a native action. |
| Session address isolation | Pass in isolated test | A second official MCP Session refused `dtw.at('W1/B1')` from the first Session, with no native request. |
| Finite semantic plan compilation | Pass in isolated test | Same test checked native target Ref and explicit `checked:true`, `selected:false`, `expanded:true` in one transaction body. The injected native boundary did not operate an OS App. |
| Actual model using this new Skill/result | Unverified | Earlier real-model success belongs to the older compact result experiment; no new model claim. |
| macOS real App through this new object surface | Blocked | Both `/private/tmp/dtw-world-helper` and historical `/private/tmp/dtw-exact-poc-helper` reported `accessibility:not_requested` in their own managed hello. The selected Obsidian test returned `permission_denied` before any AX nodes/actions; the exact-window declaration stayed `unresolved:discovery_incomplete`. No click, keyboard, note or foreground borrow was sent. |
| Mouse/keyboard and automatic foreground through this facade | Unverified | Existing native historical evidence is retained, but this new facade has no fresh real provider execution while Accessibility is unavailable. No fixture result is substituted for real acceptance. |
| Windows | Unverified | No Windows machine. |

The current and historical helper SHA-256 values in the blocked comparison
were `b750b891e06ef66cb5b13b3e30a142a9e9cc159dd7c76332f972d3849dda8747`
and `b504289af017fb766d4064d543d49649f2743ba5b3dd14a81bab11a81d0828ad`.
Both were ad hoc linker signed with identifier `a.out`, no team ID. A fresh
system Accessibility approval for the **actual helper process identity** is
the minimal external prerequisite for real macOS retest; changing an App grant
or silently activating Obsidian cannot repair an OS permission denial. No TCC
reset, prompt, configuration write or permission bypass was performed.

The full POC phase gate remains closed. The text-only output and object facade
are an experimental path, enabled explicitly; prior default MCP behavior and
all historical assets remain present. No product migration, push, PR, merge
or release occurred.
