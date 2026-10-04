# 面向 Agent 的持久 JavaScript 入口

SDK 是唯一桌面执行内核，helper 管理原生 World；`clients/javascript/desktop.mjs` 在它上面提供调用链。需要 Node 20+，无 npm 包、构建或模型侧传输桥。脚本执行器是可信本机工具，不是安全沙箱；不能用于执行网页/文档提供的代码。

## 宿主启动一次

`host.json` 中由可信宿主设置 helper 路径和已有授权参数，不由 Agent 扩权：

```json
{"helper":"bin/dtw","args":["serve","--write-app","文本编辑","--audit","harness/audit.jsonl"]}
```

```sh
node clients/javascript/desktop.mjs serve --host host.json
```

`node clients/javascript/desktop.mjs status`（或 doctor）读取现有会话健康和**启动时**权限，不新建 World；环境变化后不能把启动信息当成实时权限。`--version` 读取 adapter/Node 版本，helper 的 `version` 还给出可获得的构建 revision。

保持该进程运行。使用标准管道启动 helper，避免 PTY 行截断。会话文件、socket 和完整日志限制本机访问；macOS/Linux 使用本地 Unix socket，无 TCP 端口。退出后不自动重启，旧 Ref 失效。Windows named-pipe 路径尚未实机验证。

## 每个模型回合执行一段脚本

```sh
node clients/javascript/desktop.mjs exec <<'JS'
const inventory = await dw.observe();
state.inventory = inventory;
print(dw.list(inventory, ['kind','name','app']));
JS
```

`uri` 可按 fields 请求原生文档/链接 URL，用于区分同名文档；缺失时为 unknown，Windows 当前为 unsupported。完整 URI 不按文本预览长度截断，但仍受总输出字节预算约束。它不能替代 Ref 身份或成为任意原生调用入口。

`dw.observe(appOrWindowRef)` 是 `scope:{refs:[ref]}` 的 summary 简写；不使用 `scope.apps` 或 `scope.windows`。

脚本体支持 `await`、变量、分支和循环。下一次 exec 可继续使用 `state`；局部 const/let 不跨脚本保留。用 `node .../desktop.mjs help` 获取精简接口，无需先加载完整原生 act schema。

```js
// name 必须来自观察，下面仅说明通用组合方式，不是任务配方。
const ob = await dw.find(state.window, {role:'button', name_equals:state.observedName});
const button = dw.one(ob, {name:state.observedName});
await dw.invoke(button.ref);
const after = await dw.outline(state.window, {fields:['name','role','states']});
state.after = after;
print(dw.rows(after, ['name','states.enabled','states.focused']));
```

`dw.list(ob, fields, {offset, limit, max_bytes})` 默认输出最多 20 行 / 4 KiB，返回 total 和 next_offset；翻页使用同一个保存在 state 的 observation，无需重新调用原生接口。它只是展示分页，原生 coverage 仍必须检查。`dw.rows` 返回全部行，适合本地筛选；直接 print 大树可能超过预算。角色使用 button、text_field 等规范名，不是 AXButton。`dw.value` 同时接受已知 Fact 和 role 等原始字段，未知或脱敏 Fact 仍报错。

`dw.focus` 的目标是 window 或支持 focus 的 UI，不接受 application；先选择实际窗口。

`dw.next(ob)` 使用保存的原查询获取原生下一页，避免手抄 continuation 或修改 budget 引发 query mismatch；检查 `ob.coverage.continuation`。这与 `dw.list` 本地展示翻页不同。observe 默认 32 条 summary / 8 KiB；outline 默认 name/role、深度 4、32 条 / 8 KiB。value_preview、uri、states、capabilities 均按需请求。find 默认搜索深度 12，仍受原生访问预算限制。

`dw.expand(ref, true/false)`、`dw.check(ref, true/false)`、`dw.select(ref, true/false)` 达到明确的展开、勾选、选择状态；`dw.scrollIntoView(ref)` 让目标进入 provider 的视口。它们始终验证，已达到状态时不重复发送；缺少布尔参数会在本地拒绝。本库不主动清空其他选择，provider 的单/多选规则可能拒绝操作或联动修改其他项。动作摘要保留 channel、delivery、verification 和不确定结果。后台宿主可以选择 no_shared_input 策略，脚本遇到 requires_shared_input 应停止，不能自行放宽策略或把语义操作改成物理输入。具体边界见 [语义动作](semantic-actions.md)。

`dw.one` 要求覆盖完整、未截断且无缺失来源；只匹配到一个结果但覆盖不全时不能当作唯一。`dw.rows` 保留 Ref、false、空字符串与未知状态。它支持嵌套字段呈现，**不声称降低后端属性读取成本**。更少原生读取需使用 observe 的 scope/fields/match/budget。

