# C5 完整验收：核心合同、生命周期与 macOS 闭环

日期：2026-10-28。C5 的共享骨架、五个 macOS 原子方法、单 app 协议、有界脚本和生命周期已完整落地。C6 可以沿这条基线按工作流批量扩展能力；本轮不再增加 C5 细分编号，也没有发布新版安装包。

- 合并基线：#59，`579727df5386c708ca4074e5df033aed5f0f7494`。
- 实现提交：`f669069b1b5687c1fa3490af9cdb3181555870f5`；后续提交只整理合同文档和证据。
- 提交后验收二进制：`/private/tmp/dtw-c5-complete-target/debug/dtw`，SHA-256 `5955513dff79ae3b57701b9818778e0a7609af1ee70ae27d302893ede86b5835`。
- 环境：macOS 27.0.1、arm64、Rust 1.98.0、Apple Swift 6.4。以下是开发构建与本机 GUI 证据，不是签名、安装或跨平台证明。
- 可随 PR 审阅的[结构化证据](evidence/c5-completion-20261028.json)包含各项结果、原始本地路径及 SHA-256；原始 MCP、AX/CG、OS 输入时序、图片和失败日志保留在本机，未把 Harness 私有 store 或凭据放入仓库。

## 完成的合同

| C5 退出条件 | 落地结果 |
| --- | --- |
| 库存结果可解释 | Core 使用 typed inventory；公开 `coverage`、有界 `diagnostics` 和 `window_inventory_unproven`。支持范围内为空与未证明不混用；无固定重试、尺寸猜测或自动 fallback |
| 脚本接口可发现 | `exec.describe.script` 给出五方法的参数/返回 schemas、能力、引用规则、预算、错误和组合例；[类型声明](app-script-api.d.ts)同步。脚本仍仅使用一个可信 app context |
| 首次证据不会丢失 | 原子返回、JS `error.details`、步骤错误一致；成功的 windows coverage/diagnostics 也留在步骤账本，后续异常或值载荷回收不能抹掉首次空库存事实 |
| 授权到期与结果稳定 | idle watcher 回收 native worker；describe 不再显示有效 open authority；纯 JS 和原子入口均保留 `grant_expired`。旧结果、去重记录与先发生的终态不被改写 |
| 输入收尾与清理诊断 | 取消/关闭/父死封闭输入准入；已接纳的 mouse down/up 成对收尾，AXPress 共用准入门闩。worker 排空预算 400ms，supervisor TERM 宽限 500ms；超出后强制回收并报告未确认清理 |
| 异常退出仍有证据 | native 排空失败、外部 signal/非零退出和 OS 清理 API 错误持久记录；回收后的重复 close 不会消除首个故障。transport 主错误与可能已派发事实保留 |

这些合同不把 `dispatched` 当作 UI 效果确认。门闩保护最终 mouse down/up 与 AXPress 派发段；像素点击的前置指针定位可能已经发生，仍按可能产生效果处理。取消无法撤销已发生的效果；SIGKILL 或 OS/native 卡死也不能保证补发 up。未知结果保持 Unknown，必须独立读回，不能自动重放。

## 本地验证与 CI 边界

| 验证 | 结果 |
| --- | --- |
| 完整默认 Cargo 包 | 162 项库测试、4 项 CLI 测试通过；默认二进制构建成功 |
| 轻量源码检查 | 格式、版本一致性；40 项 std-only Core、20 项 std-only IPC、12 项 Python 负控通过 |
| Swift 纯逻辑 | 库存 74 项、输入门闩 14 项断言通过，无 GUI 调用 |
| 实际 native worker 控制 | TERM、EOF、畸形输入、父进程退出 4 项通过；仅 release 不存在的 binding，无 AX/SCK/输入 |
| 独立审阅 | 修复异常退出与 OS 清理错误诊断遗漏；检查 Child 单一所有权、锁序、输入准入、排空与回执保留 |

