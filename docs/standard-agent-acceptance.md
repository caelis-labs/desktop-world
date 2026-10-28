# 标准 Agent 入口验收

日期：2026-10-28。修改基于 #60 的 `d0d8279130b74a58496e13b7f6a4c4565f6f00cf`。
本轮落实标准 MCP + Skills 接入和稳定应用 ID；不增加 C6 原子能力，也不发布新版安装包。

## 本轮合同

- 普通客户端启动 `dtw mcp`，使用 `apps / exec / result`。执行参数为
  `app: "com.google.Chrome"` 一类平台稳定字符串，没有 PID、generation 或结构化 handle。
- DTW 不接收或验证审批事实，不创建 grant，不依赖定制 Host。内部仍核验运行实例、窗口、观察和资源归属。
- `exec` 等待有界执行完成；MCP 标准取消使用 JSON-RPC ID；`result` 使用原 app ID 和原 `request_id` 查询回执，不重放。
- 五个脚本方法返回 Promise。未处理的 rejection 导致失败并阻止 state 提交；已经发生的应用效果不能回滚。
- 后端支持项、结构化元素与错误信息通过 schema 暴露；TypeScript 声明从同一 schema 生成。

## 源码和本地构建

最终 GUI 使用的本地 debug 二进制：

```text
/private/tmp/dtw-standard-agent-target/debug/dtw
SHA-256 9b2d510e1a8aabe3406e51a97e268d4590a7aaa92d4d242b5d520132360cbfef
```

执行过的检查如下，均通过：

| 检查 | 结果与范围 |
| --- | --- |
| `cargo +1.98.0 test --manifest-path cmd/dtw/Cargo.toml --locked --offline --target-dir /private/tmp/dtw-standard-agent-target --all-targets --quiet` | 161 library + 4 CLI 测试；Core、MCP、脚本、错误与生命周期，不是全部真实 GUI |
| `cargo +1.98.0 build --manifest-path cmd/dtw/Cargo.toml --locked --offline --target-dir /private/tmp/dtw-standard-agent-target --bin dtw` | 构建本轮原生桥和脚本引擎；无上游 CUA 编译 |
| `sh scripts/check-source.sh` | 版本、rustfmt、40 Core / 20 IPC、12 Python 控制检查；Core/IPC 与完整测试有重叠，不重复计数 |
| `python3 tests/ci/run_native_inventory_tests.py` | 100 Swift 断言，窗口归属与结构化字段/序列化纯逻辑，无 GUI |
| `python3 tests/ci/run_native_input_tests.py` | 14 Swift 输入清理纯逻辑断言，无 GUI |
| Skill 校验、JSON/TOML 解析、`claude plugin validate` | Skill 和标准插件配置结构通过；不能代替模型操作验收 |
| 本地 stage | 最终二进制、Skill、来源和已保留许可证按字节核对；拒绝覆盖现有目录和不可执行输入 |

针对本轮行为的回归覆盖 app 字符串解析、同 ID 多实例拒绝、实例替换与 state 清空、重复 ID 不重放、跨 app 引用拒绝、标准取消、迟到连接 owner 回收、清理失败保留 owner、有限资源准入、输出失败收尾、原始错误保留、Promise 拒绝及声明生成一致性。
日常 CI 仍只有既有两项轻量检查，未加入完整 Rust/CUA 编译。

## 标准 MCP 客户端 + 真实 macOS GUI

使用仓库的 `tests/acceptance/standard_agent_probe.py`，通过普通 stdio JSON-RPC 调用最终二进制。
探针创建两个具有不同随机 bundle ID 的自有 AppKit 应用；所有动作只作用于指定目标，另一应用提供独立对照。
这是真实原生窗口、AX、ScreenCaptureKit 和鼠标输入验收，**不是模型驱动验收**。

```sh
python3 tests/acceptance/standard_agent_probe.py \
  --dtw /private/tmp/dtw-standard-agent-target/debug/dtw \
  --output /private/tmp/dtw-standard-agent-gui-20261028-switch-verified \
  --pixel-click
```

| 场景 | 实测结果 |
| --- | --- |
| initialize / tools/list / apps | 三个标准 MCP 工具可发现；应用参数仅为字符串，返回方法/schema/支持范围 |
| windows → observe → press → 独立读回 | Increment 的 `role=button`、`name`、`enabled`、`actions` 与 fixture 对照；目标计数 `0 → 1`，对照保持 `0` |
| 原始回执 | `result` 返回原记录，不执行第二次动作 |
| 旧观察、跨 app 引用、缺失应用 | 分别得到 `invalid_observation`、`target_changed`、`app_unavailable`；计数不变，动作未派发 |
| 窗口 capture | PNG 为 `640 × 504`；元数据与图片一致；人工查看图像确认窗口中 Count: 1 与 Increment 按钮 |
| 新鲜像素观察 → click → 独立读回 | 目标计数 `1 → 2`，对照保持 `0`；没有依赖派发回执宣称效果成功 |
| 标准取消 | 已进入脚本后按 JSON-RPC ID 取消；原记录为 `script_cancelled`，state 未提交，计数不变 |
| 取消后新执行 | 自动建立新上下文，`context_reset=true`、state 为空，旧取消记录仍可查询 |
| A→B→A 顺序截图 | 同一连接切 app 前关闭旧 owner；三次截图均成功，回到旧 app 报告 context_reset、state 为空；原动作回执不变，计数无额外变化 |
| EOF 与进程回收 | DTW 和两个 fixture 均 exit 0，无强制终止；关闭前记录的两个原生 worker PID 全部被回收 |

