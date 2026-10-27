# C5-macOS.2：窗口库存归属与受控复验

> 历史检查点记录：下文构建参数、默认入口和发行安排属于所记录的版本。当前源码已收敛为重写版，使用方式见 [构建说明](building.md)；既有证据不等于新构建二进制已重新验收。

日期：2026-10-27。基线为 #56 合并提交 `87a6e9fb12584f5a3c315255c9232919d619a5d4`；CUA 来源仍固定 0.38.0。**C5-macOS.2a 的归属诊断、负控和冷启动/后台/前台 fixture 复验通过；C5-macOS.2b 的历史自然偶发触发原因仍未闭环。** 默认入口不切换，C6 批量扩展不准入。

## 确证问题与限定修复

旧路径把非标准窗口、无法取得精确 AX ID、无法匹配 WindowServer owner 等不同原因都过滤掉；Rust Backend 只消费窗口数组，丢弃原生 coverage。真实负控让自有应用通过 `setAccessibilityWindows([])` 返回空 AXWindows，但独立状态仍有两个可见窗口，CG 仍有 6 个同 PID 合格表面。原子与脚本均返回成功的 `windows=[]`，脚本还提交 state。这证明空成功无法表达未证明的归属，**不证明 #56 时自然偶发的触发原因就是这个负控**。

修复保持 std-only Core、Backend `Vec<WindowInfo>`、公开 v1 成功结构和五个脚本方法不变：

- 分类同 PID CG 原始/合格/非零 layer/无效元数据，以及 AX 库存、范围排除、PID/role/subrole 未证明、精确 ID 失败或零值、CG 不匹配、重复 ID、成功候选。
- 出现范围内未证明项时，整次库存失败，即使已有其他成功候选也不发布部分集合。AX 空而仍有合格 CG 表面也失败。原生 `window_inventory_unproven` 经既有 `backend_failed` 错误消息到达原子入口；脚本保留原始步骤错误，未捕获异常表现为 `script_exception`，不提交 state。
- 诊断只有固定计数和最多 8 个 SPI AX 错误码，没有标题、跨 app 明细、额外 AX 扫描或新授权通路。计数和内部原生错误名是 provider 诊断，不成为需要上层解析的稳定 wire 类型。
- 权限、deadline、模态及 AX 读取错误保留原错误。明确排除的非零 layer/非标准角色不被伪装成未证明。CG 表面多于 AX 窗口本身不导致失败；一个窗口可能拥有多个装饰表面。
- 不重试、自动聚焦、猜标题/尺寸、派发输入或恢复动作。既有注册、实例复验和调用预算保持。

`windows=[]` 只表示当前支持范围没有窗口：例如 CG/AX 都为空，或候选均被明确排除；不表示应用在所有 Space、helper 或其他路由上没有窗口。

## 验证结果

[脱敏证据索引](evidence/c5-window-inventory-macos-20261027.json) 包含源文件/二进制 hash、各样本原始记录 hash、独立计数、原始诊断和失败记录。开发二进制 SHA-256 为 `3d2d3599cbd7d06e168f3294e01efd52faa59aed699f7aaa0c655cf596270f3a`。所有 GUI 工作串行；仅使用自有 AppKit fixture，bundle ID 相同但完整 app 实例不同。

每个格子为 **3 个新上下文 × 3 次预定读取**。冷启动每个上下文对应一个新 fixture 进程，第一次 AX 库存读取前没有原生诊断预热；后台/前台用独立 fixture 状态确认激活状态。所有格子合计 36 个通过上下文、108 次库存读取，均为两个正确窗口，零 press/click 和计数变化。

| 状态 | 参考 Host：原子优先 | 参考 Host：脚本优先 | 实际 Harness：原子优先 | 实际 Harness：脚本优先 |
| --- | --- | --- | --- | --- |
| 冷启动 | 3 × 3 通过 | 3 × 3 通过 | 3 × 3 通过 | 3 × 3 通过 |
| 后台 | 3 × 3 通过 | 3 × 3 通过 | 3 × 3 通过 | 3 × 3 通过 |
| 前台 | 3 × 3 通过 | 3 × 3 通过 | 3 × 3 通过 | 3 × 3 通过，独立补验 |

原子优先为 atomic → script → atomic，脚本优先为反序。它们是不同 request ID 的固定对照序列，不是失败触发的重试。实际 Caelis 路径经过 Stack/Session/Runtime 政策、Control 审批、Headless Surface、DesktopApp Host 和 DTW；模型及审批响应为确定性测试驱动。每次 9 次工具调用、2 次审批（先拒绝再批准），仅授 observe，最后关闭并验证旧上下文拒绝读取。Caelis 新提交 `ec6d463b` 与既有 `96b1ffe9`、`e63a0fc7` 均仅本地保留，未推送/发布。

