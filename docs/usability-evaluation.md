# 真实任务可用性与发行验证

基线：`6185e89`。本轮区分“宿主能接入”与“陌生 Agent 能有效完成任务”。fixture 用于证明单项契约，不能代替本评估。

## 实验约束

每个受试 Agent 不继承实现上下文。SDK 轮只收到冻结发行包、公开文档和任务说明；helper 轮只收到二进制、通用使用指南和同类任务。禁止实现者提供针对任务的定位、快捷键序列或异常恢复提示。桌面输入串行，评估者在受试轮次中不操作桌面。

每轮固定 25 分钟，包含首次接入。预算外的结果另算，不把事后补做算进盲测成功。宿主预先启动 Finder、TextEdit 和一个新的 Chrome 资料页；预置任务目录，不预先做文件改名、资料整理、内容修订或保存。该环境准备不计入 Agent 接入时间，并单独披露。

真实交付任务：

1. **资料交付**：用 Finder 将项目真实 SPEC、实现说明、验收记录移入新建的评审资料目录；重命名使用说明，保留 API。独立 oracle 校对文件名、目录与原始 SHA-256，不能以 UI 的移动动画判定成功。
2. **交接文档修订**：用 TextEdit 打开真实基线交接草稿，纠正 Windows 验收状态，保存独立 UTF-8 文本；保留原草稿。oracle 读取最终文件核对内容与原稿未变。
3. **平台依据核查**：通过 Chrome 的原生可访问性阅读 Microsoft 官方 SendInput/UIA 文档，在 TextEdit 保存结论与精确 URL。评估者核对结论和来源；不以“网页打开了”作为完成。

受试者只可用 Desktop World 观察/操纵桌面。shell 可以构建宿主和写测量日志，不能直接读写任务资料、调用文件管理 API、HTTP 抓取网页、AppleScript、Playwright 或其他 Computer Use 来完成任务。屏幕捕获也须来自 Desktop World。验证者可独立读取落盘文件；这不是受试者的任务捷径。

## 指标与归因

| 维度 | 度量 |
| --- | --- |
| 正确性 | 独立验证通过的任务数/断言数；保留原文件；结论与来源正确；错误“已完成”数 |
| 冷启动 | 首次有效观察、首次真实动作的 wall time；文档读取、构建次数、接入代码量 |
| 效率 | 每任务时间、API 调用数、act 批次与步骤数、观察访问节点、请求/响应 bytes、重复扫描 |
| 易学性 | invalid_argument/错误参数次数；是否必须翻实现源码；任务专用代码；人工提示次数 |
| 恢复 | 焦点/新窗口/过期 Ref/未知结果后能否停止、重观察、查询原收据；是否重复副作用 |
| 分发 | 只提供发行包是否能跑；是否需要 Go/CGO；权限宿主身份；会话保持与进程退出行为 |

原始审计、构建源、Agent 报告和 verifier 结果各自保留。接口耗时包含序列化/传输；不是纯 native 延迟。API 调用次数不是模型推理轮数。模型 tokens/轮数若无法可靠读取则记为“不可得”。不从一轮任务计算总体成功率、P95 或 token 节约百分比。

失败分成：入口/文档、参数表达、观察覆盖、目标/焦点、原生兼容性、Agent 决策、环境/权限、验证缺口。先保留失败证据，再改实现，并以新的独立 Agent 重测；同一个 Agent 看过修复后成功属于热启动回归。

## 发行选择的判定

Go SDK 保留为唯一执行内核和嵌入入口。helper 是薄宿主：若它显著降低接入代码、首次有效操作耗时与参数错误，而原生能力和恢复语义不退化，则优先作为通用 Agent 发行入口。原生 Go 应用仍可直接嵌入 SDK。两者不应变成互斥产品或复制两套实现。

这一选择仍需用真实结果支持：helper 要验证固定进程内 Ref/去重、取消、退出、权限身份与输出预算；SDK 要验证宿主线程/生命周期集成。预编译 helper 的签名、公证、更新、完整 Windows 实机验收属于发行门槛，不能由本地构建通过替代。

