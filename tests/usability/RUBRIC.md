# 仅评估者阅读

不要把本文件、oracle 或基线盲测报告交给受试 Agent。只打开 participant 目录作为其工作区，粘贴 participant/PROMPT.md。两包分离是实验约束，不是 OS 安全沙箱。

在新鲜副本上开始。宿主预先打开 Finder 到 participant/workspace、启动 TextEdit，并在 Chrome 新开一个本轮标签页（SendInput 文档）。不预先整理文件、修改文本或填写输出。记录准备耗时但不计 Agent 接入耗时。原生 UI 语言、屏幕布局和权限宿主应与基线一致；差异必须注明。Antigravity 的 OS 授权与 Codex 不一定相同，单独记为环境成本。

host.json 默认仅授权本机观察到的“访达”和“文本编辑”。若 Chrome 名称唯一，可加入 --write-app Chrome；存在多个同名实例时，由可信宿主只读 observe summary 后，加入 --write-app-window 和测试窗口精确标题。它授权该窗口所属应用，不是单标签页沙箱。需要截图时由宿主给 --assets-dir harness/captures。不要让 Agent 为恢复方便自行选择 desktop-write/raw-input。

宿主启动 `node clients/javascript/desktop.mjs serve --host host.json`，确认 ready 后开始计时。冻结并记录版本、Node 版本和 host.json；记录宿主准备时间。使用 Antigravity App 的全新对话，通过 Computer Use 提交 prompt，不用 CLI/ACP。同一时间只允许受试者发送桌面输入。记录 App 中可获得的实际 token 用量；不可得时填 null，不能用准备阶段 CLI 数据替代。分别统计 script 次数、底层 API 次数、helper bytes 和模型呈现 bytes。

执行结束、确认停止桌面输入后，从 evaluator 目录执行：

```sh
python3 verify-usability.py . --output verification.json
```

文件验收：SHA-256 校验 3 份移动资料、改名 README、保留 api.go 和原稿；根目录无旧名；新交接文档 UTF-8 且逐字匹配（容许 BOM/CRLF）。13 个文件/格式断言不代表研究质量通过。额外的 .DS_Store 可忽略；任何任务资料的非预期修改需单列。

人工研究验收分别判定：

- SendInput 返回成功插入输入流的事件数，不能据此保证应用业务完成；后者是推论，应明确。[官方来源](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- UIPI 限制向相同或更低完整性级别注入；返回值/GetLastError 不能明确指出 UIPI 是失败原因。[官方来源](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)
- UIA 不保证所有可能事件都发布，标准代理的一些属性变化不会触发事件；不是仅仅“没有订阅就没有事件”。[直接来源](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-eventsforclients)。概览还说明事件也可能不对应实际 UI 变化。

检查 Agent 的调用日志/传输桥有没有绕过 Desktop World。文件产物正确但来自直接 IO/HTTP，实验无效，不算成功。保留 scope 拒绝、输入未知、恢复、人工干预和重试，不事后替受试者修文档再记为通过。

体验门槛：有效观察≤30秒、首次动作≤60秒、完整任务≤10分钟、无上下文压缩、无人工任务提示、无需 Go adapter。25分钟为硬截止。还需给出调用数、错误、返回字节数、模型实际 tokens（未知填 null）和任务正确性；一轮结果不外推总体成功率或 P95。

后台隔离不属于首版 MVP 验收范围。当前 helper 没有独立光标或独立后台焦点，不将其作为已提供能力。
