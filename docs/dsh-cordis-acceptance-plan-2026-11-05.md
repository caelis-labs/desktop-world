# DSH 完整 Cordis 候选实机验收方案与功能矩阵

日期：2026-11-05。**这是待执行方案，不是 DSH 验收完成报告。**
本轮共 56 项：4 项宿主门禁、现有 42 项 Computer Use 基线、6 项本次修复回归、4 项真实应用任务。
P0 / P1 / P2 分别为 44 / 8 / 4 项；所有 execution/functional/agent/experience/evidence 状态均为 `not_run`。
已知不支持、平台或环境限制另列在 scope，不能把它们省略后宣称全能力通过。
完整可筛选矩阵：[CSV](evidence/dsh-cordis-acceptance-plan-2026-11-05/matrix.csv)、
[JSON](evidence/dsh-cordis-acceptance-plan-2026-11-05/matrix.json)。

## 现有证据能说明什么

目前不能代表完整的真实 Computer Use 能力矩阵。
[本次修复报告](dsh-v035-input-regression-2026-11-05.md)证明了特定终端候选的真实 TextEdit、
普通 AppKit 和外部焦点保护；自有夹具与源码测试还能定位底层错误。
它们没有证明完整插件经 DSH 加载后的进程身份、匹配 Skill、模型图片链路与自然语言任务能力，
也没有覆盖同一最终包上的全部控件、框架、生命周期和环境条件。
历史 42 项基线同样混有不同候选的通过、失败、blocked 和未执行记录，不能拼成一次完整通过。

这里区分：真实桌面上的固定脚本、真实应用、真实模型、真实 DSH 插件、最终不可变包五个条件。
缺哪个就保留哪个边界。即使本轮 eligible 项通过，也仅代表本机 macOS/DSH 与所测框架，
不外推到 Windows/Linux、任意 Qt/Electron/Office 应用、mixed-DPI、安全桌面或 Dock/menu-extra 根。

## 已准备的候选与当前安装

- 源码：`7767de9dd91c0af860aeb4611ba777f877dbcb23`，从干净提交导出后按原生候选构建流程构包。
- 完整本地包：`/private/tmp/dtw-dsh-host-acceptance-20261105/candidate/desktop-world-0.3.5-aarch64-apple-darwin.zip`。
- ZIP SHA-256：`9a85cf1719ded4ec60b5191dc9080b40b65dd97156ffbeb721449d73ca59e00a`。
- 二进制 SHA-256：`e796777ee049e8ae134afd3b0f458660fb16e1d30b45dbb8749ecf51a3aa40db`。
- 完整包 ID：`0d66381529ff4a2c76a0dd83e965a80b4e7b6d61b210d5786dc5d5657b0dc04d`。
- 新包含 executable、bundled Skill/API、Cordis、各宿主清单、BUILD、来源/许可证与 SHA256SUMS。
  包级 MCP/EOF 检查及独立临时 store 的逐文件 import 均通过。
- 它是 unpublished local candidate，未请求正式签名/公证，不能当作发行包验收。
  这次重建的二进制不同于旧 GUI 冻结候选 `ecc870c6…`，不能继承旧 GUI 通过结果。
- DSH 实际安装为 `/Applications/DeepSeek Harness.app`，版本 `0.2.1-rc.5`。
  当前仍指向稳定包 `5417a476…`，二进制 `5bad3937…`，pending 为空。
  新旧版本字符串同为 `0.3.5`，必须靠包 ID、源码、路径与 SHA 区分。
- **本轮未修改 DSH 注册或正式 store 的 pin，未发出 DSH 测试任务。**
  预演目录为 `/private/tmp/dtw-dsh-host-acceptance-20261105`，包含 `baseline.json`、
  `candidate/CANDIDATE.json`、`packaged-mcp/report.json`、`package-import-validation.json`、
  `registration.preview.diff`；后者只有目标插件 name 行的预览，尚未应用。

## 切换、执行与回退顺序

1. 核对上述冻结 ZIP、BUILD 和 SHA256SUMS，以及 Desktop profile 当前摘要；若出现并发配置变化，
   重新识别目标行，保留其他编辑。确认当前 DSH 任务已结算，不中止其他会话。
