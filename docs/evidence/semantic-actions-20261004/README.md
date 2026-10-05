# P1 语义动作独立实机验收

后续 Windows 日常任务范围见 [2026-10-05 证据](../windows-20261005/README.md)，本页语义状态业务的 Windows 专项仍待验证。下文保留当时 macOS 轮次的状态。

2026-10-04，轮次 `20261004T065658Z-be2049`，macOS 27.0.1 arm64 / Go 1.26.8。每项新受控应用、新前台输入应用、新 `dtw` helper、唯一窗口标题与应用日志。操作全部经过 `dtw` / host SDK，AppKit/WebKit 独立回调证明真实业务；没有 DOM 自动化或 AX action shim。

由本批未提交源码构建，base commit `6e0f06e`、vcs.modified=true。环境/helper 哈希见 [summary.json](summary.json)，[源码 SHA256](source-sha256.json) 保存时逐项与工作区校验一致；它不是 main 的已发布包。保存的 jsonl 只含合成 fixture 内容，去除临时 PID，保留应用时间戳。

| 独立 feature | 结果 | 时间 / 后台 SDK 数据调用 / host.Content bytes |
| --- | --- | --- |
| [F6 set_selected](f6-test.txt) | Order A true→false→true，重复 true 为已验证 no-op，原有 B 保留，实际提交 A+B；不支持的按钮拒绝且不触发回调 | 3.96 s / 12 / 17,717 |
| [F7 set_checked](f7-test.txt) | 标准 AppKit checkbox 1→0→1，重复 true 无额外回调，实际批准 order-42；mixed 为 unknown，拒绝 toggle 且无 mixed 回调 | 4.01 s / 14 / 20,298 |
| [F8 scroll_into_view](f8-test.txt) | 系统 WebKit 的远处订单从 offscreen=true 变为 false，应用 scroll 事件一次，重复调用 no-op；再由 AXPress 完成 order-900 | 4.12 s / 11 / 16,914 |

每项业务回调时间均落在前台首/末字符之间，证明实际重叠。前台 `Human-🙂-input` 完整并提交一次，焦点/指针未变化，后台无 key_down/pointer 或 fallback 事件。拒绝与业务收据通过原请求恢复，RunID 不变。日志分别见 f6/f7/f8-background.jsonl 与对应 human.jsonl。

同轮回归：[F1](f1-test.txt) 3.13 s、6 次 / 12,420 bytes；[F2](f2-test.txt) 0.61 s、6 次 / 12,004；[F3](f3-test.txt) 1.15 s，窄查询 value getter 0 次，显式宽读取 12 次，任务 6 次 / 15,062，对照另 1 次 / 7,367。

费用包括后台实际发现、目标详情、业务操作、no-op/false 验证及未支持拒绝；F7 另包括 mixed 查询和拒绝。前台模拟输入 helper 不计入后台调用量。没有运行 LLM，这些是 SDK 请求数和单份 host.Content 文本体积，不等于 LLM tokens；启动 hello、可信授权控制、cached Reconcile 和 structuredContent 重复体积未计入。完整测试耗时包括前台输入。

按需 schema 的实测大小见 [schema-bytes.json](schema-bytes.json)：入口 157 bytes，逐动作约 5.2–5.6 KiB，完整 act 约 15 KiB。普通发现仅 name/role，选中目标才请求 states/capabilities；WebKit 异步 AX 树最多 8 次窄范围发现，单次深度 12 / 128 节点 / 4 KiB，有界重读不会扩大桌面范围。

[check.txt](check.txt) 记录 Go race/vet、Windows amd64 交叉构建/vet、9 个协议示例和 18 个 JavaScript 测试。契约回归覆盖 false/参数缺失、no-op、强制验证、不满足状态、mixed 计划整份拒绝、未支持不发送及 unknown delivery 不重放。CI 和实机分别标注；全部 Windows 实机验收与适配后置，当前不承诺 Windows 可用。AppKit fixture 和 WebKit 证据不证明所有第三方 provider 兼容。

复跑：`python3 scripts/accept-features.py F6 F7 F8 F1 F2 F3`。脚本不申请 OS 权限，只清理本次创建的应用实例。本轮不创建 release、不更新 caelis-bot M0。
