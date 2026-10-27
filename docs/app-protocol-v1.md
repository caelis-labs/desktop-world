# 单 app 协议 v1

当前 DTW 源码仅提供单 app 协议入口。DTW 持有 wire 类型、请求记录及执行语义；每个需授权的窗口读取与动作经过同一个 Core（低权限 catalog 直达原生 discovery）。[单 app 脚本 v1](app-script-v1.md) 复用同一授权和五个原子能力。裸 `dtw` 和 `dtw --help` 显示新版帮助；MCP 服务由 `dtw app serve --grant` 显式启动，启动 grant 由可信 Host 提供。

```sh
CARGO_TARGET_DIR=/private/tmp/dtw49-target cargo +1.98.0 build \
  --manifest-path cmd/dtw/Cargo.toml --locked --bin dtw
/private/tmp/dtw49-target/debug/dtw app catalog
/private/tmp/dtw49-target/debug/dtw app serve --grant /absolute/host-grant.json
```

协议、脚本和原生后端随默认构建启用，无需 Cargo feature。原生执行平台目前只有 macOS；其他平台的 app 操作明确返回 `unsupported_platform`。执行只使用 DTW 自有原生 worker，CUA SDK、CUA worker 和旧执行器依赖已移除。当前没有重写版发行包；历史发布包不代表当前源码，构建及发行边界见 [构建说明](building.md) 和 [发行说明](releasing.md)。

## 授权入口和身份

`catalog` 仅返回运行应用的低权限元数据：`identity`、`pid`、十进制字符串 `generation`、`name`。窗口 discovery 需要 `observe` 授权。Host 从可信 discovery 取得完整实例，展示 app、能力及绝对到期时间，经自身政策批准后写入启动配置：

```json
{
  "version": 1,
  "grant_id": "host-decision-identifier",
  "app": {"identity": "exact catalog identity", "pid": 123, "generation": "123456789"},
  "capabilities": ["observe", "press"],
  "expires_at_unix_ms": 1793082000000
}
```

示例身份和时间不能直接使用。启动文件必须是绝对路径、当前用户拥有的普通文件、权限精确 `0600`，最多 16 KiB；不跟随末端符号链接，不等待 FIFO。字段、类型和能力严格校验，重复能力及未知字段拒绝。有效期必须大于零且不超过 24 小时；绝对到期时间在加载后一次转换为单调时钟期限。修改文件不会修改已运行上下文，重启也不会延长同一文件的期限。启动时重新 discovery 并匹配完整 identity + pid + generation，不能只凭 PID 或标题选择应用。

该文件是**可信宿主的输入**，不是 DTW 自行作出的授权决定。MCP 请求不能提供 grant、approved、PID、能力扩张或续期。文件权限隔离其他账号，不构成同一用户任意本地代码的安全沙箱；能够改启动配置或执行任意本地程序的主体属于宿主信任域。Host 仍负责其他工具的授权，以及在撤销授权时关闭此上下文。

