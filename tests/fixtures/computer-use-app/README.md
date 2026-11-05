# DTW Computer Use Workbench

统一的 Computer Use 验收 APP。先固定任务与独立效果，再通过当前 `apps` / `exec({app,source})` 执行，保留失败以推动 DTW 修复。目标基准为 [computer-use-v1.2](scenarios.json)：42 项，其中首批自动化 core 业务 16 项，另有虚拟光标、真实光标与后台焦点三项强制目标。完整基准包含尚未执行或尚未支持的目标；core 通过不代表 42 项全部通过。v1 的 39 项原始目标保留，v1.1 追加 P01/P02，v1.2 按前台占用反馈追加 P03。

## 界面和技术栈

| 部分 | 实现 | 信息与交互 |
| --- | --- | --- |
| 原生工作台 | macOS AppKit；Windows WinForms | 共用任务清单与 runner 生命周期。Windows 复用原有 `Fixture.cs`，增加 190 个按钮、独立 Unicode 编辑与提交、checkbox、超过 20k UTF-16 的只读文档、原生菜单、owned modal 和真实 Open/Save；AppKit 另外覆盖重复名称、滑块、并列滚动 |
| 原生消息工作台 | AppKit 自绘 NSView | 30 个房间、240 条消息、时间/状态/反应、滚动、真实独立弹出窗口、Pin/Edit/Reply、Unicode 输入；不提供消息和菜单项级 AX |
| Web 工作台 | 原生 HTML/CSS/JS，在 WKWebView 或 Chromium 中运行 | 240 行、720 个链接/控件、重复 Edit/Reviewed、筛选与分页、嵌套滚动、长文本、异步表单与校验、禁用控件、tooltip、range |
| 可移植消息页 | HTML Canvas + 少量原生输入控件 | 自绘消息和菜单，独立消息动作日志，滚动与 Unicode composer |
| 既有浏览器 fixture | 复用 browser-workbench 实例和源码 | 原库存编辑与文件上传、输入可靠性事件、360-link 遍历、纯图片挑战；不复制或删除原入口 |
| 文件与多窗口 | AppKit 与真实系统面板 | peer 窗口、普通 Close Window、系统 Open/Save、指定临时文件的读写 |

原生消息页用于制造低 AX 条件，不声称复刻 Telegram 实现。Web 页可在 Chromium 中复用，但 WKWebView、Chromium、真实 Telegram 和各系统桌面必须分别验收。Windows 原生 shell 与已有 smoke fixture 共用 C# 源码，不使用旧 IE WebBrowser 冒充 WKWebView/Chromium；Web/Canvas 页由同一 runner 的 loopback 服务交给独占 Chromium profile。Linux 暂用可移植 Web 页。

Windows 使用普通 Windows PowerShell 5.1/.NET Framework 编译，无外部 GUI 依赖：

```powershell
python -X utf8 tests/fixtures/computer-use-app/run.py --output C:\Temp\dtw-super-manual-001 --case windows-manual
```

Windows 启动普通自有 WinForms 窗口，会按系统正常规则取得前台；不继承 macOS 的后台启动或脚本级前台归还承诺。关闭窗口或创建 `stop` 文件正常收尾。Windows 直接脚本核心入口是 `tests/acceptance/windows/computer_use_probe.py`，与三项原有回归探针互补；报告保留全部 42 个目标，未执行项仍为 `not_run`。本轮整合没有宣称 Windows 已实现 AppKit 自绘消息页、原生 WebKit、Dock 或无窗口系统根。

## 手工运行

macOS 14.2+、Xcode/Swift、Python 3.11+，不需要 npm 依赖或联网加载脚本：

```sh
python3 tests/fixtures/computer-use-app/run.py \
  --output /tmp/dtw-computer-use-manual-001 --case manual-001
```

程序编译并以不主动激活的方式启动唯一 bundle，同时启动两个仅监听 loopback 的自有服务。手工查看时可显式传 `--foreground-preview`；自动验收默认保持目标后台。首行 JSON 给出 app 路径和页面 URL。使用 shell 顶部的 Native controls / Web workspace / Canvas chat 切换；Web 页导航可进入原有 Inventory / Reliability / Workbench vision。

只运行 Web 页，在普通浏览器或独立 Chromium profile 中打开返回的 URL：

```sh
python3 tests/fixtures/computer-use-app/run.py --web-only \
  --output /tmp/dtw-computer-use-web-001 --case web-001
```

每次使用新目录。正常退出 APP、按 Ctrl-C，或创建输出目录中的 `stop` 文件，runner 都会清理其自有资源。`command` 文件只接受 `quit`，用于收尾，不提供业务操作捷径。未正常退出、强制终止和清理错误会使验收失败。

