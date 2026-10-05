# 验收记录

日期：2026-09-29（以下为当时的 macOS 验证记录）。2026-10-05 新增 [Windows 实机适配验收](windows-validation.md)，覆盖 Chrome、记事本、计算器、Win32、UIA 续扫和 managed host。尚未完成 SPEC 的全部环境矩阵；历史记录不等于当前所有平台状态。

## 环境

- macOS 27.0（26A428），arm64；Go 1.26.8。
- 原生测试在已登录桌面、脱离开发沙箱的宿主进程中运行。系统返回 Accessibility、输入和屏幕捕获权限已授予；本轮未更改系统权限。
- 目标为本项目构建的独立 AppKit 应用，窗口标题 `Desktop World Native Fixture 20260929-F`。除只读窗口发现外，写操作限定在这个窗口及明确选定的控件。
- 验收由 Desktop World 自己的 AX / CGEvent / ScreenCaptureKit 后端执行。fixture 用自身事件处理和业务回调生成日志，不使用 Desktop World 的观察结果作为提交证据。

## 真实操作结果

`TestNativeFixture` 于 21:49（Asia/Shanghai）通过，耗时 3.40 秒，启用了截图与取消测试。

| 场景 | 证据与结果 |
| --- | --- |
| 发现、窗口范围、Anchor | 按唯一标题发现窗口；取得文本控件与几何锚点；后续 Actor 限定窗口范围 |
| 六步输入计划 | 聚焦窗口、聚焦控件、语义设值、Cmd+A、Unicode 输入、Enter；应用收到 `Desktop World 验收 🌍!` |
| 请求去重 | 重复完全相同的 RequestID / Plan，返回相同 RunID；键盘阶段独立日志只有一次提交 |
| 指针操作 | move / click / drag / scroll 完成；点击带来第二次且仅第二次提交 |
| 拖动取消与恢复 | 两秒拖动在 100 ms 步骤期限处停止；原生释放完成后新的 move 执行成功 |
| 事件配对 | 独立日志共 3 次鼠标按下、3 次释放、12 次拖动、1 次滚轮；包括正常拖动和取消拖动 |
| 控件生命周期 | invoke 替换输入框后得到不同 Ref；旧 Ref 的 set_value 被拒绝，delivery=none；新控件仍为空 |
| 屏幕捕获 | 取得真实可见窗口区域的 PNG（500×342），图片中显示正确文本与 `submitted:2`；这是替换控件前的截图 |

fixture 日志总计：ready 1、text_changed 20、key_down 21、submit 2、pointer 19、replaced 1。两条 submit 的文本均完全一致。

本机原始证据（位于忽略的 `artifacts/`，不包含在仓库源码中）：

- [独立事件日志](../artifacts/native-fixture-20260929-F.jsonl)
- [实际捕获图片](../artifacts/native-fixture-capture.png)

