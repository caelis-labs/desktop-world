# 功能与支持范围

此页描述当前源码能力；已发行版本以对应 [Release](https://github.com/caelis-labs/desktop-world/releases) 的资产和说明为准。构建、原生测试、真实 Agent 任务和最终安装包验收是不同证据，不相互替代。当前开发分支的实机条件与结果见 [PR #107 Windows 验收](windows-pr107-acceptance-2026-11-05.md)、[Windows 修复验收](windows-fixes-acceptance-2026-11-03.md) 和 [macOS 连续操作验收](macos-usability-acceptance-2026-11-04.md)。DSH 输入问题的[定向回归](dsh-v035-input-regression-2026-11-05.md)与完整 Cordis 插件的[56 项待测方案](dsh-cordis-acceptance-plan-2026-11-05.md)另列；不能把后者视为已通过。

| 能力 | 当前支持 | 限制 |
| --- | --- | --- |
| Agent 接入 | 标准本地 stdio MCP + Skill；Caelis、DSH、agy、OpenCode、Pi、Grok Build；`apps` + `exec({app, source})`，脚本含 14 个操作 | 新增三宿主仅有 [macOS 基本安装/GUI 验收](agent-install-acceptance-2026-11-17.md)；Pi 原生入口需要含适配器的新包；Harness 自行负责审批；远程传输尚未接入 |
| 方法发现与结果 | `apps` 检索应用及运行状态；API 文档随 Skill；已知 selector、键盘及默认滚动按需准备接收者，无需每段脚本先 look；`exec` 返回界面增量、必要动作证据和按需截图；print 可选 | 显式 ID 需已有观察，坐标需已返回的 capture；未知效果不能自动重放 |
| 应用与窗口 | 稳定平台 app ID；同 app 多实例窗口聚合、精确 owner 引用；macOS/Windows 发现安装目标、按需启动并正常 quit | Windows 读取 Start Menu、App Paths、AppsFolder，以 exe 路径或已证明的 `win:<AUMID>` 定位；跳过/不可读来源明确标记覆盖不足；Linux 仍仅发现运行目标 |
| 基本视觉操作 | macOS/Windows 按已证明归属和全局窗口顺序合成可见 surface；原坐标可连续命中同一进程实例内出现或消失的弹层，画布锚点和映射须保持；Windows 使用精确 HWND 和 DWM 可见边框 | Windows 的 owned modal、原生 Open/Save/覆盖确认已有实机证据；全局 alpha/color-key 可读取，未证明的 per-pixel layered alpha 仍拒绝，真实 SysShadow 的本机限制见专项报告；covered capture 不放宽输入可见性 |
| 语义发现与阅读 | look 输出当前顶层至多 80 个语义元素；可用 role/text 按需筛选原生遍历；readText 按观察目标读取原生文本分页；语义不足可截图兜底 | 截断字段标记长度与内容版本；筛选完整性只适用于该查询，complete 空 AX 不等于全画面无内容；文本分页使用同一观察及 UTF-16 偏移；不默认输出整棵树或全文 |
| 常用操作 | Agent 使用 look/capture/readText/wait/waitForNewWindow/click/setValue/move/drag/typeText/keypress/scroll/focus/quit；点击可选语义目标、选择器或图像坐标；focus 显式准备前台且不恢复原状态 | macOS/Windows move/drag 限一个窗口和单个显示器内的可见路径；支持已证明的标准标题栏搬窗；不暴露跨调用的按下/释放状态；X11 支持同一可见 client 内的悬停和内容拖拽 |
| 菜单与对话框 | `look({scope:'menu'})` 通过已有应用窗口读取原生菜单；macOS sheet、dialog 及有正向归属证明的关联 surface；Windows 当前窗口内的原生菜单和已证明窗口归属的 modal | 无窗口应用根、Dock 和 menu extras 尚无操作合同；能发现不代表可输入；Windows 弹出菜单键盘路由需原生 menu-owner 证明；helper 按平台证据处理 |
| 多 app 与多会话 | DTW 维护各 app 的观察状态及回执；跨连接原生资源协调 | 同连接单执行槽；原生上下文更换重置观察；Agent 无 Session 或游标管理；连接不隔离物理桌面 |
| macOS 输入 | 尽可能后台直接输入；必要时直接准备精确前台／真实鼠标；虚拟光标覆盖 click/move/drag/scroll | 不恢复原前台、焦点或鼠标；逐次证明精确目标，用户切走停止当前执行，unknown 不重投，键鼠配对释放。已完成原生／WKWebView 精确文本 receiver 与自绘菜单定向实测，见[直接输入策略验收](direct-input-policy-acceptance-2026-11-17.md)及[输入路由](macos-input-routing.md) |
| Windows 输入 | 默认前台操作；DTW 准备精确接收者；UIA、Unicode 和正常关闭 | 不使用后台定向鼠标，不恢复原前台、层序、焦点或光标。用户切走停止当前执行；保护保存提示，安全桌面、提权与 mixed-DPI 需分别验证 |
| Agent 光标 | macOS/Windows 按会话颜色显示动作位置和点击反馈；不显示内部身份编号 | 视觉反馈不代表业务完成；只有经证明属于 DTW 的辅助窗口可从截图排除 |
| 取消与资源回收 | 标准 MCP cancellation、EOF/父进程退出收尾、配对释放、原回执；Windows 新输入前复核当前输入桌面，失败返回 desktop_unavailable | 已排队输入不可撤回；配对释放不被新输入禁令阻止；桌面恢复后重新观察；Windows 已验证真实非 input desktop 准入与四类 held drag 中断，未诱发锁屏、安全桌面或中途 desktop 切换 |
| 浏览器 | 原生窗口、AX/UIA 与像素路线，支持复杂网页通用交互 | DOM、CDP、profile、exact-tab provider 未接入 |
| 锁屏 | 不承诺支持 | 后台输入不意味着锁屏可用；无解锁或绕过认证能力 |
| Linux X11 | 运行中应用、窗口元数据、无遮挡 client 截图、前台 focus、点击、悬停、内容拖拽、滚轮、映射文本和键盘组合键 | 要求本机 X11、EWMH、XRes 1.2、XInput 2、XTest；Wayland、XWayland、AT-SPI、原生文本读取和应用启动/退出不在覆盖内 |
| 安装与构包 | macOS arm64、Windows x64、Linux x86_64、Linux arm64；单可执行文件、Skill、MCP 示例与来源/校验清单 | macOS 最低构建目标 14.2 不等于所有旧系统已验收；全新权限环境、其他架构独立验证 |

macOS 多屏坐标、跨屏移动和不同缩放比例依实际布局验证；单机结果不外推全部屏幕配置。Windows 已发行包的实机条件与结果保留在其 RELEASE.json。Linux 发行构建以 Ubuntu 24.04/glibc 2.39 为基线。Fedora 和 Xvfb 的验证方式见 [Linux X11 验收](linux-x11-validation.md)。

使用方法见 [安装](../install.md)、[脚本参考](../skills/dtw/API.md) 和随包 Skill。扩展方向与边界见 [架构](architecture.md)；真实任务指标与验证方法见 [构建和验收](building.md)；底层来源见 [原生来源](native-source-provenance.md) 和 [跨平台来源](native-source-provenance-cross-platform.md)。

现行调用合同见 [Skill API](../skills/dtw/API.md)；已关闭的协议迁移见 [历史收敛记录](agent-protocol-convergence-plan.md)。
统一 [Computer Use 验收 APP](../tests/fixtures/computer-use-app/README.md) 定义密集/低 AX、多窗口、文件及光标目标；[首轮基准](computer-use-benchmark-2026-11-04.md) 区分通过、失败、阻塞、未复核和未执行。目标场景不等于平台支持声明。
合成视图目前限制 16 个可见 surface、1600px 画布、64 MiB 解码数据。
macOS move/drag、真实系统 Open/Save 面板及单屏内手势的双屏搬移检查见
[能力补齐验收](agent-capability-followup-acceptance.md)。跨显示器窗口的物理指针输入
因缺少充分命中证明而前置拒绝，截图仍可用；跨窗口拖放、更多系统 helper、不同缩放
仍需按平台和具体显示配置独立验收。
