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
| `exec` | `app, request_id, source`；可选 `timeout_ms, max_calls` | 等待有界脚本执行收尾，返回结果、回执和最后一张截图 |
| `result` | `app, request_id` | 只读原请求记录，不执行、不重放 |

`app` 必须是平台稳定 ID 字符串。`exec` 的 `source` 是 JavaScript 异步函数体，可使用 `await` 和 `return`。`request_id` 是调用者提供的 1–128 字符操作标识，应为每项新工作选择新值。可选预算只能缩小默认上限：总执行时间 5000 ms、最多 32 次 Host bridge 调用。其余预算见[脚本合同](app-script-v1.md)。未知参数拒绝。源码仍限 64 KiB UTF-8；完整换行 MCP JSON 输入帧限 512 KiB，以容纳转义。

一次发现后执行窗口查询的工具参数示意：

```json
{
  "app": "com.apple.TextEdit",
  "request_id": "inspect-windows-1",
  "source": "const inventory = await app.windows(); return inventory;"
}
```

每次执行都显式携带 `app`，不存在隐式默认应用。结果回显该 app ID，Harness 可以从普通工具入参与输出识别目标。脚本中的所有原子调用复用同一 app 绑定；窗口或 observation 字符串不能改变绑定。

同一已接受 request ID 与相同请求返回原记录；更换内容得到冲突错误，不重新执行。重复查询不会刷新观察、切换目标或延长执行期限。`result` 读取原请求记录；即使 app 已退出或重启，也不会解析新实例来替代旧回执。

## 结果、效果与图像

MCP 工具响应同时提供文本 JSON，并在协议版本支持时提供 `structuredContent`。截图作为 MCP image content 返回，图片字节不进入脚本 VM。脚本结果与步骤账本见[脚本返回合同](app-script-v1.md#查询步骤账本与图像)。

执行完成与业务效果分开：动作记录 `not_dispatched / dispatched / unknown`，并单独记录 `confirmed / unverified / mismatch`。`dispatched` 不能证明保存、输入或点击达成预期；调用者检查独立观察。脚本先前的动作不会因后续异常、取消或 JSON state 未提交而回滚。

原生诊断保留机器可读 code 和 message。OS 权限不足、窗口类型未支持、目标变化与原生未知错误应可区分；不能通过解析错误文字推断动作没有派发。

## 标准 MCP 取消和连接关闭

JSON-RPC 的 `id` 标识传输请求；`exec.request_id` 标识操作回执。工具执行期间可按 [MCP 取消规范](https://modelcontextprotocol.io/specification/2025-06-18/basic/utilities/cancellation)发送：

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/cancelled",
  "params": {"requestId": "the in-flight JSON-RPC id"}
}
```

取消针对该活动执行，不能证明已派发输入被撤销。客户端可能不接收被取消调用的最终响应；保留原 app ID 和 `request_id`，使用 `result` 查询其记录。不要用新的 ID 重提未确认效果的脚本。

EOF、断管、背压和父进程退出关闭原生资源；应用上下文的关闭使旧 observation 和 state 不再可用。新进程不继承旧账本。若连接丢失且无原始结果，只能将效果保留为未知并独立重观测。过程内回执不是跨进程持久审计系统。

## 兼容与证据

普通 MCP 初始化、工具发现和调用遵循服务器广告的协议版本。该合同不要求 elicitation、专用审批扩展或客户端自定义回调。MCP tool annotations 是提示，不是 DTW 的授权策略。

C4/C5 的 `dtw app serve --grant`、`exec.describe/submit/cancel/close` 流程属于前一接入设计，已退出产品入口。其[历史验收](app-protocol-acceptance.md)只适用于记录的代码和二进制，不证明当前入口已经通过相同验收。当前支持范围见[产品矩阵](product-matrix.md)。
