# PR #107 Windows 实机验收

本轮在 Windows 上对照 [PR #107](https://github.com/caelis-labs/desktop-world/pull/107) 的全部功能变化，修复验收发现的问题，并将 macOS 的专用 Computer Use workbench 接入同一套 Windows fixture。这里的证据对应源码构建候选，不代表已安装的 0.3.4 或完整 42 项基准全部通过。

## 环境与候选

| 项目 | 实际条件 |
| --- | --- |
| OS | Windows 11 23H2，10.0.22631；普通、未提权、解锁的交互桌面 |
| 显示 | 单屏 2560×1440，工作区 2560×1380，120 DPI / 125%，原点 (0,0) |
| Windows 原生源码候选 | 干净提交 `7822f1017d5c7c8c60f366729dd15adfc141b5bf`；后续报告/CI 调整不改变 Windows 生产实现。同提交已含 macOS C ABI 修复；macOS 的原生 CI、ARM 构包及 GUI 历史分别记账 |
| dtw.exe SHA256 | `a80b1ae6e980f58e4a264f252f71b69598a6035c2a0aa4814d1478430dc764bb` |
| SKILL.md SHA256 | `3d213f3ed75a8d5b3a301b39126c3208d23f1ee4add779d5fa65e86c4fb7c559` |
| API.md SHA256 | `3d84080774cd2aa13e9b550277ee4ac218a693a1544b1f43fe22d84b84354781` |
| PATTERNS.md SHA256 | `90734729b06164021b99aac68870ec0b16806bd8258b85cb2a6e373f770b83af` |
| 最终原生 fixture / observer SHA256 | `02f69855ff9fb9e740a46bfaff6b9d4e59ebbca5452803f53b95155cd2749d4e` / `e1709dba136dcb6077f5dd90acf121f74dfd34dca30abc0827da1bc195662c54`（observer-build-a08；更早 helper 的身份分别保留） |
| 浏览器 | Chrome 156.0.8078.74；Edge 156.0.4314.58；均用独占临时 profile，另建同 executable 邻居 profile |

原始文件保存在 `artifacts/pr107/`。`frozen-7822f10/IDENTITY.json` 固定最终二进制、源码文件、Skill 和原生观察器哈希。[脱敏证据摘要](evidence/windows-pr107-2026-11-05/summary.json) 记录每次尝试的报告路径/哈希和结果；原始 wire frames、截图、独立业务文件与失败均保留。本轮没有修改用户已安装的插件或浏览器配置。更早 `b422b29` 候选及其 `strict-a01-*` 结果单独保留；构建桥接声明调整后，最终当前提交重新冻结并完整执行 `strict-a02-*`。

## 优化点与平台对齐

| PR 的优化点 | Windows 对齐与实测 |
| --- | --- |
| 公开 `apps` / `exec`，十个 app 方法 | 修正旧验收的“九方法”断言，按准确方法集合检查；协议 baseline 5 项、feature audit 26 项通过 |
| 冷上下文已知 selector / 键盘 / 默认滚动按需准备 | workbench 的首个 exec 直接点击 Commit；真实 Chromium Unicode、替换、快捷键、默认中心滚轮通过；没有用预先 look 代替冷操作 |
| focused `look` 的 role/text/menu 检索 | 190 个按钮中准确检索并点击 Checkpoint 137；原生菜单只读检索；Windows 未声称应用根/Dock 能力 |
| 分页 `readText`、UTF-16 边界与截断提示 | UIA 的标准 WinForms Edit proxy 会截短全文；增加同 root/PID、Unicode、非密码、原生 HWND 连续性验证后的有界只读消息路线。28029 个 UTF-16 单元按 4095 上限分七页，拼接 SHA 和独立文件完全一致；旧观察被拒绝 |
| 同实例 popup/modal 连续动作 | 读取 owner 时不再穿透发布另一个顶层 modal 的控件；慢 selector 扫描结束后重新核验接收 surface，允许一次有证明的只读切换。owned modal 在同一脚本中打开/关闭通过 |
| Open / Save / 覆盖确认 | 原生打开取消、保存、重新打开、覆盖选择“否”后取消均通过。修复 DPI 转换把独占右下边界当窗口内点造成的无效句柄；现在在接收窗口 DPI 上下文中读取 backing 尺寸并恢复本线程上下文 |
| macOS 整段脚本前台借用、单调失效与恢复 | 属于 macOS resident worker 合同；Windows 保留自己的前台协调和 input-desktop 证明。UIA Invoke 可由 provider 激活窗口/移动真实指针，已如实记录；不宣称 Windows 实现了 macOS scope 或后台键盘保证 |
| macOS AXPress 两秒回执预算 | macOS 保留实际 AX 成功/unknown 的区分；Windows 使用自身 UIA/posted/SendInput 的回执。业务效果独立核对，不把派发成功当完成 |
| Windows 每个新输入的 input-desktop 准入 | 在真实 owned 非 input desktop 中验证九类新输入均 `desktop_unavailable / not_dispatched`，业务未改变；恢复后使用新的 fresh 请求。没有切换安全桌面、改 ACL 或伪造中途切换 |
| unknown、取消、deadline、EOF/宿主退出 | 四类真实 held drag 均配对释放一次、无 Increment suffix、无未知输入重放、worker 退出为 0。取消抑制原响应仍记 unknown；deadline 保留原 timeout/interrupted。宿主退出测试有意终止 exact owned MCP，不能将其退出 1 写成正常退出 |
| 独立业务 oracle、连续焦点/真实光标与 fixture 整合 | Windows 使用一个 Workbench 原生壳，复用 Web/Canvas/workbench loopback 服务、场景清单和生命周期；原生观察器是单独的只读进程。支持的 posted 点击/滚轮及只读操作在 witness 前台下通过，真实光标不变；background-only 的键盘/hover 干净拒绝 |

## Windows 待验证清单逐项结论

| PR 清单 | 结论与实际范围 |
| --- | --- |
| 原生构建与协议回归 | 通过：Rust 1.98.0 release，584 个库测试、4 个 CLI 测试；该 Windows 配置的 MCP integration 目标为 0 项，不能借用 macOS 数字。baseline、26 项 feature 和 workbench 选定原生核心通过 |
| input-desktop 准入 | 通过：press、click、scroll、keypress、typeText、move、drag、close_window、quit_app 的九个准入拒绝；worker/fixture 正常退出，owned desktop 删除。这是在 payload 参数解释前的真实准入测试，不声称已经走到标题栏移动派发阶段；中途切换与已提权/secure desktop 未诱发。配对释放由实际中断拖拽及适配器单测分别覆盖 |
| S03 真实 tooltip/shadow | 完整阴影图像子项按用户条件跳过；上游调研及本机原生诊断见[专项报告](windows-tooltip-upstream-research-2026-11-04.md)。真实文件选择仍要求语义 receiver、全局窗口排序、准确文件内容、tooltip 消失后的 fresh PNG；没有预停放光标或广泛排除未知 popup/menu |
| S04 mixed-DPI / 负原点跨屏 | 用户明确选择跳过；本机只有单块 125% 屏幕。26 项 feature 中的单屏搬窗、旧几何拒绝与新捕获命中通过，不冒充 mixed-DPI |
| 中断与清理 | 通过四种真实 held drag。公开键盘 chord 是单次配对，不暴露跨调用 held-key 状态；没有把单元测试写成真实持续按键 GUI 验收 |
| Chrome / Edge 与邻居实例 | 两者各 14 项业务、同 executable 邻居零输入、正常清理均已通过；最终整段严格采样另见下表 |
| 留存与报告 | 每次 fresh 目录、准确候选哈希、业务/观察/清理分别记录；旧候选、原失败、unknown 和强制清理均未改记为通过 |

`computer_use_probe.py` 本轮选定 N01/N02/N03/N04/N05/N08/N10/N11/N12，以及额外 owned popup 连续动作。其余场景保持 `not_run`，`target_baseline_passed:false`。Windows 壳未以旧 IE WebBrowser 冒充 macOS WebKit，也没有声称所有 chat/canvas/model 图像通道已完成。

## 观察与重试边界

原生观察器记录 QPC、UTC、前台 HWND/PID、真实光标和全局 held keys。最终门槛同时要求**整段采样最大间隔 ≤50ms**，以及每个实际 MCP 调用/中断请求至 worker 回收区间均有前后样本包围。CLR 首次 JSON 序列化的 47–50ms 预热在就绪信号之前完成；探针等到首个已 flush 样本后才启动，之后没有丢弃样本或放宽上限。

| 最终已核对尝试 | 业务 / 清理 | 整段最大间隔 ms | 调用或中断区间最大间隔 ms |
| --- | --- | --- | --- |
| `strict-a02-super` | 通过 / 正常退出 | 10.2454 | 10.2454 |
| `strict-a02-baseline` | 5 项通过 / 正常退出 | 8.8382 | 6.6007 |
| `strict-a02-feature` | 26 项通过 / 正常退出 | 12.1476 | 12.1476 |
| `strict-a02-cancel` | 通过 / 正常退出 | 10.4318 | 8.8235 |
| `strict-a02-eof` | 通过 / 正常退出 | 8.0555 | 7.0297 |
| `strict-a02-host-death` | 通过 / 按预期注入宿主退出，workers 正常 | 10.3411 | 8.6419 |
| `strict-a02-deadline` | 通过 / 正常退出 | 8.8222 | 8.8222 |
| `strict-a02-background` | 五项通过 / 正常退出 | 10.0037 | 7.9596 |
| `strict-a02-chrome` | 14 项通过 / 正常退出，S03 图像限制保留 | 11.6652 | 11.6652 |
| `strict-a02-edge` | 14 项通过 / 正常退出，S03 图像限制保留 | 11.1244 | 11.1244 |

两个浏览器的邻居独立 HTTP journal 都只有初始 ready 一条；所有记录及最终完整状态与初始状态一致，覆盖 saved/picker 等字段，未只依靠目标窗口标题或派发回执推导隔离。

旧 `feature-b422-a01`（51.5510ms）和 `background-b422-a04`（50.0302ms）虽然业务及调用区间成立，整段采样仍不通过。历史采用调用区间单独门槛的过渡报告在摘要中保留原 scope，未计入上表；最终十组全部重新执行，满足整段及调用区间两重要求。

前几次 workbench 的分页/观察过期断言、状态空读、把“是/否”确认误当可 Escape 的对话框，以及后台探针的 Python DPI 坐标空间错误均作为探针失败保留。产品缺陷与探针错误分别修复。邻居窗口最初因 Chromium 逻辑坐标在 125% 屏幕上越界，截图准确拒绝；修正独占测试实例的初始布局后 fresh 重跑，未放宽产品边界。

## CI 与发布

原始 macOS CI 的 Swift 私有 initializer 导致编译失败，已补明确 initializer。Windows CI 的 Python fixture 启动握手在并发冷启动时失败，已用 `-S`、仅串行化启动握手和精确 PID marker 修复；没有延长产品 deadline 或削弱进程回收断言。

Intel macOS CI 实际通过了 601 个库测试、5 个 CLI、3 个 MCP 测试和包升级/lease 验证，随后纯 Swift controls 编译发现 `@_silgen_name` 的 Swift 调用约定与 C stub 不一致。改用共享 `NativeBridge.h` 的 Clang C ABI 导入，并让产品构建及纯 controls 使用同一 header。旧 Swift 编译器对同一 module 中导入/定义相同 C symbol 会触发 SIL linkage assertion，因此测试通过独立 C translation unit 转发到原 Swift controls，实际穿过 C ABI，计数与断言保留；没有删除 stub、关闭 warnings-as-errors 或绕过原生断言。纯 Swift controls 移到冷 Rust 编译之前，缩短同类失败的反馈时间。

macOS 26 与 15 ARM runner 均被 GitHub 以 capacity / 未获取 runner 取消，曾用受支持的 Intel runner 定位上述桥接问题。新的标准 GA `xcode-27` ARM64 镜像已在[固定源码构包预检](https://github.com/caelis-labs/desktop-world/actions/runs/41870189478)中完整通过源测试、Developer ID 签名、公证及最终 ZIP 原生验证；因此最终 CI、构包和字节审计统一到该 ARM64 镜像，保留全部原生门槛。冷 release 编译及第二版本升级构建保留 30 分钟作业预算，产品调用与测试断言的 deadline 未改变；14.2 部署目标不变，不使用付费 larger runner。该预检为源码版本 0.3.4 的非发布候选，不能拿来冒充下一版 Release 字节。

Windows 实机范围按以上两项用户跳过边界完成。合并仍以 PR 当前提交的全部 required checks 成功为门槛；正式 Release 的精确 source/tag、平台候选和校验和由发布资产中的 `RELEASE.json` 绑定。本报告不预先宣告排队作业、尚未执行的 ARM64 构包或公证成功。
