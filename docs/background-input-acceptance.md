# macOS 后台输入与会话历史验收

本轮基于 main `57fc54d6a711af44b4c82bda044a770a6131c57a`，保持标准 rmcp stdio 与 `exec(app, source)`。本轮验证本地签名开发候选及标准 MCP + Skill 的外部模型任务；不代表新的公证安装包或正式 release。

## 2026-10-30：#68 焦点归还审查修复

本轮只收尾焦点协调和原窗口归还。最终候选 P 的二进制 SHA-256 为 `9ae1aa0d27c8196bf9f74711b1fca10285ce936e5a23ff40526621e4f889141e`，Developer ID 签名与严格校验通过，64 个源码及构建文件与 manifest 无漂移。310 项 Rust 库测试、96 项 Python 验收判据、Swift 原生纯控制通过；没有全量构建上游 CUA。

- 后台输入/语义写入先持共享协调租约，前台操作从借用到 inline 归还持独占租约；锁序统一为协调 → 实例 → 物理输入。显式 focus 的脚本终结和退出清理重新取得同一独占协调，不能无锁修改原应用。独立后台操作仍可并发。
- 只在原进程身份、原 AX 对象、窗口 ID 和实际前台均得到证明时报告 `restored`。真正没有原 focused window 时单独报告 `application_restored`，读失败不能伪装成无窗口。原应用由新鲜 AX 与已有 CUA WindowServer PSN 查询共同证明，避免常驻 worker 的 AppKit 缓存成为归还依据。
- 焦点采样携带内部阶段版本；较早开始、较晚返回的采样不能误伤已建立或已重定向的焦点。真实 app/window 通知及新的外部变化仍使借用失效。
- 激活后短暂缺少 WindowServer 身份，只在已有归还预算内重新读取。缺失符号、无效 PID、真实身份变化和取消仍终止；不会重复 raise、activation 或输入。

| 同一 P 候选的实际验收 | 结果 |
| --- | --- |
| 完整 AppKit / 标准 stdio MCP | 后台文本/点击、同 PID 双窗、两 app 后台并发、前台回退、重复 focus、同 PID 原窗口归还、外部 app/window 留返及闭窗均通过 |
| A1 原前台 → B 前台输入，同时请求 A2 后台长文本 | 全套及独立第二次均通过；两个请求重叠，原生输入区间串行；两段全文、目标窗口、按键释放与最终 A1 全部核验 |
| cancel / EOF / parent exit | 三条前台输入中断均保留原始部分前缀、释放配对且无后缀/重放，精确原窗口归还，worker 清理通过 |
| 30 秒脚本超时 | 原 `script_timeout` 保留，无 shutdown error，原 A1 窗口归还 |
| Chrome 真实页面 | 普通后台文本、Unicode/快捷键自动前台路线、遮挡坐标点击且恰好保存一次、真实滚动及清理全部通过 |

混合场景的最终 A1 断言覆盖 B 前台归还与 A2 后台 key-window 恢复的合成结果；它不是对 B 归还瞬间的独立时间证明。同 PID A1→A2→A1 用例另行确认精确窗口。显式 focus 终结若等待其他后台操作超过原有清理预算，仍报告归还未确认，不中断其他操作或无锁归还；300/350/400 ms 清理预算没有延长。

以下失败保留原始记录，不用最终通过样本改写：

| 原始样本 | 保留的结论 |
| --- | --- |
| L / 新混合回归 | 确认两条原生输入区间重叠；两段全文仍正确，未复现误投或截断损害 |
| M / mixed | 输入已串行且全文正确，但最终前台仍停在 B；原日志没有归还分支诊断，具体跳过原因未证明 |
| N / all | 原窗口快照校验在派发前拒绝，独立 fixture 保持 A；旧日志未记录捕获的 previous PID。源码确认存在缓存权威问题，不能据此宣称 N 的具体分支已定位 |
| O / all | 切窗控制比脚本 hold 开始早 19 ms，未按预定阶段施加干预。探针增加 native focus 完成同步点；最终脚本时间区间与外部选择断言保持不变 |
| O / Chrome | 第二次归还在激活后的 WindowServer 只读查询缺少身份时立即结束，原 unknown 保留。旧桥接没有区分查询失败或空 PSN；P 已保留细分原因，并以纯控制验证有界只读恢复 |