本轮是发现阻碍的探索性样本。正式“好用”门槛应扩展到两平台、多个独立 Agent、多次重复、原生/Electron/浏览器/长流程/故障恢复任务；阈值预先登记，再统计成功率与耗时分布。当前不宣称任何总体成功率。

## 实测结果

### SDK 基线轮（2026-09-29）

结论：能完成任务，但当前上手与恢复成本不合格，不能称为顺滑。独立 Agent 不继承实现上下文；使用冻结 `6185e89`，没有读取内核、历史报告或 oracle。实现者并行编译过产品，CPU 负载未受控，时间不是严谨的性能基准。

| 指标 | 实测 |
| --- | --- |
| 首次可用桌面观察 | 2 分 54 秒（14:22:22.445 UTC） |
| 首次真实动作 | 3 分 39 秒 |
| 首个文件变更 | 4 分 50 秒 |
| 完成最终 UI 自检 | 23 分 21 秒；25 分钟预算内 |
| 接入代码/构建 | 61 个物理行 Go adapter；6 次构建尝试；3 次进程启动 |
| 操作 | 75 observe、48 act、6 read；20 条含错误的调用 |
| 错误构成 | invalid_argument 10、needs_user_focus 4、user_interrupted 2、request_failed 2、target_not_hittable 1、permission_denied 1 |
| 全部 API 调用耗时之和 | 26.44 秒；剩余 wall time 不能全归因于推理，但原生执行不是主要占比 |
| world.* 流量 | 请求 45,400 B；返回 747,316 B |
| 模型 token / 推理轮数 | 无可靠计量；用户报告经历一次上下文压缩，之后出现 3 次参数错误 |

独立文件校验 13/13 断言通过：整理文件 SHA-256 不变、原稿保留、新文档内容逐字正确且是 UTF-8。官方资料核查已落盘，SendInput/UIPI 结论与引用符合来源；UIA 使用“按订阅可选择性发布”的概览依据，结论基本正确，但没有找到更直接说明代理 provider 会遗漏属性变化的 [Subscribing to UI Automation Events](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-eventsforclients)。不把“文件存在”当成研究质量满分。

原始证据保留在本地忽略目录 `artifacts/usability/sdk-baseline-01/`：harness/report.md、harness/calls.jsonl、harness/main.go、verification.json、analysis.json。其原始日志可能包含桌面内容，不放进公开发行包。

### 上下文开销来源

`cmd/dw-analyze` 对上述真实日志做可重复分析：

```sh
go run ./cmd/dw-analyze -input artifacts/usability/sdk-baseline-01/harness/calls.jsonl
```

观察返回 707,581 B，占 world.* 返回 94.7%；动作收据 33,430 B，长文本读取 6,305 B。1,620 个返回对象中，sample_start/end 字段约 138,980 B；Ref 61,069 B，版本/几何版本 56,700 B，name/value_preview 两组约 166,972 B（包括包装）。这些字段统计不含 JSON 分隔符，不能当模型 token；177 个对象在忽略采样时间后完全重复。

最大几次观察接近 60 KB，来自 Chrome 深树与 TextEdit 对话框重复查找。修饰键猜测、焦点关系失败、scope 不可判断导致额外往返；PTY 两次截断长行，不到达 SDK；最后上下文恢复又遗失了参数约定。因此仅压缩最终文档或少显示几条 receipt 不会解决主要成本。

新 compact 呈现对**同一日志离线回放**将 747,316 B 降到 496,230 B，减少 33.6%。未删 UI 对象、值、能力、错误、分页或 unknown，只压缩 Fact 包装、省略逐对象/Fact 的精确采样时间。该数据不证明新 Agent 更快，也不是实际模型 token 降幅。

### 修复与开发者热回归