2. 用候选的原生 `package import` 导入默认 managed store；以实际返回值核对包 ID，
   `package bind dsh-desktop <新包ID>` 建立 pending。只为这个 profile 备份并修改
   `~/.dsh/profiles/desktop/cordis.patch.yml` 内现有 `desktop-world-plugin` 的 name，
   使用 bind 返回的 `cordis_url`。不追加第二个 MCP/Skill，不修改其他 provider、模型和 Skill roots。
3. 该宿主的外部 Cordis patch 不热加载。按[DSH 安装边界](install-dsh.md#dsh-desktop-one-file-url-row)，
   配置准备到此暂停，报告 prepared/waiting for restart，等用户保存工作并完整退出宿主。
   刷新插件列表、新会话或独立握手不能替代宿主重启；尚未重启不能预报 installed/verified。
4. 在这个同一次退出/重开的边界，为运行时路线审计，仅给本轮 DSH 进程设置 `DTW_INPUT_TRACE=1`。本机 `open --help` 已确认
   支持 `open --env DTW_INPUT_TRACE=1 -a "/Applications/DeepSeek Harness.app"`，
   只在原实例退出后启动，不用 `-n` 创建第二个宿主，不设置 launchctl 全局环境。
   先确认 DSH 的支持日志入口能保留原生 stderr；取不到路线/借用区间时，体验证据必须标不足，
   不靠动作名称反推“合法借用”。不通过私有 RPC、二进制扫描或杀共享 bridge 找热重载绕法。
5. 宿主确已重开后只做一次 live identity check，再进入 G01–G04：实际 DSH-owned MCP 路径与
   SHA、实际 Skill 路径/摘要、真实 tools 与图片。旧会话 schema 过期时才在已更新宿主中新建验收会话。
   门禁未通过就停止业务输入，后续项目标依赖阻塞，不把没执行的动作写成功能失败或通过。
6. 默认使用已配置的 DeepSeek-V4.1-Flash / High，逐组串行执行矩阵：先 T01–T05 和基础原生，
   再真实应用文件工作流，随后密集/低 AX/Web/浏览器，最后 T06 与取消等干扰测试。
   环境 actor 只制造受控焦点/遮挡/取消，不能完成业务。R02 只走独占传输诊断，不能关闭共享 DSH MCP。
7. G 门禁和 eligible P0 功能/目标所有权/清理均满足后，才对 `dsh-desktop` activate。
   体验与观察质量分别报告；功能通过不自动得到“全后台独立”评级。
   activate 的 GC 保留旧稳定包作为 rollback、其他 host 引用、活跃旧进程与保留期内包。
8. 若候选不合格，保留原始失败；需要回退时 bind 旧稳定包 `5417a476…`，仅还原对应 Cordis 行，
   按同样的正常重启和 live check 返回旧包，再 activate。不要用 unbind 删除整个 host，
   不盲目用整份备份覆盖并发设置。Caelis、agy 及其引用始终保持独立。

## 业务、模型使用成本、体验和证据四个判定

| 维度 | 判断依据 | 不算通过的情况 |
| --- | --- | --- |
| 功能 | 真实 UI/文件/独立业务日志后置条件 + 原始工具结果；次数、目标、文本、邻居与清理均核对 | 发了事件却无效果；模型说完成；正向任务依赖 unknown；错误被后续恢复抹掉 |
| Agent 易用性 | 自然语言目标后模型自行读匹配 Skill、发现对象和完成任务；记录步骤、调用、token/耗时、脚本长度/分支、人工帮助、非必要 sleep/重试 | 必须人工置前台、注入预知坐标/隐藏答案、手写复杂准备状态机；只有指导脚本才能跑通 |
| 体验 | 后台不动真实光标/焦点，或已证明必要的有界借用且归还；witness 零误输入 | 结束不归位、不归还；外切后抢回；未分类位移；把临时借用称为全程独立 |
| 证据质量 | DSH call/result/图片关联、原生路线、连续真实光标、WindowServer 前台、窗口/进程身份、独立 oracle | 缺记录、未知 actor、采样/边界不可证明；仅 endpoint 相同；用终端连接替代 DSH |

预期负向拒绝与意外工具错误分开计数。取消/EOF 可以证明配对释放和恢复合同，
同时原任务仍为 interrupted/unknown、`clean_run=false`；不能改写成原任务完成。
未知输入不重放；纠正方案或新候选必须是新 attempt，保留第一轮成功率和所有失败。
安全拒绝不是“平台永久不支持”的证明。

## 实际模型任务与测试编排

G、T、P 和专门负向项按受控步骤回归；普通 N/C/W/B/A 项优先用 goal-only 提示。
初始只给业务目标、准确的自有 app/目录授权和公共 Skill；不给现成 JS、元素 ID、像素坐标或答案。
业务只能调用 DSH 当前 DTW 的 `apps`/`exec`；不使用 shell、AppleScript、CUA、DOM/CDP、
fixture command/state 写入来代做业务。CUA 只控制 DSH 的提交/Stop 或独立只读观察，不能向业务目标派发输入。
模型不得读 fixture 源码、配置或私有 judge；独立 judge 在业务提交后才读取。

以短的新验收会话分组，不沿用旧安装演示中的错误默认策略结论。
普通操作由 DTW 自动选择路线、准备、等待和清理；显式 focus 如有需要，只用同一 exec 的明确入口。
`look()` 可能启动已停止应用，因此退出判定用 apps/独立进程观察，不能为“确认退出”再次 look。
长文本的公开分页循环是正常能力；任意 sleep、手工焦点准备和未知后的重试不是默认 recipe。
若 goal-only 失败而指导脚本通过，分别报告，不能把 Agent 易用性一并标绿。

第一模型完成后，再从已经配置且具备工具/图片能力的模型或推理等级中选一个较低能力条件，
复测 N01、A02、B03、B05、T01。只在自有测试会话切换，不改 provider/凭据或全局默认。
若没有第二条件，就明确标未测，不凭一个模型外推所有能力层级。

## 独立观察与尚需补齐的执行接线

- 业务 runner 必须接 DSH 宿主，不能把现有会自行启动 AppClient 的终端 runner 包一层就称 DSH 验收。
  可复用 fixture、只读 oracle、焦点/光标观察器和负向 actor。
- 已核实 DSH 会话含 `tool/call` 的 callId/name/arguments/time，以及 `tool/result` 的
  message.toolCallId/isError/content/time。image 部分携 attachmentId/mediaType/width/height/bytes。
  执行前补只读会话适配器，保持 host session/turn/toolCallId 与原始时间，不伪装成 MCP wire log。
  缺 result 就记录未知结束；不能把独立观测停止时间伪造为工具回复。
- PNG 用 DSH 支持的会话/附件导出路径取原图，保存 SHA。只有附件元数据不能替代原始像素；
  拿不到原图则证据为 partial。B05 必须是模型实际收到的图像和一次性盲答，不能用本代理的 ViewImage 代答。
- 现有 cursor audit 的默认输入是直接客户端 transcript，需要接真实 DSH call 边界后再用于 P02。
  记录 UI 控制宿主、准备与收尾的区间；不能把这些操作的光标移动算进 DTW 业务派发，
  也不能直接删除调用外移动。actor 无法确定时保留待归因，不默认指责 DTW 或用户。
- 焦点使用即时 WindowServer PSN/PID/PSN，历史激活通知保留辅助诊断；光标连续采样并检查归位。
  **50ms 不是功能通过条件。** 最大空档原样记录；真实未知 PID/缺失边界仍是观察证据不足。
  功能结果不会因此被改成失败，体验独立性也不会凭相同两个端点被判成功。
- B05 执行前确认挑战能每轮生成新 nonce/栅格和私有答案；若现有 fixture 固定，先补隔离挑战，
  不把公开固定答案的正确回答当模型识图证据。种子/答案不进入模型提示或可读 AX。
- 所有 GUI 串行，冻结包不覆盖；至少完整执行一次后保存首次矩阵，再独立复测缺陷，避免挑绿汇总。

## 环境及资源边界

只操作自有临时目录、fixture、浏览器 bundle/profile 和本轮创建的文稿/窗口。
Calculator 已有用户实例不能清零或退出；TextEdit 不退出已有用户进程；Finder 必须证明自有窗口。
Chrome 明确 copied bundle、fresh profile、PID/birth 和 argv，无法证明隔离则 blocked，不能碰用户会话。
缺 Chrome、缺 AX/录屏权限、没有 mixed-DPI/隔离桌面时保留实际原因，不修改用户显示或锁屏来凑覆盖。
数据 oracle 独立只读；新目录和进程记录来源、身份与清理，强制结束/清理失败不算干净通过。

下面所有项目均为本轮待测；scope 是当前支持/环境边界，不是实测结果。

## 完整功能矩阵

### 宿主链路门禁

| ID / 优先级 | 对象与自然语言任务 | 独立通过依据 | 执行边界 |
| --- | --- | --- | --- |
| G01 / P0 | DSH Cordis 完整插件：确认 DSH 加载预期本地候选，而非旧安装或终端副本。 | 实际 DSH-owned MCP 路径、二进制 SHA、包 ID、完整注册行与候选一致；临时握手进程不误判成重复注册。 | eligible；待测 |
| G02 / P0 | DSH Skill 与 facade：从实际加载的 bundled Skill 发现方法并确认 focus 入口。 | Skill/API 路径和摘要属于同一包；实际发现 apps/exec 与 11 个 app 方法；不以旧会话缓存为证据。 | eligible；待测 |
| G03 / P0 | DSH 工具与图片链路：枚举运行应用，观察并截图自有目标。 | 实际工具调用成功；原始 tool result 与 image attachment 可关联；模型上下文收到图像。识图正确性另由 B05 判断。 | eligible；待测 |
| G04 / P0 | 新 DSH 会话与模型：使用当前已配置的模型和匹配 Skill 开始独立验收会话。 | 记录宿主、模型 ID、推理级别、权限和 argv；确认默认 auto；不用旧会话关于 none/前台要求的结论。 | eligible；待测 |

### 既有 42 项基线

| ID / 优先级 | 对象与自然语言任务 | 独立通过依据 | 执行边界 |
| --- | --- | --- | --- |
| N01 / P0 | AppKit 工作台：首次调用直接让计数器增加一次，不预先 look。 | counter 精确 +1，无无关变化。 | eligible；待测 |
| N02 / P0 | AppKit 密集 AX：在密集控件中找到 Audit checkpoint 137 并操作一次。 | 只命中指定 checkpoint，邻居不变；不泄露 fixture 坐标。 | eligible；待测 |
| N03 / P0 | AppKit 原生编辑：替换并提交指定中英、emoji、换行文本。 | 原生字段与提交内容逐字节/UTF-16 一致，提交一次。 | eligible；待测 |
| N04 / P0 | 原生长文本：读完 Long reference，给出末尾标记及文本核对值。 | 同一观察分页到真实末尾，全文指纹一致；不可读/截断不称全文。 | eligible；待测 |
| N05 / P0 | 原生 checkbox：切换 Reviewed 一次。 | 状态反转一次，无邻居副作用。 | eligible；待测 |
| N06 / P0 | 原生 slider：把滑块拖至指定位置。 | 真实拖动回调和数值变化，鼠标释放；不能只写状态。 | eligible；待测 |
| N07 / P0 | 原生并列滚动区：滚动左侧指定区域，保持右侧不变。 | 左侧偏移变化，右侧及另一轴不变。 | eligible；待测 |
| N08 / P0 | 真实应用菜单：只读观察 File 菜单，报告实际项目。 | 菜单内容来自 DTW；业务、前台与真实光标不因读取改变。 | eligible；待测 |
| N09 / P0 | 同应用 peer 窗口：新建 peer，仅增加其计数器，然后关闭该窗口。 | 新窗口真实出现，peer +1、主窗口不变，只关闭自有 peer。 | eligible；待测 |
| N10 / P0 | 真实系统 Open 面板：打开文件选择面板并取消。 | 面板真实出现后消失，无误打开文件、unknown 或误报焦点。 | eligible；待测 |
| N11 / P0 | 真实系统 Save 面板：将提交内容保存到指定自有文件。 | 实际文件精确字节、保存日志一次；真实系统面板操作。 | eligible；待测 |
| N12 / P0 | 覆盖确认与取消：触发自有 seed 文件的覆盖确认，取消覆盖并取消面板。 | seed 摘要不变，保存计数零增量；效果与工具 unknown 独立记录。 | eligible；待测 |
| N13 / P1 | 原生 hover/双击：悬停事件区域并双击指定目标。 | 真实 hover/double-click 回调正确；必要物理借用结束归位。 | eligible；待测 |
| N14 / P0 | 几何/图层变化：在真实窗口移动后继续定位，验证过期几何不会误击。 | 命中归属无法证明则无派发；合法的新观察/同实例刷新可成功，不要求旧数值坐标一律拒绝。 | eligible；待测 |
| N15 / P0 | 未保存退出与重启：编辑后取消退出，确认内容保留；再正常退出并重新打开。 | 真实确认对话框、内容保留、新 PID/birth 和新 baseline；不关闭用户文档。 | eligible；待测 |
| C01 / P0 | 低 AX 原生画布：识别可见的 Release check 137 消息。 | 模型检查真实 DTW PNG 后回答；图像摘要和视觉复核记录匹配。 | eligible；待测 |
| C02 / P0 | 低 AX 独立弹出菜单：对指定消息右键并 Pin，再回到消息列表。 | 只 pin 目标消息一次；真实 popup 及邻居不变。 | eligible；待测 |
| C03 / P0 | 低 AX 回复：回复指定消息并发送一次 Unicode 内容。 | reply-to 精确、文本完整、发送恰好一次。 | eligible；待测 |
| C04 / P1 | 低 AX 编辑：只编辑指定历史消息。 | 目标正文精确变化，邻居和日志计数正确。 | eligible；待测 |
| C05 / P1 | 低 AX 历史滚动：滚到指定旧消息并操作。 | 滚动到真实历史目标；不能用附近可见消息替代。 | eligible；待测 |
| W01 / P0 | WKWebView：替换 Web note 并提交一次。 | 真实页面内容、Unicode 和一次提交计数正确。 | eligible；待测 |
| W02 / P1 | WKWebView 密集 AX：在大量控件中定位并点击指定 checkpoint。 | 只命中目标，遍历覆盖和歧义诊断可解释。 | eligible；待测 |
| W03 / P1 | Web 长文本：读完 Web reference 到末尾。 | 全文指纹和末尾标记正确；无重复/漏页。 | eligible；待测 |
| W04 / P0 | Web 原生表单：先提交非法表单，再填入有效内容与日期，保存并重开。 | 非法保存零次；有效值、日期、保存次数及重开数据精确；不写 DOM。 | eligible；待测 |
| W05 / P1 | Web 嵌套滚动：分别滚动目标纵向与横向区域。 | 内外层和轴隔离，邻区不动。 | eligible；待测 |
| W06 / P0 | Web Canvas：从画布菜单 pin 目标消息并发送一条回复。 | 实际 canvas 动作与文本正确；后台/借用路线单列，不人工先置前台。 | eligible；待测 |
| B01 / P0 | 隔离 Chrome：在独占实例中完成网页 Unicode 编辑及 checkpoint 操作。 | 明确 owned bundle/profile/PID；实际渲染、输入与提交正确；不碰用户 profile。 | eligible；待测 |
| B02 / P1 | 浏览器库存分页：找到不同页的两条记录，编辑、保存、重开。 | 两条目标精确变化，各保存一次，全邻居不变。 | eligible；待测 |
| B03 / P0 | 重复 label：在重复 Edit/Reviewed 中拒绝歧义，再定位精确目标。 | 预期歧义无派发；定位后只改指定行；预期负向错误不算模型失控。 | eligible；待测 |
| B04 / P0 | 浏览器真实输入事件：分别完成普通及 trusted-pointer 模式的提交。 | 真实 down/up/click 和 trusted 模式独立效果；不以 AXPress 的普通提交替代。 | eligible；待测 |
| B05 / P0 | 真实模型图片挑战：只从 DTW 返回的随机栅格图回答并提交一次。 | 无 AX/源码/配置答案泄露；实际 DSH 图片附件、模型回答和私有 judge 一致。 | eligible；待测 |
| R01 / P0 | DSH Stop / MCP cancellation：在自有 held gesture 中用 DSH Stop 中断，随后只读恢复。 | 实际 partial 路径、配对释放、无 suffix、读取可恢复；原未知回执保留，clean_run 单列。 | eligible；待测 |
| R02 / P1 | 独占传输诊断：在独占 MCP 测试连接 EOF 后验证 worker 回收和新连接恢复。 | 不关闭 DSH 共享桥或其他会话；仅证明传输边界，不算 DSH 模型通过。 | isolated_transport_only；待测 |
| R03 / P0 | 错误实例/外来遮挡：让同 executable sibling 或外来窗口尝试获取旧图点。 | 双方业务不变、明确拒绝；不通过公有 PID 猜测或用户窗口试探。 | eligible；待测 |
| R04 / P0 | 前台 witness：在另一自有应用前台期间操作后台目标。 | 真实背景点击/编辑生效，witness 不收业务按键；不得人工激活目标后声称后台成功。 | eligible；待测 |
| S01 / P2 | Dock 根：记录授权 Dock item 的真实 root 能力边界。 | 当前缺 root action 合同；不把任意桌面截图点当精确授权。 | unsupported_current；待测 |
| S02 / P2 | 无窗口 menu extra：记录 accessory/status-item 的 root/popup 能力边界。 | 不能把本次 windowless Cmd+N/focus 支持外推为 menu-extra 支持。 | unsupported_current；待测 |
| S03 / P2 | Windows chooser/tooltip：在 Windows 原生宿主选择文件并验证真实 tooltip/shadow。 | 本机 macOS 不执行，不从其他平台结果外推。 | blocked_platform；待测 |
| S04 / P2 | 显示/不可用桌面：分项记录单屏、多屏搬移、mixed-DPI 与 unavailable-desktop 条件。 | 执行前读取实际显示条件；无 mixed-DPI/隔离桌面则 blocked，不改用户显示或锁屏。 | environment_dependent；待测 |
| P01 / P0 | DTW 虚拟光标：核对 click/move/drag/scroll 的真实 overlay 位置和反馈。 | 独立实际窗口/像素证明；AXPress/键盘不伪造鼠标动画。 | eligible；待测 |
| P02 / P0 | 真实系统光标：连续审计全部 DSH 业务调用、借用区间与结束归位。 | 移动逐段记录；必要借用有真实路线证据且结束归位；调用外/未归因位移单列。 | eligible；待测 |
| P03 / P0 | 真实系统焦点：连续审计后台稳定、必要借用、归还及外部选择。 | 即时 WindowServer PID、真实按键目的地；外部选择不被重新抢回；空档只作诊断。 | eligible；待测 |

### 本次修复定向回归

| ID / 优先级 | 对象与自然语言任务 | 独立通过依据 | 执行边界 |
| --- | --- | --- | --- |
| T01 / P0 | 无窗口应用：在无窗口状态直接新建并输入，不要求用户手动激活。 | 新建一次、文本精确、自动等待在内部；无 foreground_required/重复 shortcut。 | eligible；待测 |
| T02 / P0 | TextEdit Open → New：从后台标准 Open 面板点击新建文稿。 | 真实新文稿替换面板、前台归还、必要光标借用归位；不是只证明事件到宿主。 | eligible；待测 |
| T03 / P0 | TextEdit 关闭：关闭本轮自有空文稿。 | 窗口确实关闭、没有访问旧 AX 焦点引起的假 unknown。 | eligible；待测 |
| T04 / P0 | TextEdit 取消：再次打开 Open 面板并按 Cmd+. 取消。 | 真实取消、修饰键配对释放、无 AXParent 假错误及残留按键。 | eligible；待测 |
| T05 / P0 | 明确 focus 入口：在同一 exec 中 focus 后输入/点击，覆盖窗口及无窗口情况。 | 入口简单、后续输入成功、结算归还；不把跨 exec 的 focus 当持久会话。 | eligible；待测 |
| T06 / P0 | 外部切走再返回：在本次借用中外切并返回，然后尝试后续输入。 | 当前执行不可重获；后续输入拒绝且不改业务；下一次新执行独立验证。 | eligible；待测 |

### 真实应用端到端任务

| ID / 优先级 | 对象与自然语言任务 | 独立通过依据 | 执行边界 |
| --- | --- | --- | --- |
| A01 / P0 | 真实 Calculator：清零，完成 1234 × 56，并显示结果后正常收尾。 | 实际计算器 UI 为 69104；不能只用模型心算报告结果。 | eligible；待测 |
| A02 / P0 | 真实 TextEdit 文件工作流：在本轮自有文稿输入 Unicode，保存到指定新文件、关闭并重开核对。 | 实际文件字节、重开文本、窗口与焦点正确；不保存/关闭用户原文稿。 | eligible；待测 |
| A03 / P0 | 真实 Finder：只在自有临时目录窗口定位文件、重命名并关闭该测试窗口。 | 实际自有文件名变化，内容及邻居不变；窗口身份未证明不得操作既有用户窗口。 | eligible；待测 |
| A04 / P0 | 真实 Chrome 多步工作流：在独占 Chrome/profile 中找到指定记录，修改表单，保存并重开。 | 实际页面、持久数据、邻居及真实输入事件一致；禁止 DOM/CDP/用户 profile。 | eligible；待测 |