采样阶段竞态有固定顺序的回归证明，但不认定它就是 L/M 失败的原因。P 的真实运行记录了一次过期采样被丢弃；成功 GUI 没有自然触发 WindowServer 缺失恢复分支，其覆盖来自纯控制。闭窗使用前台路线，不声明后台闭窗。新后台约束、光标和滚动锚点未在本 PR 加入。L 的 Claude/agy 任务结果仍只属于 L；本轮没有重新运行外部模型、安装包、公证或 release 验收。

原始记录在本机 `/private/tmp/dtw-review68-*`；只提交[脱敏证据与源码哈希](evidence/focus-return-2026-10-30.json)。

## 2026-10-29 阶段状态

2026-10-29 首轮实现和本机验收完成。该轮签名候选 L 的核心后台输入、Chrome、同 PID 浮层、TextEdit、生命周期及两种外部 Agent 任务均通过。保持原失败证据和未覆盖边界，不等同于发版验收。

L 二进制 SHA-256：`0f6d9ffee25bf3c90965e64cae81669ff225801388bdd8d7c5ea4d6343ca8c2b`。源文件 manifest 已与当时工作树逐项复核；307 项 Rust 库测试、93 项 Python 验收控制、轻量 source pipeline 和 Swift 原生契约通过。本轮没有全量编译上游 CUA。

| L 模型任务 | exec / MCP | 耗时 | compact JSON | 结果 |
| --- | --- | --- | --- | --- |
| Claude / gpt-6.1-terra | 5 / 6 | 66.641 秒 | 9141 字节 | 业务、易用性、清理全部通过 |
| agy | 5 / 6 | 283.942 秒 | 10201 字节 | 业务、易用性、清理全部通过 |

两者在独立 Chrome profile 中通过真实 AX 操作 240 条记录的业务页，修改两条记录且各保存一次，再跨 app 编辑并保存 TextEdit 多行报告。无 issue、无 result 查询。步数门槛已通过；agy 本次总耗时仍明显高于 Claude，不能把少调用次数等同于低延迟。


## 范围与可核验目标

- 已完成执行按记录数、回执字节和 payload 字节滑动回收，不再以累计 512 次拒绝新工作。应用最新 state、存活引用和 owner 独立保留。
- 从固定 CUA SHA 搬入后台输入依赖闭包，内部选择目标投递；只有尚无准备/输入副作用的预检拒绝才允许转前台。效果不明时不重投。
- 同进程实例写操作串行，独立 app 的后台输入可以并行；前台借用及归还使用独立共享输入租约。归还时尊重外部 app/window 选择。
- Unicode、快捷键、坐标点击、默认滚动以 AppKit/Chrome 独立读回为验收目标。后台路由成功发布事件仍只表示 dispatched；本轮实际覆盖见下表。

## 保留的失败样本

原始证据保留于本机 `/private/tmp/dtw-background-probe-20261029-*`，不将含本机窗口库存的原始记录提交至仓库。

| 样本 | 结果与解释 |
| --- | --- |
| a / b / c | owned fixture 未能可靠成为前台，未开始 DTW 输入；修正 fixture 的 LaunchServices 激活，并确认签名已完成后再开始测试 |
| d | 后台 Unicode/Backspace 实际文本正确；点击回执 dispatched，但 AppKit click pad 未收到事件。原适配对全部 app 使用 Chromium recipe，随后按框架选择已有 generic AppKit recipe；保留失败，不以回执冒充点击成功 |
| e | 同一 PID 两个窗口的后台文字分别落入正确窗口，前台与指针采样保持不变 |
| f | 两会话各 8 次、约 200 字符的并发样本触发 30 秒 script_timeout。每事件重复完整 AX 校验耗时过高；保留原负载和期限，改为首次完整准入、后续精确单窗/身份/模态检查，等待期仅检查本地取消和期限 |
| g | 原候选的一次显式 focus 后归还通过；不推导用户离开再返回或同 app 切窗已覆盖 |
| h | 新候选后台文字和真实 pad down/up 均成功；旧探针将键盘到点击全程的指针位置不变作为统一判据，因约 0.4 点变化失败。用户确认当时在使用鼠标；这是验收判据过宽，不是已确认的输入缺陷。不再追查或用微调阈值掩盖，改为按动作/目标判定并保留无关桌面变化记录 |

原候选 SHA-256：`fd42bcc36553e61693415171a28f9f99842c84de7f816931c2bc41814f9a0358`。

