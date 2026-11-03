# Windows DTW 功能覆盖验证与优化项（2026-11-03）

> 这是 **0.3.3 发布包修复前的历史快照**，问题、源码行号与失败结果均对应下列基线身份。当前分支的修复与验证结果见 [Windows 修复验收](windows-fixes-acceptance-2026-11-03.md)。本文保留原始事实，不表示当前源码仍有同样的能力缺口。

当前连接和安装的 DTW **可用，且为核验时最新稳定版 0.3.3**。单实例 WinForms 的观察、截图、点击、Unicode 编辑、快捷键保存及普通模态框流程通过；Windows 尚未达到 macOS 功能范围。除已知的 running-only `apps` 外，本轮确认了多实例观察状态丢失、跨实例窗口顺序错误、拒绝菜单操作前抢前台三项缺陷，并复现了真实 Chromium 编辑框的 emoji 输入兼容性问题。

本文逐项覆盖当前 `apps` / `exec` 与公开九方法；“已调用”“派发成功”“独立效果通过”分别记账。浏览器系统对话框、横向滚动业务效果和部分系统配置没有形成通过证据，详见覆盖表。macOS 对照来自同版本源码、公开 API 和现有验收材料，本机没有重新执行 macOS 测试。

**版本、环境与证据身份**

| 项目 | 核验结果 |
| --- | --- |
| 实际使用的安装包 | `%USERPROFILE%\.caelis\builtin-plugins\desktop-world\e705a7f1efde6f891584bcef0d025ebeb8dee858148d8c0c1e94e436a5c73b4c` |
| CLI | `dtw 0.3.3` |
| 官方 latest | [v0.3.3](https://github.com/caelis-labs/desktop-world/releases/tag/v0.3.3)，2026-11-03 发布；核验了 GitHub `/releases/latest` |
| 仓库 HEAD / 安装 BUILD.source.commit / release target | 均为 `f07fb78e5013b16744a6694e39e5b2f2479a554f` |
| 安装二进制 SHA-256 | `32d5585f5bdfa130603cf3bcd32f0aae3b54ddb5a8c6e9070ac70883a7985f83`，与 BUILD.binary.sha256 一致 |
| 安装 BUILD.json SHA-256 | `f6bcc79940fd06a7da0dbefe29609e5c6b5cf6fcada97dfe267f30d038eedc1b`，与 GitHub release asset digest 一致 |
| 安装 SKILL.md SHA-256 | `a38efb044dd5f2a281b9071b05a022a789d38d484215f04afee264148461d52d`；API/PATTERNS 哈希另见证据摘要 |
| 安装目录名 | 与 GitHub Windows ZIP asset SHA-256 一致；本轮没有重新下载整个 ZIP，因此目录名本身不作为本地 ZIP 字节验证 |
| OS / 桌面 | Windows 11，`10.0.22631`；交互式桌面，主工作区 2560 × 1380，120 DPI / 125% |
| 实际应用 | 独立 WinForms fixture；Chrome `156.0.8078.74`；Edge `156.0.4314.58` 的独立临时 profile |
| 当前连接可用性 | 实际 MCP `apps` 成功；实际 MCP `exec` 返回 `connected-mcp-ok`、九方法及受限 JS 全局信息 |
| 工作区 | 起始干净；本轮增加验证探针、报告及证据，未修改生产实现、安装包或 Agent 配置 |

[证据摘要 summary.json](evidence/windows-audit-2026-11-03/summary.json) 保留精确身份、用例结果、独立效果、错误码及原始证据路径；[单窗口 PNG](evidence/windows-audit-2026-11-03/single-window.png) 和 [模态框合成 PNG](evidence/windows-audit-2026-11-03/opaque-modal-stack.png) 已人工目视检查。

**macOS 对照与 Windows 覆盖矩阵**

macOS 基线以 [API.md](../skills/desktop-world/API.md)、[product-matrix.md](product-matrix.md)、[macOS backend](../cmd/dtw/src/backends/macos/mod.rs) 为准。历史 atomic 的 `find/setValue/readText/focus/launch/closeWindow` 等不是当前公开九方法，不能把旧探针通过记到当前接口。本轮使用的官方当前入口是 `app_protocol_probe.py`；Windows README 原有“Historical atomic-protocol diagnostics”明确区分了旧接口。

| 能力 / 合同 | macOS 范围 | Windows 本轮结果 |
| --- | --- | --- |
| `apps` 默认 / running / stopped | 安装及运行应用 | 已实测：运行应用可查；`stopped` 为空，coverage 明确 `installed:false / complete:false / running_applications` |
| `apps` 查询、大小写、offset/limit | 支持有界查询 | 已实测：大小写查询、单页边界、offset 空页正常；未把运行中清单当成安装全量清单 |
| 应用名称与过滤 | bundle 用户名称；排除部分 helper | Windows 以 exe basename 命名；清单实际包含 DTW、输入宿主等后台/辅助窗口程序，见 G-06 |
| `exec` app 必填、工具结构 | `apps` + `exec` | 官方当前探针通过；当前连接也执行成功，无额外公开 session/result 方法 |
| `look` 自动启动停止应用 | 支持安装目标启动 | 已实测停止 fixture 后 `look` 为 `app_unavailable`；Windows 不支持该生命周期 |
| `look` 语义和 coverage | 顶层 surface、有界语义 | WinForms 语义完整；真实 Chromium 可观察，复杂页面会明确 `output_budget / has_more`；部分 UIA/provider 异常未做故障注入 |
| `look({full:true})`、返回去重、增量 | 完整 JS snapshot，输出 baseline/delta | 官方入口通过；重复 unchanged 输出 207 bytes；未把自动 readback 当成效果证明 |
| 同 app 多进程聚合 | 聚合及精确实例归属 | `apps.instances=2`、窗口聚合通过；跨 exec 状态与全局顺序失败，见 W-01/W-02 |
| `capture` 单窗口 | 应用像素 | PNG、尺寸及坐标映射通过，目视可读 |
| `capture` 同进程多 surface | 按真实层级合成 | 普通 WinForms 模态框与被其遮挡的父窗通过；当前 exact-HWND 路线可读取被遮挡的不透明窗口 |
| `capture` 跨实例层级 | 应用窗口的真实顺序 | 观察到实际前台 A、返回首窗 B；重叠像素画反是代码推导风险，本轮未做重叠像素断言 |
| layered/透明 surface | 支持可证明的 alpha 合成 | 代码明确拒绝 `WS_EX_LAYERED / WS_EX_TRANSPARENT`；未构造该类窗口实测，见 G-05 |
| 语义 click：selector / ID | 唯一、可 press 的目标 | WinForms selector 和最新 ID 均通过，计数器独立增加一次；Chrome 页面语义 increment 也有独立 HTTP 成功证据 |
| 坐标 click：左 / 右 / 双击 | 当前图像上的接收 surface | 全部通过；独立 MouseUp / DoubleClick 计数证明效果 |
| 输入绑定过期 | 无派发拒绝并更新观察 | fixture 移动 12px 后旧坐标被 `failure.code=stale_scene` 拒绝，返回 `coordinate_reset` |
| 无目标、混合 click 参数、图外坐标 | 前置拒绝 | 已实测拒绝；未发现误增计数或重放 |
| `move` / hover | 已实现 | 已实际调用；`unsupported_capability`、`not_dispatched` |
| `drag` 内容 / 标题栏 | macOS 单窗单屏内已实现 | 已实际调用合法内容路径；`unsupported_capability`、`not_dispatched`；不能完成选择拖动和窗口拖动 |
| `typeText` 中文、emoji、多行 | Unicode 键盘输入 | WinForms 精确回读通过，包括 UTF-8 保存内容；Chrome/Edge emoji 路径失败，不能外推所有 app 支持，见 W-04 |
| `keypress` 数组 / 字符串 / 对象、CTRL/CONTROL | 原生 chord | Ctrl+A、CONTROL+A、Ctrl+S、Escape 均有独立应用/文件效果；本轮未穷举所有键名或按 Win 键启动系统 UI |
| `scroll` 纵向、显式 anchor | 有界滚轮步数 | fixture pad 三步准确收到 -360；无派发重试 |
| `scroll` 默认中心 | 捕获当前 surface 后取中心 | 同样准确收到 -360；默认中心不保证正好是网页里的可滚区域 |
| `scroll` 横向 | 支持 delta_x | 已调用并报告 dispatched；WinForms 现有 fixture 不记录 WM_MOUSEHWHEEL，浏览器验证受阻，**业务效果未验证** |
| `scroll` 非法范围 | [-100,100] 整数 | 101 前置拒绝；具体码为 `failure.code=invalid_request` |
| 普通同进程 modal | 路由、层级、键盘/关闭后重读 | 语义打开、合成捕获、语义关闭和 Escape 关闭通过；保存内容、计数保持正确 |
| menu scope selector | macOS 支持 | Windows 明确 unsupported；拒绝前却先抬升窗口，见 W-03 |
| 系统 Open/Save / 跨进程 helper | macOS 有正向归属证明的面板 | Windows provider 边界较窄；真实浏览器 Open/Save 本轮没有成功完成，不能记为通过 |
| `quit` 正常退出 / 保存提示 | macOS 已实现，保留保存提示 | 已实际调用；`unsupported_capability`；Windows 正常 quit/save-prompt 合同未实现 |
| 多步脚本及自动读回 | 内部续绑定，动作不自动重试 | 单实例编辑保存、打开对话框后键盘关闭等流程通过；多实例不稳定，见 W-01 |
| JS 隔离 | 无文件/进程/网络/timer API | `process/require/fetch/setTimeout` 为 undefined；本轮未做安全沙箱攻击测试 |
| 输出预算 | print / JSON / 语义有界 | unchanged 和语义预算标志已验证；print 200 行、64 KiB result 极限未重新压测 |
| 同连接并发 busy | 单执行槽 | 活跃只读执行期间第二个请求返回 `busy` |
| MCP cancellation、原 ID 重传、恢复 | 停止对应调用、保留原记录 | 取消后恢复到 `after-cancel`；原 ID 重传不重放动作；取消诊断为 `backend_failed / Stopped...`，见 U-03 |
| EOF 与资源收尾 | 正常关闭及释放 | 官方及 WinForms 各轮 MCP、所有 fixture 均正常退出；未重复旧协议的 in-flight EOF/父进程死亡测试 |
| Agent 光标 | 动作位置、会话颜色、闲置退出 | 未重新运行旧协议 cursor 探针；本轮截图不是物理屏幕光标/闲置/多连接颜色的验收 |
| 多屏 / 混合 DPI / 最小化 / 提权 / 锁屏 | 有平台边界 | 当前仅 125% 普通桌面；未覆盖多屏、跨屏、DPI-unaware、最小化恢复；锁屏/安全桌面等不是既有支持承诺 |

**确认的问题与修复验收**

**W-01 · P1 · 多实例仅枚举顺序变化，就清空上一次 look 的状态。**

同一 exe 启动 A/B 两个独立进程，分开执行 `await app.look()` 和 selector click。最终一轮 6 组中 3 组 click 报 `look_required`，且 `context_reset:false`。更早两个独立运行也出现相同现象。成功的 click 只改变一个实例计数；失败的 click 未改变计数。本轮比例是诊断样本，不作为稳定失败概率。

代码链：`backends/windows/native.rs:395,426` 每次新建 HashMap，再无序输出 `into_values()`；`app_protocol/server.rs:442,447,485` 每个 exec 重 resolve，实例去重但保序并 reconcile；`app_protocol/mod.rs:359,385` 用顺序敏感的 Vec 比较，把 `[A,B] -> [B,A]` 视作变化并清空 SceneState。`runtime/mod.rs:1309` 的 normalization 也只去重。`scene.rs:291` 因 active 为空而报错。`server.rs:479` 的 context_reset 只检测完全无交集，所以状态被清空时还可能没有重置提示。

本轮没有额外记录失败 admission 前后的内部 PID 列表，不能对某一次失败声称已经直接捕获到顺序反转；上述代码机制确定，且与多轮公开接口现象吻合。

修复：在统一的实例边界 canonicalize 或使用集合成员语义；把成员变化与展示顺序分开。验收：resolver 交替 `[A,B]` / `[B,A]`，跨 exec 的公开 look→click 始终有效；成员不变时不清观察，真实成员替换时正确重置。不要依靠 Agent 每步补 look 来掩盖。

**W-02 · P1 · 跨实例丢失全局 z-order，可能选错观察/隐式输入对象。**

同一执行内 capture 后对 A 的 pad 发右键，独立 state 证明 A foreground=true、B=false；随后同脚本 full look 的窗口数组仍为 `[B,A]`。这是独立于 W-01 的顺序错误。

代码链：`windows/native.rs:434-454` 枚举全桌面后先按 PID 过滤，用 `result.len()` 填 rank，导致每个进程的第一个窗口都 rank=0；`runtime/mod.rs:365-385` 直接 extend；`scene.rs:1162` 按 rank 排序不能恢复全局顺序。`scene.rs:792` 取首窗作 active，`scene.rs:1317` 反序合成，故错误还可能影响语义目标、默认输入选择及重叠窗口像素。macOS native 使用全局窗口源索引，不能据其验收推断 Windows 正确。

修复：保留同一全局枚举中的窗口次序，在聚合后仍可比较；窗口 inventory 与物理前台证据一致。验收：两个实例交换前台、部分重叠，look 顺序、捕获重叠区像素和键盘接收实例分别有独立断言。

**W-03 · P2 · 不支持的菜单操作先抢前台，effects 却只显示 not_dispatched。**

A 在后台、独立不同 exe 的 blocker 在前台时，调用 `app.click({selector:{scope:'menu',text:'ABSENT-OWNED-CONTROL'}})`。结果为 `unsupported_capability`，A 的独立 foreground 从 false 变成 true；响应 effects 只计一个 `not_dispatched`。

代码链：`scene.rs:406-410` 在真正的 press 准入前先执行 focus；Windows `mod.rs:50-54` 没有 Menu scope，后续才拒绝。这个 focus 的 receipt 没有合并进最终动作摘要。已有 atomic `selection_tests.rs:1088` 的“unsupported menu 不应产生 focus action”约束没有覆盖 scene 包装层。

修复：先完成 scope/capability 准入，再考虑 focus；保留实际 preparatory effect。验收：unsupported menu 保持原前台窗口不变；任何已产生的 focus 必须在回执中可见。

**W-04 · P2 · 真实 Chromium 的 emoji 文本输入兼容性没有达到 WinForms 的结果。**

Chrome 在独立本地页面空 textarea 输入 `Chrome 中文 😀\nowned page`，派发前拒绝：`backend_failed / Focused native text changed before Unicode input admission`；页面 HTTP state 的 text 仍为空。Edge 的 ASCII 输入有独立 HTTP 成功证据；Ctrl+A 后输入相同 Unicode payload 时 typeText 为 `unsupported_capability`，页面仍为 ASCII。同版本 WinForms 的中文/emoji/CRLF 和保存文件精确匹配，因此不能用 fixture 通过推断浏览器通过。

代码落点：`windows/text_delivery.rs:69-125` 在 supplementary-plane 文本发送前证明可读 selection、所属元素及精确分区；`:127-144` 处理空 Chromium U+FFFC 占位；`windows/mod.rs:33` 也明确此限制。已确定失败发生在这条准入路径，**尚未采集对应 UIA selection/range 原值，不能断言是哪一个判据错误**。Chrome 同时受多实例问题干扰，Edge 后续 surface 选择也发生变化，均保留原始失败，没有重试输入。

修复：针对当前 Chromium 记录有界的失败分支、range 数量/端点、所属控件与 HRESULT，建立空字段、非空选择、尾部 caret、多行、已有 surrogate、字面 U+FFFC 的当前协议复现；修复真实兼容性原因并保留归属/精确文本证明。验收同时要求 UIA 回读、页面 input 事件和独立保存结果。不能放宽成不验证就发送或 clipboard/setValue 兜底。

**平台缺口与产品优化项**

| ID / 优先级 | 已确认范围 | 优化方向与验收 |
| --- | --- | --- |
| G-01 / P1 | 已知 running-only apps；停止后的绝对 exe 路径 look 也 app_unavailable | 安装来源枚举、Start Menu/AppsFolder/package identity、稳定身份和可证明的启动；用未运行的 Win32 与打包 app 分别验收，不做任意命令执行 |
| G-02 / P1 | 公开 `quit` 实测不支持；backend 也无 Launch/Quit/CloseWindow | 增加正常生命周期，覆盖多实例、未保存文档、保存提示和用户取消；不得 force kill 或丢文档。CloseWindow/Launch 是内部能力，不应误列为新的公开方法 |
| G-03 / P2 | `move/drag` 实测不支持 | Windows 接收 surface、路径、DPI 与配对释放证明后补实现；覆盖内容选择/拖动、标题栏、取消及遮挡中断 |
| G-04 / P2 | Menu scope 无实现；普通 WinForms modal 已通过 | 补原生菜单及系统对话框归属支持；区分同进程 modal、同窗口网页 dialog、跨进程 helper。不要把所有 modal 笼统列为失败 |
| G-05 / P2 | `native.rs:911` 明确拒绝 layered/transparent；不是当前不透明 covered capture 的限制 | 为可证明 alpha 的 surface 补渲染或明确 capability/单 surface 错误；加入自绘/Chromium overlay 验证。本轮仅代码确认，未构造透明 fixture |
| G-06 / P2 | app 名为 exe basename，清单含后台/辅助窗口程序 | 提供友好显示名与 package 信息、可解释的应用/辅助进程过滤；让本地化应用名称可搜索，保留 coverage 信息 |
| U-01 / P2 | 公共对象列出全部九方法，Windows 调用部分才报 unsupported | 在既有返回中提供简明 provider/能力范围，或提供明确随包 Windows 矩阵；使 Agent 在动作前知道可用能力，不必试错 |
| U-02 / P2 | 动作失败仍 `status:completed`；外层 `script_exception`，业务 code 在 failure.code；参数码还出现 invalid_args/invalid_request/invalid_arguments | 保留执行完成和业务失败的区别，统一稳定的业务原因与恢复提示；API 示例说明如何判断错误，覆盖 stale/unsupported/invalid 参数 |
| U-03 / P2 | busy/取消/恢复通过，但原 ID 重传两轮均为 `backend_failed: Stopped (request may have been sent: true)`，无结构化 cancelled 原因 | 保留 cancellation/底层诊断和 effect uncertainty 的各自信息；补 resolve 阶段与脚本阶段独立取消测试。本轮 busy 只证明执行槽已占用，未证明取消发生在 while 循环开始之后 |
| U-04 / P2 | Windows `mod.rs:23,27` 仍描述 unobstructed/occlusion foreground preparation，与当前 exact-HWND covered capture 及 API 文档不一致；产品矩阵也仍写实机待验收 | 按当前实现更新 provider 元数据和能力文档，同时保留具体条件；当前探针作为独立证据引用，不能倒改历史验收结论 |
| U-05 / P3 | 目视 PNG 的模态框下/侧边存在实黑矩形边缘 | 检查非客户区/不可见 resize border、PrintWindow 背景及 alpha/crop；先与物理屏幕或独立 DWM frame 对照。当前只登记视觉候选，不宣称已证明根因 |
| T-01 / P1 | 历史 Windows 真实 app/光标/生命周期探针大多调用旧 atomic/result；CI 发布不要求 GUI 验收 | 迁移有独立效果证明的现有场景至当前 two-tool；特别补实例重排、z-order、菜单无副作用、Chromium Unicode、横向 wheel 和取消阶段。编译/发布成功不替代桌面验收 |

静态附录候选：`native.rs:1128` 和 `:1240` 的 element-anchor 滚轮序列每轮要求原 rect 不变，可能在首步内容滚动后中断。但当前公开 `app.scroll` 只有像素/默认中心入口，不能把这个旧/内部 element-anchor 合同候选当成本轮公开 API 的已确认 bug。

**执行结果、排除项与复现入口**

- 官方 `app_protocol_probe.py`：5 组全部通过，MCP 和 fixture 正常退出；精确二进制/Skill 身份已记录。
- 自定义 `feature_audit.py`：最终一轮收集 36 个用例；包含预期 unsupported、非法参数和明确失败，不是“36 项功能全部通过”。另保留了 33/35 项的前两轮原始结果。
- 首轮 stale 检查错误地读取了外层 error.code，修正为 failure.code 后通过；首轮取消等待原响应超时，结合 SDK 可抑制取消响应的合同，改用同原 ID 保留记录及恢复检查后通过。这两项探针问题未计作 DTW 功能缺陷。
- 当前 state-publication 控制测试 Host 3/3 通过。沙箱中的 FILE_SHARE_DELETE 用例未能打开状态文件，导致 2/3；同一命令 Host 全通过，归类为运行边界，不登记为产品回归。
- Chrome：8 个诊断用例；截图/语义 increment 有成功证据，Unicode 拒绝。后续多项被错误实例选择拦住；默认中心滚动曾报告 dispatched 但无页面效果，首轮页面布局未证明该中心在 scroll pane 内，因此**不登记为滚轮失效**。
- Edge：9 个诊断用例；单窗口观察/截图与 ASCII 输入通过，Unicode 拒绝；后续 owned-window guard 拦住变化后的 surface，不能把被拦项目记为实现失败或通过。
- 浏览器两轮均无法经所选 owned 窗口完成正常关闭，探针对自己创建的精确 Popen 句柄作强制收尾，记录为 `forced_browser_cleanup:true / exit=1`；因此两轮都不是完整通过的浏览器验收。原有浏览器窗口没有被作为关闭目标。
- GUI 探针顺序执行；输入不自动重放。WinForms 所有测试进程、blocker 与 MCP 均正常退出。对比身份未发现安装包在验证中变化。
- 本轮未重跑全仓 Rust 测试；生产代码未改。新增 Python 探针均通过语法编译，真实运行结果与脚本局限均记录在案。

复现时从仓库根目录，在普通解锁桌面上使用**全新**输出目录，逐个执行：

```powershell
python -X utf8 tests/acceptance/windows/app_protocol_probe.py `
  --dtw '<installed-package>\bin\dtw.exe' `
  --installed-package '<installed-package>' --output '<new-baseline-output>'

python -X utf8 tests/acceptance/windows/feature_audit.py `
  --dtw '<installed-package>\bin\dtw.exe' `
  --installed-package '<installed-package>' --output '<new-features-output>'

python -X utf8 tests/acceptance/windows/browser_feature_audit.py `
  --dtw '<installed-package>\bin\dtw.exe' `
  --chrome '<Chrome-or-Edge-executable>' --output '<new-browser-output>'

python -m unittest discover -s tests -p 'test_windows_app_protocol_probe.py' -v
```

原始记录保留在本机 `%LOCALAPPDATA%\Temp\dtw-win-audit-20261103-*`：`baseline`、`features`、`features-controls`、`multi-controls`、`chrome`、`edge`。摘要使用环境变量形式记录这些报告的本机路径，原始目录还包含逐帧 MCP 请求/响应、PNG、应用状态和事件。仓库证据仅包含摘要与自有 WinForms 截图。

建议先修 W-01/W-02 的实例和场景合同，再修 W-03 的副作用前置检查；随后处理 W-04 与平台生命周期。修复前不宜把 Windows 定义为“macOS 全功能等价”，也不能因单实例 fixture 通过而宣布真实应用全场景通过。
