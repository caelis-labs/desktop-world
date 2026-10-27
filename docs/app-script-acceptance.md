# C5-macOS.1 验收记录

日期：2026-10-27；基线为 #55 合并提交 `95c7bb3f304d852e591f87109f8122e801abc85b`。本检查点完成 [单 app 脚本 v1](app-script-v1.md) 的最小切片：五个既有原子方法、一个脚本任务槽、逐次 Core 校验、有界 VM、JSON state 和不可随 payload 驱逐的步骤回执。CUA 来源保持 0.38.0 / `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`；没有扩展额外原子能力或切换默认 MCP。

[C4-macOS.2](app-protocol-acceptance.md#c4-macos2实际-harness-授权入口2026-10-27) 已取得实际产品代码的审批接线和原生 GUI 证据。Caelis 配套本地提交为 `96b1ffe9`（授权 Host）与 `e63a0fc7`（C5 描述和真实 Harness 验收）。用户指定保留本地，均未推送/发布；不能把本地测试通过解释为已发布 Caelis 支持此入口。

## 自动化与审阅

- `cargo test --features app-protocol`：137 项库测试、26 项既有执行器测试通过；1 项既有原生恢复测试本轮仍 ignored。库测试覆盖 Core 绝对预算、单槽、逐次授权/引用、异常前效果、state 结算、payload 驱逐后的账本、取消/到期、VM 内存/栈/调用/JSON/console 限额及 panic 回收。
- source-check：独立 std-only Core 40、IPC 12、Python 14、固定 CUA prelude 70 项，以及 TypeScript/rustfmt 通过；版本一致性通过。它们与库测试有重合，不应相加当作独立覆盖数。
- Caelis：所属包与重点 race、完整 `control/desktopapp` 和 `app/gatewayapp` race、最后的 DesktopApp 定向 race 及 `make commit-check` 通过。真实 GUI 的 opt-in 测试与这些自动化测试分开记录。
- 并行审阅发现并修复 provider panic 后 VM 线程可能脱离任务生命周期、超时收尾中的 cancel 回包误报 cancelled 两处问题；均有回归。VM 所有者现在在 unwind 路径取消并 join，首个上下文终态保持。未增加自定义 allocator；被脚本捕获的 OOM 可以在原硬配额内恢复，合同不声称每次内存异常都强制失败。

```sh
CARGO_TARGET_DIR=/private/tmp/dtw49-target cargo +1.98.0 test \
  --manifest-path cmd/dtw/Cargo.toml --locked --offline --features app-protocol
NPM_CONFIG_USERCONFIG=/dev/null NPM_CONFIG_REGISTRY=https://registry.npmjs.org \
  CUA_SOURCE_ROOT=/private/tmp/dtw-cua-0380-source sh scripts/check-source.sh
python3 scripts/release-artifacts.py check
```

## 实际 Harness 与 GUI

环境为 macOS 27.0.1（26A434）、arm64、Rust 1.98.0；显式编译 `app-protocol` 的开发二进制。两个自有 fixture 具有相同 bundle identifier/name，按完整 app 实例区分。执行前由操作者将 A 带到前台并保证点击点无遮挡；DTW 没有自动 focus、猜测目标或重试动作。

实际 Stack → Session → Runtime policy → Control canonical approval → Headless callback → DesktopApp Host → DTW 跑完 9 次工具调用、2 次审批。模型与审批 Surface 是确定性测试驱动，生产审批、Host 和原生路径均实际执行。

| 场景 | 独立结果 |
| --- | --- |
| 拒绝后批准 | 首次拒绝无 grant/context/效果；随后批准的完整 app、能力、截止时间、grant ID 与实际 describe 一致 |
| 同脚本组合 | windows → observe → press → capture → click → observe；两次动作，AX 与外部计数从 6 到 8 |
| 图片 | 实际模型请求收到 640×504 PNG；已视觉检查，仅 owned Target、Count: 7 和 Increment；截图位于第一次 press 后、pixel click 前 |
| 动作后异常 | 第二个脚本再 press 一次后抛错，Count 到 9；原动作 receipt 保留，state.runs=999 未提交 |
| state 与重复 ID | 下一脚本读回 runs=1 和 Count: 9；再次提交失败脚本原 ID 返回原回执，无第四次动作 |
| 撤销与清理 | close 后原 context 的 script 拒绝；另一 app 始终未改变；Runtime 关闭无错误，私有授权目录为空 |

动作 receipt 保持 `dispatched / unverified`，没有因为 GUI 读回成功而改写 provider 证据。独立计数与 AX 读回是额外证据。MCP 图片位于 VM 外，只保留最后 capture，并标识来源步骤。

在本地 Caelis checkout，配置四个绝对路径后使用精确测试名：

```sh
CAELIS_DTW_APP_BINARY=/path/to/dtw \
CAELIS_DTW_FIXTURE_A=/path/to/fixture-a \
CAELIS_DTW_FIXTURE_B=/path/to/fixture-b \
CAELIS_DTW_APP_EVIDENCE=/new/evidence-directory \
GOWORK=off go test ./app/gatewayapp \
  -run '^TestDesktopAppNativeScriptHarnessAcceptance$' -count=1 -v
```

## 超时、取消与迟到效果

`tests/acceptance/app_script_probe.py` 使用独立的参考 Host fixture 政策，不能替代上述生产审批验证。每个场景只启动一次 2 秒慢动作：

- **VM 超时**：AX 先返回 Unknown，脚本捕获该错误后运行无限循环；总期限在约 1.006 秒结束执行并回收 worker。已结束的原生步骤不被追改为“在调用中超时”，其原始 Unknown receipt 保持无 interruption；脚本本身为 timeout，state 未提交。
- **原生调用中取消**：等待 fixture actions_started 屏障后 cancel，约 142 毫秒完成收尾，worker 回收，动作回执为 Unknown + Cancelled，保留原 provider outcome。
- 两次场景中，fixture 随后各完成一次迟到效果（4 → 5、5 → 6），新只读上下文用 AX 独立确认；原 ID 查询/重复提交均不改回执、不再派发，关闭上下文拒绝新脚本。关闭 worker 不代表撤销已送达应用的动作。

```sh
python3 tests/acceptance/app_script_probe.py /path/to/dtw \
  /path/to/fixture-a /path/to/fixture-b /new/evidence-directory \
  --host-decision approve
```

## 保留的失败与下一检查点

[脱敏索引](evidence/c5-app-script-macos-20261027.json) 保存 binary、成功原始记录及图片的 SHA-256、完整关键 receipt 和失败摘要。原始文件仍保留本机，不上传包含其他应用 metadata 的 catalog：组合通过记录位于 `/private/tmp/dtw-c5-native-harness-20261027-r4`，生命周期通过记录位于 `/private/tmp/dtw-c5-native-lifetime-20261027-r3`。同前缀 r1/r2/r3 失败和 `/private/tmp/dtw-c5-native-semantic-harness-20261027-r1` 均保留。

早期组合尝试中，语义按压已发生后，像素点击因真实遮挡返回 `target_occluded / not_dispatched`；没有把整段脚本说成“未执行”，也没有放宽遮挡校验。验收模型起初缺少 image_input 声明，已修正；完整流程在操作者明确提供桌面使用时间、将 owned A 放到前台后通过。生命周期测试起初误以为 AX 会等待完整 2 秒，实际先返回 Unknown，现分别验证 VM 超时和原生调用中取消。另一次动作屏障失败没有留存终态，原因不推断；探针现会在屏障前保存并报出提前结束的实际结果。

**偶发空窗口枚举仍未解释。** 一次组合尝试报目标数量不符，后续独立 semantic-only 尝试明确返回 `inventory=[]`，均无动作。随后三个新上下文中的 9 次只读 atomic/script 枚举均返回两个 fixture 窗口；这不足以证明问题消失，也不能推断根因就是遮挡。

因此 C5-macOS.1 只通过本次前台 fixture 的限定脚本闭环。下一检查点 **C5-macOS.2**：在固定代码/权限/fixture 下控制冷启动、后台、前台转换，分别经原子入口与生产 Harness 留存窗口归属各筛选阶段的有界诊断，解释空列表；每种受支持状态至少三个新上下文复验，失败原始结果必须留存。不能通过 sleep、标题猜测、自动 focus 或动作重放掩盖失败。若属于支持范围外，明确区分“无窗口”和“归属未证明”，再决定最小契约调整。

在这条可靠性检查点完成前，不切换默认入口、不宣称普遍后台可用、不推进 C6 批量扩展。Windows C2/C3、新入口安装包/签名/TCC、自主模型、交互 TUI 和已发布 Caelis 均未验收。

## C5.2 后续

归属诊断和受控复验现已完成 C5-macOS.2a，详见 [库存验收记录](window-inventory-acceptance.md)。上文历史失败保留；自然偶发触发原因仍在 C5-macOS.2b 定位，不以新的 fixture 成功覆盖旧限制。