## 前一轮候选 D 与结果

前一轮本地候选 D：`0e7386ec48332bf73e3974cbc210cb9ecf106d8a9c180d9237ae97e48069729c`。Developer ID 签名及严格验证通过；源码 manifest SHA-256 为 `ad92f7efe7bc1964a64a59447a564d94355bbadbeaaa728a496f545c7c1a41ee`，复核无源码漂移。前一候选 C 为 `077bfb22680a02925bfee722ef055d6fe69cb3e9f55e67dab71a50f61b906929`；C 的结果不冒充 D 的全量验收。

自动检查通过：301 个 Rust 库测试；轻量 source pipeline（Core 63、脚本 21、macOS pure input 20、Windows pure input 5、Python 123）；Swift 严格编译与原生契约，包括新增 11 组首次点击就绪控制。既有 CLI 4、stdio 3 项在本轮较早候选通过。本轮不重建全量上游 CUA。

| 真实验证 | 结果与证据边界 |
| --- | --- |
| AppKit 后台 Unicode、Backspace、坐标左击 | D / m 通过实际文本与 down/up；目标未出现在采样前台，用户指针变化不作为失败 |
| 两个独立 MCP 会话并发 | D / final-concurrent 两 app 各 8 次、192 字符，分别 4.384 / 4.363 秒；原生事件区间重叠，完整文本路由正确；此前 C / j 为 4.373 / 4.411 秒，没有削减原超时负载 |
| 取消、EOF、父进程退出 | C / lifecycle-a 三项通过；原始部分前缀、各 down/up、未执行后缀、无重放及 exact worker 清理分别验证；断管不伪造完整终态账本 |
| 自动前台回退 | C / k 独立 right double-click 通过；先后台输入再回退的 C / i、l 与 D / m 失败，不能据 k 宣称问题已解决 |
| 显式/重复 focus 归还 | D / final-focus-return 的一次、两次及同脚本 primary→secondary→primary 三组均通过；C 的重复 focus 失败样本保留，不宣称主动外部留返已覆盖 |
| 闭窗后 owner / 库存 | D / final-close-window 已见真实 close 通知和释放配对，原 unknown 保留；随后窗口库存仍含旧窗口，整项未通过，尚不能区分关闭过程延迟与库存边界缺陷 |
| Chrome 冷 AX / 深层发现 | C / browser-c 最初三轮仅 53 节点空结果；第 4 轮、总计约 2.442 秒找到深层按钮，后续 12 页到 visited 1213 的定位、筛选和开表单成功；只重复读取，未重放写入 |
| Chrome 后台键盘 | C / browser-c 首字符 `s` 已到达真实 textarea，随后 `background_input_failed` / unknown；保存次数为 0，坐标保存和滚动尚未进入 |

## 前一轮阻塞清单（后续结果见本轮复核）

1. **后台输入后的物理前台回退。** D 已移除“AX focus 成立即可跳过 raise”的逻辑，并在首个 pointer/down 之前给原目标点最多 750 ms 的只读就绪窗口；第二击仍严格单次验证。D / m trace 证明 raise/focus 已请求，但原 owned observer 仍以 layer 0、alpha 255 遮挡目标点，最终 `target_occluded / not_dispatched`。没有绕过遮挡证明，也没有重投点击；根因仍待定位。
2. **闭窗后的库存收敛。** D 已观测原窗口关闭，后续 `windows()` 仍含旧引用。需核对原生关闭生命周期及有界只读收敛，不通过挑选其他窗口或直接忽略旧引用掩盖。重复 focus 归还已在 D 修复并通过三组验证。
3. **Chrome 后台键盘目标连续性。** 查明首字符后哪个真实目标/原生焦点条件变化；保留 original unknown，不通过取消检查或自动重放掩盖。修复后再进入保存、滚动及外部 Agent 任务复测。
4. 同一最终候选补齐后台同 PID 多窗、主动 app/window 留返、闭窗及 lifecycle 回归。新的 agy/Claude 场景、正式安装包、公证和发布仍未执行。

## 本轮复核：E → F → G

候选 G SHA-256 为 `3c69c2d9e840b02cf7c70ae762be84e0c4786f57ccc5d9125e1d185022451b1f`，Developer ID 签名并严格验证。对应 `/private/tmp/dtw-background-gui-20261029-g/source.json` 保存源码哈希。

