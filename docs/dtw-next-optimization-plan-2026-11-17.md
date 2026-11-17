# DTW 下一阶段优化方案：实机操作、公开契约与 Skill

日期：2026-11-17。性质：调查与规划；本轮没有修改实现或现行 Skill，也没有发布、提交或切换当前 MCP。

## 判断

DTW 已有较完整的**应用内 Computer Use 基础操作**，覆盖语义定位、截图、全文读取、键鼠、拖拽、滚动、原生菜单、文件面板、多窗口等待和正常退出。当前最有价值的投入是把这些操作组合成可靠任务：先修复实测的文本接收字段错误与自绘弹层输入阻塞，再改善错误恢复提示和 Skill。跨窗口拖放、系统桌面入口和更广多屏支持应以单独任务合同推进。

公开操作数量不能代表任务完整性。当前连接只有两个 MCP 工具，但 `exec` 包含全部 14 个公开 app 操作；同时，“已派发”“语义遍历完整”“脚本已结算”都不能单独证明用户任务完成。

## 版本、环境与证据边界

| 项目 | 本轮证据 |
| --- | --- |
| 实机 | macOS 27.2 / 26B123，arm64，交互桌面 |
| 当前插件 | DTW 0.5.0；二进制 SHA-256 `39e2bb4aecb2652a1b0fd6e6ee850aff10031e8f9398decb5ce156159dd86074` |
| 工作区 | `ac90e69`，0.5.1；初始工作区干净 |
| 版本关系 | `git diff v0.5.0..HEAD -- cmd/dtw/src skills/dtw` 为空；安装与仓库的 SKILL/API/PATTERNS 三份文档逐字节一致。这不是本轮构建包验收 |
| 实际工具发现 | `apps`、`exec`；未切换为 atomic 模式 |
| 测试界面 | 当前仓库编译的 AppKit ComputerUse 工作台；其中 WKWebView 加载独立 loopback 库存 fixture；TextEdit 独立空白窗口 |
| 核验方式 | 模型通过 DTW 语义或截图决定操作；另用 fixture 日志、实际保存文件、全文比对核验效果。未用 DOM/CDP/浏览器驱动输入 |
| 原始记录 | 47 次 exec 留有本地记录；其中 23 次关键回执归档至 [receipts.json](evidence/dtw-next-optimization-2026-11-17/receipts.json)，索引与独立核验见 [summary.json](evidence/dtw-next-optimization-2026-11-17/summary.json) |

图像通过 MCP 原始图片附件实看，归档 JSON 保存图像几何与语义，未保存 PNG 字节。47 次不是预定义基准的通过率分母；其中包括诊断、预期拒绝、测试编写错误和清理。

未做 Windows/Linux 实机、atomic GUI 任务、WPS/Obsidian 真实业务任务、连续前台/物理光标采样、锁屏或跨屏验收。本轮不能据此声称全平台通过或后台零干扰。Chrome 自动启动未成功，浏览器交互证据仅来自自有 WKWebView。

测试 runner 在 sandbox 中直接启动 GUI 时退出 -6，随后使用 DTW 从编译出的 app 路径成功启动；这项准备失败不计为 DTW GUI 回归。一次多行 JS 字符串转义错误同属测试编写问题。首次 Save 使用了 fixture 禁止的文件名，也不能归为 DTW 保存缺陷。

## 实机结果

