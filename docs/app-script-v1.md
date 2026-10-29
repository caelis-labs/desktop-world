# 单 app 脚本 v1

脚本通过标准 MCP `exec` 工具运行，外层必填当前 `apps` 返回的稳定 app ID 字符串。DTW 只验证目标和执行边界，审批与授权属于 Harness。[MCP 合同](app-protocol-v1.md)定义发现、调用和标准取消；`apps` 提供紧凑方法索引；`api({methods:[...]})` 按需返回详细参数/返回 schema。引用、限制和 state/receipt/image 语义在同一合同下保持一致。

## 调用与执行范围

```json
{
  "app": "com.apple.TextEdit",
  "source": "const inventory = await app.windows(); return inventory.windows;"
}
```

示例 app ID 必须与 `apps` 返回的目标应用一致。`source` 是 JavaScript 异步函数体，可直接使用 `await` 和 `return`，不是模块或文件路径。DTW 自动分配执行标识；模型只需提供 `app` 和 `source`。公开输入严格只有这两个字段，未知字段拒绝；当前内部执行期限为 30 秒，不设调用次数配额。预算属于执行层策略，不要求模型配置。没有 grant、approved、授权能力集合或续期参数。

`source` 是解码后的脚本文本。MCP 消息仍需符合 JSON 字符串规则：例如脚本中的正则 `/\bword\b/` 在原始 JSON 字符串里写为 `/\\bword\\b/`；少一层转义会得到合法 JSON，但脚本内变成退格字符。调用程序应把原始字符串交给 JSON 序列化器，不能先手工转义再序列化。DTW 不重复解码或自动修复源码。回归测试覆盖引号、反斜杠、正则、模板字符串、换行、CRLF、中文/emoji、Unicode 转义及结果中的控制字符；这保证已接收内容的保真，不保证模型构造的参数符合其原意。