| 已定位问题 | 修正和同一 G 候选的实际结果 |
| --- | --- |
| Chrome 首字符后的通用 target-changed | 原始 trace 实际为 `AXModal` / `cannotComplete (-25204)`。仅对该暂时读失败在原 deadline 内有界重新读取同一对象，每次前后复核 surface/app；不重发输入。保留最初原生原因，真实目标失效立即结束。15 个纯控制覆盖此恢复；不宣称最终 GUI 必然触发了该暂时错误 |
| 后台转物理路线、同 PID 切窗 | 移植固定 CUA 的 exact-window front-process + make-key 记录，在既有物理租约内执行一次，随后 AX raise / 精确焦点证明。G 全套通过后台输入后自动右双击、一次/重复/同进程切窗 focus、外部 app 和 sibling window 离开再返回时保留用户选择 |
| 关闭后旧窗口库存 | 全量 CG 列表含已关闭 backing store；库存现逐个使用动作相同的精确查询。空结果可排除，非空畸形/换 owner 仍返回错误。关闭通知先于 WindowServer 退场；G 保留每次只读库存，0.212 秒后原 ref 消失，应用 state/主窗保留；原 close unknown 不改写、不重放 |
| Chrome `Cmd+A`、补充平面 Unicode | E 收到 keydown 但未实际全选/插入 emoji。针对这两类能力在任何 prime/input 前整体选择现有前台路线；普通 BMP 和普通键保持后台资格。G 独立 DOM 精确读回 `后台复核：已核对 🐦 café`，不使用剪贴板或 setValue 替换输入测试 |
| Chrome 坐标点击 | F 实际落点额外偏移窗口 origin。将 donor 的 window-location 字段改为已有 window-local 坐标，主事件坐标仍为 screen。G 被自有窗口遮挡的保存按钮收到精确点击、恰好保存一次；后续滚动有真实 wheel 和 scroll offset 变化 |
| 旧版同 PID 浮层遗漏 | 补齐非零 layer surface 库存，复用原 capture/click。G layer 3 浮层截图、精确一次 down/up、主窗无误投、关闭后旧 ref 拒绝全部通过；见[回收审计](legacy-capture-recovery.md) |
| 生命周期 | G 的 cancel / EOF / parent exit 各仅一份输入请求，均保留原始部分前缀（44 字符）、匹配 key up、未执行后缀和 exact worker 清理；没有补发或自动 replay |

G 的 AppKit 同 PID 双窗、两 MCP 独立 app 并发全部通过。Chrome 普通文本、点击、滚动的采样后台覆盖成立，遮挡在动作边界成立；含快捷键/emoji 的部分使用前台，不宣称该部分全后台。307 项 Rust 库测试、轻量 source pipeline、原生纯合约通过；新增四个浮层 oracle 已纳入轻量入口。无全量 CUA 构建。

E、F 的失败分别保留于 `/private/tmp/dtw-background-followup-*-e*`、`*-f`，最终 G 的原生证据位于 `/private/tmp/dtw-background-followup-{all,browser,lifecycle}-g` 与 `/private/tmp/dtw-application-surface-g`。关闭动画的读取收敛保留每一条原始 receipt，没有用重试动作消除失败。

外部 Agent：已核对 npm 最新 acpx 与本机均为 0.21.1，两处 Skill 与已安装包 SHA 一致。Claude 0.98.0 官方 ACP adapter / gpt-6.1-terra 首轮约 34 秒后因 Chrome `app_unavailable` 停止，尚未修改业务数据；agy 也遇到同一发现问题，之后偏离限定工具路线，已停止且不算有效模型验收。相同期间监督 MCP 仍能列出两个 Chrome 实例。PID 定点只读诊断证明进程存在、birth stamp 和 `proc_pidpath` 成功，失败在 bundle path 分类；独立原生读取也能复现，尚不归因于 Host 权限。H 仅为含 opt-in catalog trace 的诊断候选，不用它替代 G 的上述 GUI 证据。

## 外部 Agent 与输入边界：H → I → J → K