## 当前 MCP 自动基准

先构建并冻结当前 DTW 二进制。不要在验收期间覆盖它，也不要把已安装版本当成工作区代码：

```sh
python3 tests/acceptance/computer_use_acceptance.py \
  --dtw /absolute/path/to/frozen/dtw \
  --output /tmp/dtw-computer-use-core-001 --case core-001 \
  --source-revision <revision-or-development-candidate>
```

core 包含首次无 look 输入、噪音 AX 定位、Unicode 提交、完整长文本分页、checkbox、滑块、定点滚动、只读菜单、peer 窗口、系统 Open 取消、低 AX 消息/Pin/Reply 和 WebKit 的输入/噪音/长文本。它使用当前 MCP AppClient；坐标来自 fixture 的独立几何记录，是直接脚本证据，不能当成真实模型识图通过。

标准入口同时编译只读观察器，检查 P01/P02/P03。P01 在真实 click/move/drag/scroll 后验证授权 helper 的真实窗口位置和渲染像素；P02 连续采样真实系统光标，并逐段核对原生借用区间。当前 `observed-pointer-borrow-v5` 允许有证据的 `native_hover_tracking` move、`addressed_event_api_unavailable` 指针动作及标准 Open 面板的 `native_open_panel_click`；每段还必须证明结束归位。AX 缺失、普通错误、权限/身份/几何失败都不是借用理由。一次合法 hover 不会放行同脚本其他区间的位移。最大采样空档只作诊断，不设 50ms 硬门槛；实际未知 PID、缺少采样或边界证据仍独立标为不足，不因端点相同就宣称全程独立。功能结果、观察质量及调用外未归因位移分别报告。

低层 `computer_use_benchmark.py` 保留为业务/fixture 诊断入口；不单独证明光标策略。标准报告中的 `acceptance_passed` 同时要求 core、P01 虚拟光标、P02 真实光标策略和 P03 后台焦点合约通过。AXPress、键盘和 AXPosition 不伪造鼠标轨迹。

C01 还要求检查实际保存的 `cursor/probe/mcp/native-chat.png`：房间与消息应可见，Release check 137 清晰可读，无关窗口不得替代目标。自动部分通过后仍记录为 `pending_review`。检查原图后显式追加复核记录：

```sh
python3 tests/acceptance/review_computer_use_capture.py \
  --report /tmp/dtw-computer-use-core-001/report.json --case-id C01 \
  --reviewer <reviewer> --outcome passed \
  --notes '<observed rendered content and limits>'
```

复核绑定原 PNG 的 SHA-256；图像被替换时拒绝。复核失败和后续复核都保留，不能将未检查图像直接标为通过。自动 runner 在仍待复核时返回非零，报告区分 `core_automation_passed`、`core_passed`、`acceptance_passed` 与 `target_baseline_passed`。恢复后的业务效果可通过，但原始工具错误会保留并使 `clean_run:false`，不能通过严格 core/acceptance。

P03 使用一个独立的自有小窗口作为前台见证者，另开采用默认 `auto` 策略的当前 MCP 连接操作后台目标；独立焦点入口可传 `--input-policy background-only` 作严格策略对照。测试准备阶段只显式激活见证者，目标保持后台。它核对实际 UTF-16 全选范围、Unicode 替换、见证者零按键/修饰键事件，并连续采样前台 PID 与激活通知；失去见证前台条件就停止，不重新抢回前台。普通 AX 错误、未证明的快捷键、菜单/内容读取不完整不会触发焦点准备；未验证的后台 hover 干净拒绝。

