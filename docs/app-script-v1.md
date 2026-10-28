# 单 app 脚本 v1

脚本通过标准 MCP `exec` 工具运行，外层必填当前 `apps` 返回的稳定 app ID 字符串。DTW 只验证目标和执行边界，审批与授权属于 Harness。[MCP 合同](app-protocol-v1.md)定义发现、调用和标准取消；`apps` 的 `script` 描述广告五个方法的参数/返回 schema、引用规则、限制和 state/receipt/image 语义。

## 调用与执行范围

```json
{
  "app": "com.apple.TextEdit",
  "request_id": "script-1",
  "source": "const inventory = await app.windows(); return inventory.windows;",
  "timeout_ms": 5000,
  "max_calls": 32
}
```

示例 app ID 必须与 `apps` 返回的目标应用一致。`source` 是 JavaScript 异步函数体，可直接使用 `await` 和 `return`，不是模块或文件路径。未知字段拒绝；`timeout_ms`、`max_calls` 可省略，分别默认 5000 和 32，只能缩小。没有 grant、approved、授权能力集合或续期参数。

脚本在一个执行上下文内串行运行；Host bridge 调用进入同一个 Core，不递归提交新任务。`exec` 等待该脚本收尾。相同已接受 ID 和相同参数返回已有记录，更换参数得到 `request_conflict`，失败也不释放 ID 供重放。`result` 仅用于读取原记录；正常执行无需轮询。标准 MCP cancellation 使用外层 JSON-RPC request ID。

## 五个 app 方法

所有方法都返回 Promise，并绑定外层 app；用 `await` 顺序组合，也可使用 `.then()` / `.catch()`。原生调用仍顺序完成，不提供并行桌面执行语义，`Promise.all` 不会扩大准入范围或并发槽。

| 方法 | 返回值 | 执行范围与绑定 |
| --- | --- | --- |
| `await app.windows()` | `{windows:[{window_ref,title,visible}],coverage,diagnostics}` | 发现当前 app 的窗口 |
| `await app.observe({window_ref})` | `{window_ref,observation_ref,kind:"semantic",elements:[{element_ref,role,name,value,enabled,actions,label,can_press}]}` | 创建语义观察 |
| `await app.capture({window_ref})` | `{window_ref,observation_ref,kind:"pixels",image:{width,height,coordinate_space:"image_pixels"}}` | 返回像素观察元数据，图片字节在 VM 外 |
| `await app.press({window_ref,observation_ref,element_ref})` | 原协议的动作 receipt | 当前语义观察中的可按元素 |
| `await app.click({window_ref,observation_ref,x,y})` | 原协议的动作 receipt | 当前像素观察及有效图片坐标 |

语义元素的 `role` 是 DTW 规范角色；`name`、`value` 为字符串或 null，`enabled` 为 Boolean 或 null。不可读信息保留 null。`actions` 列出当前已实现且可用的操作，本轮仅可能包含 `press`。`label` 用于展示；不能从拼接 label 反推结构化值或状态。

`windows` 不接受参数，其他方法恰好接受一个上述字段构成的对象。多余 app_ref/PID、未知方法或 raw native 参数在 bridge 拒绝；跨上下文引用、伪窗口和未实现的能力由协议/Core 拒绝。每次原生调用重新校验完整 app 实例、目标、provider、观察种类和生命周期。窗口发现、新观察和动作对旧引用的影响与原子调用完全相同；不能用 state 中保存的字符串恢复失效引用。

macOS 窗口归属未证明时，`app.windows()` 保留与原子入口相同的错误，不把失败改成空数组。未捕获错误会使脚本失败且不提交 state，原步骤诊断仍保留；[C5.2 库存验收](window-inventory-acceptance.md)记录冷启动/后台/前台及闭窗残留的具体边界。

例如，在明确选择的窗口中执行一次动作并独立读回：

```javascript
const inventory = await app.windows();
const matches = inventory.windows.filter(w => w.title === "Target window");
if (matches.length !== 1) throw new Error("Target window is unavailable or ambiguous");
const window = matches[0];
const before = await app.observe({window_ref: window.window_ref});
const buttons = before.elements.filter(e =>
  e.role === "button" && e.name === "Increment" && e.actions.includes("press"));
if (buttons.length !== 1) throw new Error("Target control is unavailable or ambiguous");
const button = buttons[0];
await app.press({
  window_ref: window.window_ref,
  observation_ref: before.observation_ref,
  element_ref: button.element_ref
});
const after = await app.observe({window_ref: window.window_ref});
state.completedRuns = (state.completedRuns ?? 0) + 1;
return after.elements;
```

标题和标签只是脚本对当前 app 观察结果的选择条件，不构成应用或窗口的唯一身份。动作回执中的 `dispatched` 也不证明预期效果；调用者必须检查独立读回的内容。