当前 `exec` 不接受文件路径。未来文件便利入口由调用方读取本地文件后发送内容，远端继续使用相同脚本合同；文件与远程传输的边界见[架构基线](next-architecture-baseline.md#脚本内容文件输入与远程执行)。

脚本在一个执行上下文内串行运行；Host bridge 调用进入同一个 Core，不递归提交新任务。`exec` 等待该脚本收尾。每次新的 `exec` 都代表新工作，即使脚本相同也会再次执行。已接受执行的相同标准传输请求与参数复用原执行，改换参数得到 `request_conflict`；这部分由客户端传输层管理，不是模型入参。`result({app})` 读取该 app 最近接受的执行，可选 `execution_id` 精确选择历史记录；正常执行无需轮询。标准 MCP cancellation 使用外层 JSON-RPC request ID。

## App 方法

所有方法都返回 Promise，并绑定外层 app；用 `await` 顺序组合，也可使用 `.then()` / `.catch()`。原生调用仍顺序完成，不提供并行桌面执行语义，`Promise.all` 不会扩大准入范围或并发槽。

| 方法 | 返回值 | 执行范围与绑定 |
| --- | --- | --- |
| `await app.windows()` | `{windows:[{window_ref,title,visible}],coverage,diagnostics}` | 发现当前 app 的窗口 |
| `await app.observe({window_ref,role?,text?,limit?})` 或 `await app.observe({cursor})` | `{window_ref,observation_ref,kind:"semantic",elements:[{element_ref,role,name,value,enabled,actions,label,can_press}],next,coverage:{complete,reason,visited}}` | 创建或继续原生语义查询，返回当前页 |
| `await app.capture({window_ref})` | `{window_ref,observation_ref,kind:"pixels",image:{width,height,coordinate_space:"image_pixels"}}` | 返回像素观察元数据，图片字节在 VM 外 |
| `await app.press({window_ref,observation_ref,element_ref})` | 原协议的动作 receipt | 当前语义观察中的可按元素 |
| `await app.click({window_ref,observation_ref,x,y,button?,count?})` | 原协议的动作 receipt | 当前像素观察及有效图片坐标；button=left/right，count=1/2 |
| `await app.focus({window_ref})` | 动作 receipt | 验证、激活并聚焦指定窗口 |
| `await app.typeText({window_ref,text})` | 动作 receipt | 向指定窗口当前文本焦点输入，内部取得物理输入 owner |
| `await app.keypress({window_ref,keys})` | 动作 receipt | 一个组合键，如 `["META","S"]`；平台支持的键名按原生合同校验 |
| `await app.scroll({window_ref,delta_x?,delta_y?})` | 动作 receipt | 在指定窗口中心发送整数 wheel steps，每轴 ±100，至少一轴非零；实际距离由 OS/app 决定 |
| `await app.setValue({window_ref,observation_ref,element_ref,value})` | 动作 receipt | 当前语义观察中明确广告 `setValue` 的可编辑文本控件 |

文本按 UTF-8 限 16 KiB，JSON 编码后限 48 KiB，不含 NUL；`setValue` 允许空串清空，`typeText` 不允许空串。键盘输入与语义 setter 的事件语义不同，业务效果必须独立读回。focus/输入动作同样消费目标窗口旧观察；其他窗口和 app 的状态保留。跨进程输入 lease 覆盖聚焦、目标复核和完整输入事务；中断不重放，已按下的键/按钮按原操作清理。

用户或外部程序仍可改变系统焦点。macOS 输入会按事件段重新核验系统级 AX 当前应用和精确窗口；失焦返回 `focus_changed`，无法证明焦点返回原始错误或 `focus_unproven`。已开始的输入保留 `unknown` 与原回执，并停止脚本后续动作；不会在输入中途主动抢回焦点或重发文本。首次执行输入时建立焦点仍是该方法的显式行为。

首次请求聚焦时，已验证的目标不要求当前已有 focused app/window；缺失焦点允许进行一次请求中的 raise/activate，但不算聚焦成功。之后已有的聚焦等待阶段可对焦点缺失及 AX `CannotComplete` 重查只读状态；不再次激活。其他 AX 错误、身份变化、中止或请求截止立即停止；等待耗尽时保留当前未解决错误。输入阶段不采用这种等待重查。

遇到 `focus_changed`、`focus_unproven`、`stale_observation` 或 `target_changed`，Agent 应刷新受影响窗口的身份/语义观察/截图，核对实际内容，再决定剩余步骤。此机制不是任意业务内容变化的全局版本锁，也不能撤回外部切换前已排入系统队列的事件；其他窗口、实例和 app 的上下文保留。

语义元素的 `role` 是 DTW 规范角色；`name`、`value` 为有界字符串摘录或 null，`enabled` 为 Boolean 或 null。不可读信息保留 null；摘录不能作为完整长文档读回。`actions` 列出当前已实现且可用的操作，当前可能包含 `press`、`setValue`。`label` 用于展示；不能从拼接 label 反推结构化值或状态。

查询按原生遍历过程筛选：`role` 精确匹配，`text` 对名称、值和描述做不区分大小写的子串匹配，两者同时提供时取交集。默认每页 40 个匹配元素，`limit` 为 1–100。原生节点、时间和字节边界也可能提前结束一页，所以零匹配仍可能返回 `next`。续页只传 `{cursor: page.next}`，不重复窗口或筛选条件；游标使用一次，保留原生 frontier，不从根重扫。

每个窗口独立保留一条语义查询和一份像素绑定。所有查询页共用 `observation_ref`，早先页的 `element_ref` 仍可操作；读取其他窗口、刷新未变化窗口清单、同窗截图不会清空该查询。新同窗同类读取替换旧绑定，失败的替换或续页也使对应查询失效；获准执行的动作消费本窗两类绑定。引用自初次获取起最多保留 15 秒，续页不延长期限。

`coverage.visited` 是查询累计访问节点数。`next=null` 仅表示无法继续，不保证完整；资源上限和已知读取缺口会保留 `complete=false` 与 `reason`。`complete=true` 只表示本次暴露出的活树遍历无已知缺口，不是 UI 原子快照或目标不存在的证明。WebKit 冷启动可能延迟发布内容树。macOS 内部区分应用 AX opt-in 与每个窗口的内容就绪；浅层空内容容器会有界重新探测，仍未证明时保留 `content_unproven`，不把它报告为完整空树。此策略不等待任意网页业务数据，也不假定所有原生辅助窗都必须包含 WebArea。

`windows` 不接受参数，其他方法恰好接受一个上述字段构成的对象。多余 app_ref/PID、未知方法或 raw native 参数在 bridge 拒绝；跨上下文引用、伪窗口和未实现的能力由协议/Core 拒绝。每次原生调用重新校验完整 app 实例、目标、provider、观察种类和生命周期。窗口发现、新观察和动作对旧引用的影响与原子调用完全相同；不能用 state 中保存的字符串恢复失效引用。

macOS 窗口归属未证明时，`app.windows()` 保留与原子入口相同的错误，不把失败改成空数组。未捕获错误会使脚本失败且不提交 state，原步骤诊断仍保留；[C5.2 库存验收](window-inventory-acceptance.md)记录冷启动/后台/前台及闭窗残留的具体边界。

例如，在明确选择的窗口中执行一次动作并独立读回：

```javascript
const inventory = await app.windows();
const matches = inventory.windows.filter(w => w.title === "Target window");
if (matches.length !== 1) throw new Error("Target window is unavailable or ambiguous");
const window = matches[0];
let page = await app.observe({window_ref: window.window_ref, role: "button", text: "Increment"});
const buttons = [];
while (true) {
  buttons.push(...page.elements.filter(e =>
    e.name === "Increment" && e.enabled === true && e.can_press));
  if (!page.next) break;
  page = await app.observe({cursor: page.next});
}
if (!page.coverage.complete || buttons.length !== 1)
  throw new Error("Target discovery is incomplete or ambiguous");
const button = buttons[0];
await app.press({
  window_ref: window.window_ref,
  observation_ref: page.observation_ref,
  element_ref: button.element_ref
});
// 在已知的计数器 fixture 上读回；业务应用应替换为自己的效果判据。
let after = await app.observe({window_ref: window.window_ref, text: "Count:"});
const counts = [];
while (true) {
  counts.push(...after.elements.map(e => ({name:e.name, value:e.value})));
  if (!after.next) break;
  after = await app.observe({cursor:after.next});
}
return {counts, coverage:after.coverage};
```

标题和标签只是脚本对当前 app 观察结果的选择条件，不构成应用或窗口的唯一身份。动作回执中的 `dispatched` 也不证明预期效果；调用者必须检查独立读回的内容。

## VM、总预算与 JSON

每个脚本使用独立 QuickJS VM。JavaScript 标准语言、Promise 和有界 console 可用；没有 timers、文件/进程/网络 API、OS 模块加载器、应用启动或任意原生调用。`await` 可以用于上述 app 方法和可结算 Promise；等待永不结算的 Promise 且没有剩余微任务时返回 `script_stalled`，不会等待外部事件。

| 边界 | 当前限制 |
| --- | --- |
| 源码 | 最多 64 KiB UTF-8；整个换行 MCP JSON 输入帧上限为 512 KiB，为 JSON 包装和转义保留空间 |
| 总 deadline | 当前内部策略为 30 秒；从接受任务时起计算一次绝对执行期限，MCP 不暴露预算参数 |
| Host bridge 调用 | 当前 MCP 入口不设调用次数配额；内部 Core 仍可应用预算，模型不配置 |
| 步骤回执准入 | 累积到 1 MiB 后停止下一步派发；保留跨阈值的最后一整步及其效果，不截断回执 |
| QuickJS 内存 | 32 MiB 分配配额，包含 VM 内的脚本数据；不是整个 DTW/native worker 的 RSS 上限 |
| QuickJS 栈 | 512 KiB |
| result 和 state | 分别最多 64 KiB JSON UTF-8 |
| 单次 Host 参数 / 成功回复 | 参数最多 64 KiB JSON；回复 envelope 最多 1 MiB；不含图片字节 |
| JSON 遍历 | result、state、Host 参数均拒绝深度超过 64 或遍历节点超过 32768 的值 |
| console | 最多 200 条、合计 32 KiB UTF-8；单条超过 2000 个 Unicode 字符保留前缀并加省略号，截断会设置 `console_truncated` |

总 deadline 同时约束 VM、bridge 等待和每次同步原生调用；Core 还会校验观察期限。到期检查覆盖准入、旧绑定释放后、provider 返回及绑定释放后、state 提交前。下一步不会重新获得整份脚本预算，预算也不延长观察 TTL。晚到动作保留 provider 原始结果；若已可能派发，顶层结论仍为未知效果并记录中断，已知 `not_dispatched` 不改写成未知。

严格 JSON 编码拒绝非有限数、BigInt、函数、symbol、循环、访问器/隐藏属性、非普通 JSON 对象、数组空洞及额外数组属性；不会悄悄丢字段或把非法值改成 null。唯一顶层便捷规则是脚本没有返回值或返回 `undefined` 时，`value.result` 为 null。嵌套 `undefined` 仍拒绝。

普通 Host 错误可在 JavaScript 中 `try/catch` 后继续，先前失败步骤仍在账本中。deadline、取消、显式调用次数配额耗尽及步骤回执准入耗尽会锁定执行失败，不能通过 catch 再返回成功。内存配额由 VM 强制执行；如果脚本捕获一次内存分配异常，并在硬配额内完成后续执行及 JSON 结算，可以成功，不能把“捕获过 OOM”直接等同于突破配额。未捕获的内存/栈错误、语法错误和普通异常均不提交 state。

## state 提交与效果

上下文初始 `state` 为 `{}`。每次新 VM 收到上一次成功提交的 JSON 副本；可修改对象字段及其嵌套数组/对象，但 `state = ...` 替换根会得到 `invalid_state`。普通变量、函数和 VM globals 不跨脚本保存。只有脚本成功、result/state 编码通过且收尾时未取消/超时，才原子提交新 state；失败时沿用旧值。新逻辑 app 上下文、所有旧 app 实例均被替换或 MCP 进程重启从空 state 开始。切换 app、取消/超时或原生 owner 重建不清空已提交 state；原生 refs 的有效性与 state 的 JSON 存续分开管理。

state 提交不是桌面事务。动作一旦派发，之后的异常、取消、超时、JSON 编码失败或 state 未提交都不能回滚它。调用者应保留回执、按需重新观察；DTW 不补偿、不重放脚本。

## 查询、步骤账本与图像

`exec` 回显 `app` 和内部生成的 `execution_id`，保留 `status/value/error/payload_expired`、状态提交和恢复标记；默认返回 `script.summary`，不展开步骤列表。摘要累计步骤、失败、不完整读取、派发、证据和中断，`last_issue` 只代表最后一个注意项。`incomplete_reads` 包含不完整库存，以及已无续页但 `complete=false` 的语义查询；正常尚有续页的观察不计入。对应注意项保留 `inventory` 或 `discovery` 证据，不把成功只读步骤改为失败。此计数非零时，不能依据缺失条目断定窗口或控件不存在。详见 [MCP 结果合同](app-protocol-v1.md#结果效果与图像)。正常流程无需追加查询。

同次执行中，后续同一 `window_ref`、不同 `observation_ref` 的查询以 `complete=true, has_more=false` 结束，可将此前该窗口的不完整查询计入 `recovered_incomplete_reads`。中间一旦出现动作 receipt、失败或未完成步骤，之前尚未恢复的查询永久退出恢复候选。库存、其他窗口或其他执行不参与此恢复判定；该字段仅在非零时输出，原 `incomplete_reads` 和 `last_issue` 不删除。它证明后来的独立查询完整，不证明两次筛选条件相同、原查询已补全或业务动作成功。

`result({app})` 或带历史 `execution_id` 的查询返回完整原始记录，其中 `operation` 为 `script`，外层 `receipt` 通常为 null，动作证据在 `script.steps` 中：

| 字段 | 含义 |
| --- | --- |
| `value.result` | 正常返回的 JSON；失败时可能没有该字段 |
| `value.console`, `value.console_truncated` | 有界日志及截断标记 |
| `value.calls` | 已计入 Host bridge 预算的尝试数，不一定等于 ledger 长度 |
| `value.last_capture` | null 或 `{step,observation}`，关联最后返回图片的调用编号及观察元数据 |
| `script.steps[]` | `{number,line,operation,status,error,receipt}`；进行中也可查询，status 为 running/completed |
| `script.steps[].discovery` | 成功 observe 的 `{window_ref,observation_ref,complete,reason,visited,has_more}`；保存当页覆盖证据，不保存元素、游标或页面文本 |
| `script.failure` | null 或 `{kind,message,line,operation,code,call}`；无可定位信息时为 null，内部错误可能只有部分诊断 |
| `script.state_committed` | 新 state 是否实际提交 |
| `script.shutdown_error` | 关闭资源失败的独立 `{code,message}`，不覆盖脚本失败或动作 receipt |

步骤编号来自 bridge 尝试编号。某些参数拒绝已经计入 bridge 预算，但没有进入 Coordinator，因此步骤可有空号；在 JavaScript 参数检查或 JSON 编码阶段就被拒绝的调用未必计数。`line` 指向调用者源码，无法定位时步骤使用 0，failure 使用 null。编号与源码行用于定位调用，不改变目标身份。

成功解析为动作的步骤保存完整 `outcome/provider_outcome/interruption`；在 Coordinator 的协议解析阶段被拒绝的步骤只记录 error，receipt 为 null；读步骤也保留错误。普通异常和 provider panic 后，之前的步骤及 receipt 不丢失；panic 时尚无最终回执的动作记为未知效果。脚本失败后的 `completed` 只表示任务已收尾，不能据此推断没有桌面效果。`error.code` 可为 `script_syntax / script_exception / script_memory / script_timeout / script_cancelled / script_stalled / script_internal`，也可能是 Core/协议收尾错误；细分原因如 `call_limit`、`receipt_limit`、`invalid_args`、`invalid_json` 在 `script.failure.code` 中，原生动作事实以步骤 receipt 为准。

一个连接保留多个 app 上下文，A→B→A 不重置 state 或窗口引用。每个 app 的 AX worker 与短命 scene worker 分开：只有 capture/click/scroll 才占用 scene 资源，完成后停止并回收该子进程，空闲 app 不持有 scene lease。当前同一连接一次执行一个脚本，多个连接保持独立 state、引用和回执。连接保留最多 512 份已接受执行记录，不再每 64 次重建 app owner。value/image 共享 64 MiB payload 预算；驱逐后 `payload_expired: true`，`value` 和 image 消失，但 `script.steps`、failure、state_committed、shutdown_error、提交身份和动作 receipt 保留至进程退出，不重新执行以恢复 payload。回执准入阈值为 16 MiB：保留最后一份完整回执后停止接收新执行，不截断该回执。此时仍可读取原结果；重连不会恢复旧账本，也不是重放未知效果的许可。

capture 只把尺寸、坐标系和 opaque observation_ref 交给 VM；PNG 字节始终在 VM 外。常规结算只返回最后一次 capture 的 MCP image，并用 `value.last_capture` 指明来源步骤。此前截图不会全部累计。保留的图片及引用可能已被后续操作取代或消费，不能当作仍然有效的点击依据。

## 中断与所有权

总 deadline、标准 MCP 取消或连接关闭会中止相应执行并收尾原生资源，清理失败独立记录在 `shutdown_error`。VM 线程由任务所有者 join；provider panic 的 unwind 路径同样通知并 join，不能留下脱离任务生命周期的线程。`result({app})` 可读取该 app 最近接受的执行；返回的 `execution_id` 可用于精确查询其最终账本。

首个终态原因保留。动作可能在取消后才产生外部效果；关闭 worker 或 join VM 都不证明效果已撤销。普通被捕获的 Host 错误或脚本异常本身不必关闭上下文。取消、超时或资源回收导致上下文更换时，后续新执行会报告 `context_reset`；已提交 state 继续保留，其中保存的原生 refs 可能已失效，需要重新发现。所有旧 app 实例均被替换时额外报告 `state_reset` 并清空 app state；仅一个实例加入或退出时保留其他实例引用及 app state；旧回执仍保留。

实现位于 `cmd/dtw/src/app_protocol/{wire.rs,scripts.rs,script_engine/}`，Core 预算入口位于 `cmd/dtw/src/runtime/mod.rs`。窄脚本引擎与 prelude 基于固定 CUA 0.38.0 donor 做 DTW 适配，第三方来源见 [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)；不会调用既有 CUA 脚本执行器或开放其完整能力表。

## 可发现合同与库存证据

`apps.script.methods` 是紧凑签名索引；参数、返回值、执行条件和引用规则通过 `api({methods:[…]})` 按需查询。组合行为有 Core/VM 回归与真实原生验收。`dtw api` 从同一 Rust schemas 生成 TypeScript；仓库中的 [TypeScript 声明](app-script-api.d.ts) 是生成文件，提交的仍是 JavaScript 函数体，不需要类型编译器或安装旧插件。

`app.windows()` 的成功 coverage 与原子入口相同；`empty_within_scope` 不等于应用无窗口。失败可通过 `e.code === "window_inventory_unproven"` 与 `e.details.coverage/diagnostics` 处理，无须解析 message。每次成功 windows 的 `script.steps[].inventory` 保留 coverage 与诊断，即使脚本随后抛错或 value/image 被驱逐，仍可调查首次读到空清单的原因。它不重复保存标题、图片或原生窗口 ID；失败步骤保持完整 error/details。

成功 observe 同样在 `script.steps[].discovery` 保留覆盖状态；脚本丢弃页面、随后抛错或结果 payload 被驱逐都不删除此证据。`has_more` 表示该页当时是否提供续页，不保证游标当前仍有效；`visited` 是该查询累计访问数。不同新查询的页面及引用不能混合，旧的不完整记录不会因后来另一查询成功而消失。失败 observe 仍保存原始 error，不虚构覆盖状态。

脚本期限同样约束没有 Host Call 的纯 JavaScript。超时不提交 state，已发生动作和步骤证据不抹去；后续取消或关闭不覆盖先发生的终态原因。

未处理的 Promise 拒绝会使脚本失败且不提交 `state`；正常 `.catch()` 或 `await` 处理仍可继续。引擎最多跟踪 256 个未处理拒绝，超限以 `promise_rejection_limit` 停止。
