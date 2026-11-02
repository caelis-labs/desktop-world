# DTW 整体回归与发布验收准备：2026-11-02

本轮只做整体回归、证据核对和工作排序。没有继续增加能力，没有修改产品实现，也没有进行真实模型/Harness A/B。

## 结论

当前已具备继续做定向真实应用回归的基础，尚未具备冻结候选并进入安装、升级层面发布验收的条件。接下来优先收尾协议与验收入口，然后验证真实应用边界，根据失败补内部能力，最后准备固定源候选。继续增加手势和外部方法的优先级低于这些工作。

最直接的下一步是一个有限的“协议收尾”工作包：修正真实二进制打包冒烟的错误契约不一致，清理仍对外展示的旧 API/安装示例，恢复跨平台类型检查入口。完成后立即转入新协议的关键真实场景回归。

## 核对基线

- GitHub main：`3c0a01b805afe51f31833a4f5cccd7138b079092`；当前最新正式版本为 `v0.2.3`。2026-11-02 查询时没有开放 PR。
- 施工分支：`codex/pointer-and-native-surfaces-20261102`，工作目录 `/private/tmp/dtw-app-lifecycle-session-20261102`，HEAD `0f4a22f78047bac55d4f054935edc781fb1b6a9a`。
- 相对远端 main 有三个实现提交：`5b186b1`、`cbb9e07`、`0f4a22f`；尚未进入远端 PR/CI。用户原工作目录的 local main 停在更早的 `48d0714`，不能用它计算待发布增量。
- 开发二进制 SHA-256：`9c014dc2ef87bb0ea066bc4badcfb04f312f887069cc255cf9539aae9f017867`。本轮补跑测试后该二进制哈希不变。
- 依据包括用户已确认的简化目标、所引用的 macOS 自动化讨论、[Issue #93](https://github.com/caelis-labs/desktop-world/issues/93)，以及当前源代码和真实执行日志。

## 目标回归

| 目标 | 当前实现与证据 | 仍需落实的边界 |
| --- | --- | --- |
| 两个工具、一种授权入口 | MCP 仅公开 `apps` 与 `exec({app,source})`；缺少 app 被拒绝；公开脚本没有 Session/state/result 查询。 | CLI `dtw api` 和已分发安装指南仍泄漏旧协议；错误码与包级检查不一致。 |
| 熟悉、简单的脚本 | async JavaScript；9 个 app 方法；普通变量只在一次执行内存活；print/小型返回值作为输出。 | 生成的旧 TypeScript 声明与当前可执行方法不一致；需要让所有当前入口指向同一份 Skill。 |
| 看→操作→看 | 增量场景、输入后自动观察、语义不可用时图像回退；上一轮原生回归验证了这些路径。 | 语义当前集中于顶层窗口；隐式键盘路由依赖最新窗口排序。真实浮层/焦点窗口是否一致需要专门验证。 |
| 应用截图自然呈现窗口堆叠 | macOS 合成已证明归属的窗口，处理层级、透明度和坐标绑定；普通 AppKit 浮层、系统 Open/Save 已验证。 | Chrome 翻译浮层、真实 LSUIElement 应用及不同 helper 家族未由本轮 37 项回归覆盖；Windows/Linux 缺少合成所需场景元数据。 |
| 生命周期闭环 | macOS 已安装应用发现、自动启动/关注、无窗快捷键、正常 quit、保存提示、重启后基线重置。 | 新协议下 LSUIElement 的发现→观察→操作闭环需要恢复；Windows/Linux 的停止应用发现和启动仍未实现。 |
| 退役 result、隐藏 Session | 工具和脚本均已收敛；原生取消后以普通 capture 恢复，运行时配对释放按键/鼠标，不需要管理 Session 或轮询结果。 | 需把旧验收迁到此恢复方式；保留“未知输入先读回”的操作约束。原调用丢失时不能把后续读取当作原业务操作已成功的证明。 |
| 少步数、少 token、速度 | 已有已知步骤合并、自动后续观察、增量和有界输出等结构性基础。一次已有 trace 中的无变化 look 响应为 162 字节 JSON。 | 尚无统一输出/调用次数/读耗时预算回归；该单例不是 token 或速度提升比例。可以检查确定性协议预算，无须 Harness A/B。 |

## 本轮实跑与证据分级

### 新补跑

| 检查 | 结果 | 范围与日志 |
| --- | --- | --- |
| `cargo test --release --locked --all-targets --quiet` | 493 unit + 4 + 3 integration，共 500 项通过。 | `/private/tmp/dtw-readiness-release-tests.log`；macOS 当前源的 release profile，不能代表 Windows 构建/GUI。 |
| 原生 input controls | 通过，包括 14 项 input controls、键盘契约及 109 项 native input contracts 等。 | `/private/tmp/dtw-readiness-native-input.log`；纯 Swift，没有发布桌面输入。 |
| 原生 worker 退出/清理 | 两个 worker 角色的 TERM、EOF、坏消息、父进程死亡共 8 种控制通过。 | `/private/tmp/dtw-readiness-native-worker.log`；没有 AX/截图/桌面访问，父死亡场景的预期终止与普通退出分开记录。 |
| 本地 staging 布局 | 开发二进制复制成功，Skill 的三份文件与源一致。 | `/private/tmp/dtw-readiness-stage-20261102`；local-development-stage，版本仍是开发中的 0.2.3，不是正式候选或已签名安装验收。 |
| staging 中真实二进制的 CLI/MCP 冒烟 | **失败**：缺少 app 返回 `invalid_app`，检查器要求 `invalid_arguments`。CLI 版本、工具列表、拒绝请求、EOF 清理此前步骤正常。 | `/private/tmp/dtw-readiness-packaged-mcp-20261102/report.json`；这是当前候选 workflow 会执行的检查，属于明确构建阻塞。 |
| Windows/Linux 原始 provider 类型检查 | **均失败**：独立临时 Cargo 工程遗漏 `png`，共享 native_client 的测试现在使用该依赖。 | `/private/tmp/dtw-readiness-{windows,linux}-types.log`。产品 Cargo.toml 已有 png，因此这条失败不能归因为产品缺依赖。 |
| provider 类型检查诊断 | 仅在 `/private/tmp` 的检查器副本加入与产品一致的 `png=0.18.1` 后，两平台均通过。 | `/private/tmp/dtw-readiness-{windows,linux}-types-diagnostic.log`。仓库检查器没有被修改；结果只覆盖 provider 源和测试类型检查，不含全产品链接、测试运行或 GUI。 |

### 复用的同源开发证据

`docs/agent-capability-followup-acceptance.md` 记录了同一开发二进制的 37 项原生场景，报告位于 `/private/tmp/dtw-capabilities-native-j/report.json`：鼠标 hover/drag、标准 NSSlider 读回、真正的 Open/Save/覆盖确认、两块实屏分别放置、取消及配对释放、Unicode、生命周期等通过，native workers 正常回收。它仍主要依赖受控 AppKit 应用，不能代替真实应用家族或安装包验收。

此前源检查的 294 项选定 Python 测试、布局和打包单元检查通过。它们没有证明上面这次真实二进制的包级冒烟通过。本轮发现的失败保留原始日志，不用模拟检查的绿色覆盖它。

Finder/Chrome/WPS 的既有只读 corpus 使用的是更早的协议收敛构建，能作为调研线索，不能算本次 pointer 构建的真实功能任务通过。Clippy 当前工具链未提供，未声称通过。

## 按依赖安排的工作顺序

### 1. 协议与验证入口收尾：下一工作包

1. 固定缺少 app 的错误契约，让真实服务器、错误文档与 `run_packaged_mcp.py` 一致。继续在 provider 连接前拒绝请求。通过实际 staging 二进制复验。
2. 让 `dtw --help` / `dtw api`、生成声明、构建文档及包内安装指南收敛到当前 Skill。旧原子 DSL 可以保留内部测试用途，但不应继续作为当前 Agent 使用说明。
3. 更新正在使用的安装验证示例与工具白名单。`docs/install-dsh.md`、`docs/install-agy.md` 仍有 `app.windows()`，后者当前步骤还要求 api/result；这些文件会被 staging 打入包内。历史记录保留版本边界，当前操作步骤使用两工具协议。README 的“seven methods”也应更新。
4. 修复原始 provider 检查脚本的依赖清单，使用原入口复验。加强生产 facade 的关键错误/输出契约覆盖；500 项测试中大量验证私有原子层，`scene_tests.rs` 当前只有 7 项 facade 测试，数量不能代替新入口覆盖。

完成条件：实际包级冒烟退出 0；缺 app 一次拒绝且没有桌面连接；当前公开安装/使用路径没有 Session/window_ref/api/result 指令；原始跨平台检查入口可执行并清楚声明验证范围。

### 2. 恢复新协议关键验收路径：先覆盖原问题和常见边界

保持 `apps` + `exec` 和现有 9 方法，优先构建/迁移下列回归入口。旧脚本只复用通信和独立观察能力；不为本轮做真实模型任务或 Harness A/B。

| 顺位 | 场景 | 完成证据 |
| --- | --- | --- |
| 2a | Issue #93：有真实窗口的 LSUIElement/accessory 应用 | 新协议发现→look/capture→操作控件→独立读回；嵌套 helper 等负例保持过滤。可先用等价拥有型菜单栏应用，再对真实 Caelis Bot 做无破坏验证；不执行它的真实更新。旧 lifecycle/session probe 已有 fixture 原料，但入口仍要求三个工具和 session.result，当前不能运行。 |
| 2b | Chrome 真实浮层/菜单、窗口堆叠、AX 不可用场景 | 观察真实前景、截图层级、命中对象和键盘目的地；必要时明确拒绝并提供可走的图像/语义路径。尤其覆盖最初指出的翻译浮层，不以普通 NSPanel 代替它。 |
| 2c | 当前协议下的常规真实应用任务 | 拥有的临时浏览器页面/文档：输入 Unicode、选择/编辑、打开/保存/取消覆盖、无窗口恢复、正常退出与保存提示。独立事件/文件/控件读回；复用已经验证的 fixture 负例，避免只添加 happy path。 |
| 2d | Windows 既有基础能力迁移 | 先完整 Windows 编译与包级冒烟，再在 Windows 桌面跑 look→click/key/type→look、常规对话框、Unicode 和清理；新 move/drag 可继续明确不支持。已有 Windows probes 需要迁到新协议后才可作为本次证据。 |

这一步验证后，只针对真实失败补内部路由、窗口归属或截图能力。不要用增加 Session、公开 window binding 或恢复 result 查询规避问题。

### 3. 解决跨平台场景缺口并固定本次能力承诺

Windows/实验性 X11 的共享窗口解析当前将 `scene` 置为 None，而 facade 对多可见窗口在缺少 bounds 时必然返回 `scene_geometry_unavailable`。这是源代码可确定的截图缺口，尚无本次 Windows GUI 复现。常规 Open/Save、弹窗可能因此丢失图像兜底，应作为双平台候选准备事项处理。

若继续把 Windows 纳入本次正式平台承诺，至少要补足常规多窗/对话框的布局与路由，或者在实机中证明与承诺一致的可行流程。provider 类型检查不能关闭这项工作。Windows 不必在本次获得所有 macOS 新手势；Linux 继续维持实验性边界。

macOS 当前以“可见窗口排序”决定隐式输入窗口。前景浮层、提示窗、模态窗口、实际编辑焦点不总等价；这是需要复现的设计风险，目前不能直接断言存在误投。2b 的证据决定是否需要内部 focused/modal recipient 解析。

多屏跨度物理输入、跨 app/window 拖拽、混合 DPI/负 Quartz 原点、Spaces/fullscreen/hotplug、clipboard 和所有 helper 家族不作为此次无限扩容目标。当前跨屏窗口会在物理输入前明确拒绝，截图仍可用；这类边界须保留在能力说明及验收负例中。

### 4. 输出预算与候选准备

- 从当前协议 trace 固定有限的输出预算：首次 baseline、无变化 look、增量、一次语义动作、一次像素动作、失败/取消恢复，以及 print/return 的有界行为。检查响应字节和可避免的重复内容、显式调用次数、读取耗时，不换算未经测量的 token 节省或模型成功率。
- 以已确认范围冻结源 SHA，走现有两平台 CI 和开发候选通道。当前三个提交没有远端 CI 证据；本地测试不能补写成 hosted CI 通过。
- 这是破坏性协议变更：核对 Release Please 版本/变更日志、api/result 与 Session 迁移说明、插件清单和 Skill 同包交付，并做消费者 MCP/schema 兼容检查。无需真实模型 A/B，也不能假定旧调用方会自然迁移。
- macOS 验证 Developer ID / notarization，Windows 验证完整构建、依赖和包内容；记录 source、binary、Skill 和包哈希，保留原始构建/公证证据。

### 5. 进入真实发布验收

以固定源、包级检查通过的开发候选进行真实安装/升级和桌面验收：干净/既有权限环境、安装后 Skill 与二进制一致、冷启动、观察/输入/截图、保存提示与正常退出、中断恢复、worker 清理及已声明的平台边界。记录具体操作系统、显示器和应用家族，不把一次机器的结果扩写成全部支持。

现有发布流程是 CI→Release Please→固定源构建/校验→自动发布，GUI 和 clean-install 不是当前自动发布门槛。因此开发候选验收应安排在正式版本 PR 合入前。正式发布后还要对真实下载资产做安装/升级复验；开发候选证据不能冒充最终发布字节的证据。本轮不新增手工发布审批或修改自动发布流程。

工作路径：**协议收尾 → 新协议关键真实回归 → 按失败补内部能力并固定范围 → 固定源开发候选 → 安装/升级验收 → 发布资产复验**。

## 主要代码定位

- 两工具与脚本说明：`cmd/dtw/src/app_protocol/mcp.rs`、`skills/desktop-world/{SKILL.md,API.md,PATTERNS.md}`。
- CLI 旧 API：`cmd/dtw/src/main.rs:11`、`cmd/dtw/src/app_protocol/description.rs:368`、`docs/app-script-api.d.ts`、`docs/building.md:52`。
- 包级错误码检查：`tests/ci/run_packaged_mcp.py:257`；当前真实返回在上述 staging report/mcp/stdout.bin。
- Windows/X11 缺少 scene：`cmd/dtw/src/backends/native_client.rs:381`；多窗合成拒绝：`cmd/dtw/src/app_protocol/scene.rs:681`。
- 旧验收入口：`tests/acceptance/app_lifecycle_session_probe.py:119`、`tests/acceptance/real_app_tasks.py:263`、`tests/acceptance/browser_workbench_probe.py` 及 Windows probes。
- 当前原生回归：`tests/acceptance/app_scene_probe.py`、`docs/agent-capability-followup-acceptance.md`。
- 自动发布与证据边界：`docs/releasing.md`、`.github/workflows/rewrite-{macos-,}candidate.yml`。
