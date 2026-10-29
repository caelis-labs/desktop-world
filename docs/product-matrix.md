# 重写版产品矩阵

日期：2026-10-29。当前范围是首版[发布计划](first-rewrite-release-plan.md)的第一、二批实施；逐项证据与易用性准入见[本轮验收](release-core-acceptance.md)。历史 #61–#64 报告只证明其记录的平台、字节和场景；旧 CUA SDK/worker/安装包不属于下一 release。

| 能力 / 工作流 | 当前实现与条件 | 证据 / 尚待完成 |
| --- | --- | --- |
| Agent 入口 | 官方 rmcp stdio；apps/api/exec/result；exec 仅 app/source；稳定平台 app ID | 普通 MCP + Skill，无定制 Host；真实模型任务按本轮门槛验证 |
| 按需 API | apps 紧凑索引；api 可选方法详情；result 原始账本 | 默认输出不展开全方法 schema，不为成功操作追加账本查询 |
| 窗口基础路线 | macOS 逐实例验证 PID、layer 0、正面积 WindowServer 表面；不要求 AX 语义库存；精确 SCK 截图及可见点点击 | 有归属/生命周期/像素几何证明；模态、遮挡和空间边界按原生条件拒绝，不承诺任意后台点击 |
| 同 app 多实例 | 稳定 app ID 聚合各实例窗口，window_ref 路由到精确实例；成员变化保留存活兄弟引用与 state | 真实双实例退出/重启通过；部分库存失败保留可用窗口、原错误及摘要提示，不伪装成完整库存 |
| 语义发现与动作 | 原生 role/text 筛选、分页 frontier；press/setValue 仅在元素广告对应 action 时可用 | 每窗观察独立；动作消费该窗观察；浏览器冷态内容有界就绪；区域/子树过滤后置 |
| 常用输入 | focus、Unicode typeText、keypress、wheel-step scroll；左/右键及单/双击 | foreground 输入受 OS 权限/焦点/目标复核约束；每次效果独立读回 |
| 多 app 与多会话 | app state/refs 保留；MCP 会话独立；跨进程物理输入 lease | 串行仲裁共享系统键盘/鼠标；不宣称多个独立后台指针 |
| 取消和未知效果 | 标准 MCP cancellation、配对释放、原回执、按需 owner 重建 | unknown 不重放；晚到/清理失败保留；强杀不能证明输入释放 |
| Windows provider | 稳定应用身份、精确 HWND incarnation、UIA 筛选/分页/动作、GDI 可见无覆盖截图、SendInput、独立 worker | 源码及目标 API 类型检查；完整目标链接/运行、DPI/焦点/取消与真实 GUI 验收待完成，由用户独立实机确认 |
| Linux provider | 实验 X11/XRes 身份、窗口、XGetImage、XTest 输入；只使用当前可证明的键盘映射 | 不支持 Wayland/XWayland，不提供 AT-SPI 语义承诺；仅源码/类型证据，不承诺桌面支持或首版正式发行平台 |
| 浏览器范围 | 真实浏览器窗口上的原生 AX 和基础像素路线 | DOM、profile、CDP/browser bridge 未接入 |
| 输入后续组 | 拖拽、复杂菜单、应用启动/退出 | 后置批量能力；不扩张当前已广告方法的语义 |
| 后台、锁屏与鼠标独立 | 架构扩展边界保留 | 尚未实现，不从部分后台 AX 动作推导整机能力 |
| 脚本文件、远程传输 | source 原文，经标准 JSON 传输；当前仅 stdio | 文件便利入口和 HTTP 后置，不引入调用方路径耦合 |
| macOS 基线 | 构建/链接最低 14.2；本机 native GUI 与模型验证 | 本机通过不代表所有系统或 x86_64 平台；最终安装字节/TCC 另验 |
| 构建与发行 | rewrite-only、固定 CUA 0.38 原子源码适配，无全量上游编译 | 保留轻量源码/Core/原生纯控制 CI；签名、公证、独立安装包和公开 release 属于第三、四批 |

本轮先收口产品能力与真实模型易用性，再制作固定候选。Windows 未经独立验收不标正式支持，Linux 不作支持承诺。详细来源保留在[适配说明](native-source-provenance.md)及[跨平台来源](native-source-provenance-cross-platform.md)。