C4-macOS.1 的参考 Host 记录 proposal/decision，真实 Caelis 验收当时使用隔离 Store、临时 plugin 和已批准的启动文件。后续 C4-macOS.2 已在 Caelis 实际产品代码的本地 opt-in 入口接通 app/能力审批、可信 grant 创建及上下文撤销：9 次工具调用、2 次 canonical approval、一次授权动作的记录见 [C4 验收记录](app-protocol-acceptance.md#c4-macos2实际-harness-授权入口2026-10-27)。配套 Caelis 按用户要求保留本地，未推送、未发布；这不代表线上 Caelis/Bot 或交互 TUI 已具备该入口。模型参数始终不能作为审批事实。

## MCP 和请求

使用标准 stdio JSON-RPC，先 `initialize`，再 `notifications/initialized`。支持 MCP `2025-06-18`、`2024-11-05`；其他版本（包括 `2025-03-26`）返回当前支持的 `2025-06-18`，由客户端决定是否继续。`2025-03-26` [要求接收 batch](https://modelcontextprotocol.io/specification/2025-03-26/basic#batching)，本切片不实现批请求，因此不宣称该版本兼容。只公开一个 `exec` tool；`tools/list` 的 JSON Schema 描述请求结构和数值范围，严格解析器另执行脚本源码的 UTF-8 字节限制，传输层限制完整帧大小。未知字段和版本拒绝，标识符为 1–128 个 Unicode 字符的字符串，坐标必须有限。

| exec 操作 | 字段 | 行为 |
| --- | --- | --- |
| `describe` | `version: 1, operation: "describe"` | 返回 app_ref、完整实例、固定 grant、后端支持能力、上下文状态、限制及覆盖范围 |
| `submit` | 上述 version/operation，加 `request_id, target, command` | 一个活动任务，立即返回 running 或 completed；忙时拒绝，无执行队列 |
| `result` | `request_id` | 读取已接受任务的原始记录，不等待、不执行、不重新授权 |
| `cancel` | `request_id` | 仅命中当前活动 ID 时关闭整个上下文并停止 worker；读最终结果仍用原 ID |
| `close` | 无其他字段 | 关闭准入并释放原生资源；允许继续查询已有记录，直到进程退出 |

`target` 总是包含 `app_ref`；原子 `observe/capture/press/click` 还必须包含 `window_ref`；`windows` 和 `script` 必须省略该字段，传 `null` 也拒绝。只有当前连接签发的引用可用，引用不暴露 Core 数值、native window ID、provider stamp 或 AX token。

| command.kind | 附加字段 | 授权及引用 |
| --- | --- | --- |
| `windows` | 无 | `observe`；准入后旧观察即失效；成功后替换窗口引用集合 |
| `observe` | 无 | `observe`；返回 semantic observation_ref 和 element_ref/label/can_press |
| `capture` | 无 | `capture`；返回 pixels observation_ref、图片尺寸/坐标系及 MCP PNG |
| `press` | `observation_ref, element_ref` | `press`；必须来自当前语义观察 |
| `click` | `observation_ref, x, y` | `click`；必须来自当前像素观察，坐标系为 image_pixels |
| `script` | `source`；可选 `timeout_ms, max_calls` | app-only target；每步分别需要上述能力，见 [脚本 v1](app-script-v1.md) |

窗口 discovery 需要 `observe`，Host 应明确授予所需组合；DTW 不自动补充能力。平台覆盖继承 C2/C3：同 PID、标准 layer-0 AX 窗口；无 sheet/menu/helper routing 或自动聚焦。窗口 discovery 或新观察一经 Core 准入，旧观察即失效，即使后端读取失败也不能恢复；新观察成功才发布新的引用。动作消费其观察。错误 app、错误窗口或无效坐标等准入失败不自动消费仍有效的观察；Core 决定是否仍可使用。AXPress 或输入派发返回成功不等于效果已确认，必须独立读回。

macOS `windows` 仅在归属可证明时发布集合。AX 库存为空但仍有合格同 PID CG 表面、精确 ID/owner 无法证明或部分候选不明时，既有 `backend_failed` 消息保留有界 provider 诊断，不发布部分或伪空集合。明确范围排除仍可得到空集合。闭窗后 CG 残留也可能报未证明，不能据此断言 AX 损坏；详见 [C5.2 库存边界](window-inventory-acceptance.md)。

请求示例：

```json
{"version":1,"operation":"submit","request_id":"observe-1","target":{"app_ref":"app_...","window_ref":"window_..."},"command":{"kind":"observe"}}
```

## 记录、回执与取消

任务记录固定包含 `version, request_id, app_ref, operation, status, value, error, receipt, payload_expired`；脚本额外包含 `script` 进度及步骤账本，其动作回执位于 `script.steps[].receipt`。完成时 `status` 为 `completed`，并不表示动作成功。错误为 `{code,message}`；动作回执保留三个独立字段：

```json
{
  "outcome": {"dispatch":"unknown","evidence":{"kind":"unverified"},"error":{"code":"cancelled","message":"..."}},
  "provider_outcome": {"dispatch":"unknown","evidence":{"kind":"unverified"},"error":{"code":"backend_failed","message":"..."}},
  "interruption": {"code":"cancelled","message":"..."}
}
```

`dispatch` 只取 `not_dispatched / dispatched / unknown`；`evidence.kind` 只取 `unverified / confirmed / mismatch`，后两项附明确 postcondition 或 expected/observed。协议逐字段编码 Core 回执，不用 Debug 文本推断状态，也不把 `unknown` 转成失败未发生。取消后可能有迟到效果，不自动重放。

同一已接受 request_id + 相同 target/command 返回已有记录；更换参数返回 `request_conflict`。busy、wrong_app 等尚未接受的 ID 不被占用。重复提交不等于重新派发，也不重新激活旧观察。此保证只在当前进程上下文成立，不能用于断线后恢复或跨上下文重试。进程退出后没有持久化结果账本；未收到结果的动作必须视为不确定并先读回。

取消检查活动 ID 与关闭准入在同一锁内完成，旧 ID 的迟到取消不能停止新的任务。cancel/close、超时或授权到期的首个终态保留；关闭不会重新开放上下文。已有非取消终态时，后续 cancel 返回 `cancel_requested: false` 与真实的 `context_state: "closed"`。`cancel_requested: true` 只说明关闭请求命中了活动记录，最终效果以原子任务的 `result.receipt` 或脚本的 `result.script.steps[].receipt` 为准：若 Core 已完成而记录尚未发布，不改写已经返回的真实回执。`shutdown_error` 明确报告清理失败。MCP `notifications/cancelled` 不作为原生任务取消，因为对应的提交/查询传输调用已经完成；必须调用 `exec.cancel`。

`error` codes 包括 Core 的 `closed, cancelled, busy, grant_expired, wrong_app, capability_denied, unsupported_capability, stale_observation, wrong_provider, target_changed, invalid_observation, invalid_point, element_not_found, element_not_pressable, timed_out, backend_failed`，以及协议的 `invalid_request, unsupported_version, request_conflict, request_limit, unknown_request`。脚本的 `script_*` 及细分 failure codes 见 [脚本错误与账本](app-script-v1.md#查询步骤账本与图像)。启动及主读取循环的传输错误通过 stderr 的版本化 error 对象与非零退出码报告；输出断管直接清理并以 2 退出，不保证附带 stderr 对象。

结果的 MCP text 内容是协议 JSON；`2025-06-18` 同时返回相同 `structuredContent`。截图通过额外 image content 携带，文本只有尺寸/坐标元数据。旧 MCP 版本只返回 text/image，不附 structuredContent。

## 有界生命周期与下一步

每上下文最多 64 个已接受 ID，记录和回执保留至进程退出；不循环复用 ID。结果 payload 总预算 64 MiB，超限时从旧记录依次移除 value/image 并设置 `payload_expired: true`，不删除提交身份、动作回执或脚本步骤账本，也不重放以恢复内容。脚本 state/result 还有各自的 64 KiB 限额。引用注册表只保留当前窗口集合（最多 256）和一份观察；Core 观察 TTL 为 15 秒。

输入必须是完整换行 JSON，单帧至多 64 KiB。输出由独立线程处理，最多缓冲 8 条响应；背压饱和、断管、EOF 和宿主父进程退出都会关闭原生上下文。输出阻塞不会占用取消/EOF 的读取线程。常规关闭只有一处资源清理：先调用原生 shutdown，再等待执行收尾最多 2 秒，正常 EOF 的输出 drain 最多 200 ms；这不是整个进程退出的总上界，原生 shutdown 仍遵循 C2 的有界 worker 合同。Host 若在动作返回前关闭传输，必须按不确定效果处理。

C4-macOS.1 的协议/并发、真实 MCP + GUI 和 Harness 预授权证据，以及 C4-macOS.2 实际产品代码的本地授权闭环，均保留在 [C4 验收记录](app-protocol-acceptance.md)。C5 以此基线增加 [单 app 脚本](app-script-v1.md)，本协议说明不替代其独立验收。Windows C2/C3、额外能力组、配套 Caelis 发布和新入口分发包验收均独立后置。

MCP 依据：[生命周期](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle)、[工具、文本与图片结果](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)。本次验证见 [C4 验收记录](app-protocol-acceptance.md)。