| 场景 | 结果与可核验事实 |
| --- | --- |
| 应用发现与启动 | `apps` 查询能找到停止的 Chrome/TextEdit；TextEdit 与自有工作台可启动。Chrome 返回 `launch_unconfirmed`，随后发现仍未运行，根因未确定 |
| 原生 Unicode 替换与提交 | `setValue` 后读回中文、英文和 emoji 正确；独立日志记录一次 `Commit note`，`saves=1` |
| 密集控件定位 | 默认返回 80 个元素并标记 `output_budget`；过滤找到第 137 个按钮，点击后 oracle 为 `audit_index=137 / audit_count=1` |
| 重复名称 | `Audit checkpoint` selector 明确返回 `selector_ambiguous / not_dispatched`；没有猜选一个控件 |
| 长文本 | `look` 给截断预览；一次 exec 内 6 页读出 20,971 UTF-16 单位，逐字与文件 oracle 一致，SHA-256 为 `e7caebb3160819e0ff9c837bbec2f0afa2047203c6efd5492b60f5a602f0ace2` |
| 原生菜单与 Open | 只读菜单发现 File/Open；`waitForNewWindow` 捕获 Open 和 Go to Folder 面板。Open selector 因遍历不完整拒绝后，截图坐标完成打开；读入内容与原生 reference 一致 |
| 路径字段的 setter/typing 差异 | Go to Folder 用 `setValue` 后即时值正确，但 Return 未关闭面板；改为点击字段、全选、`typeText`、Return 后完成路径选择。仅证明本次差异，尚未单独归因事件机制 |
| Save | 第一次自定义文件名被 fixture 拒绝，其合同只允许 `owned-note.txt`。按合同补测时 selector 因 `ownership_unproven` 拒绝，但已观察的 Save ID 可执行；实际文件内容为 `DTW saved 中文 😀`，`file_saves=1` |
| Peer 窗口 | 新建、等待、独立写入、计数与关闭成功；peer 为 `Peer only 中文 / counter=1`，主文档仍为原值 |
| Web 表单 | 搜索 DTW-0137、编辑数量和 checkbox 可完成；首次提交的备注为空。复验确认语义点击备注后 typing 写进相邻数量字段；截图坐标点击后备注正确。最后数量 37、Reviewed=true、备注正确；日志保留两次保存，标记恢复后完成，不能算 clean run |
| 自绘低 AX 消息页 | 可读图找到 message-137，右键成功打开 Chat actions；Pin 点击在跨 exec 与同 exec 两种情况下均报 `focus_unproven / not_dispatched`。oracle 为两次 right-click、零 Pin，任务阻塞 |
| 指针与局部滚动 | Hover、双击、拖拽、滑块和定点滚动有实际效果；`double_click=1`、`drag=2`、`up=3`，滑块从 25 到约 90.25；左侧滚动 40、右侧仍 0，最终 `pressed_mouse_buttons=0` |
| 等待与恢复 | 不存在目标触发预期 `wait_timeout`；后续新请求能执行计数动作并读回 `Counter: 1` |
| 目标失效 | TextEdit 旧编辑目标变化时返回 `target_changed / not_dispatched`；没有继续向旧目标输入 |
| 收尾 | 自有工作台正常 quit，回执确认 `application_exited`；本轮创建的 TextEdit 空白窗口已关闭；loopback 服务正常退出，退出码 0 |

## 一、Computer Use 能力：优先修复任务链路

### P0：编辑控件的语义 click 必须建立精确文本接收者

**已证实触发：** 库存编辑 dialog 中，Handoff note 的语义元素广告 `press/setValue`。执行 `click(note)` 后 `typeText`，note 的 `readText` 仍为空；截图与数量字段值读回显示文本进入 Target quantity。通过截图坐标点击 note 后，同样文本正确进入备注。关键回执为 `web-note-typed-readback`、`web-note-capture`、`web-note-pixel-recovery`。

**源码支持：** [Native.swift](../cmd/dtw/src/backends/macos/Native.swift) 的 `pressCapability`（4319 起）在可编辑控件分支之前优先选择 AXPress（4327）；6504 起将该路径设为 `focusText=false`，6564 执行 AXPress。6567 起只为 collection selection 验证后置状态。6002–6009 的 typing 检查同窗口及当前接收者归属，没有证明接收者等于刚点击的 note。因此同一授权窗口内的误投仍可能继续。

尚未通过原生 trace 确认的是 AXPress 的实际返回、是否曾短暂切焦，以及 provider 的 exact element 绑定细节；不能先把问题归因为 Unicode。

**最小方向：** 明确可编辑文本 click 的合同为“使精确目标成为文本 receiver”，补齐聚焦后置验证；`.action` 与 `.focus` 两条路径都要检查。保留 collection selection。可复用 6247–6291 现有 Web `setValue` 的精确 receiver 证明思路，不能用写入测试值来探测焦点。

聚焦动作已发送但未确认时，当前脚本应报错停止，保留真实 dispatch 状态；不能伪装为未派发，不能自动追加坐标点击或重放。先修 native click 合同；跨平台 `typeText({target,text})` 的一致 receiver 约束可作为随后增强，不需要以新增 API 为修复前置。

**验收：** 原生文本框、WKWebView textarea、嵌套表单；开始时让另一个字段持有焦点。目标内容正确且邻接字段不变；焦点不能建立时，首个文本 payload 前停止。覆盖用户切走、目标替换和不完整语义，不能削弱身份、归属及不重放约束。

### P1：可见但不成为 key 的 owned child，应有明确 pointer 接收合同

**已证实：** Chat actions 菜单可见、归属被识别、坐标命中来自已查看图像；Pin 的输入准备要求该窗口成为精确 focused window，最终失败。同一 exec 中打开并选择仍失败，不能归因于跨调用结算。

[Chat.swift](../tests/fixtures/computer-use-app/macos/Chat.swift) 114–118 创建 borderless child window，只做 `addChildWindow/orderFront`。Native.swift 4979 起的 parent resolution 用于 activation/raise，5181–5282 仍要求精确 child focus。这与现象相容，但具体失败阶段仍需 trace。

