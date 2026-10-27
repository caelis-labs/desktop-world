# C2/C3 macOS 原生检查点验收

日期：2026-10-27。实现提交：`a606d3320eb3ca8a78d70d72daf9e89bc94f0532`；后续文档提交仅整理本记录。环境：macOS 27.0.1 (26A434)、arm64、Rust 1.98.0，真实交互桌面。CUA 来源固定为 Driver 0.38.0 / `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`，迁入文件、依赖闭包、原始 hash 和 MIT notices 见 [来源记录](native-source-provenance.md)。

**结论：C2-macOS.1、C2-macOS.2、C3-macOS 的限定内部切片通过。** Windows C2/C3 尚未实施。新原生路径通过 `native-checkpoints` feature 和内部 example 验收，未接生产 MCP、未安装或分发，也未形成稳定公开协议。真实 GUI 对象是本仓库的独立 AppKit fixture；这不是日常应用兼容性或安装包认证。

## 实现边界

Core 继续只依赖标准库，拥有 app/实例/window、grant、观察引用、能力和结果。原生适配器持有 AX 对象、像素几何与私有 token。每个 Runtime 使用一个受监管 worker；单次 IPC 最大请求 64 KiB、响应 24 MiB，有请求关联和损坏回复处理，取消/超时后终止并等待回收，不重启重放。停止失败保留诊断和终态，不伪装成成功关闭。

应用实例用内核进程启动时间核验，窗口绑定 CG 所属 PID、AX 对象及代际。窗口精确映射隔离使用动态查找的私有 `_AXUIElementGetWindow`；缺失时明确 unsupported，不根据标题或尺寸猜测。当前只支持同 PID、layer 0 的标准 AX 窗口。helper process、sheet/menu 的动作路由、自动激活、selection、setValue 等均未迁入。

AX snapshot 和 advertised AXPress 共享原生状态所有者；只暴露启用且支持 AXPress 的 button/checkbox/radio，派发前重新核验实例、窗口、祖先与模态状态。检测到 sheet/modal 或无法证明模态状态时拒绝；普通非模态浮窗不因此阻断目标窗口。快照最多 128 节点、16 层，截断不能发布为有效观察。AX 数组使用 donor 的 CopyAttributeValue，再限制保留/遍历数量：原生整份 CFArray 的分配并不受这个节点上限限制。

已有 SCK capture/click 进入同一 Core 的独立像素 kind、grant、引用和几何校验。截图、动作前均核验窗口身份，鼠标移动后再次校验；发生指针移动却无法确认点击时保留 Unknown。SCK 租约复用原有稳定锁，持有到 worker 退出。生产路径仍沿用既有执行器；共享改动是锁逻辑抽取和增加指针已移动的诊断。

原生调用由父端单调计时监控整体 IPC 期限（每次最多 15 秒）；每个 AX RequestContext 将收到的期限转换为单调预算，AX 单次消息最多 0.2 秒。像素流程会多次建立验证上下文，其短时间输入预算仍使用墙钟检查，不能据此宣称整条 Swift 路径只做一次转换。测试上下文 grant 为 180 秒、观察 TTL 为 15 秒。example 自行构造这些可信测试 grant，不能作为生产 Harness 授权入口。取消/强制退出不承诺撤销应用已接收的工作；像素输入预算预留正常 down/up 时间，也不保证任意 SIGKILL 时点都完成 mouse-up。

## 自动化验证

| 证据 | 结果与范围 |
| --- | --- |
| std-only runner | 31 项 Core + 12 项真实子进程 IPC 测试通过；独立 rustc，`-D warnings` |
| native feature library | 47/47 通过：31 Core、12 IPC、4 原生适配/worker 契约测试 |
| 现有执行器 | 26 项通常测试通过；另配置本地默认 release binary 和固定 CUA worker，既有 ignored recovery 测试单独通过 |
| source-check | 31 Core、12 IPC、8 Python、70 固定 CUA prelude 检查，以及 TypeScript/rustfmt 通过 |
| 编译/版本 | 默认 release CLI、native example 编译与版本一致性通过；Swift arm64 静态编译和 x86_64 typecheck 通过，后者不代表 Intel GUI |
| 审阅 | 原生来源/身份/模态边界和 Core/IPC/回执分别独立审阅；修复 shutdown 诊断被取消原因覆盖，并增加回归 |

