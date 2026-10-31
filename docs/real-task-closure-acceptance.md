# 真实应用闭环验收

基线：`c95e5be150a877dc960399fae87f91a05c2039f0`（#69）。本记录覆盖[本批实施计划](real-task-closure-plan.md)。更新至 2026-10-31：闭窗焦点清理已修复，开发候选的三个真实任务完成；最终安装包与 Windows 实机仍待验收。下方保留原始失败，不把后续通过改记到旧字节。

## 当前开发候选结果

DSH 0.2.1-rc.5 使用 DeepSeek-V4.1-Flash（`deepseek-flash`），通过内置 ACP/MCP 和通用 Skill 完成三个任务，无 DTW 专属 DSH 插件。候选 Z 二进制 SHA-256 为 `204ffdb5a739d69ac01d102462add69bbe80e51a14e201495dbd3bc31676a559`，Skill 为 `a693d04ec304cf8d7bf428d7365a7ae159e0b1fbd3a30c9818efbeb26b8ec3df`；运行时/Skill 与 `0d507be` 对齐。

| 应用 | 秒 | Agent 工具 / MCP / exec | app 方法 | 含失败结果的 exec | 完整结果 |
| --- | ---: | --- | ---: | ---: | --- |
| Chrome 240 行工作台 | 80.390 | 29 / 27 / 25 | 64 | 1 | 两条记录各保存一次、重开核对、文件选择上传、其余记录未变；独立服务器读回与 GUI 复核通过 |
| WPS | 192.871 | 48 / 46 / 45 | 105 | 10 | 新文档四段正文、另存、关闭、目录权限弹窗、重开读回通过；seed 未变 |
| Obsidian | 334.575 | 64 / 62 / 61 | 186 | 7 | 隔离 vault、两份笔记、搜索重开和内部链接跳转通过；磁盘内容与 GUI 复核一致 |

三者清理通过。Chrome 的一次失败结果是关闭表单后有意捕获的 not-found 检查；WPS/Obsidian 含模型 API 误用、探索以及实际 stale/partial 拒绝，不称为零失败运行。原始自动证据保留 `pending_gui_review`；单独的 GUI 复核记录绑定原日志 hash 并确认重开/搜索/链接等子目标。原日志和图像本机保留，[摘要与证据哈希](evidence/real-task-closure-2026-10-31.json)进入仓库，不发布个人桌面清单。

WPS 的不完整 AX 查询曾扫描 58 页后超时，原 setValue 为 `not_dispatched`。`b8d8d21` 在已证明存在归属缺口时停止当前分页，保留重新查询和唯一性规则。最终开发候选 AA（SHA-256 `f13770da5c92b4b6d1e09eadcd13b36b8a87bdfafb400a1e8d550b86da7dfc67`）在同一个真实 WPS 另存为窗口做旧/新对照：旧版 62 页、6,629 节点、15.049 秒后超时；新版 1 页、105 节点、0.231 秒返回无法确认，随后 0.300 秒截图成功且上下文仍可用。菜单、模态关闭及 worker 清理通过。三个完整 DSH 任务没有在 AA 上重跑，不能把 Z 的结果直接算作 AA 或发行包验收。

WPS 闭窗后的焦点路径已在 `e167835`、`882cc13`、`9cb5b62` 中修正：精确前台身份重新求证；无法确认归还资格时保留诊断并结束借用，不再因此丢弃整个可继续的会话，也不谎称焦点已归还。后续 Z 的 WPS 完整关闭/重开和 AA 清理实机通过。H 的 `unknown` / `shutdown_failed` 回执保持原样，已不作为当前未修缺陷。

## 本轮审查修复与发布待办

#70 的 Windows 修复区分正常 UIA 空尾和真正 HRESULT 读取错误，对区域引用在语义输入及 readText 前后复核几何约束，并按 label/name/value 独立匹配。包内新增同源 UTF-8 Chrome 工作台，WinForms smoke 覆盖低成本阻断项。源码/单测/原生构建与用户 Windows GUI 结果分开记录；本机没有运行 Windows GUI。