## VM、总预算与 JSON

每个脚本使用独立 QuickJS VM。JavaScript 标准语言、Promise 和有界 console 可用；没有 timers、文件/进程/网络 API、OS 模块加载器、应用启动或任意原生调用。`await` 可以用于上述 app 方法和可结算 Promise；等待永不结算的 Promise 且没有剩余微任务时返回 `script_stalled`，不会等待外部事件。

| 边界 | 当前限制 |
| --- | --- |
| 源码 | 最多 64 KiB UTF-8；整个换行 MCP JSON 输入帧上限为 512 KiB，为 JSON 包装和转义保留空间 |
| 总 deadline | `timeout_ms` 为 1–5000 ms，默认 5000；从接受任务时起计算一次绝对执行期限 |
| Host bridge 调用 | `max_calls` 为 1–32，默认 32；不按步骤重置 |
| QuickJS 内存 | 32 MiB 分配配额，包含 VM 内的脚本数据；不是整个 DTW/native worker 的 RSS 上限 |
| QuickJS 栈 | 512 KiB |
| result 和 state | 分别最多 64 KiB JSON UTF-8 |
| 单次 Host 参数 / 成功回复 | 参数最多 64 KiB JSON；回复 envelope 最多 1 MiB；不含图片字节 |
| JSON 遍历 | result、state、Host 参数均拒绝深度超过 64 或遍历节点超过 32768 的值 |
| console | 最多 200 条、合计 32 KiB UTF-8；单条超过 2000 个 Unicode 字符保留前缀并加省略号，截断会设置 `console_truncated` |

总 deadline 同时约束 VM、bridge 等待和每次同步原生调用；Core 还会校验观察期限。到期检查覆盖准入、旧绑定释放后、provider 返回及绑定释放后、state 提交前。下一步不会获得新的 5 秒，预算也不延长观察 TTL。晚到动作保留 provider 原始结果；若已可能派发，顶层结论仍为未知效果并记录中断，已知 `not_dispatched` 不改写成未知。

严格 JSON 编码拒绝非有限数、BigInt、函数、symbol、循环、访问器/隐藏属性、非普通 JSON 对象、数组空洞及额外数组属性；不会悄悄丢字段或把非法值改成 null。唯一顶层便捷规则是脚本没有返回值或返回 `undefined` 时，`value.result` 为 null。嵌套 `undefined` 仍拒绝。

普通 Host 错误可在 JavaScript 中 `try/catch` 后继续，先前失败步骤仍在账本中。deadline、取消及调用次数耗尽会锁定执行失败，不能通过 catch 再返回成功。内存配额由 VM 强制执行；如果脚本捕获一次内存分配异常，并在硬配额内完成后续执行及 JSON 结算，可以成功，不能把“捕获过 OOM”直接等同于突破配额。未捕获的内存/栈错误、语法错误和普通异常均不提交 state。

## state 提交与效果

上下文初始 `state` 为 `{}`。每次新 VM 收到上一次成功提交的 JSON 副本；可修改对象字段及其嵌套数组/对象，但 `state = ...` 替换根会得到 `invalid_state`。普通变量、函数和 VM globals 不跨脚本保存。只有脚本成功、result/state 编码通过且收尾时未取消/超时，才原子提交新 state；失败时沿用旧值。新上下文或进程重启从空 state 开始。

state 提交不是桌面事务。动作一旦派发，之后的异常、取消、超时、JSON 编码失败或 state 未提交都不能回滚它。调用者应保留回执、按需重新观察；DTW 不补偿、不重放脚本。

## 查询、步骤账本与图像

任务结果回显 `app` 字符串，并包含 `request_id, status, value, error, receipt, payload_expired` 等执行信息；脚本的 `operation` 为 `script`，外层 `receipt` 通常为 null，动作证据在附加的 `script.steps` 中：

| 字段 | 含义 |
| --- | --- |
| `value.result` | 正常返回的 JSON；失败时可能没有该字段 |
| `value.console`, `value.console_truncated` | 有界日志及截断标记 |
| `value.calls` | 已计入 Host bridge 预算的尝试数，不一定等于 ledger 长度 |
| `value.last_capture` | null 或 `{step,observation}`，关联最后返回图片的调用编号及观察元数据 |
| `script.steps[]` | `{number,line,operation,status,error,receipt}`；进行中也可查询，status 为 running/completed |
| `script.failure` | null 或 `{kind,message,line,operation,code,call}`；无可定位信息时为 null，内部错误可能只有部分诊断 |
| `script.state_committed` | 新 state 是否实际提交 |
| `script.shutdown_error` | 关闭资源失败的独立 `{code,message}`，不覆盖脚本失败或动作 receipt |

