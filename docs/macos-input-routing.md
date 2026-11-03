# macOS 输入路由与真实光标

Agent 仍只调用 `apps`、`exec` 和原有 `app` 方法。输入路由是实现细节，脚本不选择
虚拟/物理光标、不维护 Session，也不根据内部投递方式安排重试。正常脚本回执不暴露
`delivery`；私有执行账本保留原始投递证据及派发不确定性。

本实现接续[逐操作历史审查](macos-ax-cursor-audit-2026-11-03.md)。历史失败记录保留，
其“当前产品”描述只对应当时的二进制，不代表本轮修改后的行为。

## 分类规则

先证明授权应用、进程出生标识、具体窗口及截图几何，再选择一次路由：

| 操作 / 已知条件 | 首选实现 | 自动借用条件 |
| --- | --- | --- |
| 有效语义目标 | AX action | 语义失败不自动重放成鼠标点击；返回截图 |
| 证明是标准标题栏且位置可写 | AXPosition | 不因 AX 不可读或位置设置结果不确定而改路重放 |
| 坐标左/右键、单/双击 | 精确 PID + 窗口事件 | 必要的定向事件 ABI 明确不可用，且尚无准备/输入副作用 |
| 内容拖拽，包括 AppKit 控件、Chrome 文本拖选 | 精确 PID + 窗口事件，逐点验证 | 同上；Chromium 标签本身不是拖拽借用理由 |
| 锚点或默认中心滚动 | 精确 PID + 窗口 wheel | 同上；不要求 AX children / AX window inventory |
| 已识别 AppKit / Chromium 的原生悬停 | 在派发前按已复现的原生跟踪限制选择借用 | 当前 PID 事件路径不能可靠建立悬停跟踪状态，证据见下文 |
| Qt / 未识别框架的悬停 | 定向事件 | 未知不等于不支持；不以未知框架、AX 缺失或普通错误授权借用 |
| 键盘 | 现有定向键盘路径和明确的键盘能力预检 | 需要激活的原生快捷键等沿用原有键盘路由；不是移动光标的理由 |

视觉输入的 AX 豁免仅适用于坐标指针操作。前台键盘回退仍逐次验证精确 AX 窗口及编辑
目的地，应用 PID 仍在前台不能替代该检查；同一应用内焦点离开目标窗口时停止后续按键。

模态状态本身不要求借用光标。真实 `NSApplication.runModal` 中的定向点击、拖拽已有
独立效果证据，因此指针预检不再依赖 AXModal 或语义树。框架分类读取实际 bundle 的
frameworks / AppKit 元数据，不按 Telegram、fixture bundle ID 或应用名称开白名单。

只有两种指针借用原因进入内部审计：`native_hover_tracking`（仅已验收的原生悬停）和
`addressed_event_api_unavailable`。
缺权限、进程变化、几何过期、遮挡、取消、超时、连接失败和任意 provider 错误均不是借用依据。
`fallback_allowed:true` 单独不能改路：还必须是精确的已知能力证据、`not_dispatched`、
预检能力错误，且没有 cleanup error。已准备、已投递或 `unknown` 的输入不换路重放。

## 一次全独立验收与最终例外

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

本次证明的是**当前通用原生事件路径的悬停缺口**，不宣称 macOS 在技术上无法实现独立
悬停。未知框架不因 AX 缺失或一次普通错误进入借用；后续可直接复用上游新增的已验收路径。

借用悬停后不立即归还光标，避免撤掉 hover UI。AppKit 在已建立跟踪的窗口中偶尔接受
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
    D -->|原生悬停限制或缺必要 ABI| P[在输入前选择借用]
    D -->|没有| I[定向 PID + 窗口事件]
    I --> G{逐点身份 / 几何 / 同应用表面仍有效}
    G -->|是| E[完成并回读]
    G -->|否| R[只释放已持有输入；不换路重放]
    P --> E
    N --> E
```

定向输入忽略无关应用的物理覆盖，但仍验证目标应用自身的前后层级、alpha、窗口位置和
实际服务进程出生标识；授权身份不因 helper PID 而改变。拖拽每个新点重新验证；取消或
遮挡中断的 up 使用最后一个已获准落点，不能跳到被拒绝的新点。配对释放只属于原来的
owner 和传输路径，即使原窗口已关闭也不能把释放发送给重用 PID 的新进程。

锁顺序仍是协调锁 → 应用实例锁 → Scene 锁。选择借用必须先释放共享协调/实例锁，再取
排他协调锁并重新验证；不会在持有 Scene 锁时升级成物理输入。

## 验证方式

`DTW_INPUT_TRACE=1` 仅向开发诊断 stderr 写入原生 route / reason / worker PID / window，不更改
Agent 工具 schema。`cursor_audit.py --assert-independent` 在每个原始 MCP request ID 的
完整区间采样真实系统光标，并逐条核对路由；终点归位不算独立，未知借用原因直接失败。
取消但无回复的请求保持无回执状态，清理区间截至下一次成功读回。

每条借用记录必须同时检查独立应用效果。采样默认 2 ms，报告实际最大间隔；比间隔更短的
位移不能排除，位置变化本身也不能证明由 DTW 导致。原始失败、MCP 帧、stderr、fixture
状态及采样数据保存在同一证据目录，避免重跑后覆盖。

自建 AppKit、真实 Chrome 的临时 profile、源码/纯控制测试分别记账。这些不是模型 Harness
A/B、真实账号验收或所有应用完备性证明。Windows 遵循项目既有 CI 流程；本轮仅完成
Windows 源码/测试类型检查，未做 Windows GUI 验收，不用 macOS 结果代替。