Chrome 的系统 `code_sign_clone` 可令 `proc_pidpath` 返回 `.app.bundle` 硬链接路径。I 仅在无法按普通 bundle 路径分类时读取同 PID 的 AppKit 注册路径，要求 executable 的 device/inode 一致及前后 birth stamp 不变；不按名字猜测，也不扩大 helper 归属。真实硬链接、伪装同内容不同 inode、缺文件及嵌套 helper 均有控制。受控 Python alias 实验证明路径别名可变化，但未直接令同场原生 worker 变化；H 原生失败另行保留。

I 的 Claude 已完成业务，却发现 TextEdit 收到 Command+W 而未关窗。J 将所有 app 的 printable Command 菜单等价键在无副作用预检阶段选入前台路线。J 的 agy 又发现 TextEdit 后台多行文本丢失换行；K 同样将含 CR/LF/Tab 的整段文本和独立 Return/Tab 在派发前选入前台。普通文本仍使用原后台能力，不将这些完整任务宣称为全后台。

| 模型样本 | exec / MCP | 秒 | compact JSON 字节 | 结果 |
| --- | --- | --- | --- | --- |
| Claude J / 旧 Skill | 9 / 10 | 81.102 | 12041 | 业务与清理通过，超出原 8 exec 门槛，保留失败 |
| Claude J / 调整后 Skill | 5 / 6 | 55.692 | 8756 | 业务、易用性、清理通过 |
| agy J / 调整后 Skill | 11 / 13 | 335.031 | 26890 | 换行丢失、后续模型修复和一次原始 modal 不确定性；整体验收失败 |
| agy K / 调整后 Skill | 6 / 8 | 232.604 | 19539 | 业务、易用性、清理全部通过；无 issue、无 result 查询 |
| Claude K / 调整后 Skill | 6 / 7 | 59.944 | 9741 | 业务与易用性通过；Chrome 清理报 AXRole -25202，整体验收失败 |

Skill 将初次相关字段发现与截图合并，将表单动作名和字段类型一起发现，并把后续修改、保存及回读保持在同一脚本。没有降低门槛：至多 8 exec、12 MCP、40 KiB compact JSON，且无 issue/result 补查；时间单独测量，不宣称 agy 与 Claude 速度等同。调整后 Skill SHA-256 为 `3990d734e59eb300e755b5827d7957bee7d1fd3b7e3872243636330ab1f4f978`。

K 的 TextEdit 独立控制用 UTF-8 原始字节比较整段多行、中文、emoji、Tab 和 Return；两组均通过，独立 observer 的新采样均见前台归还。旧探针的 universal-newline 文本读取不能证明原始换行字节，已经更正；原始样本仍保留。四份先前失败案例保留的自有文档，另行通过磁盘内容与新 AX 读回后关闭，未改写旧回执。

K 原生全套、Chrome、同 PID layer-3 浮层、生命周期均通过。首次 lifecycle 调用在 setup 前因缺少必须的 trace 环境变量退出；单独带 trace 的调用通过，此配置错误不作为运行时失败或通过。新的 native-only workbench 诊断保留了冷 AX 只有 browser chrome 的首次空查找，并使探针的初次 inspect/capture 与有界只读发现匹配 Skill；未重试写操作。

## 最终 L 与证据修正

L 对抬升后失效的辅助 AX 对象做最小处理：只有明确非原目标的 typed `invalidUIElement`，且重新读取完整父库存证明旧对象消失，既有 750 ms 只读 focus poll 才重验全部目标/模态状态。原目标根、仍在库存的坏对象、真实 modal、身份变化与不完整库存继续停止；不重复 raise 或输入。新增 phase/scope 诊断和 81 条纯断言。K 原始失败已定位到抬升后、按键前，具体 AX 对象未被原 trace 捕获；最终 GUI 没有自然触发新恢复分支，不用通过样本冒充该分支的现场复现。

L 首轮原生诊断还保留三类验收问题：冷 AX 首批只含 browser chrome；observer 发布空洞；EXPECTED 恰好是 EXPECTED+Z 的中间文本，探针过早检查 Backspace。修正仅在探针：native-only 任务先有界只读等待目标控件；采样阈值仍是 0.2 秒，空洞贯穿后台/焦点/遮挡覆盖并标为未证明，缺终样本或时间倒退仍失败；浏览器需同一日志读中有可信、有序 Backspace down/up，并有晚于 keyup 的最终文本 snapshot，最多只读等待 6 秒。