IPC 测试使用真实 Python 子进程，涵盖启动失败、坏帧/超限/EOF、不读 stdin 的阻塞写、超时、取消、单调用准入、并发停止、控制回调阻塞和空闲退出回收。OS 拒绝 kill 的路径经过源码审阅，未在本机人为诱发该 OS 故障。CI 增加 native-feature library 测试，macOS 另编译 example；CI 不自动执行下面的 GUI 用例，也不证明 Windows native backend 已实现。

复现自动化检查（`CUA_SOURCE_ROOT` 指向已核验的固定源码）：

```sh
python3 tests/ci/run_core_checkpoint.py
cargo +1.98.0 test --manifest-path cmd/dtw/Cargo.toml --locked --features native-checkpoints
CUA_SOURCE_ROOT=/path/to/pinned-cua sh scripts/check-source.sh
python3 scripts/release-artifacts.py check
DTW_TEST_BINARY=/absolute/default/dtw DTW_TEST_WORKER=/absolute/pinned/cua-driver cargo +1.98.0 test --manifest-path cmd/dtw/Cargo.toml --locked recovery_tests -- --ignored --test-threads=1
```

本地执行使用 `CARGO_TARGET_DIR=/private/tmp/dtw49-target` 和 `--offline`，source-check 用命令级公共 npm registry 覆盖本机镜像，未修改用户配置。默认 release binary 为该 target 下的 `release/dtw`，固定 worker 为 `/private/tmp/dtw-cua-0380-package-final/libexec/cua-driver`。

## 真实桌面结果

最终一轮原始记录：`/private/tmp/dtw-c2c3-evidence/final2/`。仓库保留 [机器可读摘要及 29 个原始文件的 SHA-256](evidence/native-checkpoints-macos-2026-10-27.json)，包含实现 SHA、example binary hash、结果、截图和 JSONL/log 的 hash。原始文件保留在本机证据目录，不进入发行包。

| 阶段 | 预期与实际结果 |
| --- | --- |
| readonly | 原生 catalog 找到 fixture；2 个窗口身份匹配独立状态文件；同名仍保持不同 ID；隐藏/恢复可见性正确；计数不变 |
| lifecycle | close、cancel、EOF、父进程退出四种场景均验证 worker 已退出并回收；终态后不接纳新执行 |
| pixels | 新截图 → 单次点击，独立 counter 0 → 1；重放、移动后旧截图、遮挡、隐藏、关闭均 NotDispatched |
| semantic | AX 观察 → 单次 AXPress → 新观察读回 Count，独立 counter 0 → 1；错误窗口、重放、attached sheet 均拒绝；普通非模态浮窗共存通过 |
| degraded AX | fixture 暂时没有 AX 内容时仍能取得 SCK 图像；2 秒 AX 延迟在约 223 ms 内返回 cannotComplete，随后独立新观察可用 |
| cancel-inflight | 按预设 150 ms 定时取消，约 159 ms 返回 Unknown + Cancelled；返回后确认应用已开始动作且 worker 已回收；应用约 2 秒后 counter 1 → 2，新上下文独立 AX 读回；没有重放，关闭窗口后的旧 element 亦拒绝 |

动作返回 Dispatched/Unverified 不会自行变成 Confirmed；成功效果由测试另一次观察和 fixture 独立计数判断。取消用例同时保存上下文 outcome、provider 原始 outcome 与 interruption；晚到的效果不会被取消响应抹去。另一轮相同取消试验曾得到 provider UnexpectedEof，最终轮得到 provider Cancelled，两者都保留 Unknown 和原始诊断。

