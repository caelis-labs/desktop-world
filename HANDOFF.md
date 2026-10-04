# caelis-bot 联调交接 · v0.1.0-alpha.5

建议接入 **Go `host` SDK + 独立 helper**。Go library 保持唯一执行内核；helper 隔离原生阻塞，Bot Runtime 保持应用 × 回合授权、停止和工具输出的最终控制权。

以下下载步骤对应已公开 alpha。当前源码命令已缩短为 `dtw`，新增批次及实机状态见 [feature 记录](docs/features.md)。本批按 alpha.5 分发；进入 caelis-bot 的 M0 更新与统一联调后置，以下是后续接入指引，并未修改 Bot 固定版本。

1. 从 [GitHub prerelease](https://github.com/caelis-labs/desktop-world/releases/tag/v0.1.0-alpha.5) 下载 macOS arm64 包，核对 `SHA256SUMS`，解压到可信固定路径。运行 `bin/dtw version` 和 `doctor`。这是 ad-hoc 签名、未经公证的开发包；首次运行按 macOS 正常安全与权限提示处理，不要求关闭系统保护。
2. Bot 开发分支固定 Go 模块 `github.com/caelis-labs/desktop-world@v0.1.0-alpha.5`，使用 `host.Start` 启动包内 helper。示例：`go run ./examples/bot-host --helper /absolute/path/bin/dtw`，默认只读。
3. Runtime 调用 `BeginTurn`；观察到精确应用 Ref 且用户批准后才调用 `Grant`。模型仅获得数据操作入口，不获得授权通道、helper 路径或回合选择权。输出经 `host.Content` 紧凑呈现，总预算为 32 KiB，保留原始收据。
4. 用户停止、会话结束或审批失效时调用 `EndTurn`。不确定结果用同一请求的 `Reconcile`，不换 ID 重发输入。宿主退出 `Close`；未知副作用和强杀后的输入状态仍需核对。

详见 [接入说明](docs/bot-integration.md)、[可执行示例](examples/bot-host/main.go)、[Agent 脚本调用链](docs/scripting.md)。发布包也包含整个 Go 源码模块。

本轮真实 AppKit 验证已完成：Unicode 设值与 Enter 提交、相同请求不重复提交、结束回合后新输入被拒绝、原收据仍可恢复。独立应用日志和界面均确认只提交一次。自动检查覆盖 race、vet、Windows 交叉构建、协议和 JavaScript 调用链。

边界：尚未接入实际 caelis-bot/Wails 打包进程；Windows 真实输入、macOS amd64/最低系统版本未验收。共享前台焦点与系统鼠标，无后台独立座席。Antigravity 真实任务能完成主要文件任务，但上一轮总耗时 10分42秒，仍超过 10 分钟目标；不能称为易用性验收全部通过。

当前仅公开预发布，**不授予开源许可**，见 [NOTICE](NOTICE)。

macOS 宿主可显式配置 `host.Options{InputMode: desktopworld.InputModeCooperative}` 开启同一桌面的短前台事务；默认 shared 和 no_shared_input 权限上限保持原契约。先阅读 [输入模式、预算与恢复](docs/cooperative-input.md) 及 [逐项实机验收](poc/background-input/FULL_ACCEPTANCE.md)。此模式依赖动态探测的私有 key-focus SPI，不承诺任意应用、最低 macOS 或 Windows 可用。Bot 的固定版本与 M0 联调未更新。