步骤编号来自 bridge 尝试编号。某些参数拒绝已经计入 bridge 预算，但没有进入 Coordinator，因此步骤可有空号；在 JavaScript 参数检查或 JSON 编码阶段就被拒绝的调用未必计数。`line` 指向调用者源码，无法定位时步骤使用 0，failure 使用 null。编号与源码行用于定位调用，不改变目标身份。

成功解析为动作的步骤保存完整 `outcome/provider_outcome/interruption`；在 Coordinator 的协议解析阶段被拒绝的步骤只记录 error，receipt 为 null；读步骤也保留错误。普通异常和 provider panic 后，之前的步骤及 receipt 不丢失；panic 时尚无最终回执的动作记为未知效果。脚本失败后的 `completed` 只表示任务已收尾，不能据此推断没有桌面效果。`error.code` 可为 `script_syntax / script_exception / script_memory / script_timeout / script_cancelled / script_stalled / script_internal`，也可能是 Core/协议收尾错误；细分原因如 `call_limit`、`invalid_args`、`invalid_json` 在 `script.failure.code` 中，原生动作事实以步骤 receipt 为准。

连接只驻留一个 app 执行上下文，切换应用前完整关闭旧 owner 及其原生资源，避免闲置截图进程继续占用 scene lease。返回原应用时 `context_reset: true`，state 和 refs 重新建立，原请求回执仍可读取。连接保留 512 个原始请求 ID；每个上下文接受最多 64 个请求后可回收重建。value/image 共享 64 MiB payload 预算；驱逐后 `payload_expired: true`，`value` 和 image 消失，但 `script.steps`、failure、state_committed、shutdown_error、提交身份和动作 receipt 保留至进程退出，不重新执行以恢复 payload。回执准入阈值为 16 MiB：保留最后一份完整回执后停止接收新执行，不截断该回执。此时仍可读取原结果；重连不会恢复旧账本，也不是重放未知效果的许可。

capture 只把尺寸、坐标系和 opaque observation_ref 交给 VM；PNG 字节始终在 VM 外。常规结算只返回最后一次 capture 的 MCP image，并用 `value.last_capture` 指明来源步骤。此前截图不会全部累计。保留的图片及引用可能已被后续操作取代或消费，不能当作仍然有效的点击依据。

## 中断与所有权

总 deadline、标准 MCP 取消或连接关闭会中止相应执行并收尾原生资源，清理失败独立记录在 `shutdown_error`。VM 线程由任务所有者 join；provider panic 的 unwind 路径同样通知并 join，不能留下脱离任务生命周期的线程。`result` 仍可读取原 ID 的最终账本。

首个终态原因保留。动作可能在取消后才产生外部效果；关闭 worker 或 join VM 都不证明效果已撤销。普通被捕获的 Host 错误或脚本异常本身不必关闭上下文。取消、超时或资源回收导致上下文更换时，后续新执行会报告 `context_reset`；旧 state 和 observation 不继承，旧回执仍保留。

实现位于 `cmd/dtw/src/app_protocol/{wire.rs,scripts.rs,script_engine/}`，Core 预算入口位于 `cmd/dtw/src/runtime/mod.rs`。窄脚本引擎与 prelude 基于固定 CUA 0.38.0 donor 做 DTW 适配，第三方来源见 [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)；不会调用既有 CUA 脚本执行器或开放其完整能力表。

## 可发现合同与库存证据

客户端以 `apps` 返回的 `script.methods` 的 parameters/returns/执行条件/reference rules 为准；其中的最小组合例经过执行测试。`dtw api` 从同一 Rust schemas 生成 TypeScript；仓库中的 [TypeScript 声明](app-script-api.d.ts) 是生成文件，提交的仍是 JavaScript 函数体，不需要类型编译器或安装旧插件。

`app.windows()` 的成功 coverage 与原子入口相同；`empty_within_scope` 不等于应用无窗口。失败可通过 `e.code === "window_inventory_unproven"` 与 `e.details.coverage/diagnostics` 处理，无须解析 message。每次成功 windows 的 `script.steps[].inventory` 保留 coverage 与诊断，即使脚本随后抛错或 value/image 被驱逐，仍可调查首次读到空清单的原因。它不重复保存标题、图片或原生窗口 ID；失败步骤保持完整 error/details。

脚本期限同样约束没有 Host Call 的纯 JavaScript。超时不提交 state，已发生动作和步骤证据不抹去；后续取消或关闭不覆盖先发生的终态原因。

未处理的 Promise 拒绝会使脚本失败且不提交 `state`；正常 `.catch()` 或 `await` 处理仍可继续。引擎最多跟踪 256 个未处理拒绝，超限以 `promise_rejection_limit` 停止。