AX 空/CG 非空负控另有 3 个新上下文、9 次读取：修复后全部明确失败，脚本原始步骤诊断保留且 state 不提交，两个应用均零动作。成功路径之后的原生读只用于补充分类计数，不能反推先前调用的瞬时 OS 状态。

自动化检查：139 项 Rust 库测试、26 项既有执行器测试通过；1 项既有原生恢复测试本轮 ignored。独立 Core 40、IPC 12、Python 16、固定 CUA prelude 70 项及 TypeScript/rustfmt/版本检查通过。纯 Swift 分类负控有 71 个断言，包括真正空数据、明确范围排除、缺元数据、混合成功与未证明、SPI 错误/ID0、样本限额、原错误保留；它们不代替 GUI 事实。macOS CI 单独运行该 Swift runner，非 macOS 明确跳过。Caelis 聚焦 race 和 `make commit-check` 通过。

## 保留的失败与支持边界

- 首次新 Harness 测试错误地断言扁平 `context_state`，实际 describe 使用 `context.state`。该次在 windows 前失败且无动作；已修正测试并补形状负控，产品协议未改。
- 最初尝试用 `close_all` 作为真空对照，但 fixture 保留 NSWindow，CG 仍为 6、AX 为 0；9 次读取均报未证明。随后清空 fixture 的窗口引用仍观察到同样 CG 残留，另外 9 次也报未证明。两次失败均保留，**不声称真实 macOS 的 CG/AX 双空分支已验收**。
- 原前台脚本优先最后一例被操作者启动另一个自有 fixture 抢占焦点，在 catalog 后、审批与 windows 前因独立状态变化失败。保留该记录；停止其他 GUI 准备后，用三个新上下文独立补验通过。未放宽前台状态断言。
- 本轮没有重现历史自然空列表，旧记录缺乏分阶段计数，无法追溯它落在哪个过滤分支。当前正常 fixture 的成功次数不证明该问题消失。

`window_inventory_unproven` **不表示断言窗口仍存在或 AX 损坏**，也包含合法闭窗后的 CG 残留。不能仅以 `onscreen=false` 将其改为空成功：公开 CG 标记只说明未排列到屏幕上；固定 donor 也记录其他 Space 的窗口可能离屏且不在 AXWindows 中。DTW 没有迁入用于区分这些情况的可靠 Space 证据，本批保持保守失败。

原始记录位于 `/private/tmp/dtw-c52-matrix`、`/private/tmp/dtw-c52-foreground-final`、`/private/tmp/dtw-c52-unproven-fixed` 及索引列出的失败目录；包含其他 app catalog 的原始 Harness 记录仅留本机，未上传。自有 fixture 已按精确进程路径核对后退出；证据保留。

## 复现与下一检查点

```sh
python3 tests/ci/run_native_inventory_tests.py
CARGO_TARGET_DIR=/private/tmp/dtw49-target cargo +1.98.0 test \
  --manifest-path cmd/dtw/Cargo.toml --locked --offline --features app-protocol

# 操作者先建立自有 A/B，按指定状态控制前后台；output 必须不存在。
python3 tests/acceptance/window_inventory_probe.py /path/to/dtw \
  /path/to/fixture-a /path/to/fixture-b /new/evidence-directory \
  --state background --active no --contexts 3 --first script --host-decision approve
```

前台使用 `--state foreground --active yes`；冷启动每轮重新启动 fixture，用 `--contexts 1`，分别验证 `--first windows/script`，各重复三个独立实例。负控在 A 的 command 文件写入 `ax_empty`，加 `--expected-error window_inventory_unproven`。探针本身不会启动、聚焦或操作应用。实际 Harness 在本地 Caelis checkout 使用 `TestDesktopAppNativeInventoryAtomicFirstHarnessAcceptance` / `TestDesktopAppNativeInventoryScriptFirstHarnessAcceptance`，四个 `CAELIS_DTW_*` 路径用法见该 checkout 的 `docs/desktop-app.md`。

**C5-macOS.2b 下一目标**：使用当前诊断在一个明确的真实目标应用上捕获自然空库存的触发状态；分别记录启动就绪、关闭最后窗口、隐藏/最小化、Space 转换和归属证据。需要区分闭窗残留与其他 Space 时，先制定并验证最小原生证据切片，不能放宽为“离屏就不存在”。每个被声明支持的状态必须有三次新上下文对照，保留失败；若无法证明，仍返回明确未证明并限定范围。完成此门槛前不切换默认入口、不进入 C6 批量扩展。Windows GUI、新入口分发/TCC、自主模型和已发布 Caelis 独立待验收。
