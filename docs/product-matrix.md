# 功能与支持范围

此页描述当前源码能力；已发行版本以对应 [Release](https://github.com/caelis-labs/desktop-world/releases) 的资产和说明为准。构建、原生测试、真实 Agent 任务和最终安装包验收是不同证据，不相互替代。当前开发分支的实机条件与结果见 [Windows 修复验收](windows-fixes-acceptance-2026-11-03.md)。

| 能力 | 当前支持 | 限制 |
| --- | --- | --- |
| Agent 接入 | 标准本地 stdio MCP + Skill；Caelis、DSH、agy；仅 `apps` + `exec({app, source})` | Harness 自行负责审批；不要求定制 Host；远程传输尚未接入 |
| 方法发现与结果 | `apps` 检索应用及运行状态；API 文档随 Skill；`exec` 默认返回界面增量、必要动作证据和按需截图；print 可选 | 不需要模型生成 request ID 或对成功执行轮询；未知效果不能自动重放 |
| 应用与窗口 | 稳定平台 app ID；同 app 多实例窗口聚合、精确 owner 引用；macOS/Windows 发现安装目标、按需启动并正常 quit | Windows 读取 Start Menu、App Paths、AppsFolder，以 exe 路径或已证明的 `win:<AUMID>` 定位；跳过/不可读来源明确标记覆盖不足；Linux 仍仅发现运行目标 |
| 基本视觉操作 | macOS/Windows 按已证明归属和全局窗口顺序合成可见 surface；Windows 使用精确 HWND 和 DWM 可见边框 | Windows 支持可读取的全局 alpha/color-key，未证明的 per-pixel layered alpha 仍明确拒绝；covered capture 不放宽输入可见性；实机结果按具体构建记录 |
| 语义发现与阅读 | look 输出当前顶层至多 80 个语义元素；可用 role/text 按需筛选原生遍历；readText 按观察目标读取原生文本分页；语义不足可截图兜底 | 截断字段标记长度与内容版本；筛选完整性只适用于该查询；文本分页使用同一观察及 UTF-16 偏移；不默认输出整棵树或全文 |
| 常用操作 | Agent 使用 look/capture/readText/click/move/drag/typeText/keypress/scroll/quit；点击可选语义目标、选择器或图像坐标 | macOS/Windows move/drag 限一个窗口和单个显示器内的可见路径；支持已证明的标准标题栏搬窗；不暴露跨调用的按下/释放状态；实验 X11 不支持手势 |
| 菜单与对话框 | macOS 菜单、sheet、dialog 及有正向归属证明的关联 surface；Windows 当前窗口内的原生菜单和已证明窗口归属的 modal | Windows 弹出菜单键盘路由需原生 menu-owner 证明；系统 helper 按平台证据处理，不按标题或矩形猜跨进程归属 |
| 多 app 与多会话 | DTW 维护各 app 的观察状态及回执；跨连接原生资源协调 | 同连接单执行槽；原生上下文更换重置观察；Agent 无 Session 或游标管理；连接不隔离物理桌面 |
| macOS 输入 | 语义、支持的键盘及坐标点击/内容拖拽/滚动优先定向；已识别 AppKit/Chromium 原生 hover 按已验证边界借用物理光标；必要聚焦后保持目标前台 | 具体动作、框架与物理路线边界见输入路由文档；普通 UI/快捷键关闭窗口，quit 正常退出；保存提示不强行处理；派发后失败不换路线重投 |
| Windows 输入 | 支持的 HWND 客户区定向左键/滚轮；其他鼠标、手势及键盘由 DTW 自动准备前台；UIA、DPI、Unicode 和正常关闭 | 不承诺后台键盘/独立物理指针；保护保存提示；安全桌面、提权与不同 DPI 环境需分别验证 |
| Agent 光标 | macOS/Windows 按会话颜色显示动作位置和点击反馈；不显示内部身份编号 | 视觉反馈不代表业务完成；只有经证明属于 DTW 的辅助窗口可从截图排除 |
| 取消与资源回收 | 标准 MCP cancellation、EOF/父进程退出收尾、配对释放、原回执；Windows 新输入前复核当前输入桌面，失败返回 desktop_unavailable | 已排队输入不可撤回；配对释放不被新输入禁令阻止；桌面恢复后重新观察；Windows 锁屏/切桌面的新增准入仍需专用实机验收 |
| 浏览器 | 原生窗口、AX/UIA 与像素路线，支持复杂网页通用交互 | DOM、CDP、profile、exact-tab provider 未接入 |
| 锁屏 | 不承诺支持 | 后台输入不意味着锁屏可用；无解锁或绕过认证能力 |
| Linux | 保留实验 X11/XRes 身份、XGetImage、XTest 实现 | 无正式桌面支持承诺；不承诺 Wayland/XWayland 或 AT-SPI 语义能力 |
| 安装与构包 | macOS arm64、Windows x64；单可执行文件、Skill、MCP 示例与来源/校验清单 | macOS 最低构建目标 14.2 不等于所有旧系统已验收；全新权限环境、其他架构独立验证 |

macOS 多屏坐标、跨屏移动和不同缩放比例依实际布局验证；单机结果不外推全部屏幕配置。Windows 已发行包的实机条件与结果保留在其 RELEASE.json。Linux 源码或类型检查不构成桌面可用性证明。

使用方法见 [安装](../install.md)、[脚本参考](../skills/desktop-world/API.md) 和随包 Skill。扩展方向与边界见 [架构](architecture.md)；真实任务指标与验证方法见 [构建和验收](building.md)；底层来源见 [原生来源](native-source-provenance.md) 和 [跨平台来源](native-source-provenance-cross-platform.md)。

现行调用合同见 [Skill API](../skills/desktop-world/API.md)；已关闭的协议迁移见 [历史收敛记录](agent-protocol-convergence-plan.md)。
合成视图目前限制 16 个可见 surface、1600px 画布、64 MiB 解码数据。
macOS move/drag、真实系统 Open/Save 面板及单屏内手势的双屏搬移检查见
[能力补齐验收](agent-capability-followup-acceptance.md)。跨显示器窗口的物理指针输入
因缺少充分命中证明而前置拒绝，截图仍可用；跨窗口拖放、更多系统 helper、不同缩放
仍需按平台和具体显示配置独立验收。
