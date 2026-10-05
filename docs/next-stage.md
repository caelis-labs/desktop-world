# #8 后的交付与补全计划

2026-10-05 更新：Windows 专项已启动并完成 Chrome、记事本、计算器、Win32、F4/F5 及 cooperative 适配，见 [Windows 验收报告](windows-validation.md)。下文保留 2026-10-04 的决策和阶段计划；其中 Windows 整体后置的状态已由新报告更新。其他 UIA 语义状态场景、混合缩放/会话矩阵与 Bot M0 仍待专项验证。

2026-10-04：选择 **1. 剩余功能补全计划**。本轮修复 #8 的三项 Review Findings 并验证、合并；不立即发布新的 pre-release，也不更新 caelis-bot M0。

## 决策依据

#8 已实现 no_shared_input、set_expanded、按需字段读取、Windows UIA 续扫和 managed helper；F1–F3 已有独立 macOS 实机证据。Windows CI 可以验证控制管道、扫描预算及游标生命周期，但 F4/F5 的真实电脑操作、F2/F3 的 UIA 状态/读取成本仍缺少实机证据。当前公开版本为 v0.1.0-alpha.2。

用户后续决定：**Windows 全部实机验收与适配整体后置**，功能达到完备临界点后在单独 Windows 环境完成。当前 Windows 仅提供功能实现与编译/CI 检查，**不承诺可用**；Windows 实机不再作为本轮功能或 macOS alpha 候选的前置门槛。Bot M0 更新和统一联调保持后置，历史记录保留其当时的验证范围。

## 交付顺序与退出条件

| 顺序 | 独立交付 | 可审查的退出条件 |
| --- | --- | --- |
| P1 | 高价值语义动作，已完成 | set_selected、set_checked、scroll_into_view 保留现有 set_value 文本/标量契约。各有独立 macOS 后台完整业务，前台 Unicode 输入、指针/焦点稳定、false/no-op、未支持拒绝和原收据证据；见 [实现](semantic-actions.md) 与 [实机记录](evidence/semantic-actions-20261004/README.md)。 |
| P2，已完成 macOS 核心验收 | 独立窗口内容捕获 | 先完成 macOS 实机；Windows 同步实现，实机后置。可靠关联已有窗口 Ref，不能按标题猜测；后台被遮挡且未最小化时返回当前帧并保留前台；最小化/隐藏/锁屏/断连/窗口重建分别返回可解释状态，旧帧不得冒充新帧。移动/缩放/跨屏、sheet/弹窗、捕获权限独立验收。图像坐标不能直接授权共享桌面点击。 |
| P3，按用户新优先级交付 | 同一桌面的短前台事务 | 接受短暂前台占用，优先降低总占用时间。先完成 AppKit / WebKit / Chromium / Electron / 真实文档的独立 POC，再把可行路径作为可信宿主选项；逐项键鼠、菜单/弹窗、恢复、取消、预算到期和用户切换分别验收。见 [实现与边界](cooperative-input.md)、[POC 结论](../poc/background-input/FULL_ACCEPTANCE.md)。更完整的人类输入监听继续低优先级后置。 |
| P4 | 长会话与有界等待 | 先测注册表/请求/游标容量，再做没有未决副作用时的安全 helper 轮换；Ref 不复用。原生事件只作范围失效提示，合并/去抖后有界刷新，保留周期校对；事件不直接触发 LLM。 |

单独 Windows 阶段在功能达到完备临界点后覆盖 F1–F8 和后续 feature 的真实电脑操作、UIA 属性成本、扫描预算与续扫、游标释放、managed host 停止/授权撤销/原收据恢复；CI 与实机分别标注，通过前不承诺 Windows 可用。P1 滚动第一版仅提供目标级 scroll_into_view；任意方向/页数的语义滚动保持 provider 专项扩展。

应用定向 PID 事件保留实验与逐 provider 证据，不自动 fallback。当前任务按用户要求只使用同一台电脑、同一用户桌面，不实施独立会话、VM 或多机管理。

## 每项实现的统一边界

- helper 命令统一使用 `dtw`。先返回小目录/summary，沿已有 Ref 读取 detail 或父链，再窄范围查询和有界续扫；值、几何、能力和图像按需披露。
- 一次性内部查询不得保留不可达 native cursor；需要续扫的调用保留有限 frontier。容量不足明确拒绝，不驱逐其他有效游标。
- 每项独立应用实例、唯一窗口和应用自身事件日志，操作均通过 Desktop World；记录完整任务的调用量、文本/图像输出、原生属性成本和耗时。字节与 getter 数不冒充模型 token；实际模型成本另行测量。
- 保留现有授权、身份、coverage/dirty、unknown、channel、delivery、verification 与原收据语义，预算失败不能被包装成任务成功。

## 下一次 pre-release 的候选门槛

用户后续授权直接合并并产出 pre-release：本批完成 P2 macOS 核心场景与 Windows 功能实现，发布 alpha.3 后，tag CI 暴露已有关闭测试的启动竞态；改为等待 native-entry barrier，最终发布 v0.1.0-alpha.4，已发布 alpha.3/tag 保持不变。后者仅收尾测试与版本指引，无需等 P3/P4 或 Windows 实机。P2 实机证明与未验收边界见 [窗口捕获](window-capture.md)；锁屏/断连/跨屏保留专项验证项，当前 alpha 不承诺这些场景。用户随后调整 P3：以短前台事务完成完整键鼠任务，优先总占用时间；本批依据扩展 POC 实施 cooperative 模式。最低 macOS / amd64 / IME 与更多真实应用的兼容性扩展保留后续专项，Windows 实机和 Bot M0 继续后置。

候选提交需要三平台 CI 和发布所承诺平台的独立实机任务通过；分发包验证 `dtw` 路径、版本/revision、校验值、权限诊断和 managed host 停止/原收据恢复。若先发 macOS alpha，Windows 明确列为功能实现、实机适配后置、不承诺可用，不声明双平台可用或正式发布。CLI 名称变化与逐 provider 支持矩阵须写入发布说明。

正式创建 tag/release 后独立验证下载资产、归档内容与校验值；签名/公证只声明实际验证结果。caelis-bot 的版本更新和 M0 统一联调另成任务。