本地完整证据为 `/private/tmp/dtw-standard-agent-gui-20261028-switch-verified/evidence.json`；
截图为同目录的 `capture.png`，SHA-256
`44efb69cdc1cf817377ea45848c5acad7412f4789e68e3cbdf3879a5b6d9fc69`。
fixture 源文件 `tests/fixtures/native-core/main.swift` 的 SHA-256 为
`3fd71bf67e5e971d1e4754c51756819ed7d86918be0cc94310d34670a9696036`。
本地 `/private/tmp` 文件不是仓库交付物；仓库保留可复跑的探针与上述摘要。

## 普通 Agent 与插件

两个普通 Agent 都在最终构建上完成了真实模型工作流。各自使用新的自有 fixture；
模型获得目标 app ID 与测试意图，自己编写脚本，未直接提供完成操作的脚本。

| 客户端 | 标准接入与实测结果 |
| --- | --- |
| agy，language server 1.4.8，当前默认模型 Gemini 3.8 Flash (High) | `agy mcp add` 注册普通 stdio 服务，通过 Skill 发现加载标准 `SKILL.md`。模型调用 apps、以字符串 app 执行观察、恰好一次 press、独立再观察和 capture；原生计数 `0 → 1`，PNG 为 `640 × 504` |
| Claude Code 2.1.312，当前配置 `codex/gpt-6.1-terra` | 普通 MCP JSON + 本地插件 Skill；初始化显示 connected。模型实际调用 Skill、apps、两次 exec，完成同一流程；原生计数 `0 → 1`，PNG 为 `640 × 504` |

两者的全部 exec 参数都匹配各自指定的稳定 app ID，工具回执均无错误；只派发了一次 press，
没有通过 shell 或其他应用工具执行动作。接收的 PNG 已检查，与模型报告的 Count: 1 一致。
共同图片 SHA-256 为 `44efb69cdc1cf817377ea45848c5acad7412f4789e68e3cbdf3879a5b6d9fc69`。
两个 fixture 均 exit 0，没有强制终止。模型选择使用原客户端配置，未改模型或凭证。

为避免与用户安装的旧版插件混淆，测试副本只改了插件/Skill/MCP 的名称，使用唯一测试命名空间。
agy 先通过官方 `plugin install` 加载 Skill，再以官方 `mcp add` 注册标准服务器；测试会话由 Harness 的
非交互权限模式允许本次动作。Claude 使用临时 MCP 配置和该服务器的工具 allowlist。
DTW 不感知这些策略，没有审批回调、grant 或专用 Host。
agy 的临时插件和 MCP 条目已移除，原有 desktop-world 0.1.0 插件仍启用；Claude 未改用户配置。

本地证据分别为 `/private/tmp/dtw-standard-agy-20261028-b/evidence.json`、
`/private/tmp/dtw-standard-claude-20261028-c/evidence.json`，同目录保留模型事件、回执、图片和 fixture 状态。

## 过程中发现的问题与保留证据

- 多 app 常驻 worker 与进程级 scene lease 冲突：闲置 A 仍会占用截图资源，阻塞 B。
  已改为单个常驻 app context，切换前完整退休原 owner，不提前释放仍有 SCK client 的锁。
  Rust owner 顺序与 A→B→A 原生验证均通过。
- `/private/tmp/dtw-standard-agent-gui-20261028-switch/evidence.json` 记录了一次首次窗口读取失败：
  两个 AX 候选都进入 non-window-role 排除，脚本没有找到目标；只有 windows 步骤，两个动作计数均为 0。
  此时 fixture 的 `active=false/key_window=0`，原探针只等待了异步 activation 请求的命令确认。
  探针现等待自有窗口的实际前台状态，不做 AX 预热，也不自动重试动作；上述新目录的独立验收通过。
  当次实际 role 字符串未保留，**仍不能证明原生分类异常的原因**，不将其直接归因于冷启动。
- agy 的 Claude 插件导入没有替换 `${CLAUDE_PLUGIN_ROOT}`；且本机插件 MCP 加载日志报告未登录 Antigravity。
  首轮只读到 Skill，未执行动作。采用该客户端原生支持的普通 `mcp add` 配置后，完整模型工作流通过；
  [安装说明](install.md)记录此标准接入方式。
- 早期 Claude 2.1.296 检查仅完成 MCP/Skill 加载，曾遇到隔离配置导致未登录、随后模型服务重试超时。
  这些未记为成功；用户完成客户端升级后，2.1.312 在当前模型配置上的完整验收通过。

## 仍保留的范围限制

- 原生 GUI 只覆盖上述自有 fixture。TextEdit 多窗口、闭窗、Space、迟到输入和强杀等更广场景仍引用指定构建的 [C5 历史证据](c5-completion-acceptance.md)，本轮没有全部重跑。
- macOS 权限、同 PID 标准窗口、遮挡、sheet/helper 等支持限制仍成立。历史 AX 偶发的原始原因依然未证明。
- Windows、应用启动/退出、文本键盘和 Browser/DOM 未迁入；本轮没有增加 C6 能力。
- 当前 stage 是本地开发包。签名、公证、目标平台依赖许可证清单、公开安装与最终发行物验证需按[发行准入](releasing.md)独立完成。

下一步按[架构基线](next-architecture-baseline.md)的能力组迁移循环推进；接入和验收范围随[产品矩阵](product-matrix.md)更新。
