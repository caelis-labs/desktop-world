# 外部 Agent 接入

目标：少量有界观察、一次短计划、完整回执；宿主负责动态授权，Agent 只获得桌面 API。Go、TypeScript、Python 和 Rust 使用同一原生执行内核。TS/Python/Rust 通过 `dtw session` 的版本化 NDJSON 通信；Python/Rust 无需 Node，也无需自行实现 Windows HANDLE 继承。

## 选择入口

| 使用方式 | 入口 | 运行要求 |
| --- | --- | --- |
| 本机 Agent Plugins 1.0 正式包候选 | `plugin.json`、`mcp.json`、`skills/desktop-world` | 随包 Node 24 LTS 和原生 helper；无需系统开发工具；`desktop_exec` 需要客户端审批完整脚本 |
| Agent 的持久脚本工具 | `clients/javascript/desktop.mjs` | Node 20+；无 npm 依赖 |
| TS 编排程序 | `clients/typescript` 的 `HostSession` / `DesktopClient` | Node 20+；发布包包含 JS 与声明文件 |
| Python 编排程序 | `clients/python` 的异步 `HostSession` / `DesktopClient` | Python 3.11+；运行时无第三方依赖 |
| Rust 编排程序 | `clients/rust` 的 `HostSession` / `DesktopClient` | Rust 1.75+、Tokio、Serde |
| Go 宿主 | `host.Start`、`BeginTurn` / `Grant` / `Declare` / `Revoke` / `EndTurn` | Go 1.23+ |

SDK 宿主可只向模型注册 `observe/read/sync/act/capture/get/cancel` 或其窄封装。标准 Plugin 默认只注册 `desktop_exec` 和 `desktop_status`；前者会执行任意本地 JavaScript，不能把工具 annotation 当作审批机制。不要把 `HostSession`、宿主控制通道、helper 路径或回合选择作为桌面工具参数。JS runner 的 VM 与同用户 worker 不是安全沙箱。Plugin 安装和数据/权限生命周期见 [通用宿主契约](agent-plugin-host-contract.md)。

## 启动与动态授权

优先用 `--write-app` 声明精确 APP 名称，重复参数可批准多个 APP。尚未启动的 APP 不阻止会话启动；声明显示为 `pending`。可信宿主也可在会话期间调用 `declare(name)` 或 `grant(observedApplicationRef)`，动态追加授权，不重启 helper、不丢失 Ref 和回执。

```sh
# 由可信宿主保持 stdin 打开，SDK 会自动管理此进程。
dtw session --input-mode cooperative --write-app '待启动的 APP' --session harness/owner.json
# 启动输出含 epoch、功能版本、实际 audit_path。
dtw auth list --session harness/owner.json
dtw auth add --session harness/owner.json --app '第二个 APP' --id approve-app-2
dtw auth add --session harness/owner.json --app-ref '<观察到的 application Ref>' --id approve-ref-2
dtw auth revoke --session harness/owner.json --grant '<授权 ID>' --id revoke-2
```

Windows 在 PowerShell 中使用 `dtw.exe`，参数和 JSON 协议相同。CLI 的 `--session` 指向原生 **owner descriptor**；JS runner 的 exec descriptor 是另一文件，启动输出明确列出两者。

| 状态 | 含义与处理 |
| --- | --- |
| `pending` | 尚未运行；继续观察，可信宿主启动 APP 后再查看状态或执行 |
| `unresolved` | 原生覆盖不完整，无法证明唯一性；收窄或直接批准观察到的 APP Ref |
| `ambiguous` | 多个同名实例；使用精确窗口标题或观察到的 APP Ref |
| `active` | 已绑定一个确切的当前 APP 实例 |
| `expired` | 已绑定实例退出；新实例不会继承权限，宿主须显式重新声明或授权 |
| `revoked` | 宿主撤销，阻止后续输入并取消涉及该 APP 的在途调用 |

`grants()` 和写计划前的有界观察刷新声明。唯一性必须由完整、未截断、无 dirty/unavailable 的发现证明。`--write-app-window` 授权匹配窗口所属 APP，范围仍是 APP。撤销一个已绑定授权会同时撤销同 APP 的其他绑定声明；撤销尚未绑定的声明只撤销该声明。每回合最多 32 个声明；新回合权限清空，结束的回合 ID 不可复用。APP 更名、退出和重启后不按名字自动重新绑定。

