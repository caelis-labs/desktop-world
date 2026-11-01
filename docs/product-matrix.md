# 功能与支持范围

此页描述当前源码能力；已发行版本以对应 [Release](https://github.com/caelis-labs/desktop-world/releases) 的资产和说明为准。构建、原生测试、真实 Agent 任务和最终安装包验收是不同证据，不相互替代。

| 能力 | 当前支持 | 限制 |
| --- | --- | --- |
| Agent 接入 | 标准本地 stdio MCP + Skill；Caelis、DSH、agy；`exec(app, source)` | Harness 自行负责审批；不要求定制 Host；远程传输尚未接入 |
| 方法发现与结果 | `apps` 紧凑索引，`api` 按需描述；执行摘要与截图，`result` 查原始回执 | 不需要模型生成 request ID 或对成功执行轮询；未知效果不能自动重放 |
| 应用与窗口 | 稳定平台 app ID；同 app 多实例窗口聚合、精确 owner 引用 | 目标必须已运行；未提供应用启动/退出 API；窗口标题不能替代身份 |
| 基本视觉操作 | 指定窗口截图、图像坐标点击；macOS 使用 ScreenCaptureKit，Windows 使用 PrintWindow | 动作需通过归属、几何及相应可见性检查；截图成功不代表目标支持所有输入 |
| 语义发现 | AX/UIA 的类型、文本、区域/子树筛选、续页、按需文本读取 | 部分遍历会明确返回覆盖不足；自绘控件可能不提供语义对象；不默认输出整棵树 |
| 常用操作 | 语义 press/setValue、坐标单/双击与右键、focus、Unicode 输入、快捷键、滚动 | 支持依应用控件和平台而异；拖拽等后续动作未广告为可用 |
| 菜单与对话框 | macOS 菜单、sheet、dialog 及有正向归属证明的关联 surface | 不保证穷尽系统 helper；不能按标题或矩形猜跨进程归属 |
| 多 app 与多会话 | app state/refs 跨切换保留，历史回执滑动回收；跨会话原生资源协调 | 同一连接脚本串行；独立连接隔离 state/receipts，不隔离物理桌面 |
| macOS 输入 | 优先后台目标投递；无副作用预检不支持时自动聚焦；聚焦后保持目标前台 | 用户或 Agent 自行切走/关闭任务窗口；派发后失败不换路线重投；后台支持按动作判断 |
| Windows 输入 | 前台键盘与鼠标、UIA 语义动作、DPI-aware 截图坐标与 Unicode 处理 | 不承诺后台键盘/独立物理指针；安全桌面、提权与不同 DPI 环境需分别验证 |
| Agent 光标 | macOS/Windows 按会话颜色显示动作位置和点击反馈；不显示内部身份编号 | 视觉反馈不代表业务完成；只有经证明属于 DTW 的辅助窗口可从截图排除 |
| 取消与资源回收 | 标准 MCP cancellation、EOF/父进程退出收尾、配对释放、原回执 | 已排队输入不可撤回；强杀或丢失结果不能证明动作未发生 |
| 浏览器 | 原生窗口、AX/UIA 与像素路线，支持复杂网页通用交互 | DOM、CDP、profile、exact-tab provider 未接入 |
| 锁屏 | 不承诺支持 | 后台输入不意味着锁屏可用；无解锁或绕过认证能力 |
| Linux | 保留实验 X11/XRes 身份、XGetImage、XTest 实现 | 无正式桌面支持承诺；不承诺 Wayland/XWayland 或 AT-SPI 语义能力 |
| 安装与构包 | macOS arm64、Windows x64；单可执行文件、Skill、MCP 示例与来源/校验清单 | macOS 最低构建目标 14.2 不等于所有旧系统已验收；全新权限环境、其他架构独立验证 |

macOS 多屏坐标、跨屏移动和不同缩放比例依实际布局验证；单机结果不外推全部屏幕配置。Windows 已发行包的实机条件与结果保留在其 RELEASE.json。Linux 源码或类型检查不构成桌面可用性证明。

使用方法见 [安装](../install.md)、[脚本合同](app-script-v1.md) 和随包 Skill。扩展方向与边界见 [架构](architecture.md)；真实任务指标与验证方法见 [构建和验收](building.md)；底层来源见 [原生来源](native-source-provenance.md) 和 [跨平台来源](native-source-provenance-cross-platform.md)。
