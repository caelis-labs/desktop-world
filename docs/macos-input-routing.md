# macOS 输入路由与真实光标

Agent 仍只调用 `apps`、`exec` 和 `app` 方法。输入路由是实现细节，脚本不选择
虚拟/物理光标、不维护 Session，也不根据内部投递方式安排重试。正常脚本回执通过
`effects.delivery` 汇总已报告的路线和借用动作数，通过 `effects.focus_return` 单列脚本结算；
私有执行账本保留原始投递证据及派发不确定性。汇总动作数不是激活次数。
虚拟光标反馈覆盖坐标 click/move/drag/scroll；语义 AXPress、键盘和 AXPosition 不伪造鼠标轨迹。

本实现接续[逐操作历史审查](macos-ax-cursor-audit-2026-11-03.md)。历史失败记录保留，
其“当前产品”描述只对应当时的二进制，不代表下述当前源码合同。

## Agent 合同优先级

首先保证任务功能和可验证的结果。已知的路由选择、准备及必要等待由 DTW 内部处理，
普通调用不需要 Agent 编排焦点、重放或睡眠。已派发但无法证明效果时如实保留不确定性，
不把发送成功当作操作成功。只有明确的前置拒绝才能触发安全的自动路线切换。

在此基础上持续减少真实光标位移与前台占用，并归还借用的资源。协议保持少量清晰方法；
`app.focus()` 是明确需要前台时的入口，正常输入仍自动选择可用路线。

## 分类规则

`auto` 下每次 `exec` 从后台开始。先证明授权应用、进程出生标识、具体窗口及截图几何，
再选择当前动作的投递路线。只有已知不支持的能力，且预检证明干净 `not_dispatched`、
尚无焦点准备或输入副作用，才可为整段脚本借用一次前台。剩余动作都受该前台所有权约束，
但支持的指针动作仍可使用定向事件；前台所有权与真实光标路线不是同一概念。
`background_only` 不借用，所需动作返回 `foreground_required`。

以下为尚未借用时的能力分类：

| 操作 / 已知条件 | 首选实现 | 自动借用条件 |
| --- | --- | --- |
| 有效语义目标 | AX action | 不为未知 AX 状态或菜单准备焦点；语义失败不自动重放成鼠标点击 |
| 证明是标准标题栏且位置可写 | AXPosition | 不因 AX 不可读或位置设置结果不确定而改路重放 |
| 坐标左/右键、单/双击 | 精确 PID + 窗口事件 | 定向 ABI 明确不可用，或已证明为标准 Open 面板的左键单击；均需干净未派发 |
| 内容拖拽，包括 AppKit 控件、Chrome 文本拖选 | 精确 PID + 窗口事件，逐点验证 | 同上；Chromium 标签本身不是拖拽借用理由 |
| 锚点或默认中心滚动 | 精确 PID + 窗口 wheel | 同上；不要求 AX children / AX window inventory |
| 已识别 AppKit / Chromium 的原生悬停 | 预检识别跟踪限制，选择物理光标 | 干净未派发时可申请脚本级前台借用；未知框架不套用该例外 |
| Qt / 未识别框架的悬停 | 定向事件 | 未知不等于不支持；不以未知框架、AX 缺失或普通错误授权借用 |
| 键盘 | 定向路径与精确编辑目的地证明 | 已证明没有可保留的后台 key window（AX noValue）时可借用； 已证明的 native modal Command 组合键、关联进程接收者、必要定向 API 缺失，或 Chromium Command 组合键/补充平面 Unicode、AppKit 控制字符文本及未启用/未绑定的菜单等价项，可申请前台借用；不是移动光标的理由 |

视觉输入的 AX 豁免仅适用于坐标指针操作。前台键盘回退仍逐次验证精确 AX 窗口及编辑
目的地，应用 PID 仍在前台不能替代该检查；同一应用内焦点离开目标窗口时停止后续按键。

模态状态本身不要求借用光标。真实 `NSApplication.runModal` 中的定向点击、拖拽已有
独立效果证据，因此普通模态指针预检不依赖 AXModal 或语义树。框架分类读取实际 bundle 的
frameworks / AppKit 元数据，不按 Telegram、fixture bundle ID 或应用名称开白名单。

