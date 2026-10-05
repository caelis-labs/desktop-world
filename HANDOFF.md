# Desktop World 接入与 RC 交接

建议接入 **Go `host` SDK + 独立 helper**。Go library 是执行内核；helper 隔离原生 provider 阻塞，Bot Runtime 保持应用/回合授权、停止和工具输出的最终控制权。

当前源码处于 `v0.1.0-rc.1` 验收阶段，许可为 [MPL-2.0](LICENSE)。Windows 11 amd64 日常任务、状态语义、Electron 及短前台事务已通过真实操作验证。下一步是从最终同一提交完成 macOS arm64 复验，步骤见 [RC 验收指南](docs/rc-validation.md)。本轮不自动更新实际 Bot 固定版本、创建 tag 或上传 Release；历史 alpha.6 下载包不包含这些 Windows 变更。

1. 从待验收的干净提交构建包，核对 `SHA256SUMS` 并解压到可信固定路径。运行包内 `dtw version` 和 `doctor`，确认平台、协议与 `vcs.revision`。包包含对应完整源码、客户端、文档和许可。Windows 包未签名；Mac 包 ad-hoc 签名且未经公证，按系统正常安全与权限流程处理。
2. 将宿主 Go 模块和 helper 固定为同一提交/版本。使用 `host.Start` 启动 helper。示例：`go run ./examples/bot-host --helper /absolute/path/bin/dtw`，默认只读。
3. Runtime 调用 `BeginTurn`，依据用户授权向精确目标应用 `Grant`。模型只获得数据入口，不获得授权通道、helper 路径或回合选择权。`host.Content` 总预算 32 KiB，保留原回执及恢复失败原因。
4. 用户停止、会话结束或审批失效时调用 `EndTurn`。不确定结果使用同一请求的 `Reconcile`，不换 ID 重发。退出时 `Close`；先等待取消与清理，再考虑终止 helper。强杀后尚持有的系统键鼠不能假设已自动释放。

macOS / Windows amd64 可通过 `host.Options{InputMode: desktopworld.InputModeCooperative}` 使用后台语义和短前台事务。默认 `shared` 及 `no_shared_input` 权限上限保持原契约。Windows 采用公开的 SetForegroundWindow / UIA / SendInput，Mac 动态探测 SkyLight 私有 SPI；两者共享用户桌面，无法保证任意 provider 和系统版本无干扰。

接入详情见 [宿主管理](docs/bot-integration.md)、[示例](examples/bot-host/main.go)、[JavaScript 调用链](docs/scripting.md) 和 [输入事务](docs/cooperative-input.md)。逐项证据见 [Windows 报告](docs/windows-validation.md) 及 [Mac 历史实机报告](poc/background-input/FULL_ACCEPTANCE.md)。

当前未接入实际 caelis-bot/Wails 打包进程；更多机型、最低系统版本、多屏/RDP/复杂 IME 尚未形成完整实机矩阵。历史独立 Agent 文件任务仍有超过十分钟的记录，本轮脚本验收没有运行 LLM，不能据此宣布通用易用性标准全部通过。