- 完整参数 schema 和含合法值的错误提示，明确 modifiers/key/fields/budget，降低猜测；summary 默认少字段，读取期限适配真实桌面；协议保留 deadline/cancel 原因。
- 持久 helper 负责 epoch/request-ID、明确作用域、取消与审计；默认紧凑呈现，精确时间与 typed wire 可选；标准管道避免 PTY canonical 截断。
- 本机系统级 AX 焦点查询返回 AXCannotComplete，前台应用级查询正常。修为前台应用采样、前后 PID 校验；新焦点 Ref 同时注册关系，避免必须再扫整树才能通过 scope。
- TextEdit 对话框补充实时父链/焦点窗口证据；窗口/容器的鼠标命中接受真实后代，仍拒绝其他遮挡窗口。
- Finder 的无窗口内联编辑器用“明确目标 Ref + 实时焦点 + 前台 App + 已授权应用范围”验证键盘操作，不伪造窗口，不给窗口范围扩权。详见 [焦点模型修正](observation-and-seats.md)。

真实热回归：TextEdit 打开/路径面板与窗口子控件命中 **16 次调用通过**；Finder 新建、改名与 Enter **9 次调用通过**，独立检查新目录落盘。两者均没有 raw_input。日志位于 `artifacts/usability/warm-regression/`，包括失败的前置重现。开发者已知任务与故障，不能用该结果冒充第二位陌生 Agent 成绩。

### 下一轮外部 Agent

已获用户授权在本机 Antigravity App 发起新上下文验证，必须通过 Computer Use 操作 App，不使用 CLI/ACP。提供冻结 helper、JavaScript 调用链、通用指南、相同实际任务、prompt 和结果模板；验收 oracle 与脚本由评估者另行保管。已产生首轮失败证据，见下文。

预先登记的体验目标：首次有效观察 ≤30 秒、首次动作 ≤60 秒、全部任务 ≤10 分钟、无上下文压缩、无人工操作提示、无自行编写 Go adapter；必须同时保留正确性、错误恢复和输出体积。超过阈值仍记录真实结果，不降低门槛或将热回归替代。不同模型/宿主带来的差异单独披露，不将单样本差值全归因于 helper。

### 脚本入口开发者回归

MVP 聚焦调用返回成本与易用性，后台输入隔离延期。JavaScript 脚本在持久会话里组合观察、筛选、动作和条件等待，中间结果保存在 state；模型只接收 print、强制 coverage/分页/执行结果和计量。错误、partial/unknown 停止后续调用，不自动重放。

`artifacts/usability/script-warm-01` 留有前台激活失败证据：AppKit 激活请求加 AXRaise 返回已分派，但 TextEdit 没成为前台；链停止，没有继续发按键。补充应用 AXFrontmost 请求后，`script-warm-02` 的两段脚本完成 8 次调用：观察、聚焦既有验证文档、取得实时焦点、打开面板、确认并关闭。耗时之和 2,989 ms；helper 返回 27,042 B；完整脚本回复 3,209 B（含强制元数据），print 内容 1,020 B。任务未修改文档。这是已知目标的热回归，不是陌生 Agent 的完整任务成绩，也不是实测模型 token 降幅。

8 项 JavaScript 契约测试覆盖组合/跨回合状态、局部输出、未知/partial 收据、禁止错误后继续输入、唯一性与覆盖、调用与打印预算、超时、只读条件等待、断连后的后续输入阻止。Go race/vet、Windows 交叉构建/vet、协议示例通过；Windows JavaScript IPC 和桌面输入仍未实机验收。


### Antigravity App 首轮（2026-09-30 本地时间）

通过原生 Computer Use 创建项目并发送任务，模型界面为 Gemini 3.8 Flash High。冻结版本 d2039f9；没有使用 CLI/ACP 执行评测。实际发送时间 2026-09-29 16:04:07 UTC，Agent 记录起点 16:04:29 UTC。约 4 分钟后评估者提前终止：helper 自身截图已显示 Finder 为前台，但连续观察仍报告启动时的 ChatGPT；无法可靠聚焦和键入。没有完成三项任务，文件断言 2/13 仅证明保留项未变化，不能视为完成成绩。

