# OpenCode、Pi 与 Grok Build 安装及基本 GUI 验收

2026-11-17 在本机 macOS 27.2 arm64 上，使用用户已配置的 DeepSeek Flash，完成三个宿主的安装和新会话验证。三个宿主都实际加载了 DTW Skill，通过自己的实时 MCP 工具完成观察、截图、Unicode 输入和原生按钮操作。独立 fixture 日志核实三次不同内容的提交及三次递增，各发生一次；完成后激活三个宿主绑定，自有测试窗口正常退出。

这是基本安装和桌面可用性证据，不替代完整 Computer Use 基准或真实办公应用任务。结构化结果及证据摘要见 [summary.json](evidence/agent-install-2026-11-17/summary.json)。

## 安装路径与包身份

| 宿主 | 模型选择 | 实际安装路径 | 包与结果 |
| --- | --- | --- | --- |
| OpenCode 2.3.0 | `magpie/deepseek/deepseek-flash` | 用户 JSONC 的 `mcp.servers.dtw` 与 `skills` 数组；`opencode reload` | 官方 v0.5.0 完整包；安装、实时工具/Skill、基本 GUI 通过 |
| Pi 1.5.2 | provider `magpie`，model `deepseek/deepseek-flash` | `pi install <managed-package>`，原生 Pi Package 同时声明 extension 与 Skill | 本地未发布的 Pi 打包候选；安装、实时工具/Skill、基本 GUI 通过 |
| Grok Build 1.0.77 | `magpie/deepseek/deepseek-flash` | `grok plugin install <managed-package> --trust`，加载现有兼容插件 | 官方 v0.5.0 完整包；功能通过，有宿主读取恢复和额外探测记录 |

OpenCode 与 Grok 的官方 macOS ZIP SHA-256 为 `6fa2f35bca9eaf5a178c93353e570ce906fa742b27316328b6e82f78fffa74a6`，导入后的完整包 ID 为 `8c2ad64938243b34d8fd7cb272467b531c40e95451e68f1bed03074f8246cd07`。Grok 使用其插件管理器创建的完整复制件，`${CLAUDE_PLUGIN_ROOT}` 正确展开到复制件；其路径、版本和内容与托管源包核对一致。该结果证明 Grok/Claude 兼容入口可用，不等于验证了 Grok 支持 Agent Plugins 1.0。

Pi 候选完整包 ID 为 `1737c4e770107006551f0ec588eaba0595442596a331e2af628066b1b6fcb785`。它使用官方 v0.5.0 的原生可执行文件和 canonical Skill，只增加宿主适配及打包内容。原生二进制 SHA-256 为 `39e2bb4aecb2652a1b0fd6e6ee850aff10031e8f9398decb5ce156159dd86074`，`SKILL.md` 为 `385c2e278c17b0d62c308e81b36426b758ee7d85bf9fd844f105c027cc1c8db7`，Pi 入口为 `faf2fe8500fac852de9e2468200fb6e35c6e5c84e833a6b33cbc5915de754750`。候选明确标记 `unpublished-host-adapter-candidate`，没有修改已发布包。

**公开 v0.5.0 不包含 Pi 原生入口。** 本次通过的是新增打包入口的本地候选；安装指南必须先检查包的 Pi metadata 和入口文件，不能仅凭 `pi install` 成功或只发现 Skill 就报告 DTW 可用。

## 执行方式与独立效果

OpenCode 和 Grok 通过本机 acpx 0.21.1 调用已有 ACP 入口；Pi 使用已有原生 `--mode json --print`，没有引入外部 Pi ACP 适配器。每个宿主分别由 DeepSeek Flash 完成安装，再启动一个新会话执行 GUI 任务。验证没有注入临时 MCP 配置，也没有用独立客户端代替宿主已安装的连接。

GUI 操作串行运行，目标是唯一的自有 AppKit Computer Use 窗口。acting Agent 可以读取已安装 Skill/API，业务动作只能使用宿主提供的 DTW `apps`/`exec`，不能读取 fixture 私有状态、源码、HTTP 或几何答案。任务是将 Native note 替换为指定内容、点击 Commit note 一次、递增计数器一次并截图。三个宿主均使用实际 `typeText` 键盘输入，而非 `setValue`。