复现步骤见 [README](../README.md#真实桌面验收)。每轮使用新的窗口标题和日志文件，避免旧事件影响断言。普通 `go test` 不会发送真实输入。

## 浏览器原生接口与 Canvas

同一主机上的 Chrome 154.0.8037.58（读取本机应用 Info.plist）另行运行 `TestBrowserFixture`，最终 B 轮于 22:00（Asia/Shanghai）通过，耗时 2.65 秒。窗口内原生观察的 coverage.complete=true。

- 页面位于 `127.0.0.1`，自己的 DOM 回调把事件写入独立本地服务；没有向外部网站提交数据。
- Desktop World 通过 AX 发现网页输入框，CGEvent 执行五步计划；网页显示 `submitted:1 — Browser 原生验收 🌍`。
- 独立日志记录 ready 1、keydown 17、input 14、submit 1。输入、按键和提交事件的 `isTrusted=true`；重复 RequestID 未增加提交次数。
- 仅绘制到 Canvas 的 `DW Canvas Secret Action` 没有成为可绑定的语义按钮，bind 返回 stopped / ambiguous_target，delivery=not_applicable，没有生成绑定或点击。
- 首轮出现过 Chrome 原生窗口尚不可发现的情况，测试在写入前停止。只读诊断确认 provider 能返回 fixture 窗口后继续；没有回退到坐标猜测或 DOM 注入。

原始证据：[浏览器事件日志](../artifacts/browser-fixture-20260929-B.jsonl)、[真实可见区域截图](../artifacts/browser-fixture-capture.png)。截图包含当时覆盖在浏览器之上的桌面角色，符合 visible_region 的语义。复现见 [browser fixture](../tests/native-fixtures/browser/README.md)。

## 自动检查

以下检查在最终实现上通过：

- `go test -race ./...`：21 个契约测试、4 个协议测试及 fuzz seeds；真实桌面测试默认跳过，另按上文显式执行。
- `go vet ./...`。
- `GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go build ./...`。
- `GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go vet ./...`。
- 原始交付的 `verify_examples.py`：9 个 JSON 示例、1 个计划、1 个 partial receipt。

契约回归覆盖：不完整发现禁止绑定、Ref 替换、并发请求去重、原生调用超时与输入通道 fence、delivery / verification 分离、逐步授权、收据过期墓碑、Unicode 文本分页、保护字段、稳定观察分页与完整 envelope 字节预算、权限撤销、拓扑失效、焦点干预、取消与 Close 生命周期。

另已运行内存表单示例、具身 Anchor 示例，以及协议解码 fuzz（5 秒，70,730 次执行，无失败）。这些结果不替代原生桌面验收。GitHub Actions 工作流已提供，未推送仓库，未声称远程 CI 通过。

## 验证边界

当时 Windows 仅有交叉构建和静态检查；后续真实桌面运行证据以 [2026-10-05 Windows 报告](windows-validation.md) 为准。macOS 的本轮证据覆盖当前 arm64 主机上的 AppKit 与 Chrome fixture，不能推导最低系统版本、多屏混合缩放、其他浏览器 / Electron 或 Wails 宿主均已通过。

原生变化通知目前采用刷新和 Watch 轮询；AXObserver / UIA event invalidation 及其余正式发布条件见 [实现边界](implementation.md#尚未完成的正式发布条件)。

## 2026-10-04：独立 feature 批次

当前源码 helper 改名为 `dtw`，F1 无共享输入策略、F2 语义展开/收起、F3 原生字段计划各自通过独立 macOS 场景。F1 的后台填写/提交没有打断另一应用持续 Unicode 输入；F2 的期望状态、可见明细和重复 no-op 均由应用独立日志确认；F3 在同一窗口把 12 次慢值 getter 降为 0 次，并完成后续订单提交。源码指纹、操作日志、成本定义和复跑命令见 [批次记录](features.md) 与 [实机证据](evidence/features-20261004/README.md)。

F4 Windows UIA 续扫和 F5 Windows managed 私有管道随后于 2026-10-05 完成 Windows 11 实机验收。M0 进入 caelis-bot 的更新与联调继续后置。

## 2026-09-30：Bot managed helper 收尾验证

在同一台 macOS arm64 主机上，`examples/bot-host` 通过独立 Go host → 私有控制管道 → helper → AppKit 原生应用完成 Unicode 设值和 Enter 提交。活动回合先观察精确应用 Ref，再通过宿主 Grant 授权；相同请求返回相同 RunID，结束回合后的新输入返回 `turn_expired`，原回复仍可 Reconcile。

应用自身日志 `alpha-native-a.jsonl` 记录 ready 1、text_changed 1、key_down 1、submit 1；提交内容为 `Caelis Bot alpha 联调 🌍 20260930-A`。另用 Computer Use 独立读取界面，确认相同文本和 `submitted:1`。日志保留在本机忽略的 artifacts，公开发布不包含桌面原始日志。

新增回归覆盖：跨应用拒绝、模型数据通道不能扩权、回合结束撤销/取消、控制 EOF 撤销、不同回合请求隔离、原回复去重与恢复、输出预算及未知副作用保留。Go host 保留 typed wire，模型输出另行 compact。完整检查通过 `scripts/check.sh`（race、vet、Windows 交叉构建/静态检查、9 个协议例子及 13 个 JavaScript 测试）。

这证明 helper/Go host 接入边界可用于 alpha 联调，不等于实际 caelis-bot/Wails 进程、Windows 真实桌面或易用性目标已经验收。上一轮 Antigravity App 盲测的任务结果、成本和未达目标继续以 [可用性记录](usability-evaluation.md) 为准。