三应用入口已支持 `--installed-package`，保持实际安装路径、包内 Skill 和前后完整清单。当前尚未给最终签名/公证安装包记三应用通过。下一步目标为 Caelis、DSH、agy；Caelis 通过 plugin 集成，Core 的每 Session 首次 app 授权由 `deepseek-flash` 与 `gpt-6.1-terra` 验证。agy 模型探索不作阻塞，复现的 DTW 缺陷仍要修复。旧 Claude/agy 六轮硬门槛被[当前发布计划](first-rewrite-release-plan.md#agent-acceptance-policy)明确替代，既有样本留存。

通用 fresh-capture 历史偶发的自然原因仍未证明，只确认狭窄的几何变化恢复合同。Windows 后台输入、锁屏、DOM、Linux 正式支持继续后置。

## 验收方法

Claude 使用 acpx 0.21.1 与官方 Claude ACP adapter 0.99.1，任务模型为 `codex/gpt-6.1-terra`；agy 使用本机普通插件与 MCP 注册。执行 Agent 只读取随包 Skill 和 DTW 返回结果，经标准 MCP 操作真实应用，不用 shell、DOM/CDP、HTTP 或直接文件编辑代替 GUI。监督脚本独立准备与核对自有数据，并记录原始回执。

每个候选保存二进制、源树归档和全部 Skill 文件的 hash。每次模型尝试使用新资源和新会话。指标中的“方法数”是尝试调用的 `app` 方法数，不是底层平台 API 次数；“失败 exec”包括脚本、原生动作和清理失败。完整任务必须同时有独立结果和 GUI 子目标证据，磁盘数据正确不自动代表任务通过。

Skill 主文档说明协议和能力，`PATTERNS.md` 按需说明文件选择器、表单、动态控件和大树发现。具体应用的任务步骤只存在于验收材料。

## 保留的前序模型样本

以下均为 Claude 的独立尝试。失败样本保留在分母中；后续修复不改写原结果。

| 候选 / 应用 | 秒 | Agent 工具 / MCP / exec | 方法数 | 失败 exec | 完整任务结果 |
| --- | ---: | --- | ---: | ---: | --- |
| C / Chrome | 127.551 | 20 / 17 / 16 | 未完整记录 | 3 | 未完成：两条记录与附件数据正确，但文件面板关闭后的焦点清理阻断最终读回 |
| D / Chrome | 119.188 | 20 / 17 / 16 | 119 | 2 | 完成并有恢复：两次只读 AX 暂不可用后刷新，保存各一次、重开核对、附件接收均有证据 |
| D / WPS 1 | 96.903 | 13 / 11 / 10 | 20 | 5 | 未完成：截图/焦点失败；用户报告同期桌面操作，未将全部错误直接归因于干扰 |
| D / WPS 2 | 131.556 | 20 / 18 / 17 | 74 | 2 | 未完成：正文已编辑，模态文件名输入无效果，宽查询超时后旧 worker 无法继续 |
| D / Obsidian | 222.100 | 35 / 33 / 32 | 81 | 1 | 未完成：vault 已创建，编辑焦点切换时输入进入旧控件；随后遇到验收 harness 的固定轮数上限 |
| E / WPS | 297.635 | 38 / 35 / 34 | 59 | 3 | 未完成：文档保存、关闭及磁盘内容通过；重开时两个应用级 modal 相互阻挡，GUI 读回和清理未完成 |

旧 harness 的 35 轮上限已移除，保留 900 秒外部任务监督预算。该调整不把旧 Obsidian 样本改判为通过。

## 已有直接原生证据

- D 的自有 Chrome 任务验证区域/子树查询、菜单分页，以及 840 个 UTF-16 单元的中文、emoji、换行内容分七页精确读回。非法 surrogate 边界返回 `invalid_text_range`，后续合法读取仍可使用该引用。另一次人为延迟导致的 cursor 过期也原样保留。
- D 的自有 WPS 面板中，后台文本投递返回 dispatched 但字段未改变；同一面板的前台输入改变了字段。修复采用通用模态键盘预检，在派发前选择原有前台路径，不按应用名称分支，也不重放已发输入。
- F 在 E 保留的两层真实 WPS modal 上验证修复：原生焦点、根对象和所有者证明当前活动面板，截图区域查询访问 37 个节点后找到操作，语义 press 成功且未借用前台。新的窗口清单及截图证明文档已重新打开，四段正文与保存结果一致。该直接 MCP 回归不改写 E 外部 Agent 的失败结果。
- F 后续关闭快捷键因系统级焦点属性不可读而在派发前拒绝，原回执为 `not_dispatched`。独立只读检查发现应用自己的 AXFocusedWindow 可读，既有 WindowServer 双 PSN 比对也能证明前台进程。G 增加仅针对 typed 全局 AX `cannotComplete/noValue` 的候选证明，保留精确窗口、进程身份和前后复核；实际 trace 证明在全局 `noValue` 时成功记录并精确归还原窗口。
- G 的同次关闭在 AXRaise 失败后停止，键盘派发阶段未到达；由于抬升已经可能生效，原回执仍是 `unknown`，未重放。该版本没有记录 AXRaise 原始错误码，不能推断失败原因或用后续成功替代结案。
- H 仅补 AXRaise 失败诊断，不改变动作或恢复策略。显式 focus、截图与精确归还通过。随后新的关闭请求实际关闭了 Review 文档，但归还阶段读取精确 AX 窗口身份失败：`AXError=-25204, id=0`。原执行为 `unknown / focus_return:unconfirmed`，服务返回 `shutdown_failed` 并关闭后续执行；进程退出码 2，原生 worker 已回收，无强杀。独立新连接只读清单和截图确认当前为 seed、Review 标签已消失，不改变原失败回执。

G 的两次截图曾返回 `window_unavailable`；用户切回 macOS 桌面后截图恢复。此记录只证明当时画面不可共享，不将全部焦点错误归因于用户操作，也不宣称所有 Space 可后台截图。

查询的 partial 原因保持可见。例如 F 的区域查询返回 `ownership_unproven`；已证明的目标可执行，不将找到一个目标等同于整个区域完全可枚举。

## H 阶段的历史边界（已由上方状态更新）

本批使用 Developer ID 签名的本地开发候选，不是已公证的发行包。已有轻量源码检查、373 项 Rust 库回归、macOS 原生纯控制和 Windows provider 类型检查；这些不替代真实模型任务。G 的完整 native pure suite、H 的 Swift 类型检查和构建均通过；Rust 与 Cargo 文件在 E 的库回归后没有变化。Windows 包内计划和链接修复通过 26 项现有打包测试。

H 阶段将 WPS 关闭后的 `releaseFocus` 身份读取列为后续调查，区分 AX 对象失效、焦点变化和 `cannotComplete`；当时不能确定具体机制。后续修复与当前状态见上方，不再要求从 H 重启实施或重放关闭。原执行 `execution_21a52580-5ce4-42e2-9267-a9110f7b740d` 及前序 `execution_66c85659-249a-4620-980b-7d0cf3ee4ce5` 均保留。

H 暂停时尚未开始下一轮 Obsidian/Chrome，也未进入发行。其后 DSH 结果和更新后的 Agent/安装包门槛见上方；不再沿用旧的 Claude/agy 六轮硬门槛。Windows 实机验收交由用户独立完成；当前类型检查不能证明 Windows 运行行为。Linux 保持实验实现。

脱敏检查点和候选 hash 见[阶段证据](evidence/real-task-closure-checkpoint-2026-10-30.json)。H 二进制 SHA-256 为 `5e6dbc2e999f67fca8e6f86aac833770466cab2959fb72196dc8a8831de8f3a7`；源树归档和完整原始证据在本机保留。

当前真实任务未建立跨 PID 关联 helper 的 GUI 样本，不能用同 PID 文件面板成功外推。锁屏、远程传输、DOM/tab provider 和 Windows 后台投递不在本批承诺内。原始窗口截图及完整日志本机留存，仓库仅收录任务结果与必要的脱敏证据。