直接 `dtw serve` 保留旧的七操作协议，启动声明也支持延迟绑定；动态管理使用 `host` 私有控制管道或 `dtw session`，不能从模型数据报文授权。`--desktop-write` / `--raw-input` 是显式的旧入口，不是 SDK 的默认授权模式。

## 观察与计划

1. `observe()` 默认 32 个 summary 结果 / 8 KiB，只请求 name、role、app、window。
2. 在返回的窗口中 `find()` / `outline()`，按需加 states、capabilities、value_preview、uri。`version` 是固定返回元数据，不是 fields 参数。
3. 检查 coverage；不完整的零结果不能证明不存在。`next(originalObservation)` 保留原查询，原生页系列的输出上限仍生效。Python 保存最近 64 个分页观察对象，较早对象需显式重新观察。
4. `plan()` 在本地聚合，`act()` 一次提交，最多 16 步 / 10 秒。JS/TS 的 `transaction(tx=>...)` 同样只聚合计划，回调内不进行桌面读取或单独 action。
5. `focus` 与后续快捷键必须在同一计划。用 `bindFocus` / `bind_focus` 在执行时绑定观察到的 APP/窗口中的焦点 UI；焦点在范围外时明确拒绝。不要在一个独立 `focus()` 完成后再读取焦点并发送快捷键。
6. 回执区分 delivery 和 verification。dispatch 完成不等于业务成功，随后独立观察最终文本、状态或 APP 自身证据。

```javascript
await dw.transaction(tx => {
  tx.focus(windowRef);
  const focused = tx.bindFocus('input', windowRef);
  tx.press(focused, 'O', ['primary']);
});
```

`primary` 对应 macOS Command / Windows Control。事务不会回滚已投递输入，也不会为失败动作重发、换目标或切换自动化通道。`no_shared_input` 在执行前拒绝包含键鼠操作的整份计划。

## 错误、停止与恢复

客户端保留完整 Fact 状态、coverage、十进制字符串版本、Fault 扩展和原始回执，包括已投递的失败/部分结果。显式 false、空字符串、未请求和 unknown/redacted 不相互替换。宿主根据这些事实作决定。

每个会话请求使用稳定 ID。相同 ID 和相同 channel/op/body 查询原请求；不同 body 返回 `request_conflict`。`reconcile(id)` 等待原记录，不重新发送动作。SDK 不自动重启、重试或换 ID。保留 `DesktopError.reply/receipt`（Rust `Error.reply`），以及 `get(run_id)` / `cancel(run_id)`。每个会话保留最多 4096 请求，SDK 持有记录直到关闭会话。只有预输入拒绝已确认后，宿主才能决定以新 ID 创建纠正后的请求。

停止时可信宿主 `endTurn/end_turn`；读写在途调用都被取消，已产生的效果仍需核对。Python task 取消及 Rust future 丢弃会发送结束当前回合请求，并保留原回执；JavaScript/TS 调用者应显式 `endTurn`，单纯放弃 Promise 不取消原生动作。关闭 stdin 或丢失宿主私有控制管道会撤销权限。`close_incomplete` 表示超时后被迫终止进程，不能据此证明原生输入清理完成。

## 日志与兼容性

`--audit PATH` 默认碰撞轮转，hello 返回实际路径。`--audit-mode create` 独占创建；`append` 需要同一文件的排他 writer lock 和完整末行，不修复历史文件。所有模式均持有 writer lock；崩溃残留 lock 须先确认原进程已退出再显式恢复。元数据记录 session epoch、输入策略、调用大小、授权状态和回执摘要，排除 UI 文本、APP 名称声明及脚本内容。

JS 每次启动生成独立 `runs/<uuid>`，默认只有元数据。host.json 中 `"debug":true` 才写完整 wire 和 script-code，它们可能含敏感内容。私有日志、截图、owner 文件均不打入发行包；目录 0700 / 文件 0600 是 POSIX 权限，Windows 私有 owner named pipe 使用当前用户 SID DACL，日志隐私依赖宿主目录 ACL。没有 TCP 服务或公网访问要求。

rc.2 的 Windows 实机验收保留在 [历史交接](rc2-windows-handoff.md)。rc.3 的 macOS Chrome 语义修复和本轮验证边界见 [发行说明](releases/v0.1.0-rc.3.md)。自动检查不等于 Windows 桌面实测。
