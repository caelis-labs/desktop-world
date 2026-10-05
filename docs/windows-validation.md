# Windows 适配与实机验收

2026-10-05，在本机 Windows 11 x64 的已登录桌面完成。当前源码已通过 Chrome 表单、记事本编辑保存、计算器、Win32 原生键鼠与生命周期、UIA 大树续扫及 managed helper 授权验证；新增 Windows cooperative 短前台事务。README 已按产品使用指南重写。本次没有发布新版本，不能把历史下载包当作本次构建。

## 环境与构建

| 项目 | 实测值 |
| --- | --- |
| 系统 | Windows 11 家庭中文版，10.0.22631，amd64，普通用户交互桌面 |
| 显示器 | 单屏，2560 × 1440 物理像素 |
| 工具链 | Go 1.26.8、Node.js 26.2.0、Python 3.11.9；GOWORK=off |
| 浏览器 | Google Chrome 154.0.8037.93 |
| 日常应用 | Windows Notepad 11.2607.14.0、Calculator 11.2607.0.0；本机两者提供英文 UIA 标签 |
| 基础提交 | `887b863809f2ba532675668e37e91bde4dac758d`，在其上修改的源码，vcs.modified=true |
| 验收 helper | dev / windows-amd64；SHA256 `c76e3fd6bc62c200f1b603e2b8f464f35e9f35d3d3c7f9dfc5bc8b1794dee76a` |

日常任务轮次为 `20261005T095545Z-d22052`（北京时间 17:55）；F4/F5 为 `20261005T095624Z-d9e9cc`。构建版本、源码指纹、任务回执及独立业务日志见 [可审查证据](evidence/windows-20261005/README.md)。

## 真实任务结果

桌面操作通过持续运行的 dtw helper、Go SDK 或其 JavaScript 入口执行。浏览器没有使用 WebDriver/CDP、DOM 填值或剪贴板；页面自身的监听器只记录收到的事件。应用启动、测试文件准备及构建由宿主执行。初始 Chrome 窗口通过浏览器工具打开，之后的新标签、导航、填写、提交和键鼠恢复均由 dtw 操作。

| 场景 | dtw 实际操作 | 独立验收与结果 |
| --- | --- | --- |
| 浏览器表单，必选 | Ctrl+T → 本地 URL → 原生 UIA 查找 → 聚焦/全选 → 中文与 emoji 输入 → Enter | 输入 `Windows 日常表单验收 🌍`；DOM keydown/input/submit 均为 trusted，submit 恰好一次；相同 ID/正文返回同一 RunID，get 可恢复原回执；PNG 可见 submitted:1 |
| 浏览器前台事务稳定性 | 12 轮点击输入框、Ctrl+A、Unicode 输入；每轮结束后读取 Seat | 12/12 restored；原前台窗口、焦点 Ref、指针 x/y 均一致；每轮前台占用 249–274 ms。这是固定单机场景的测量，不是通用性能保证 |
| 输入预算 | 请求 129 个 emoji，即 258 个 UTF-16 单元 | 激活/投递前拒绝，input_burst_limit，delivery=none；没有换通道重发 |
| Canvas 边界 | 尝试绑定像素文字对应的语义按钮 | 返回 ambiguous_target，无按钮绑定与点击；零匹配不是对像素的识别 |
| 记事本编辑保存 | 已创建测试文件 → 原生编辑器聚焦/全选 → 两行中文及 emoji → Ctrl+S | 输入全文和修改状态先完成 verify，再验证保存后的标题；独立读取磁盘文本精确一致，UTF-8 / CRLF；文件及 SHA256 保留 |
| 计算器 | UIA invoke 清除及数字/运算按钮，计算 `128 + 256` | 原生显示 `Display is 384`、PNG 显示 384；channel=semantic，not_borrowed |
| Win32 标准控件 | 真实 Unicode、组合键、Enter、点击、拖拽、滚轮；取消拖拽后继续操作；替换 EDIT 后访问旧 Ref | TestNativeFixture 通过；独立消息日志确认键盘提交一次、指针提交一次；取消释放已持有按钮，旧 Ref 写入拒绝，新控件为空；可见区域 PNG 留存 |
| F4 大树续扫 | 每次 64 节点，跨 1,000 行发现 Deep submit，语义填写并提交 | 16 页 / 1013 节点，19 次 SDK 数据调用 / 21655 文本投影 bytes；业务提交一次，保留 dirty coverage |
| F5 宿主管道 | host.Start → Grant → 填写/提交 → EndTurn → 后续写入/原回执查询 | 私有控制/数据管道实际运行；业务提交一次，结束回合拒绝新写入，原回执保留；4 次数据调用 / 7725 bytes |
| JavaScript 持久会话 | named pipe 上两次 exec，第二次沿用第一次的窗口 Ref/state，原生读取 Chrome document URI | 同一 Epoch 保持状态，局部观察 complete=true；得到实际 loopback URL；stop 正常结束。全桌面扫描出现 uia_partial 时保持 incomplete，并缩小到已观察 Ref，没有伪装为完整扫描 |