指针借用原因进入内部审计：`native_hover_tracking`（已验收的原生悬停）、
`addressed_event_api_unavailable`、`native_open_panel_click`（只限标准 Open 面板左键单击）。

标准 Open 面板以原生 AXWindow/AXSheet 的 `AXIdentifier=open-panel` 分类；不依赖标题、
语言、应用名或 AXModal。其 NSRemoteView 不等于普通 `runModal` 控件；定向事件到达宿主
不代表远程按钮收到有效点击。该已知形状在任何点击派发前选择物理路径，不能等无效果后再点一次。
普通原生窗口的后台点击和文字录入不要求目标无遮挡；物理回退仍须即时证明可见命中点。
缺权限、进程变化、几何过期、遮挡、取消、超时、连接失败和任意 provider 错误均不是借用依据。
resident 建立视觉 scope 时必须使用调用方的 `visual` 分类。视觉输入及标准 Open 面板
使用不注入 AX-style make-key records 的 WindowServer 前台序列；随后仍以 AXRaise、
AppKit activation 和精确聚焦窗口回读证明目标。后台语义 key-window 准备记录不能当作
物理输入的前台激活；先前忽略 `visual` 会破坏原生远程面板的事件处理。

物理光标与应用前台分别准入：只有上述已知能力与干净预检可发起脚本级前台借用，
普通鼠标错误不授权激活。未知 AX/modal 错误保留原因，不能当作已证明的 modal 能力。
无窗口快捷键先证明完整窗口清单为空，再以唯一应用实例及出生标识建立 scope；
第一扇实际聚焦窗口到来时一次性绑定其根对象，之后继续执行原有窗口家族约束。
新窗口异步出现时，下一条输入只读等待最多 2 秒，沿用请求总 deadline，不重发创建窗口的按键。
选择器不会为 `content_unproven` 或菜单自动聚焦；只读语义启用继续不激活应用。
`fallback_allowed:true` 单独不能改路：还必须是精确的已知能力证据、`not_dispatched`、
预检能力错误，且没有 cleanup error。若依据为 `native_keyboard_semantics`，还必须携带已识别的 `capability_detail`：
`chromium_command_chord`、`native_modal_command_chord`、`native_menu_disabled`、
`native_menu_window_unbound`、`native_menu_unavailable`、`chromium_supplementary_unicode`、
`appkit_multiline_text`。这些能力类别只限制 `native_keyboard_semantics`，不是全部前台借用条件。
泛化的 `native_keyboard_semantics` 单独不够，实际框架不能从“不是 Chromium”推断为 AppKit。
内部 `foreground_plan` 还必须匹配当前操作及 visual 分类，且无 error、cleanup error、
focus preparation 或 interruption。已准备、已投递或 `unknown` 的输入不换路重放。

## 脚本级前台所有权与归还

`app.focus()` 是显式入口，不接受 PID、窗口坐标或策略参数。它聚焦当前已授权的窗口，
或经完整清单证明无窗口的唯一实例；后续同一 exec 内输入共享借用，结束时归还。
它不提供跨 exec 永久抢焦点的模式，`background_only` 仍在派发前拒绝。

resident worker 在首次借用时保留原前台进程出生标识与精确 AX 窗口对象/窗口 ID，
并为有窗口目标建立精确逻辑窗口锚点；完整清单证明无窗口时才允许尚未绑定的锚点。
因此坐标输入虽可不依赖 AX，持久前台借用仍需目标实例和原前台证明；
AX 不可读不等于无窗口，也不会退化成按 PID 猜测窗口。脚本内可以跟随由保留对象 ancestry
证明的 sheet/popup 后代，不能把同 PID 的兄弟窗口当作原任务窗口。