**下一步：** 先记录 child/parent/activation root、AX role/subrole、key 能力、foreground 与 AXFocusedWindow。若确认是可接收指针但不能成为 key 的正常 child，为 pointer 建立“精确 parent 已聚焦、child 归属/层序/像素命中均被证明”的专门分支。不能捕获所有 `focus_unproven` 后放行，也不能只凭同 PID、窗口标题或 borderless 推出许可；keyboard 的 receiver 证明不因此放宽。

**验收：** Pin/Edit/Reply 分别产生一次业务效果；跨 exec 和同 exec 都覆盖；增加同 app 无关 child、未知 overlay、用户切走的反例。

### P1：补齐 Chrome 启动失败诊断与真实浏览器任务

`chrome-start` 返回 `launch_unconfirmed`，其 effect 为 unknown，后续发现仍为 stopped。仅此不能判定 OS、安装、LaunchServices 或 DTW 哪一层有缺陷。下一轮先以固定候选、独立 profile 复现，记录已解析 bundle、launch 返回和进程证明失败阶段；随后执行相同搜索、表单、保存与重开任务。本轮 WKWebView 通过部分不能替代 Chrome/Edge。

### P2：按真实任务扩展边界

| 目标任务 | 当前边界 | 建议顺序与验收 |
| --- | --- | --- |
| 文件管理器拖文件到编辑器、跨窗口整理 | drag 路径限同窗口、同显示器；跨窗口拖放尚未支持 | 在当前焦点/归属问题修复后选择一个受控真实任务；定义源、目的 surface、modifier 与取消清理，再决定实现 |
| Dock、菜单栏 extras、无窗口应用入口 | 无公开操作合同 | 优先选能解除真实任务阻塞的单一系统入口，不以扩大任意桌面访问代替归属证明 |
| 多屏、不同缩放、窗口跨屏 | 需要按显示配置独立验证，部分输入前置拒绝 | 从支持包的常见布局增加独立验收；不把一次截图成功当作输入成功 |
| 拖拽/选择中的修饰键、复杂控件 | 现有 chord 与配对 gesture 不等于任意组合手势 | 先证明任务需求；若增加能力，优先每次 gesture 自包含的 modifier，保留自动释放，不暴露跨调用 held state |

Linux Alpha、无人值守和远程桌面不作为上述修复的前置。当前 app-scoped 产品范围应在文档中说清楚。

## 二、输入输出：核心形状保留，恢复决策应更直接

**应保留：** 两个工具支持批量编排；对象参数在 script/atomic 间同名；稳定目标 ID、完整性标记、分页与增量有效减少输出；dispatch 与业务效果分离；unknown 不重放。默认原生视图本轮约 9,642 UTF-8 字节，指定控件过滤后约 898 字节，自定义计数读回约 282 字节。这些是单次响应大小示例，不是统一任务基准。

| 问题 | 实机/静态依据 | 最小优化方向 |
| --- | --- | --- |
| 工具数量容易被理解成能力裁剪 | 本轮用户追问“只有 apps/exec，是否不是全量工具集” | 工具描述与 Skill 首屏明确：script=2 个 MCP 工具+14 个 app 操作；atomic=15 个工具且无 exec。单列调用边界差异 |
| 重复 selector 的恢复建议不够具体 | role/text 都已指定且同名，错误仍只建议缩小 role/text | 按原因给出下一步：观察具体 ID/子树上下文或截图；不建议添加 ordinal 猜选，不降低唯一性要求 |
| 不完整发现的恢复提示过于统一 | Save 为 `ownership_unproven`，仍提示 role/text；该次已观察 exact ID 可用 | 区分 output budget、同名、归属/读取缺口；仅在已发布对象可安全使用时建议 exact ID；其他情况建议 capture 或明确缺口 |
| `status:completed` 很容易被当成功 | `wait_timeout`、`focus_unproven` 的外层仍为 completed | 保留执行结算含义与真实 effects；补共享结果 schema/工具描述和一张决策表，明确 MCP isError/error 优先，observation 另判。不要让模型自己拼“完成=成功” |
| 方法返回与 MCP 外层不同 | input 方法返回 receipt；成功 custom return 抑制常规 view | 提供三种最小完整示例：普通动作看外层 view、脚本内读回后 return、readText return。不要建议 `return {done:true}` 充当验证 |
| complete 与“画面完整可理解”不是一回事 | 自绘菜单有 Pin/Edit/Reply，但 `elements:[] / complete:true` | 在能力描述和样例明确这是原生语义遍历完整；任务相关内容缺失时仍要 capture，不因 complete 而证明业务目标不存在 |
| 字段长度容易被当全文长度 | preview 的 `value_length=256`，readText 全文为 20,971 | 文档明确 observed field/prefix 与 document length 的区别；若调整 schema，标明长度适用范围，避免笼统改名或默认输出全文 |
| 无筛选 apps 的噪音 | running 列表包含大量系统 agent | 继续推荐 query；后续按真实选择困难评估 user-facing 分类/标签。不是本轮阻塞项 |