另通过 dtw 在 Chrome 打开并观察了 Go 安装文档、读取原生页面 URL 和捕获图像，作为探索性真实网页验证。该导航发生于修复前的一轮，恢复故障回执仍保留；正式通过结果以上表的新轮次为准。

## 平台功能对齐

| 功能 | Windows 当前状态 | 与 macOS 的关系 |
| --- | --- | --- |
| World/Actor、授权、回合撤销、回执/去重 | 共享实现；Windows 原生 managed task 通过 | 相同契约，Windows 私有匿名管道对应 Unix FD |
| 原生发现、字段/状态、Ref 生命周期 | UIA 原生实现，Chrome/Notepad/UWP/Win32 实测 | AX 对应 UIA；不同应用的原生树结构和支持能力仍不同 |
| 有界观察与 continuation | Windows F4 实测通过 | 使用真实 UIA frontier；live 树续扫明确标记 dirty |
| set_value / invoke | 浏览器、计算器与 Win32 实测通过 | 相同语义接口，不自动转为物理输入 |
| set_expanded / set_checked / set_selected / scroll_into_view | UIA pattern 已实现，契约测试通过；各自 Windows 业务专项尚未验收 | macOS 已有对应独立业务证据；本次不据此声明 Windows 全部状态场景通过 |
| 键鼠与 primary、Unicode、多行文本 | 真实操作通过；primary=Ctrl，CRLF 单个 Enter，Tab 为实际按键 | 与 macOS 对齐动作参数及完成条件；应用可能将 Enter/Tab 用作提交或跳转 |
| cooperative | 新增并通过 Chrome/Notepad 短计划及 12 轮恢复 | 相同预算、channel 和恢复回执；Windows 采用公开前台 API，macOS 使用其平台机制 |
| 保护字段及输入限制 | 沿共享契约，Windows 重新核对归属、进程实例与保护状态 | 保留 unknown/redacted；UIPI 与 OS 前台限制不能绕过 |
| visible_region / window_content | Win32 可见区域、Chrome/Notepad/Calculator 窗口图像实际查看通过 | Windows PrintWindow，macOS SCK；窗口局部图像不能授权桌面点击 |
| URI 与标准控件角色 | 补齐 UIA 角色；Chrome document URI 实测 | 按需读取 provider 暴露的只读 URL，缺失保留 unsupported/unknown |

## 修复内容