复现入口：

```sh
sh scripts/check-source.sh
python3 tests/ci/run_native_inventory_tests.py
python3 tests/ci/run_native_input_tests.py
CARGO_TARGET_DIR=/private/tmp/dtw-c5-complete-target cargo +1.98.0 test --manifest-path cmd/dtw/Cargo.toml --locked --offline
CARGO_TARGET_DIR=/private/tmp/dtw-c5-complete-target cargo +1.98.0 build --manifest-path cmd/dtw/Cargo.toml --locked --offline --bin dtw
python3 tests/ci/run_native_worker_tests.py /private/tmp/dtw-c5-complete-target/debug/dtw
```

独立 std-only 测试通过后，完整 Cargo 环境曾发现新 waitpid 回归中 `as _` 的 E0282 类型推断歧义；改成显式 `std::ffi::c_int` 后两套检查均通过。原失败日志保留在证据索引中。

CI 仍只有 `Source checks` 和 `macOS inventory controls` 两项、各 5 分钟上限；后者增加纯 Swift 输入门闩检查。没有完整 Cargo 依赖构建、CUA 源码下载/编译、GUI 或打包作业。上述完整构建和 GUI 验收在本地执行，不能从轻量 CI 绿色推断出来。

## 提交后二进制的真实 GUI 与 Harness

全部桌面操作串行执行，目标仅为本任务创建的两份 fixture 和独立 TextEdit 实例。fixture B 的计数始终为 0；其作用是独立校验未授权 app 未受影响。

**脚本生命周期**：`tests/acceptance/app_script_probe.py` 覆盖慢 AXPress 后的 VM 超时、native 调用中途 cancel、空闲 grant 到期。超时约 1.014 秒、取消约 0.304 秒返回；迟到效果各发生一次，独立 AX 与 fixture 计数一致，原始 Unknown 回执保持不变，重复 ID 不重放。空闲测试没有 MCP 流量，native worker 在 grant 到期后 79ms 消失，随后 describe 显示 closed/grant_expired；原子与纯 JS 新请求都拒绝，先前结果和去重仍一致。

**输入成对收尾**：`tests/acceptance/input_cleanup_probe.py` 对 cancel、close、父进程被终止各执行一次像素点击。独立 CG observer 观察 down/up；发送关闭请求或终止父进程后立即再次读取 OS 按键状态，三例都仍为 down，因此确实命中输入对中途。每例只出现一对 down/up、计数仅增加一次、worker 被回收，后续 2 秒稳定，没有应急释放或重放。cancel/close 原始回执保留 Unknown 和各自 interruption；父进程已消失的场景不虚构 MCP 回执。

**实际 Caelis Host**：本地 `TestDesktopAppNativeScriptHarnessAcceptance` 通过（5.34 秒），走生产 Runtime、Control 审批和测试 Surface。验证 2 次审批、9 次工具调用、语义/像素组合与独立读回、截图、state 提交/异常回滚、异常后的已发生效果回执、重复 ID 和关闭后的旧 context 拒绝；目标计数增加 3，peer 不变。

Caelis checkout 保持干净，提交为 `ec6d463ba596e3e182ae54d26087d0b441a302fe`；遵循用户要求仅保留本地，没有推送或创建配套 PR。该测试不等于 Caelis/Bot 已安装或已发布此功能。

提交后二进制原始证据：

- `/private/tmp/dtw-c5-accepted-script-20261028/result.json`
- `/private/tmp/dtw-c5-accepted-input-20261028/result.json`
- `/private/tmp/dtw-c5-accepted-harness-20261028/evidence.jsonl`
- `/private/tmp/dtw-c5-accepted-closed-20261028/result.json`
- `/private/tmp/dtw-c5-accepted-worker-20261028.json`

## TextEdit 完整窗口状态矩阵

