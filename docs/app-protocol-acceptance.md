# C4-macOS.1 验收记录

日期：2026-10-27。本轮只交付 [单 app 协议 v1](app-protocol-v1.md)、可信宿主启动配置及显式选择的新 MCP 入口。实现提交 `8c741d740da76758e8f431b8bb934009e3dc0465`，基线 `eafd12e629a89a19fd3b4bbfdebb5f27d76d2767`；后续提交仅记录验收脚本与文档。CUA 来源仍固定 0.38.0 / `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`，没有追加迁移原子能力。

## 可重复验证

环境：macOS 27.0.1（26A434）、arm64、Rust 1.98.0。编译 `--features app-protocol` 的开发二进制；两份独立的 AppKit fixture 实例具有相同 bundle identifier/name，按完整可信实例区分。测试只操作任务拥有的 fixture，桌面阶段串行执行。二进制、实现文件和本地证据 hashes 见 [证据索引](evidence/app-protocol-macos-2026-10-27.json)。

```sh
CARGO_TARGET_DIR=/private/tmp/dtw49-target cargo +1.98.0 build --offline \
  --manifest-path cmd/dtw/Cargo.toml --features app-protocol --bin dtw
CARGO_TARGET_DIR=/private/tmp/dtw49-target cargo +1.98.0 test --offline \
  --manifest-path cmd/dtw/Cargo.toml --features app-protocol
NPM_CONFIG_USERCONFIG=/dev/null NPM_CONFIG_REGISTRY=https://registry.npmjs.org \
  CUA_SOURCE_ROOT=/private/tmp/dtw-cua-0380-source sh scripts/check-source.sh
python3 scripts/release-artifacts.py check
DTW_TEST_BINARY=/private/tmp/dtw49-target/debug/dtw \
  DTW_TEST_WORKER=/private/tmp/dtw-cua-0380-package-final/libexec/cua-driver \
  CARGO_TARGET_DIR=/private/tmp/dtw49-target cargo +1.98.0 test --offline \
  --manifest-path cmd/dtw/Cargo.toml --features app-protocol recovery_tests \
  -- --ignored --test-threads=1
```

结果：91 项库测试（其中 C4 44 项）+ 26 项既有执行器测试通过，通常 ignored 的 1 项恢复测试另行通过。独立源码检查：31 项 std-only Core、12 项 IPC、14 项 Python、70 项固定 prelude 检查，以及 TypeScript、rustfmt、版本一致性通过。恢复测试只执行 JSON 脚本/无效浏览器绑定，不发送 GUI 输入。独立临时验证还用 JSON Schema validator 对比过 1,370 组 parser/schema 输入；未增加生产依赖，日常 CI 依靠仓库内契约测试。

CI 在 macOS/Windows 增加 `app-protocol` 库测试，在 macOS 另编译新 CLI；旧发行包/MCP 检查保持。远端结论以 PR 的精确 head checks 为准，不能替代下列桌面与 Harness 证据。

桌面复现时，先使用 `tests/fixtures/native-core/build-run.sh` 在两个**新目录**启动 fixture，再执行：

```sh
python3 tests/acceptance/app_protocol_probe.py /path/to/dtw \
  /path/to/fixture-a /path/to/fixture-b /new/evidence-directory \
  --host-decision approve --stage semantic
```

分别使用 `deny` 决策及 `semantic / capture / pixel / cancel / lifecycle` 阶段，每次使用新证据目录。`--caelis` 在 semantic 阶段后运行实际已安装 Caelis CLI，使用隔离 Store/workspace、临时 plugin 和本地无认证模型网关；不修改正式 plugin、用户配置、凭证或生产授权。fixture 的准备命令明确把目标放到前台；这不是 DTW 自动 focus 能力。

## 实机证据

最终证据根：`/private/tmp/dtw-c4-evidence/final`。初次与最终运行使用不同证据目录。