24 段脚本、27 次原生调用，执行耗时之和 11.608 秒；helper 回复 168,507 B，整个脚本回复 31,886 B。11 段脚本发生错误：打印超限 3、Fact 类型误用 2、能力不支持 1、焦点验证超时 3、需要用户焦点 1、目标不可命中 1。实际模型 token 未取得。准备阶段的 clipboard/Dock 故障发生于发送任务之前，单独记录，不混入 Agent 任务耗时。

修复：实时前台/GUI 进程与生命周期查询不依赖主 RunLoop；`dw.list` 对已保存观察按行和字节分页，保留 next_offset，避免打印超限后重新观察；`dw.value` 接受普通字段且仍拒绝未知事实；help 明确标准角色与窗口焦点，prompt 改为通用文档按需读取。10 项 JavaScript 契约测试通过。真实持续 helper 在 Finder 回归窗口与 TextEdit 测试文档间连续 4 次切换，9 次调用的焦点验证均通过。原始证据在 `artifacts/usability/antigravity-app-01/` 与 `native-focus-live-03.jsonl`，首轮保持原冻结工具与失败结果。


### Antigravity App 第二轮（冻结 09d471e）

仍通过 App 的新项目/新对话运行，模型相同；首次有效观察 79.462 秒、首次完成动作 84.346 秒（评估者发送起点）。Agent 自报起点晚约 5 秒，报告数值因此略低。61 段脚本、75 次 helper 请求（包含收据查询和同 ID 去重恢复，不能理解为 75 个原生输入）；14 段脚本报错。helper 响应 263,094 B，print 66,825 B，**完整脚本响应 100,175 B**。后者才是包含强制覆盖/收据元数据的模型工具输出；仍不包含宿主包装、提示词、推理，也不是实际 token。API 耗时约 13.6 秒，其他壁钟时间不能全部归为模型推理。

约 9 分钟后评估者终止：TextEdit 的“替换” AXPress 在约 294ms 返回 native_timeout，未知结果正确阻断了后续输入，但只读查询与尝试恢复又消耗数分钟。任务 0/3、独立文件断言 2/13，仅保留项通过。Agent 自报无压缩、零接入代码、零构建、零重启；实际 token 不可得。追加一次仅写报告的 App 消息发生在 helper 停止之后，没有提供任务操作提示。

环境问题也计入失败分析：初始 TextEdit 残留基线同名文档，Agent 使用了旧目录的已完成文档，未确认实际任务路径。外部报告把它列为 partial，但 evaluator 不认可为本轮完成；旧基线文档正文仍保留。下一轮须清理已经确认保存的旧测试窗口。正式桌面会有重名文件，故同时新增按需 `uri` 字段：macOS 从原生 AXDocument/AXURL 读取，不按标题猜路径；Windows 当前报告 unsupported。

本轮后修复：`dw.next` 保留原查询进行原生分页；outline 默认减少 states/capabilities，find 深度增至 12；显式根节点放在第一页，其余保留原生遍历顺序；聚焦助手拒绝请求范围外焦点；status/doctor 与 version 消除入口猜测，help 给出 scope/键盘/分页/终止 fence 的完整最小约定。错误提示明确终止未知状态不能靠重新观察恢复；不自动解除 fence 或重放。

AX 写动作不再套用 250ms 读超时，独立设为 1 秒（仍在默认 2 秒 step 限额内）。隔离 TextEdit 回归以两个新文件执行另存为和真实“替换”，该调用耗时 **314ms**，收据 completed、seat ready，独立读文件确认目标与源逐字相同。开发者回归不能替代下一轮冷启动成绩。

证据：`artifacts/usability/antigravity-app-02/{participant/harness,evaluator}`，`artifacts/usability/save-replace-regression/verification.json`。运行 `python3 scripts/analyze-script-eval.py <run>` 可重算每层字节、调用与时序，不输出 UI 内容或代码。
