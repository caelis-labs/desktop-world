# 重写版产品矩阵

当前检查点：WPS 关闭后的焦点清理已修复；DSH 的三个完整真实任务通过开发候选验收，最后 AX 修复有独立实机对照。#70 正在收口 Windows 审查修复及同源验收包；最终安装包和 Caelis 会话级 app 授权联调仍待完成。结果见[阶段记录](real-task-closure-acceptance.md)。

日期：2026-10-31。#69 后的[真实应用闭环批次](real-task-closure-plan.md)新增能力正在按 WPS、Obsidian、Chrome 完整任务验收。此前首版[发布计划](first-rewrite-release-plan.md)的第一、二批、macOS 安装候选及模型指标见[核心验收](release-core-acceptance.md)与[候选记录](rewrite-candidate-acceptance.md)，其结果仅证明对应候选，不自动覆盖当前源树。Windows 待用户独立实机验收；旧 CUA SDK/worker/安装包不属于下一 release。

| 能力 / 工作流 | 当前实现与条件 | 证据 / 尚待完成 |
| --- | --- | --- |
| Agent 入口 | 官方 rmcp stdio；apps/api/exec/result；exec 仅 app/source；稳定平台 app ID | 普通 MCP + Skill，无定制 Host；目标为 Caelis、DSH、agy；Caelis app 授权由 Core 实现，联调尚未完成 |
| 按需 API | apps 紧凑索引；api 可选方法详情；result 原始账本 | 默认输出不展开全方法 schema，不为成功操作追加账本查询 |
| 窗口基础路线 | macOS 逐实例验证 PID、正面积且精确查询可用的 WindowServer 表面（含同 PID 非零 layer 浮层）；不要求 AX 语义库存；精确 SCK 截图及可见点点击 | 有归属/生命周期/像素几何证明；模态、遮挡和空间边界按原生条件拒绝，不承诺任意后台点击 |
| 同 app 多实例 | 稳定 app ID 聚合各实例窗口，window_ref 路由到精确实例；成员变化保留存活兄弟引用与 state | 真实双实例退出/重启通过；部分库存失败保留可用窗口、原错误及摘要提示，不伪装成完整库存 |
| 语义发现与动作 | 原生 role/text 筛选、分页 frontier、菜单/子树/截图区域过滤；press/setValue 仅在元素广告对应 action 时可用 | 每窗观察独立；动作消费该窗观察；新 scope 按平台声明；macOS 区域按 AXScrollArea viewport 裁剪，Windows 按 UIA bounds 筛选，均不等同 OCR 或像素遮挡证明；partial 与原始原因保留；候选 Z 的三个真实任务完成，最终安装字节待验收 |
| 长文本读取 | readText 按 UTF-16 页读取原生文本；保留文本版本，变更后要求重新观察 | 不以 label/摘要冒充正文；macOS AXValue/StringForRange、Windows UIA；不支持的控件明确返回能力边界，待真实文档闭环验证 |
| 文件对话框与关联表面 | macOS sheet/dialog 独立窗口引用；附着 sheet 阻塞父目标，application-modal 阻塞其他普通目标；经正向证明的活动兄弟 modal 可操作；有原生关联证据的跨 PID 表面仍通过逻辑 app 调用 | association_complete:false 不承诺穷尽所有 helper；目录选择、另存为、关闭/重开纳入新一轮任务，不能由窗口可列出直接推断输入可用 |
| 常用输入 | focus、Unicode typeText、keypress、wheel-step scroll；左/右键及单/双击 | macOS 内部优先目标投递，预检不支持时借用前台；按动作复核目标，键盘不依赖无关指针或窗口位置变化。目标失效停止后续输入，已排队事件无法撤回；效果独立读回 |
| 多 app 与多会话 | 最新 app state/refs 与滑动回执分开；同实例跨进程写租约，后台共享/前台独占协调先于实例与物理输入锁 | 历史容量不再拒绝新执行；独立实例后台并发按本轮真实样本验证；同一连接仍顺序执行脚本 |
| 取消和未知效果 | 标准 MCP cancellation、配对释放、原回执、按需 owner 重建 | unknown 不重放；晚到/清理失败保留；强杀不能证明输入释放 |
| Windows provider | 稳定应用身份、精确 HWND incarnation、UIA 筛选/分页/动作、GDI 可见无覆盖截图、SendInput、独立 worker | 历史 Windows x64 候选 50dbec3 完成原生链接、286 个库测试和 3 个 CLI 测试；本轮修正 UIA 成功空值、区域引用几何复核与独立字段匹配，新增对应回归和 WinForms 检查；须使用本次同源 Windows 候选；DPI/焦点/取消与真实 GUI 验收仍由用户独立实机确认 |
| Linux provider | 实验 X11/XRes 身份、窗口、XGetImage、XTest 输入；只使用当前可证明的键盘映射 | 不支持 Wayland/XWayland，不提供 AT-SPI 语义承诺；仅源码/类型证据，不承诺桌面支持或首版正式发行平台 |
| 浏览器范围 | 真实浏览器窗口上的原生 AX 和基础像素路线 | DOM、profile、CDP/browser bridge 未接入 |
| 输入后续组 | 拖拽、应用启动/退出、尚未支持的原生控件 | 后置批量能力；菜单发现/press 已纳入本批，不扩张当前已广告方法的语义 |
| 服务级输入约束与回执 | `auto` / `background_only`；`exec(app, source)` 不变；保留实际路线和归还证据 | 签名候选 S 的 AppKit、Chrome 与生命周期验证通过；不把约束等同于所有输入已支持后台，见[验收记录](input-delivery-acceptance.md) |
| macOS Agent 光标 | 每会话共享一个独立透明窗口；实际 pointer post 驱动；精确窗口排除和有界清理 | 本轮实现与验收边界见[光标迁入说明](cursor-overlay-port.md)；不同于跨 PID 应用浮层能力 |
| 精确滚动 | macOS 默认窗口中心、截图像素锚点或原 AX 对象锚点；复用既有 wheel steps 与输入 owner | Z 候选 12 项 AppKit/Chrome 真 GUI 与光标回归通过；前台滚动回退未建立真 GUI 样本，Windows/Linux 仅默认中心；见[验收记录](anchored-scroll-acceptance.md) |
| macOS 后台输入（本机候选已验收） | 固定 CUA Rust 依赖闭包 + 薄适配；内部选择目标投递/前台借用；不增加 delivery 参数 | L 候选的 AppKit 全套、Chrome 输入/点击/滚动、同 PID 浮层、生命周期与两种外部 Agent 任务均通过；catalog 系统硬链接别名已修正；菜单等价键、控制字符和 Chrome 补充平面文本预检转前台；见[阶段报告](background-input-acceptance.md) |
| 锁屏与未证明的关联表面 | 锁屏独立后置；关联归属和生命周期无法证明的表面不广告可操作 | 不从后台输入推导锁屏支持，也不将一个文件选择器成功外推所有 helper/模态 |
| 脚本文件、远程传输 | source 原文，经标准 JSON 传输；当前仅 stdio | 文件便利入口和 HTTP 后置，不引入调用方路径耦合 |
| macOS 基线 | 构建/链接最低 14.2；本机 macOS 27.0.1 的固定签名/公证安装包完成六组原生和六个有效模型任务 | 本机既有权限环境通过不代表全新系统、所有旧系统或 x86_64 平台 |
| 构建与发行 | rewrite-only、各能力组固定来源 SHA，后台输入来源和差异单列；无全量上游编译 | 保留轻量源码/Core/原生纯控制 CI；固定 macOS/Windows 候选已生成，macOS 签名/公证和安装验收通过；Windows 实机与公开 release 待完成 |

历史开发字节和 macOS 安装候选的模型门槛为：Claude/agy 各 3 个有效任务；Claude 中位 5 exec / 6 MCP、约 52 秒，agy 中位 6 exec / 7 MCP、约 162 秒；元数据误判的原失败独立保留。这不是新 WPS/Obsidian/Chrome 复杂任务的耗时或通过率；本批按应用分别记录所有尝试、完整子目标、失败恢复与独立结果。Windows 未经独立验收不标正式支持，Linux 不作支持承诺。详细来源保留在[适配说明](native-source-provenance.md)及[跨平台来源](native-source-provenance-cross-platform.md)。

2026-10-30 焦点审查修复使用独立签名候选 P 验证：混合前台/后台协调、同 PID 精确原窗口归还、前台 cancel/EOF/parent-exit/timeout 及 Chrome 回归通过。P 未重跑 L 的外部模型任务，也未生成新发行包；证据见[焦点归还复核](evidence/focus-return-2026-10-30.json)。

本轮后台输入与历史回收的实现、失败样本和验证边界见[后台输入验收](background-input-acceptance.md)。此前签名安装包和六个模型任务的证据仍仅属于各自原始候选，不自动覆盖本轮改动。