`match` 是合法 observe 参数，但必须含 `within`；`dw.find(ref, locator)` 自动补齐该范围并选择 outline。`rows/list` 只能选择已经获取的字段：要读取链接地址，应在 `find` 的 options 中指定 `fields:['name','role','uri']`，仅向 rows 传入 uri 不会补查。

原生网页正文常在 `role:'text'` 对象的 `value_preview` 中，`name_contains` 不搜索正文；`read(documentRef)` 也不聚合后代节点。可用 `state.text=await dw.find(documentRef,{role:'text'},{fields:['role','value_preview']})`，在本地筛选、分页呈现并保留 coverage；预览不足时读取具体文本 Ref。取后续原生页用 `dw.next(state.text)`，避免为输出另一批已读取的行重新扫描同一子树。脚本不暴露 `setTimeout`，界面切换用已有的 `waitFor` 和明确就绪条件。

`dw.focused(appRef)` 每次重新观察当前焦点，并验证它属于指定前台应用；窗口 scope 则要求前台窗口完全匹配，避免把另一个已授权窗口的焦点误当目标。应用级 scope 不限制为某个窗口，必要时还应检查新对话框的对象关系。原生执行器继续做实时权限、焦点、命中、身份和生命周期检查。不要缓存一次焦点跨多个新对话框使用。

界面切换可能短暂返回不完整树。用 `await dw.waitFor(observeArgs, ob => 明确的就绪条件, {timeout_ms:3000})` 在脚本内等待实际条件；它仅重读，不重放动作，读调用仍计入预算。避免每次等待都让模型往返或把短暂空树当成“控件不存在”。

## 每次返回的内容

- `outputs`：仅脚本 print 的 JSON。原始树保存在脚本本地，不必进入模型上下文。
- `observations`：强制返回 coverage、分页、reset 和长文本截断信息。不能以过滤后的短列表声称全量枚举。
- `actions`：强制返回 run_id、outcome、delivery/verification 分布与失败步骤。completed + not_requested 仅证明分派，不是业务结果已确认；细节可用 get。
- `error`：错误、partial/unknown/pending 的 act 默认阻断当前调用链。即使脚本 catch 了错误，后续桌面调用也不发送。不自动重试或恢复绑定。
- `metrics`：分别计量 helper 返回字节、print 字节、调用数和耗时；日志另外记录整个模型响应字节。字节不是 token。

若收据 `seat_health=fenced`，输入已阻断。可查询原 run_id 并进行只读观察；observe/cancel/wait 不能手动解除 fence。迟到的原生调用若被确认安全结束，内核可能自行恢复；若仍是 terminal unknown，应停止、保留收据并让宿主核对实际副作用。不要通过重启或新请求 ID 绕过。

一个脚本最多 32 次调用、8 KiB print 输出、64 KiB 源码；异步期限 60 秒，初始同步执行限 1 秒。期限到了停止后续调用，等待已经发出的 helper 请求结束并保留收据。Node vm 不是安全边界，恶意代码及 await 后的无限同步循环可能阻塞宿主，生产宿主应另加进程 watchdog。调用串行，包括 Promise.all；不要把同一桌面的写入并行化。未 await 的调用可能在脚本退出时被拒绝，因此必须 await。

完整 transport 日志 `harness/wire.jsonl`、脚本来源 `script-code.jsonl` 可能包含 UI 文本和键入内容，是私有评估证据，不进入公开发行包。`scripts.jsonl` 只保存计量。默认独占创建日志，避免混合不同运行；重测使用新目录。

```sh
node clients/javascript/desktop.mjs stop
```

正常退出关闭 helper stdin、取消未完成工作并请求释放持有输入。强杀不等于已清理；传输不确定时不得重放原脚本，先查看原请求和收据。新 helper 进程没有跨进程 exactly-once 保证。

## Fragmented text recipe

For many short AX leaves, execute [scripts/read-fragmented-text.js](../scripts/read-fragmented-text.js)
after storing an observed document/container Ref in `state.document`. The
[workflow](../skills/desktop-world/references/fragmented-text.md) preserves source
Refs, depth-first native order, block boundaries, Unicode and duplicate visible
text. It has explicit page/node/deadline/output bounds and reports redaction,
unknown values and possible preview clipping. It never captures or expands scope.

## 按需窗口图像

先调用 `dw.captureWindows(appRef)` 取 32 条 / 8 KiB 的原生窗口目录，只有显式 `dw.capture({kind:'window_content', target:ref, max_pixel_width:1024, max_pixel_height:768})` 才生成图片。macOS 专用捕获 Ref 不与 AX 窗口按标题/位置关联，不提供 AX outline 或桌面点击映射；普通 summary 不自动披露该目录。宿主单独启用 capture。接口与平台验证限制见 [窗口捕获](window-capture.md)。
