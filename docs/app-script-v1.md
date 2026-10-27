# 单 app 脚本 v1（C5）

本扩展在 [单 app 协议 v1](app-protocol-v1.md) 的显式选择入口中增加 `command.kind = "script"`。外层协议仍为 `version: 1`，不增加 MCP tool，也不增加授权能力。`exec.describe` 的 `script` 对象广告 `version: 1`、`language: "javascript"`、五个方法、限制和 state/receipt/image 语义；客户端先读取它，再决定是否提交脚本。旧版没有这个广告时不能假定支持脚本。

执行平台、可信启动 grant、app 实例、opaque 引用和原生覆盖范围沿用原协议。默认发布入口继续使用既有路径；本文件定义当前 opt-in 实现合同，不是 C5 GUI 验收或发行声明。C4-macOS.2 的真实 Caelis 产品代码本地接入见 [既有验收记录](app-protocol-acceptance.md#c4-macos2实际-harness-授权入口2026-10-27) 及 [证据索引](evidence/c4-host-authorization-20261027.json)；配套 Caelis 按用户要求未推送、未发布，不能视为线上入口。

## 提交与执行范围

```json
{
  "version": 1,
  "operation": "submit",
  "request_id": "script-1",
  "target": {"app_ref": "app_..."},
  "command": {
    "kind": "script",
    "source": "const inventory = await app.windows(); return inventory.windows;",
    "timeout_ms": 5000,
    "max_calls": 32
  }
}
```

`target` 只允许当前上下文的 `app_ref`，不得包含 `window_ref`。`source` 是 JavaScript 函数体，可直接使用 `await` 和 `return`，不是模块或文件路径。未知字段拒绝；`timeout_ms`、`max_calls` 可省略，分别默认 5000 和 32，只能缩小。请求不能携带 grant、PID、generation、审批结论或新增能力。

整个脚本占用同一个外层活动任务槽，直到 VM 和当前原生调用收尾；它的 Host Call 直接复用该槽和 Core，不递归提交新任务。期间另一个 script 或原子 command 返回 `busy`。`submit` 立即返回任务记录；`result` 查询进度，不等待；`cancel`、`close` 使用原协议。相同已接受 ID 和相同参数返回已有记录，更换参数得到 `request_conflict`。脚本失败也不会释放 ID 供重放。

## 五个 app 方法

所有方法都绑定外层 app；用 `await` 顺序组合。当前 Host bridge 顺序完成每次调用，不提供并行桌面执行语义，`Promise.all` 不会扩大准入范围或并发槽。

| 方法 | 返回值 | Core 能力与绑定 |
| --- | --- | --- |
| `await app.windows()` | `{windows:[{window_ref,title,visible}]}` | `observe`；发现当前 app 的窗口 |
| `await app.observe({window_ref})` | `{window_ref,observation_ref,kind:"semantic",elements:[{element_ref,label,can_press}]}` | `observe`；创建语义观察 |
| `await app.capture({window_ref})` | `{window_ref,observation_ref,kind:"pixels",image:{width,height,coordinate_space:"image_pixels"}}` | `capture`；返回像素观察元数据，图片字节在 VM 外 |
| `await app.press({window_ref,observation_ref,element_ref})` | 原协议的动作 receipt | `press`；当前语义观察中的可按元素 |
| `await app.click({window_ref,observation_ref,x,y})` | 原协议的动作 receipt | `click`；当前像素观察及有效图片坐标 |

`windows` 不接受参数，其他方法恰好接受一个上述字段构成的对象。多余 app_ref/PID、未知方法或 raw native 参数在 bridge 拒绝；跨上下文引用、伪窗口和缺少能力继续由协议/Core 拒绝。每次原生调用重新校验 grant、完整 app 实例、目标、provider、观察种类和生命周期。窗口发现、新观察和动作对旧引用的影响与原子调用完全相同；不能把 state 中保存的字符串当作长效授权或恢复失效引用。

macOS 窗口归属未证明时，`app.windows()` 保留与原子入口相同的错误，不把失败改成空数组。未捕获错误会使脚本失败且不提交 state，原步骤诊断仍保留；[C5.2 库存验收](window-inventory-acceptance.md)记录冷启动/后台/前台及闭窗残留的具体边界。

例如，在可信选择的窗口中读回一次已授权动作：

```javascript
const inventory = await app.windows();
const window = inventory.windows.find(w => w.title === "Target window");
if (!window) throw new Error("Target window is unavailable");
const before = await app.observe({window_ref: window.window_ref});
const button = before.elements.find(e => e.label === "Increment" && e.can_press);
if (!button) throw new Error("Target control is unavailable");
await app.press({
  window_ref: window.window_ref,
  observation_ref: before.observation_ref,
  element_ref: button.element_ref
});
const after = await app.observe({window_ref: window.window_ref});
state.completedRuns = (state.completedRuns ?? 0) + 1;
return after.elements;
```

标题和标签只是脚本对当前 app 观察结果的选择条件，不构成身份或授权。动作回执中的 `dispatched` 也不证明预期效果；调用者必须检查独立读回的内容。

## VM、总预算与 JSON

每个脚本使用独立 QuickJS VM。JavaScript 标准语言、Promise 和有界 console 可用；没有 timers、文件/进程/网络 API、OS 模块加载器、应用启动或任意原生调用。`await` 可以用于上述 app 方法和可结算 Promise；等待永不结算的 Promise 且没有剩余微任务时返回 `script_stalled`，不会等待外部事件。

| 边界 | 当前限制 |
| --- | --- |
| 源码 | 最多 64 KiB UTF-8；整个换行 MCP JSON 帧另有 64 KiB 上限，包含 JSON 包装和转义后的源码 |
| 总 deadline | `timeout_ms` 为 1–5000 ms，默认 5000；从接受任务时起计算一次绝对期限，并取 grant 期限的较早者 |
| Host bridge 调用 | `max_calls` 为 1–32，默认 32；不按步骤重置 |
| QuickJS 内存 | 32 MiB 分配配额，包含 VM 内的脚本数据；不是整个 DTW/native worker 的 RSS 上限 |
| QuickJS 栈 | 512 KiB |
| result 和 state | 分别最多 64 KiB JSON UTF-8 |
| 单次 Host 参数 / 成功回复 | 参数最多 64 KiB JSON；回复 envelope 最多 1 MiB；不含图片字节 |
| JSON 遍历 | result、state、Host 参数均拒绝深度超过 64 或遍历节点超过 32768 的值 |
| console | 最多 200 条、合计 32 KiB UTF-8；单条超过 2000 个 Unicode 字符保留前缀并加省略号，截断会设置 `console_truncated` |

总 deadline 同时约束 VM、bridge 等待和每次同步原生调用；Core 还会取 grant/观察期限的较早者。到期检查覆盖准入、旧绑定释放后、provider 返回及绑定释放后、state 提交前。下一步不会获得新的 5 秒，预算也不延长观察 TTL。晚到动作保留 provider 原始结果；若已可能派发，顶层结论仍为未知效果并记录中断，已知 `not_dispatched` 不改写成未知。

严格 JSON 编码拒绝非有限数、BigInt、函数、symbol、循环、访问器/隐藏属性、非普通 JSON 对象、数组空洞及额外数组属性；不会悄悄丢字段或把非法值改成 null。唯一顶层便捷规则是脚本没有返回值或返回 `undefined` 时，`value.result` 为 null。嵌套 `undefined` 仍拒绝。

普通 Host 错误可在 JavaScript 中 `try/catch` 后继续，先前失败步骤仍在账本中。deadline、取消及调用次数耗尽会锁定执行失败，不能通过 catch 再返回成功。内存配额由 VM 强制执行；如果脚本捕获一次内存分配异常，并在硬配额内完成后续执行及 JSON 结算，可以成功，不能把“捕获过 OOM”直接等同于突破配额。未捕获的内存/栈错误、语法错误和普通异常均不提交 state。

## state 提交与效果

上下文初始 `state` 为 `{}`。每次新 VM 收到上一次成功提交的 JSON 副本；可修改对象字段及其嵌套数组/对象，但 `state = ...` 替换根会得到 `invalid_state`。普通变量、函数和 VM globals 不跨脚本保存。只有脚本成功、result/state 编码通过且收尾时未取消/过期，才原子提交新 state；失败时沿用旧值。新上下文或进程重启从空 state 开始。

state 提交不是桌面事务。动作一旦派发，之后的异常、取消、超时、JSON 编码失败或 state 未提交都不能回滚它。调用者应保留回执、按需重新观察；DTW 不补偿、不重放脚本，也不自动续 grant。

## 查询、步骤账本与图像

任务外层继续包含 `version, request_id, app_ref, operation, status, value, error, receipt, payload_expired`；脚本的 `operation` 为 `script`，外层 `receipt` 通常为 null，动作证据在附加的 `script.steps` 中：

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

步骤编号来自 bridge 尝试编号。某些参数拒绝已经计入 bridge 预算，但没有进入 Coordinator，因此步骤可有空号；在 JavaScript 参数检查或 JSON 编码阶段就被拒绝的调用未必计数。`line` 指向调用者源码，无法定位时步骤使用 0，failure 使用 null。编号与源码行不是新的授权依据。

成功解析为动作的步骤保存完整 `outcome/provider_outcome/interruption`；在 Coordinator 的协议解析阶段被拒绝的步骤只记录 error，receipt 为 null；读步骤也保留错误。普通异常和 provider panic 后，之前的步骤及 receipt 不丢失；panic 时尚无最终回执的动作记为未知效果。脚本失败后的 `completed` 只表示任务已收尾，不能据此推断没有桌面效果。`error.code` 可为 `script_syntax / script_exception / script_memory / script_timeout / script_cancelled / script_stalled / script_internal`，也可能是 Core/协议收尾错误；细分原因如 `call_limit`、`invalid_args`、`invalid_json` 在 `script.failure.code` 中，原生动作事实以步骤 receipt 为准。

单上下文仍最多接受 64 个 ID。结果 value/image 共用原协议 64 MiB payload 预算；驱逐后 `payload_expired: true`，`value` 和 image 消失，但 `script.steps`、failure、state_committed、shutdown_error、提交身份和动作 receipt 保留至进程退出，不重新执行以恢复 payload。这不是跨进程持久化审计账本。

capture 只把尺寸、坐标系和 opaque observation_ref 交给 VM；PNG 字节始终在 VM 外。常规结算只返回最后一次 capture 的 MCP image，并用 `value.last_capture` 指明来源步骤。此前截图不会全部累计。保留的图片及引用可能已被后续操作取代或消费，不能当作仍然有效的点击依据。

## 中断与所有权

总 deadline 或 grant 到期关闭上下文并触发原生 worker shutdown；显式 `exec.cancel` 命中活动脚本或 `close` 也关闭准入并回收资源，清理失败独立记录在 shutdown_error。正在运行的 VM 被通知结束，VM 线程在任务所有者中 join；provider panic 的 unwind 路径同样取消并 join，不能将 VM 留成脱离任务生命周期的线程。`result` 仍可读取原 ID 的最终账本。

首个上下文终态保留。已经进入 TimedOut/GrantExpired/Closed 收尾阶段时，后续 cancel 返回 `cancel_requested: false, context_state: "closed"`，不能改报 cancelled。动作可能在取消后才产生外部效果；关闭 worker 或 join VM 都不证明效果被撤销。普通被捕获的 Host 错误或脚本异常本身不必关闭上下文；后续脚本仍须使用新的 ID，并遵守剩余 grant、引用和总记录限额。

实现位于 `cmd/dtw/src/app_protocol/{wire.rs,scripts.rs,script_engine/}`，Core 预算入口位于 `cmd/dtw/src/runtime/mod.rs`。窄脚本引擎与 prelude 基于固定 CUA 0.38.0 donor 做 DTW 适配，第三方来源见 [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)；不会调用既有 CUA 脚本执行器或开放其完整能力表。
