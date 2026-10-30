# 标准 MCP App 协议 v1

产品入口为 `dtw mcp`，使用官方 `rmcp 3.8.0` 的 stdio transport、协议协商、消息类型和服务分发；不维护另一套 MCP 解析器。Agent 使用普通 stdio MCP 配置和可选的 [DTW Skill](../skills/desktop-world/SKILL.md)，无需 DTW 专用 Host。审批与授权完全属于 Harness；DTW 只要求显式平台稳定 app ID，并执行目标、引用、预算和生命周期校验。

## 服务输入策略

启动参数为 `dtw mcp [--input-policy auto|background-only]`，默认 `auto`。macOS 的 `auto` 优先选择可用的后台投递，只有在尚未准备或派发输入的预检阶段确认后台不可用，才选择前台路径；未知或已派发结果不会换路重试。`background-only` 禁止 DTW 主动取得前台：显式 `focus` 及需要前台投递的动作，在前台准备之前返回 `not_dispatched` / `foreground_required`。Windows/Linux 当前物理输入没有后台路线，因此此模式拒绝 click/focus/typeText/keypress/scroll；已实现的原生语义操作仍可用。

`apps` 和 `api`（包括按方法过滤的返回）包含 `service: {input_policy: "auto" | "background_only"}`。命令行用连字符 `background-only`，返回值用下划线 `background_only`。策略固定于服务启动，脚本不覆盖它；`exec` 参数仍只有 `app + source`。这是工具的输入路由配置，不是授权、审批或 Harness 权限。语义动作可能使应用自行打开窗口或改变焦点，策略不承诺屏蔽应用自身的 UI 效果。Agent 遇到 `foreground_required` 应保留回执并说明当前路线不可用，不通过重试 `focus` 解决。

## 应用发现与稳定 app ID

`apps` 工具接受空对象，返回去重后的应用列表、紧凑脚本方法索引、引用/限制摘要和原生支持条件。详细 schemas 由只读 `api` 按需提供，无需先绑定应用或连接原生 worker。应用条目形如：

```json
{"app": "com.apple.TextEdit", "name": "TextEdit"}
```

`app` 是平台稳定唯一标识；macOS 使用 bundle ID。`name` 只用于展示。调用者复制返回的 app 字符串，不自行组合路径、PID 或代际。进程解析、进程出生代际和窗口归属全部由 DTW 内部维护；app ID 不是秘密、授权 token 或用户批准证明。

同一 ID 可对应多个独立实例。每次 exec 前刷新实例集合；`app.windows()` 聚合它们的窗口，`window_ref` 内部保留精确实例身份。单段脚本可操作多个实例的窗口，无需额外实例参数。标题相同时按观察或截图内容选窗，不能猜选第一个。普通 Chrome helper 不作为独立应用条目。部分实例库存失败时保留已证明的兄弟窗口并明确报告不完整覆盖。

应用发现读取运行实例元数据。窗口内容、AX、截图和输入在相应脚本调用时执行，并受 OS 原生权限及覆盖条件限制。DTW 不另设 grant、授权能力集合、审批回调或续租接口。

## 工具

| 工具 | 参数 | 返回行为 |
| --- | --- | --- |
| `apps` | `{}` | 应用列表、稳定 app IDs 和当前脚本合同 |
| `api` | 可选 `methods` 方法名数组 | 只读所选方法的完整参数/返回 schemas；省略时返回方法索引 |
| `exec` | `app, source`（仅这两项） | 等待有界脚本执行收尾，返回结果、步骤/动作摘要和最后一张截图 |
| `result` | `app`；可选 `execution_id` | 只读本会话该 app 最新仍保留的执行账本，或指定仍保留的历史记录，不执行、不重放 |

`app` 必须是平台稳定 ID 字符串。`exec` 的 `source` 是 JavaScript 异步函数体，可使用 `await` 和 `return`。DTW 在接受执行时自动生成 `execution_id`；模型不生成、传入或维护请求编号。当前内部总执行期限为 30 秒，不设调用次数配额；公开工具不接受预算、文件路径或请求编号。其余预算见[脚本合同](app-script-v1.md)。未知参数拒绝。源码仍限 64 KiB UTF-8；完整换行 MCP JSON 输入帧限 512 KiB，以容纳转义。

一次发现后执行窗口查询的工具参数示意：

```json
{
  "app": "com.apple.TextEdit",
  "source": "const inventory = await app.windows(); return inventory;"
}
```