| 执行宿主 | 独立提交内容 | `saves` | `counter` |
| --- | --- | --- | --- |
| OpenCode | `OpenCode 安装验证 café🙂 20261117` | 1 | 1 |
| Grok Build | `Grok Build 安装验证 café🙂 20261117` | 2 | 2 |
| Pi | `Pi 安装验证 café🙂 20261117` | 3 | 3 |

fixture 的 `commit-note` 和 `increment` 独立事件分别只有三条，与上表逐项一致。界面没有单独的提交成功提示，因此 Agent 的单击回执和字段读回本身不能证明持久提交；这里由 supervisor 事后读取独立日志和 `saved_note` 完成确认，没有要求 Agent 重复提交。

OpenCode 和 Pi 的截图字节可从对应宿主 transcript 提取并已目视检查。OpenCode 在 ACP 中提供 JPEG 图像，Pi 提供 PNG，画布均为 1600×1268。Grok 的 ACP 工具结果提供截图元数据和 `[image content will be provided separately]`，Agent 报告图像已渲染且能描述界面，但 ACP transcript 没有原始图像块。Grok 最终界面的文字、Counter: 2 和 Reviewed 状态由后续 Pi 首张截图交叉复核；这不是对 Grok 原始 PNG 字节的独立审阅。

## 保留的问题与边界

- Grok 的 `read_file` 对工作目录外的插件 Skill 路径报错，随后用宿主正常支持的 shell 文件读取加载原路径，未复制 Skill、改策略或变更注册。其安装 transcript 未保留下来；安装结果、插件检查输出及后续新会话 transcript 已保留。
- Grok 在寻找提交确认时额外点击一次 Reviewed，使其从 false 变为 true。该动作不是任务要求，原始记录保留；不将这轮描述为零错误或无额外操作。它没有重复 Commit note，独立提交结果正确。
- 三个 Agent 都遇到语义输出预算截断，并通过过滤观察继续读取。这是只读查询收敛，没有重放未知输入。DTW GUI 工具没有返回失败；Grok 的一次失败来自宿主文件读取工具。
- 初次沙箱预检分别阻止了 OpenCode 日志写入、Pi trust 锁和 Grok 会话持久化；自有 GUI fixture 在沙箱注册应用时退出。经正常宿主授权执行必要命令后继续；没有修改 ACL 或沙箱策略。一次 Pi 安装授权请求超时且没有启动，之后重新申请并执行。
- 测试结束时 `opencode`、`grok-build`、`pi` 均为 current，pending 清空；既有 `agy` 和 `dsh-desktop` 绑定字节未变。fixture 退出码 0，没有强制终止，两个自有 loopback 服务正常关闭。
- 本轮覆盖 macOS 当前版本和已有 OS 权限环境、默认 `apps`/`exec` 模式。未验证 Windows、Linux、全新 TCC 授权、atomic 模式、取消中断、完整 42 项基准或任意 OpenCode 二开 Agent。

## 仓库变更与验证

新增无第三方依赖的 Pi extension，通过宿主 `registerMcpServer` 注册包内同一个 DTW 可执行文件；`package.json` 的 `pi.extensions` 和 `pi.skills` 同时声明入口与 canonical Skill。原 DSH Cordis `main`/`exports` 保持可用。没有改动 Rust/native 运行时或 Skill/API 内容。

开发 staging 和发行打包均携带三份新宿主指南。发行校验检查 Pi 声明、入口及 Skill 配对，拒绝重算 checksum 后的路径重定向、缺失入口或遗漏声明；确实没有 Pi 内容的历史 schema 1、schema 2 包继续接受。审阅发现的 schema 1 提前返回绕过已补校验及回归。

包合同、发行校验和 CI 路由共 45 项测试通过，包含 Python/Node 入口加载、指南打包、历史包兼容及重算 checksum 后的拒绝场景；Markdown 链接、代码围栏和最终差异空白检查通过。OpenCode 自有下载暂存由安装 Agent 清理，Grok 自有解压暂存经 supervisor 核对包/二进制 hash 和托管目标后清理；验收证据保留在系统临时目录。本轮没有发布新版本，Pi 原生入口需要随后发行的完整包才能通过公共稳定版安装流程获得。