截图已人工查看：640×504，fixture 按钮中心为 (320,324)；像素测试据 fixture 几何定位按钮。测试设置会显式把 fixture 置前，这属于受控验收准备；provider 本身不自动 focus。此处未覆盖多个显示器、缩放组合、真实 PID 复用、同安装路径多实例重启、复杂应用树或分发后的权限身份。

## 失败与修复记录

- 初次真实 AXChildren 读取用 indexed CopyAttributeValues 遇到 `illegalArgument (-25201)`；改回 donor 的 CopyAttributeValue、验证数组类型后保留有限前缀，最终实际观察与动作通过。
- 初期 AXWindows 曾连续三次在 0.2 秒超时后返回 `cannotComplete (-25204)`；后续最终源码重建及新 fixture 验收通过，但该早期现象的原因没有证实。未添加 provider 自动重试或 focus/title fallback。
- 恢复窗口后的首次 capture 曾正确拒绝为 StaleObservation：fixture 的确认不等于 compositor 已稳定。验收 readiness 改为最多三次新的只读 capture；任何动作都不自动重试。
- 最终回归第一次启动 fixture 时，测试端过早读取尚未产生的 state.json；失败发生在原生操作前。启动器和测试入口增加有界就绪等待，随后完整 final2 五阶段通过。
- 独立审阅发现 worker 回收错误可能被通用取消诊断覆盖；只在 transport Control 错误时恢复上下文原因，并保留失败回收诊断。相应契约测试通过。

早期记录仍位于 `/private/tmp/dtw-c2c3-evidence/` 的其他目录，不能把它们全部描述成成功验收。上表只汇总最终 final2 结果。

## 串行 GUI 复现

需要当前宿主已有辅助功能/屏幕录制权限及可交互桌面。只启动本仓库的独立 fixture；脚本会操作其窗口、移动鼠标并点击其中的测试按钮。两组用不同的新 fixture 目录，因为 pixels 与 cancel-inflight 都会关闭目标窗口。

```sh
set -eu
export CARGO_TARGET_DIR=/private/tmp/dtw-checkpoint-build
cargo +1.98.0 build --manifest-path cmd/dtw/Cargo.toml --locked --features native-checkpoints --example native_checkpoint
DTW_CHECKPOINT_ROOT=$(mktemp -d /private/tmp/dtw-checkpoint.XXXXXX)
sh tests/fixtures/native-core/build-run.sh "$DTW_CHECKPOINT_ROOT/c2"
for DTW_CHECKPOINT_STAGE in readonly lifecycle pixels; do
  python3 tests/acceptance/native_checkpoint_probe.py "$CARGO_TARGET_DIR/debug/examples/native_checkpoint" "$DTW_CHECKPOINT_ROOT/c2" "$DTW_CHECKPOINT_ROOT/evidence/$DTW_CHECKPOINT_STAGE" --stage "$DTW_CHECKPOINT_STAGE"
done
printf quit > "$DTW_CHECKPOINT_ROOT/c2/command"
sh tests/fixtures/native-core/build-run.sh "$DTW_CHECKPOINT_ROOT/c3"
for DTW_CHECKPOINT_STAGE in semantic cancel-inflight; do
  python3 tests/acceptance/native_checkpoint_probe.py "$CARGO_TARGET_DIR/debug/examples/native_checkpoint" "$DTW_CHECKPOINT_ROOT/c3" "$DTW_CHECKPOINT_ROOT/evidence/$DTW_CHECKPOINT_STAGE" --stage "$DTW_CHECKPOINT_STAGE"
done
printf quit > "$DTW_CHECKPOINT_ROOT/c3/command"
```

下一次循环按 [实施检查点](implementation-checkpoints.md) 进入 C4-macOS.1，先定义公开契约并取得真实 Harness/GUI 闭环；不同时引入脚本 DSL 或更多原子动作。Windows C2.1 另行取得平台证据后再推进 C3-Windows。
