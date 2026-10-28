# 标准 MCP App 协议 v1

产品入口为 `dtw mcp`。Agent 使用普通 stdio MCP 配置和可选的 [DTW Skill](../skills/desktop-world/SKILL.md)，无需 DTW 专用 Host。审批与授权完全属于 Harness；DTW 只要求显式平台稳定 app ID，并执行目标、引用、预算和生命周期校验。

## 应用发现与稳定 app ID

`apps` 工具接受空对象，返回去重后的应用列表、脚本方法描述、schemas、限制和原生支持条件。应用条目形如：

```json
{"app": "com.apple.TextEdit", "name": "TextEdit"}
```

`app` 是平台稳定唯一标识；macOS 使用 bundle ID。`name` 只用于展示。调用者复制返回的 app 字符串，不自行组合路径、PID 或代际。进程解析、进程出生代际和窗口归属全部由 DTW 内部维护；app ID 不是秘密、授权 token 或用户批准证明。

执行时同一 ID 对应多个独立可操作应用实例则返回 `ambiguous_app`，不猜选；普通 Chrome helper 不作为独立应用条目。应用重启不改变 app ID，但旧实例的 state 和 observation 不会移植到新进程。

应用发现读取运行实例元数据。窗口内容、AX、截图和输入在相应脚本调用时执行，并受 OS 原生权限及覆盖条件限制。DTW 不另设 grant、授权能力集合、审批回调或续租接口。

## 工具

| 工具 | 参数 | 返回行为 |
| --- | --- | --- |
| `apps` | `{}` | 应用列表、稳定 app IDs 和当前脚本合同 |
| `exec` | `app, source`；可选 `timeout_ms, max_calls` | 等待有界脚本执行收尾，返回结果、回执和最后一张截图 |
| `result` | `app`；可选 `execution_id` | 只读本会话该 app 最近接受的执行，或指定原记录，不执行、不重放 |

`app` 必须是平台稳定 ID 字符串。`exec` 的 `source` 是 JavaScript 异步函数体，可使用 `await` 和 `return`。DTW 在接受执行时自动生成 `execution_id`；模型不生成、传入或维护请求编号。默认总执行期限为 30 秒，可通过 `timeout_ms` 调大或调小；省略 `max_calls` 不设调用次数配额，显式提供时按调用方配额执行。两者均为正整数，编码范围为 u32。其余预算见[脚本合同](app-script-v1.md)。未知参数拒绝。源码仍限 64 KiB UTF-8；完整换行 MCP JSON 输入帧限 512 KiB，以容纳转义。

一次发现后执行窗口查询的工具参数示意：

```json
{
  "app": "com.apple.TextEdit",
  "source": "const inventory = await app.windows(); return inventory;"
}
```

每次执行都显式携带 `app`，不存在隐式默认应用。结果回显该 app ID，Harness 可以从普通工具入参与输出识别目标。脚本中的所有原子调用复用同一 app 绑定；窗口或 observation 字符串不能改变绑定。

对于已接受的执行，标准传输层同一个 JSON-RPC ID 与相同 exec 参数的重复帧复用原执行；仍在运行时合并等待，完成后返回原记录。同 ID 改换输入得到冲突错误。busy、参数错误等未接受调用不占执行记录，之后是否接受仍由当次准入决定。新的 JSON-RPC 调用始终代表新工作，即便 app/source 完全相同也会再次执行；不按脚本内容去重。这只用于同一存活连接内的传输识别，不提供跨连接 exactly-once 保证，也不要求定制 Host。

`result({app})` 只读此连接中该 app 最近接受的执行（包含 running、failed 或 completed），返回 `selection: "latest_accepted"`；`result({app, execution_id})` 选择指定历史记录。即使 app 已退出或重启，也不会解析新实例替代旧回执；不同会话记录不互通。这里的会话指 MCP 连接；复用同一连接的 Agent 也会共享该连接的 app state 和最近记录，DTW 不猜测模型对话身份。重复查询不会刷新观察、切换目标或延长期限。

“最近接受”不等于“最后尝试”：若新调用未收到或未被接受，最近记录仍可能是上一次执行。无记录或旧记录都不能证明不确定的新操作没有产生效果；需要结合客户端传输事实或独立观察判断，不能自动重放。

## 结果、效果与图像

MCP 工具响应同时提供文本 JSON，并在协议版本支持时提供 `structuredContent`。截图作为 MCP image content 返回，图片字节不进入脚本 VM。脚本结果与步骤账本见[脚本返回合同](app-script-v1.md#查询步骤账本与图像)。

执行完成与业务效果分开：动作记录 `not_dispatched / dispatched / unknown`，并单独记录 `confirmed / unverified / mismatch`。`dispatched` 不能证明保存、输入或点击达成预期；调用者检查独立观察。脚本先前的动作不会因后续异常、取消或 JSON state 未提交而回滚。

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

取消/超时可能重建受影响 app 的原生 owner（`context_reset: true`），已提交 state 继续保留，旧原生 refs 需重建；实际 app 实例更换才清空 state（`state_reset: true`）。普通 app 切换保留各自上下文。EOF、断管、背压和父进程退出关闭整个会话及其原生资源；新进程不继承旧 state、refs 或账本。若连接丢失且无原始结果，只能将效果保留为未知并独立重观测。过程内回执不是跨进程持久审计系统。

## 兼容与证据

普通 MCP 初始化、工具发现和调用遵循服务器广告的协议版本。该合同不要求 elicitation、专用审批扩展或客户端自定义回调。MCP tool annotations 是提示，不是 DTW 的授权策略。

C4/C5 的 `dtw app serve --grant`、`exec.describe/submit/cancel/close` 流程属于前一接入设计，已退出产品入口。其[历史验收](app-protocol-acceptance.md)只适用于记录的代码和二进制，不证明当前入口已经通过相同验收。当前支持范围见[产品矩阵](product-matrix.md)。