后续每次输入均复核前台所有权和精确目的地。外部切换到其他 app 或兄弟窗口会使 scope
永久失效；即使焦点后来回来，也不重新武装、不再次激活，后续 payload 停止。
脚本结束释放已持有输入，仅在所有权及原身份/窗口仍有效时恢复原 app 实例及精确原窗口；
若原前台确无窗口，则只恢复原 app。外部切焦点或原身份变化时不强制恢复。
正常脚本结算的焦点归还有独立、最多 2 秒的清理预算；恢复失败是 sticky cleanup failure，
关闭后续派发准入，不能当作恢复成功。

窗口关闭和前台 app 离开分别记录：旧根窗口销毁永远不授权新输入。清理阶段可在
原 app 实例持续前台、没有离开再返回、原根已从 WindowServer 消失时，恢复原前台。
这仅适用于完整清单证明目标已无窗口，或已准入的标准 Open 面板点击产生了先前不存在的
新窗口；已有兄弟窗口、未知身份或实际外部切焦点都不适用。新窗口的输入需要重新观察并建立有效 scope。

只有同 app 的 `quit_app` 得到 confirmed `application_exited`，或 `close_window`
得到 confirmed `window_closed` 且关闭的是锚点窗口，才是终态。保存提示、后代 sheet
关闭、按键关闭效果或未知退出结果不自行建立这个终态。确认终态后的清理仅观察原前台是否
已自然返回；不再激活任何 app，也不会把第三 app 拉到前台。观察不明不能声称恢复成功。

语义 `AXPress` 的 action reply 预算为剩余请求 deadline 与 2 秒的较小值，区别于属性读取
的短超时。进入派发后 AX 错误或超时均保留 `unknown`，不转换成成功、不改成鼠标重投。
正向 AX acknowledgement 只证明派发被接受，不证明界面效果；deadline/cancellation
的不确定性仍由执行层保留。

本节描述当前源码合同。新的脚本级借用、失效及归还证据与剩余限制见
[脚本级前台验收](computer-use-script-scope-2026-11-04.md)；历史鼠标验收不能代替该生命周期验收。

## 关闭窗口后的按键释放

CoreGraphics 为 Command、Shift、Option、Control 的按下和松开都生成 `FlagsChanged`，
不能仅凭 `KeyUp` 枚举判断释放。定向输入按 keycode 与对应 modifier flag 判定方向，
为按下保留实际 `FlagsChanged` 释放事件；释放只检查原应用出生标识，不再要求已被操作
关闭的窗口仍能读取 AXParent。修饰键松开不授权新输入，也不能切换投递路线。
这修复了 TextEdit 取消面板实际生效后仍返回 `unknown` 的问题，后台能力得以保留。

前台归还遇到新面板短暂的 typed AX `cannotComplete` 时，仅在原有清理预算内重读完整
所有权证明；不重发激活或输入，不忽略权限、进程替换和外部焦点变化。

原生 worker 的 stdin 读取放在有界队列的独立线程；主线程在空闲时和接纳请求前处理
Cocoa 回调。否则上一执行的归还通知可能堵在 stdin 后，直到下一次激活才送达，错误地
作废新借用。这里的运行循环调度不是采样通过门槛，也不忽略真正的外部 leave/return。

## 观察与功能结果

验收优先验证真实 UI 后置条件和确定的工具结果。采样间隔只保留实际统计，不以 50 ms
或其他固定空档阈值判定功能失败，也不要求 Agent 分析这些开发诊断。

观察器检查实际发生的异常：未获准的真实光标位移、借用结束未归还、预期之外的前台应用
及错误的焦点归还。焦点读取使用即时 WindowServer PSN/PID/PSN 证明；NSWorkspace
通知只作历史辅助，不能把延迟通知中的旧应用当作回调时的前台应用。
缺少可用观察数据仍明确标为不可验证，不伪造成功；实际间隔、边界不确定性和原始记录保留供定位。

报告分别输出功能效果、观察可用性和鼠标体验。发生过合法的物理借用就不能声称全程后台独立；
`borrowed_and_restored` 表示真实光标临时移动且归还，`fully_independent` 只用于未观察到真实位移
且未发生指针借用的调用。一次合法借用不放行后续动作的位移，回到原位置也不抹去中间移动。

## 一次全独立验收与最终例外