- 为 Windows 增加前台事务：保存原窗口、焦点及指针；目标进程生命周期、归属、焦点与命中检查；一秒输入预算；取消/恢复清理；恢复失败仍 fence。
- 输入与清理统一采用线程 DPI awareness，修复恢复指针时的一像素偏差。焦点只在实际变化时请求，并有界等待 UIA 更新，修复 Notepad 的异步焦点与重复 SetFocus 故障。
- Unicode 以完整键对及完整 surrogate pair 投递，在事件边界检查取消/前台变化；仅清理由本次已接受输入持有的键/按钮。CRLF 不重复按 Enter。
- 应用名称改为实际进程名称；UWP 控件绑定到真实顶层 HWND，并分别检查内容进程与宿主窗口生命周期，修复 Calculator 的 ref_stale。
- 窗口渲染使用 PW_RENDERFULLCONTENT，修复 Chrome 黑图；继续拒绝隐藏/最小化/非交互桌面等不可捕获状态，不用桌面 blit 代替窗口图像。
- 验收定位加入角色以区分同名 STATIC/EDIT；Win32 fixture 明确支持 Ctrl+A，替换 EDIT 时保留原标签顺序；记事本验收使用不带末尾换行的种子，避免旧末尾段落干扰精确覆盖判定。

Windows 前台限制依据 [SetForegroundWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow)，线程 DPI 切换依据 [SetThreadDpiAwarenessContext](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setthreaddpiawarenesscontext)。[PrintWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow) 是同步、由应用渲染的调用；不能从三个应用成功推导任意 GPU/provider 都支持。

## 复现

打开 Chrome 并取得当前精确窗口标题，在已登录的 Windows 11 amd64 桌面运行：

```powershell
$env:GOWORK = 'off'
$env:PYTHONUTF8 = '1'
.\scripts\check.ps1
python scripts/accept-windows.py --browser-window '当前窗口标题 - Google Chrome'
python scripts/accept-features.py F4 F5
```

`accept-windows.py` 默认从当前源码构建 dtw 和 fixture。每轮新建文件、标题、日志及随机 loopback 端口，不覆盖旧证据；`--helper` 可指定现有二进制，`--rounds` 选择 1–20 轮，默认 12。脚本会编辑新建的记事本文件、改变计算器当前计算，并打开一个测试浏览器标签；最后停止自己的 fixture/helper，文件与页面结果保留供检查。应用的英文标签与本机相符；其他语言可通过 `--editor-name` / `--calculator-window` 调整名称，计算器按钮及显示文本须先观察后适配脚本中的准确标签。

完整产物在 `artifacts/windows-acceptance-*`、`artifacts/feature-acceptance-*`。wire 和截图可能包含桌面应用信息，留在本机忽略目录；仓库只保存合成任务日志与摘要，不公开整桌面观察及用户浏览器截图。失败轮次也保留，未知效果通过原回执和只读观察核对；没有重放不确定动作，新复验使用新的任务样本。

验收后清理失败样本时，直接点击 Chrome 标签还出现了一次 `target_not_hittable`，该次前台恢复也未能确认。回执保持 unknown/fenced，点击 delivery=none、关闭步骤 skipped，完整日志在本机 `artifacts/windows-owned-cleanup-2`；没有重放这份计划。确认当前窗口后，新的标题前置条件和键盘计划成功关闭 8 个已知失败标签。成功表单、记事本文件及计算器结果保留；本轮创建的 Win32/loopback 服务进程已停止。这项额外结果说明复杂标签命中与前台恢复仍需更多 provider/用户介入验证。

## 自动检查与剩余范围

最终 `scripts/check.ps1` 通过：Go race、vet、Windows 原生 build、headless/embodied 示例、9 个 JSON 协议示例及计划/部分投递示例、19 个 JavaScript 测试。真实操作通过独立 opt-in 脚本验证，普通 go test 默认不发送桌面输入。

本次未在 macOS 主机复跑。更多 Windows 浏览器/Electron、语义状态业务、多屏混合缩放、IME、用户介入压力、锁屏/RDP/用户切换、捕获完整 F9 环境矩阵、Windows arm64 和实际 caelis-bot/Wails 集成仍未形成完整验收。本机只验证单屏、当前普通权限及上述应用。Windows 受 SetForegroundWindow 与 UIPI 限制，拒绝时明确返回 needs_user_focus；系统级独立输入设备、OCR、自动视觉定位和事件推送不属于当前能力。