每次执行都显式携带 `app`，不存在隐式默认应用。结果回显该 app ID，Harness 可以从普通工具入参与输出识别目标。脚本中的所有原子调用复用同一 app 绑定；窗口或 observation 字符串不能改变绑定。

对于已接受的执行，标准传输层同一个 JSON-RPC ID 与相同 exec 参数的重复帧复用原执行；仍在运行时合并等待，完成后返回原记录。同 ID 改换输入得到冲突错误。busy、参数错误等未接受调用不占执行记录，之后是否接受仍由当次准入决定。新的 JSON-RPC 调用始终代表新工作，即便 app/source 完全相同也会再次执行；不按脚本内容去重。这只用于同一存活连接内的传输识别，不提供跨连接 exactly-once 保证，也不要求定制 Host。

`result({app})` 只读此连接中该 app 最新仍保留的执行（包含 running、failed 或 completed），返回 `selection: "latest_accepted"`；`result({app, execution_id})` 选择指定历史记录。即使 app 已退出或重启，也不会解析新实例替代旧回执；不同会话记录不互通。这里的会话指 MCP 连接；复用同一连接的 Agent 也会共享该连接的 app state 和最近记录，DTW 不猜测模型对话身份。重复查询不会刷新观察、切换目标或延长期限。 已完成历史按滑动窗口回收，整条回执过期返回 `execution_unavailable`；缺失不证明没有效果，不构成重放依据。历史去重只覆盖仍保留的传输记录，应用最新 state 和存活 refs 不随历史回收重置。

“最近接受”不等于“最后尝试”：若新调用未收到或未被接受，最近记录仍可能是上一次执行。无记录或旧记录都不能证明不确定的新操作没有产生效果；需要结合客户端传输事实或独立观察判断，不能自动重放。

## 结果、效果与图像