| 项目 | 实际结果 |
| --- | --- |
| Host 拒绝 | 所选 app 和另一 app 的拒绝都不签发配置；没有启动执行 MCP 上下文 |
| 语义闭环 | 精确 app/window → observe → 一次 AXPress → 原生计数与新 AX 观察读回；另一进程计数不变 |
| 授权与引用 | 模型 approved/grant/capabilities/session 字段拒绝；wrong app、跨 context、旧观察拒绝；Observe-only grant 的 press 未进入 provider |
| 请求记录 | 同 ID/相同参数返回原记录；不同参数 conflict；完成后的迟到 cancel 不关闭上下文；没有重复效果 |
| MCP 图片 | 640×504 PNG，经 view_image 独立检查：仅 DTW Native Target、Count: 6、Increment；尺寸与协议元数据一致，另一 app 未混入 |
| 像素闭环 | `pixel-verified`：负坐标返回 InvalidPoint，未消费有效图片；同引用的一次 (320,324) click 经计数/AX 读回确认 10 → 11；消费后引用返回 StaleObservation，另一 app 不变 |
| 派发中取消 | 先等 fixture `actions_started` 屏障，再 cancel；返回 Unknown + Cancelled，保留原 provider outcome，worker 回收；应用随后完成一次迟到动作，新只读上下文独立读回，没有重放 |
| 资源与传输 | close、EOF、server death、Host parent death、stdout 背压、stdout 断管共 6 项；原生 worker 都已回收。close 保留协议进程以便查询，EOF/父进程退出会结束协议进程；背压/断管在 stdin 仍开放时以 2 退出 |
| 实际 Caelis | 实际 CLI 经新 MCP 完成 9 次 DTW 调用；工具轨迹独立核对 Count: 8 → Count: 9，fixture 外部计数 +1，另一 app 不变。流程只使用允许的 Skill/ToolSearch/Read 和 desktop_world__exec |

首次 pixel 尝试完成一次点击（9 → 10）后，脚本错误地预期消费后的错误码为 InvalidObservation；实际 Core 合同明确为 StaleObservation，且返回 NotDispatched/provider_outcome=null。保留 `pixel/failed-assertion.json` 与原始 transcript，仅修正这条精确断言，在新的 `pixel-verified` 上下文完成整组验证，没有改变生产行为或放宽为多个可接受错误码。

取消测试的最终 receipt 原样留存在索引对应的 `cancel/result.json`，不把清理成功解释为撤销效果。实际 Caelis 输出和独立 fixture 状态共同作为端到端证据；模型的完成陈述本身不算通过。

## 审阅与边界

独立子代理审阅协议、grant、入口和生命周期；发现并修复关闭/取消终态竞态，补通道屏障回归。另修正整数值 MCP ID（1.0/1e0）的兼容性，保持拒绝小数/null；统一正常退出清理所有权，避免重复等待。最终实现及验收经过独立复核。

- 只证明本机 macOS、此权限组合和 owned fixture；没有新入口发行包/签名/TCC 安装验收，也没有 Windows GUI。
- 文件异 UID 拒绝及本机无法保存的 set-ID 位属于源码审阅，未伪造 foreign-owner 实机测试；其他权限、符号链接、FIFO、大小和固定期限有自动化测试。
- 真实 Caelis 使用**宿主预授权**。没有实现生产动态 app 选择/审批 UI、approval → context 的接线或 Bot 发布；现有 MCP 参数通道不承担宿主授权事实。
- 未开放脚本、任意 PID/native call、额外键盘/滚动/菜单/浏览器能力，也没有跨连接的持久化结果恢复。

C4-macOS.1 以这个窄合同和实机预授权闭环作为检查点。下一步 C4-macOS.2 要验证生产 Harness 的 app/能力审批、上下文创建与撤销一致性；该门槛通过后才进入 C5，不能把本轮预授权 fixture 结果视为动态审批已完成。