下述 2026-11-03 固定候选仅保留鼠标路线的历史证据；
[统一验收基准](computer-use-benchmark-2026-11-04.md) 也对应其当时的候选，
均不能外推为当前脚本级前台生命周期的 GUI 验收。

先撤掉全部框架鼠标例外，冻结同一个候选，将点击、悬停、内容拖拽、滑块和滚动全部交给
独立输入，再完成整套验收。以真实回调、控件值、DOM trusted 事件/选区为效果依据，
没有把“工具返回成功”当作成功，也没有在失败后切到物理路径掩盖结果。

| 全独立候选 | 检查 | 通过 | 失败 | MCP 调用 / 检测到系统光标位移 |
| --- | --- | --- | --- | --- |
| 四种 AX 故障（count/range、range、children 不可读、windows 不可读） | 43 | 39 | 4，全部为悬停 | 84 / 0 |
| 原生窗口场景、弹层、滑块、多显示器、取消与遮挡中断 | 44 | 42 | 2，全部为悬停 | 81 / 0 |
| 真实 Chrome，临时独占 profile | 3 | 2 | 1，悬停 | 6 / 0 |
| 合计 | 90 | 83 | 7，属于同一操作形状 | 171 / 0 |

通过的项目包括左右/双击、AppKit 滑块、Chrome 文本拖选、跨显示器内容拖拽、模态窗口
输入、遮挡/取消后的配对释放、原生标题栏移动。七个失败不是七种能力限制，均为原生
悬停缺少目标回调或 DOM hover 效果。因此只对这个已确认形状加入预检例外。

