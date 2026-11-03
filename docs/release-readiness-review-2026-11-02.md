# DTW 整体回归与发布验收准备：2026-11-02

本轮只做整体回归、证据核对和工作排序。没有继续增加能力，没有修改产品实现，也没有进行真实模型/Harness A/B。

> 状态：本文是 2026-11-02 的快照，其中的诊断与失败证据作为历史记录保留，工作顺序已由后续批次执行。现行参考是 [Skill API](../skills/desktop-world/API.md)、[功能与支持范围](product-matrix.md)和[构建和验收](building.md)。

## 结论（当时）

当前已具备继续做定向真实应用回归的基础，尚未具备冻结候选并进入安装、升级层面发布验收的条件。接下来优先收尾协议与验收入口，然后验证真实应用边界，根据失败补内部能力，最后准备固定源候选。继续增加手势和外部方法的优先级低于这些工作。

最直接的下一步当时被确定为一个有限的“协议收尾”工作包：修正真实二进制打包冒烟的错误契约不一致，清理仍对外展示的旧 API/安装示例，恢复跨平台类型检查入口。该工作包随后完成，见[协议发布准备](protocol-release-preparation.md)。

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
| 应用截图自然呈现窗口堆叠 | macOS 合成已证明归属的窗口，处理层级、透明度和坐标绑定；普通 AppKit 浮层、系统 Open/Save 已验证。 | Chrome 翻译浮层、真实 LSUIElement 应用及不同 helper 家族未由本轮 37 项回归覆盖；当时 Windows/Linux 缺少合成所需场景元数据，Windows 侧其后补了精确 HWND 读取。 |
| 生命周期闭环 | macOS 已安装应用发现、自动启动/关注、无窗快捷键、正常 quit、保存提示、重启后基线重置。 | 新协议下 LSUIElement 的发现→观察→操作闭环需要恢复；Windows/Linux 的停止应用发现与启动当时未覆盖，后续由平台自己的验收记录更新。 |
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
| staging 中真实二进制的 CLI/MCP 冒烟 | **当次失败**：缺少 app 返回 `invalid_app`，检查器要求 `invalid_arguments`；其后按缺少 app 的错误契约修复。CLI 版本、工具列表、拒绝请求、EOF 清理此前步骤正常。 | `/private/tmp/dtw-readiness-packaged-mcp-20261102/report.json`；这是当时候选 workflow 会执行的检查，属于当时的明确构建阻塞。 |
| Windows/Linux 原始 provider 类型检查 | **当次均失败**：独立临时 Cargo 工程遗漏 `png`，共享 native_client 的测试现在使用该依赖；其后在检查入口清单中修复。 | `/private/tmp/dtw-readiness-{windows,linux}-types.log`。产品 Cargo.toml 已有 png，因此这条失败不能归因为产品缺依赖。 |
| provider 类型检查诊断 | 仅在 `/private/tmp` 的检查器副本加入与产品一致的 `png=0.18.1` 后，两平台均通过。 | `/private/tmp/dtw-readiness-{windows,linux}-types-diagnostic.log`。仓库检查器没有被修改；结果只覆盖 provider 源和测试类型检查，不含全产品链接、测试运行或 GUI。 |

### 复用的同源开发证据

`docs/agent-capability-followup-acceptance.md` 记录了同一开发二进制的 37 项原生场景，报告位于 `/private/tmp/dtw-capabilities-native-j/report.json`：鼠标 hover/drag、标准 NSSlider 读回、真正的 Open/Save/覆盖确认、两块实屏分别放置、取消及配对释放、Unicode、生命周期等通过，native workers 正常回收。它仍主要依赖受控 AppKit 应用，不能代替真实应用家族或安装包验收。

此前源检查的 294 项选定 Python 测试、布局和打包单元检查通过。它们没有证明上面这次真实二进制的包级冒烟通过。本轮发现的失败保留原始日志，不用模拟检查的绿色覆盖它。

Finder/Chrome/WPS 的既有只读 corpus 使用的是更早的协议收敛构建，能作为调研线索，不能算本次 pointer 构建的真实功能任务通过。Clippy 当前工具链未提供，未声称通过。

## 工作顺序与后续去向（历史）

以下是当时按依赖排定的顺序，各步已由后续批次处理；现行指引不在本页：

| 当时步骤 | 后续去向 |
| --- | --- |
| 1 协议与验证入口收尾 | 已完成：错误契约、CLI/声明清理与包级冒烟在[协议发布准备](protocol-release-preparation.md)的合并检查中关闭。 |
| 2a 有窗口的 LSUIElement/accessory 应用 | 已完成：见[协议边界验收](protocol-boundary-acceptance.md)的七项生命周期用例。 |
| 2b Chrome 真实翻译浮层/菜单 | 已完成：同一文档保留首次失败与纠正后的通过。 |
| 2c 当前协议下的常规真实应用任务 | 按应用家族继续分别验收；任务规格见[构建和验收](building.md)，Windows 主机任务见[Windows 主机验收](windows-host-acceptance-2026-11-01.md)。 |
| 2d Windows 既有能力迁移 | 官方 Windows 探针已迁到两工具入口；GUI/生命周期边界仍按平台单独验收。 |
| 3 跨平台截图与路由缺口 | Windows 已补精确 HWND 读取与场景元数据（见[Windows 光标与原生对齐](windows-cursor-port.md)）；其余条件由各平台验收决定。 |
| 4 输出预算与候选准备 | 由发布流程承担，见[发布](releasing.md)。 |
| 5 真实发布验收 | 按[发布](releasing.md)；GUI 与干净安装不是自动发布门槛。 |

当时确定的边界仍然有效，后续实现不应默默改写：跨屏窗口的物理指针输入在缺少充分命中证明前保持前置拒绝（截图仍可用）；不用增加 Session、公开 window binding 或恢复 result 查询来规避场景问题；平台记录必须来自该平台自己的证据；跨窗口拖放、其他 helper 家族、混合缩放与负 Quartz 原点等仍需独立验收。

## 主要代码定位（历史）

- 两工具与脚本说明：`cmd/dtw/src/app_protocol/mcp.rs`、`skills/desktop-world/{SKILL.md,API.md,PATTERNS.md}`。
- CLI 旧 API（已退役）：`dtw api`、生成声明与旧 DSL 已从 CLI 和分发包移除，当前入口是 `dtw mcp` / `dtw app catalog`。
- 包级错误码检查：`tests/ci/run_packaged_mcp.py`；当时的真实返回在 staging report/mcp/stdout.bin。
- Windows/X11 缺少 scene：`cmd/dtw/src/backends/native_client.rs`；多窗合成拒绝：`cmd/dtw/src/app_protocol/scene.rs`。两者均已按上述去向改写。
- 旧验收入口：`tests/acceptance/real_app_tasks.py`、`tests/acceptance/browser_workbench_probe.py` 及 Windows probes；当时的 `app_lifecycle_session_probe.py` 已不在仓库中。
- 当前原生回归：`tests/acceptance/app_scene_probe.py`、`docs/agent-capability-followup-acceptance.md`。
- 自动发布与证据边界：`docs/releasing.md`、`.github/workflows/rewrite-{macos-,}candidate.yml`。