相同 L 二进制的 L2 原生业务、全套 AppKit、Chrome 及生命周期全部通过；各采样区间满足原 0.2 秒阈值。同 PID 双窗、两 MCP 并发、前台自动回退、显式/重复/兄弟窗口 focus 归还、用户切 app/window 后留返和闭窗库存分别验证。当前 Cmd+W 按预检使用前台，闭窗不宣称后台。L 的独立浮层和 TextEdit 原始 UTF-8 字节控制也通过。原失败未覆写，未对其重发动作。

## 验证边界

- fixture 以 20 ms 采样前台/指针，并记录激活和本地输入事件。用户可操作鼠标与其他 app；无关指针/窗口变化不判操作失败，也不从采样差异推定 DTW 因果。不主动归位/移动真实光标；目标投递路线的代码审查与目标实际效果分别记录。
- Chrome 使用独立临时 profile 与 240 条记录的本地业务页面；AX 查找、编辑、真实事件、唯一保存和滚动由网页独立日志检查。坐标来自只读页面几何，不代表模型视觉定位验收。
- 最终 L 的 agy/Claude 完整任务通过；既有六个模型任务仍仅证明其原始安装候选。
- 未承诺锁屏、任意 app/系统版本、复杂菜单/系统对话框、远程 MCP、Windows 后台输入或 Linux 桌面支持。
- scroll 的现有公开合同仍以窗口中心为锚点；面向嵌套区域的坐标/元素锚点是后续能力组，不用“先点击”掩盖这一限制。


## 复现与证据位置

最终签名候选与 source manifest 位于本机 `/private/tmp/dtw-background-gui-20261029-l/`；最终证据为 `/private/tmp/dtw-final-l-*` 与 `/private/tmp/dtw-final-l2-*`。原始证据分别位于上述 `dtw-background-probe-20261029-*`、`dtw-background-final-*`、`dtw-background-browser-20261029-*`、`dtw-background-lifecycle-20261029-a`。它们保留原始失败与清理信息，但可能包含本机窗口库存，所以只提交[脱敏摘要](evidence/background-input-2026-10-29.json)。

使用新的输出目录运行，禁止复用失败目录或自动重放写入：

```sh
DTW_AX_READINESS_TRACE=1 DTW_SCENE_TRACE=1 python3 tests/acceptance/background_input_probe.py --dtw <signed-binary> --output <fresh-directory>
DTW_AX_READINESS_TRACE=1 python3 tests/acceptance/background_browser_probe.py --dtw <signed-binary> --output <fresh-directory>
DTW_AX_READINESS_TRACE=1 python3 tests/acceptance/background_lifecycle_probe.py --dtw <signed-binary> --output <fresh-directory> --case all
DTW_AX_READINESS_TRACE=1 python3 tests/acceptance/application_surface_probe.py --dtw <signed-binary> --output <fresh-directory>
DTW_AX_READINESS_TRACE=1 python3 tests/acceptance/textedit_control_probe.py --dtw <signed-binary> --output <fresh-directory>
DTW_AX_READINESS_TRACE=1 python3 tests/acceptance/browser_workbench_probe.py --dtw <signed-binary> --output <fresh-directory> --client <native|agy|claude>
```

后台 probe 不主动移动鼠标或要求无关桌面静止。foreground-fallback 用例会实际借用前台和物理指针；其他 app 的无关事件只记录，当前动作目标的冲突仍保留原错误。

## 后续能力组（未在本次焦点修复中实施）

继续保持 `exec(app, source)` 与按能力组迁入固定 CUA 源码的方式，依次验证：

| 能力组 | 可验证的交付标准 |
| --- | --- |
| 服务级后台约束与实际路线回执 | 默认仍自动选路；服务可禁止借用前台，不支持时在任何准备/输入前拒绝；现有回执带出实际路线、借用原因及归还结果。不给每个 Agent 动作增加 delivery 参数 |
| 独立 Agent 光标 | 只反馈实际派发位置；会话持有、穿透点击、退出清理；截图/遮挡只排除 DTW 确切拥有的 overlay。允许用户正常移动鼠标，分别验证输入实际效果和绘制 |
| 精确滚动锚点 | 复用新鲜截图坐标或语义元素引用，默认中心合同明确；双栏、树和嵌套表格只滚动指定区域，不增加全局坐标入口 |

后续再分别推进菜单/文件对话框、浏览器 exact-tab 生命周期及 Windows WGC + 定向输入。Linux 保持独立实验验证范围，不由 macOS 证据推导支持。