自有 TextEdit PID 34924；NS launch `1793116398.0468469`，Core 进程代际 `1793116398054910`；二者来自不同证据源，不能互相推算。文档窗口 ID 225864，文件和内容均由本任务创建。测试不干预真实应用 AXWindows，也不靠重试把失败转成成功。

每状态 6 个新 context/provider，script-first 与 atomic-first 各 3 个，每 context 各读取一次原子与脚本结果，共 **72 context / 144 读取**。只有 cold 的首次 DTW 读取前不运行独立 AX 检查，不能把其余新 context 都称作应用冷启动。

| 状态 | 公开结果 | 独立 OS 观察 |
| --- | --- | --- |
| cold | available | AXStandardWindow；CG6 |
| background | available | 同一文档窗口；CG6 |
| hidden | empty_within_scope | 同一 ID 转为 AXDialog；CG6 |
| restored | available | 同一 ID 恢复标准窗口；CG6 |
| minimized | empty_within_scope | AXDialog；CG6 |
| unminimized | available | 标准窗口；CG6 |
| fullscreen-current | available | 同一 ID、全屏标准窗口；CG7 |
| fullscreen-other-space | unproven | AX 有效空数组、CG7；原 ID 仍在 CG 且离屏 |
| fullscreen-return | available | 返回同一 ID、全屏标准窗口；CG7 |
| windowed-restored | available | 同一 ID、窗口模式；CG6 |
| closed | unproven | 文档 ID 消失，AX0，但 CG5 仍存 |
| closed-settled | unproven | 等待 5 秒后仍 AX0/CG5 |

合计 available 84、empty_within_scope 24、unproven 36 次。隐藏/最小化诊断明确记录 `excluded_non_standard_subrole=1`；跨 Space 和闭窗返回结构化未证明错误。关闭前获取的引用在关闭后 observe 返回 `target_changed`，capture 返回带 `stale_window` 的 `backend_failed`，没有返回图片或把旧引用绑定到另一窗口。关闭前文档截图已人工查看，图片与 hash 留在证据中。

这组完整矩阵使用本分支较早构建，SHA-256 `5196bebdf593a9d1461ffee5fc18e64ff3d7c39201c7fe3ccd975125420b0f9d`，原始路径 `/private/tmp/dtw-c5-complete-real-app-20261027`。随后修改的是 Process 异常退出/清理 API 错误持久化及其测试，库存实现与窗口状态判断未变。不能把这 144 次标成提交后二进制结果；最终 `595551…` 构建另以 6 新 context / 12 读取复验了同一 TextEdit 已闭窗状态，均为 `window_inventory_unproven`，并通过上述全部输入、脚本和实际 Harness 验收。

历史 PID41684 的自然偶发缺少当时的 AX/CG/窗口状态，无法追溯确证根因。现有证据说明隐藏、最小化、跨 Space 和真正闭窗必须分别解释；它们不能倒推历史现场。C5 用明确覆盖合同、错误诊断和首次库存账本消除了“成功空列表等于应用没有窗口”的歧义，不再为不可恢复的历史现场拆新检查点。

## 收尾与 C6 准入

验收后按精确 PID/launch 或 fixture 可执行路径退出三个自有 app，核验 PID 34924、33526、33569 及本轮 native worker 均已消失；保留证据文件。没有清理其他应用、共享 checkout 或 Caelis 本地提交。

C6 继续使用现有 Core → 平台适配 → 单 app 协议 → 有界脚本的结构，按选定工作流扩展文本/键盘、滚动、拖拽、菜单、focus 或 Windows 能力组。每组补来源与依赖闭包、明确效果与清理责任、执行真实平台/应用读回，再更新[产品矩阵](product-matrix.md)；稳定且需要保护的行为才增加针对性 CI。

本轮不声明 Windows 原生、Browser/DOM、helper process/非标准窗口控制、任意真实应用流程、签名/TCC 安装包或产品分发已完成。新版包合同和发布验收仍按[发行边界](releasing.md)单独交付。