MCP 工具响应同时提供文本 JSON，并在协议版本支持时提供 `structuredContent`。截图作为 MCP image content 返回，图片字节不进入脚本 VM。脚本结果与步骤账本见[脚本返回合同](app-script-v1.md#查询步骤账本与图像)。

`exec` 保留 `value`（脚本结果、console、调用数和最后截图元数据）、错误、状态提交及重建/驱逐信息。默认省略内部 `app_ref`、冗余 operation 和空 receipt；`script.steps` 替换为 `script.summary`，不改变保留的原始记录、`isError` 或实际派发。`result` 继续返回完整原记录，包括所有 `script.steps`、错误、原生回执和库存诊断；正常执行无需追加查询。

摘要包含 `steps`（记录步骤数）、`failed`（有 step.error 的步骤数）、`pending`（尚未完成数）和 `incomplete_reads`（不完整窗口库存，以及没有续页且覆盖不完整的语义查询步骤数）。正常语义分页不计入后者；该计数不改变成功读取的状态，脚本丢弃页面或稍后抛错也不会删除诊断。`result` 的 observe 步骤保留小型 `discovery` 覆盖信息，不保留整棵树。

同一次执行中，只有同一窗口的不同 observation 最终完整、且中间未出现动作回执、错误或未完成步骤，才增加可选的 `recovered_incomplete_reads`。窗口库存不参与此恢复判定，跨执行、其他窗口或复用同一 observation 也不能证明恢复。原 `incomplete_reads` 和 `last_issue` 保留；新字段说明只读刷新已经成立，不改写原读取，也不证明业务效果成功。

存在动作回执时，`actions` 按 dispatch、evidence、provider_dispatch、provider_evidence 累计计数，并报告 interrupted 数；不存在的状态键表示零。尚无回执不代表动作未派发，应同时查看 pending。`last_issue` 仅为最后一个错误/未知效果/证据不符/中断/未完成/不完整读取步骤的诊断样本；库存附上 `inventory`，语义查询附上 `discovery`。它既不是全部问题，也不一定是脚本失败根因；较早问题仍计入汇总，完整详情在原账本中。脚本捕获动作错误后可能成功提交 state，摘要不能将它改写为未发生错误。

投影层不会再次截断脚本返回值、console 或截图（原有 VM/console 限制仍有效）；这次改动控制的是默认账本展开量，并不承诺整个响应固定小于某个字节数。

执行完成与业务效果分开：动作记录 `not_dispatched / dispatched / unknown`，并单独记录 `confirmed / unverified / mismatch`。`dispatched` 不能证明保存、输入或点击达成预期；调用者检查独立观察。脚本先前的动作不会因后续异常、取消或 JSON state 未提交而回滚。

原生提供证据时，动作 `outcome` 及可选的 `provider_outcome` 保留 `delivery`：

| 字段 | 含义 |
| --- | --- |
| `route` | `semantic` / `background` / `foreground`；前台策略预检拒绝时为 null |
| `reason` | 路线选择或拒绝的诊断字符串，未知时为 null；不是授权依据 |
| `foreground_borrowed` | 是否保留了需要归还的临时前台所有权；不表示所有可能的焦点变化。Windows/Linux 当前前台输入不提供归还合同 |
| `focus_return` | `pending`、`restored`、`application_restored`、`changed_externally`、`previous_app_closed`、`unchanged`、`unconfirmed` 或 null |

`restored` 表示原窗口归还已证明；`application_restored` 仅证明原应用；`changed_externally` 表示所有权已被外部选择改变；`previous_app_closed` 表示原应用已退出；`unchanged` 表示没有待归还的借用；`unconfirmed` 表示不能证明收尾状态。缺少整个 `delivery` 时不推断路线或归还状态。路由证据与 `dispatch/evidence` 独立，不能据此前台/后台投递或一次成功归还推断输入业务效果。

显式 `app.focus` 先返回的动作 receipt 可保留 `focus_return: "pending"`；脚本收尾结果通过可选 `script.focus_return` 单独报告，原动作回执不被重写。已知待归还的前台在取消或 worker 关闭后失去精确结果时，保留 `unconfirmed`，不从正常退出码推断 `restored`；收尾错误继续独立出现在 `shutdown_error`。

紧凑投影仅在原生报告 delivery 时加入 `script.summary.actions.delivery`，按 `reported/routes/reasons/foreground_borrowed/focus_returns` 汇总，每个动作只计一次，优先采用 provider 原始证据。`script.summary.last_delivery` 保留最后一个报告样本及其 outcome/provider_outcome delivery；`script.focus_return` 在紧凑与完整记录中均保留。运行中断的 unknown 包装也保留 provider delivery，不伪造缺失证据。

原生诊断保留机器可读 code 和 message。OS 权限不足、窗口类型未支持、目标变化与原生未知错误应可区分；不能通过解析错误文字推断动作没有派发。

## 标准 MCP 取消和连接关闭

JSON-RPC 的 `id` 由标准客户端管理并标识传输请求；DTW 内部生成的 `execution_id` 标识保留的执行回执。模型不需要提供它们。工具执行期间可按 [MCP 取消规范](https://modelcontextprotocol.io/specification/2025-06-18/basic/utilities/cancellation)发送：

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/cancelled",
  "params": {"requestId": "the in-flight JSON-RPC id"}
}
```

取消针对该活动执行，不能证明已派发输入被撤销。客户端可能不接收被取消调用的最终响应；可用 `result({app})` 查询最近接受的执行，无需在提交脚本时预先指定 ID；已获得的 `execution_id` 可用于随后精确查询原记录。不要重提未确认效果的脚本来代替结果查询。

取消/超时可能重建受影响 app 的原生 owner（`context_reset: true`），已提交 state 继续保留，旧原生 refs 需重建；所有旧实例均被替换才清空 state（`state_reset: true`）；实例加入或退出保留仍存活实例的引用及 app 共享 state，旧实例引用不能移植到新进程。普通 app 切换保留各自上下文。EOF、断管、背压和父进程退出关闭整个会话及其原生资源；新进程不继承旧 state、refs 或账本。若连接丢失且无原始结果，只能将效果保留为未知并独立重观测。过程内回执不是跨进程持久审计系统。

## 兼容与证据

协议协商和消息编码由官方 SDK 管理；当前验收覆盖 2024-11-05、2025-06-18 的初始化流程，以及 2026-07-28 的逐请求 metadata 流程。超长帧由 SDK 丢弃后继续读取，未完成尾帧不执行；无效 JSON 和非法消息按 SDK 的标准行为处理。DTW 的 I/O 生命周期包装只负责 EOF、读写失败、输出持续停滞时取消执行并等待原生收尾，不参与协议解析。该合同不要求 elicitation、专用审批扩展或客户端自定义回调。MCP tool annotations 是提示，不是 DTW 的授权策略。

C4/C5 的 `dtw app serve --grant`、`exec.describe/submit/cancel/close` 流程属于前一接入设计，已退出产品入口。其[历史验收](app-protocol-acceptance.md)只适用于记录的代码和二进制，不证明当前入口已经通过相同验收。当前支持范围见[产品矩阵](product-matrix.md)。