标准入口使用 [cursor_audit](../../acceptance/cursor_audit.py) 的 `--assert-policy` 验证有界借用；更严格的 `--assert-independent` 另行要求没有真实光标位移。复用已编译的观察器可传 `--observer`、`--overlay-observer` 和 `--focus-observer`；通用独立探针用法见 [构建文档](../../../docs/building.md#current-external-client-gui-probe)。采样必须保留实际间隙；前后台的两个端点不证明全程焦点稳定。P03 覆盖受控见证条件下的后台合约；R04 的更多应用/动作与并发用户键盘目的地仍是单独目标。

上述直接客户端 runner 不证明 DSH 已加载同一插件或实际模型能完成任务。完整本地候选经 Cordis 接入 DSH 的宿主链路、自然语言任务、独立观察与待测矩阵见 [DSH 实机验收方案](../../../docs/dsh-cordis-acceptance-plan-2026-11-05.md)。方案不是已完成的验收结果。

## 单项复验

扩展/边界/系统目标和任意 core 可在 fresh fixture 中单独执行：

```sh
python3 tests/acceptance/computer_use_acceptance.py \
  --dtw /absolute/path/to/frozen/dtw --scenario C04 \
  --output /tmp/dtw-computer-use-C04-a01 --case C04-a01 \
  --source-revision <source-identity>
```

重跑使用新的输出目录。单项入口仍记录完整目标清单，未选目标保持 not_run；`targeted_acceptance_passed` 只证明该项效果、错误分级、清理与严格真实光标，不会使完整 baseline 通过。确定的预期拒绝须精确对应 execution ID、纯 not_dispatched、无 focus preparation/interrupted，原始 tool error 与 clean_run 仍保留；unknown 不属于豁免。

默认保持后台。只有已有同项目后台失败证据并经调用者核对后，才可显式使用 `--fixture-foreground --background-failure <原始报告>` 建立前台诊断条件；报告保留原报告 SHA256、实际配置与前台条件。这不增加产品自动激活许可，也不能替代后台通过。

B05 异步启动后会在 `cursor/probe/B05-answer-request.json` 发布实际 PNG 路径和 SHA256，最多等待 `--vision-answer-timeout` 秒（默认 180，最大 600）。实际检查该 DTW PNG 后，将 `{capture_sha256,answer,reviewer,input_point:{x,y},notes?}` 写入同目录 `B05-answer.json`；输入点为 PNG 像素。用例根据当前已观察的字段 press 能力或该像素点聚焦，公开读回答案后只提交一次，之后才读取隐藏 judge。超时为 blocked；旧图片答案、未知输入与失败不自动重试。

## 独立效果和失败规则

- `native-state.json`、`native-events.jsonl`：真实控件、消息和窗口效果、实际输入事件、窗口与几何。`native-reference.txt` 是原生文档的私有全文判据。
- `web/state.json`、`web/events.jsonl`：Web 实际提交、每次动作、消息、表单和渲染几何；每次相同内容提交仍是独立效果。
- `workbench/state.json`：复用库存、保存、附件、可靠性及纯图挑战的原有独立判据。
- `report.json`、MCP 原始 transcript、protocol metrics、PNG：记录二进制/源码/目标哈希、各项状态、原始错误及清理。失败不会被下一次运行覆盖。

这些私有文件只供 supervisor 准备和验收。acting Agent 只能通过 DTW 的 GUI 操作完成任务，不读取 fixture 源码、状态文件、HTTP API、DOM/CDP 或几何答案。真实模型图片挑战的答案只在私有状态与 PNG 像素中，每次独立运行重新随机生成。

长文本同时检查 UTF-16 偏移连续性、总长度和全部内容的 FNV-1a 64 位指纹；该指纹用于检测内容差异，不是安全认证。checkbox 等会同时核对最终值和实际动作次数，避免重复操作偶数/奇数抵消。

`failed`、`blocked`、`unsupported`、`pending_review`、`not_run` 都不能通过完整基准。前置导航失败后，依赖其模式的场景标为 blocked，不继续向错误界面输入。未知效果不自动重放。源码单测、脚本集成、人工/模型视觉复核、原生平台和安装包证据分别记录。

## TDD 推进顺序

1. 保持 `scenarios.json` 的目标稳定，为新任务补独立效果判据。
2. 用固定二进制运行，保留初始失败；先判断是 fixture、runner、DTW 还是环境问题。
3. 对确证的 DTW 问题增加最小回归，再修拥有该契约的实现。
4. 重跑受影响任务和 core，保留原失败与新候选身份；扩展/系统/真实模型任务逐项补齐。
5. 场景全部完成且平台与安装包证据闭环后，才更新完整基准的支持声明。

首轮执行和修复证据见 [首轮基准记录](../../../docs/computer-use-benchmark-2026-11-04.md)；剩余 23 项、47 次单项尝试及当前限制见 [扩展验收记录](../../../docs/computer-use-extended-acceptance-2026-11-04.md)。

## 无桌面检查

```sh
python3 -m unittest discover -s tests/fixtures/computer-use-app -p test_web_server.py
python3 -m unittest discover -s tests/acceptance -p 'test_computer_use*.py'
python3 -m unittest discover -s tests/acceptance -p test_cursor_audit.py
node --check tests/fixtures/computer-use-app/web/app.js
node --check tests/fixtures/computer-use-app/web/canvas.js
```

HTTP 控制覆盖重复提交、并发、非法请求、私有判据隔离、UTF-16、日志失败及半截 HTTP headers 的有界清理；报告控制覆盖遗漏目标、未复核截图、强制清理与原始失败保留。普通 CI 只执行这些无桌面检查，不自动启动 APP 或派发桌面输入。