悬停借用复用 CUA `41404b6e797d9c44894ae910859db60303801b4a` 的
[`move_cursor_desktop` / `post_pointer_moved`](https://github.com/trycua/cua/blob/41404b6e797d9c44894ae910859db60303801b4a/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs)：
Warp 后按其既有配方，间隔 15 ms，用新的 HID-state source 投递两次 mouseMoved。
两次投递是同一预选动作的固定序列，每次都重新验证目标及取消状态，不是失败后重放。
借用过程沿用现有协调、实例和输入 admission，不建立第二套执行器或 Session。

上游 [action support](https://github.com/trycua/cua/blob/41404b6e797d9c44894ae910859db60303801b4a/libs/cua-driver/docs/action-support.md)
的已验收边界继续作为复用依据，不重新枚举每一种原语。其 AppKit 系统拖放会话、部分
WebView/Electron 拖放另有边界；普通文本拖选通过不等于任意 drag-and-drop 均已支持。

最终候选只改变悬停分支，针对受影响行为复验：

| 最终候选复验 | 结果 | 内部路由 |
| --- | --- | --- |
| 四类 AX 故障中的悬停、故障证明和正常退出 | 12/12 | 4 次悬停借用，全部有真实回调 |
| Chrome 悬停、点击、文本拖选 | 3/3 | 仅悬停借用；点击和拖选仍独立 |
| 真实光标审计 | 18 次 MCP 调用，0 个违规 | 5 次借用均为 `native_hover_tracking`；检测到的 2 次位移均在借用区间 |

该历史验收证明的是**当时通用原生事件路径的悬停缺口**，不宣称 macOS 在技术上无法实现独立
悬停。未知框架不因 AX 缺失或一次普通错误进入借用；后续可直接复用上游新增的已验收路径。

上述历史候选在借用悬停后不立即归还光标，以避免撤掉 hover UI；这不代表当前脚本结束会留下目标 app 前台。AppKit 在已建立跟踪的窗口中偶尔接受
定向悬停，但“真实指针处于窗口几何范围内”不能证明跟踪就绪，新建窗口会吞事件，
因此不把该位置推断用作路由规则。

证据根目录：`/private/tmp/dtw-ax-visual-evidence-20261103/`。全独立候选为
`candidate-independent-all/dtw`，对应 `independent-ax-all`、`independent-scene-all`、
`independent-chrome-all`；最终候选为 `candidate-routing-final/dtw`，复验在
`final-hover`、`final-chrome`。每组保留原始 MCP、私有 route、独立应用状态及完整采样。
更早的失败实验保留原目录，不覆盖、不算作最终候选验收。

## 路由与停止边界

```mermaid
flowchart TD
    A[现有 app 方法] --> B{身份 / 权限 / 截图几何有效}
    B -->|否| X[停止并保留原回执]
    B -->|是| C{可证明的语义操作或原生窗口位置}
    C -->|是| N[原生 action / position]
    C -->|否| D{明确的预检能力边界}
    D -->|已知不支持且干净未派发| P[建立一次脚本级前台所有权]
    D -->|没有| I[定向 PID + 窗口事件]
    I --> G{逐点身份 / 几何 / 同应用表面仍有效}
    G -->|是| E[完成并回读]
    G -->|否| R[只释放已持有输入；不换路重放]
    P --> E
    N --> E
    E --> S{已借用且结算时所有权与原身份有效}
    S -->|是且非确认终态| F[恢复原 app 与精确原窗口]
    S -->|未借用或已外部切焦点或确认终态| O[不激活；终态只观察原前台]
```

定向输入忽略无关应用的物理覆盖，但仍验证目标应用自身的前后层级、alpha、窗口位置和
实际服务进程出生标识；授权身份不因 helper PID 而改变。拖拽每个新点重新验证；取消或
遮挡中断的 up 使用最后一个已获准落点，不能跳到被拒绝的新点。配对释放只属于原来的
owner 和传输路径，即使原窗口已关闭也不能把释放发送给重用 PID 的新进程。

锁顺序仍是协调锁 → 应用实例锁 → Scene 锁。选择借用必须先释放共享协调/实例锁，再取
排他协调锁并重新验证；不会在持有 Scene 锁时升级成物理输入。

## 验证方式

`DTW_INPUT_TRACE=1` 仅向开发诊断 stderr 写入原生 route / reason / worker PID / window，不更改
Agent 工具 schema。`cursor_audit.py` 在每个原始 MCP request ID 的
完整区间采样真实系统光标，并逐条核对路由；终点归位不算独立，未知借用原因直接失败。
取消但无回复的请求保持无回执状态，清理区间截至下一次成功读回。

真实光标审计策略为 `observed-pointer-borrow-v5`。私有 `dtw_input_interval` 以 worker PID
与 trace ID 配对指针借用的 begin/end，覆盖准备、派发和收尾。允许原因仅为
`native_hover_tracking` 的 move、`native_open_panel_click` 的 click，或已证明定向 ABI
不可用的指针动作。未知、缺失或不配对的路由记录不能通过借用策略。

`cursor_audit.py --assert-policy` 检查真实位移是否得到分类和区间许可，并强制在请求完成时
归还原光标位置（允许 1 point 的系统坐标舍入）。跨借用边界的采样变化保留为时间归属不明，
明确在区间外发生的变化仍失败；间隔变长不会抹去已观察到的异常，也不会单独导致失败。
`--assert-independent` 则要求没有任何已观察到的真实位移，包括合法的临时借用。

报告另列 MCP 调用以外的位移，避免漏掉准备、迟到清理或调用间发生的移动；这些记录保留
actor undetermined，不仅凭位置变化就归因于 DTW。连续观察仍可能漏掉短于采样间隔的变化，
因此报告说明观察范围，不宣称数学上的连续不变。原始 MCP、路由、应用状态、观察数据及失败尝试均保留。

统一 [Computer Use APP](../tests/fixtures/computer-use-app/README.md) 的标准入口强制执行该策略，
并单独捕获经过身份验证的虚拟光标 helper 窗口，核对实际热点坐标和渲染像素。

自建 AppKit、真实 Chrome 的临时 profile、源码/纯控制测试分别记账。这些不是模型 Harness
A/B、真实账号验收或所有应用完备性证明。历史记录中的 Windows 源码/测试类型检查
也不是 Windows GUI 验收，不能用 macOS 结果代替。本轮 macOS 定向验收的候选身份、
业务效果、连续焦点、严格光标及清理结果分别记账；未运行 Windows GUI 验收。