暂不新增 Session、持久焦点 lease、原始按下/释放工具或默认全桌面截图。现有内部状态由 DTW 管理的方向应保持。

## 三、Skill：信息已齐，首屏和常见失败路径需要重组

当前 [SKILL.md](../skills/dtw/SKILL.md) 的主要参数与风险信息基本准确，API/PATTERNS 分层也合理。改进重点是让第一次使用就能作出正确动作选择。

1. **修正一项确证的恢复措辞。** SKILL 90–91 写 native cleanup failure 会封锁整个 connection；API 78–81 允许后续新请求恢复。server.rs 508–571 有退役重建路径，1733 起测试覆盖 failed retirement 后新读取与新 press。应改为“当前执行与受影响原生上下文停止；保留原回执，后续新请求由 DTW 判断恢复；不重放旧输入”。PATTERNS 相关段落也需一起核对，保留 fencing。
2. **重写首屏的能力定位。** 先说明适用任务、app-scoped 边界、默认两工具包含 14 操作，再讲 atomic 配置。description 可候选为：`Operate desktop app interfaces through DTW: discover apps, inspect UI or screenshots, and edit text, use menus, file dialogs, mouse and keyboard. Default apps+exec provides all 14 app operations; atomic exposes them separately.` 平台细节留正文，避免入口描述过长。
3. **把输入选择写成短决策表。** 完整原生值替换→setValue；需要输入事件/搜索/路径补全→聚焦后 typeText；未知/低 AX 控件→capture。强调 setter 读回正确不证明应用处理了输入事件，也不证明保存完成。用本轮 Go to Folder 和 Web 表单作样例。
4. **把完整任务样例放在反复出现的低层警告之前。** 给出 apps→look→操作→业务读回的 script 完整调用；示例文本通过可靠 JSON 序列化进入 source。atomic 给出独立完整调用，并明确 Windows 原生菜单跨调用不共享焦点作用域，不能声称任务完全等价。
5. **补六类恢复分流。** invalid_arguments→改参数；target_changed→重新观察；selector_ambiguous→上下文/ID/图像；selector_unproven→检查具体 coverage；wait_timeout→只读核验；unknown/interrupted→先观察效果、禁止重放。permission_required 保留现有系统权限指引。
6. **把 success、完整性与输出选择讲清楚。** 用上节的三个最小返回示例，解释 completed、dispatch、acknowledgement、observation 各自回答什么；补“空且 complete 的 AX 树仍可能是自绘 UI”。

Skill 的正确验收不是只检查关键词存在。应让未接触过 DTW 的 Agent 只读发现结果和 Skill，完成字段编辑、原生文件面板、长文本、密集重复控件与低 AX 菜单五类任务，记录错误接收者、无效重试、调用数、输出量及完整效果。脚本与 atomic 分别记录，不能用静态工具同源替代任务验证。

## 建议执行顺序与退出条件

| 顺序 | 交付 | 退出条件 |
| --- | --- | --- |
| 1 / P0 | 精确文本 receiver 修复与定向回归 | WKWebView/native 正确字段输入；邻接字段零变化；未确认焦点停止后续 payload；原失败不被改写 |
| 2 / P1 | owned non-key child 定位、pointer 合同与修复 | Pin/Edit/Reply 各一次成功；已知不归属/遮挡/切走反例继续拒绝；keyboard 约束保持 |
| 3 / P1 | 错误恢复提示、结果说明、Skill 同步打磨 | 五类任务按新文档可独立完成；cleanup 恢复描述一致；不会把 completed、complete 或 dispatch 当业务成功 |
| 4 / P1 | 固定候选真实应用基线 | macOS Chrome + 原生编辑器任务闭环；Windows Chrome/Edge + 原生文件/菜单同合同；分开记录 script/atomic。最后再验证实际安装包 |
| 5 / P2 | 单项能力扩展 | 按一项真实任务定义支持/拒绝合同与独立效果后实施，不用全量功能列表替代验收 |

每轮报告同时保留业务结果、首次是否顺畅、原始错误、派发次数、实际效果次数和清理结果。恢复后完成与一次顺畅完成分别统计；本轮 Web 的两次保存与 Pin 的两次失败就是下一轮回归种子。
