# Windows 适配与实机验收

2026-10-05，在本机 Windows 11 x64 已登录桌面完成 Chrome、记事本、计算器、Win32 和 Electron 的真实 dtw 操作。稳定性收敛补齐状态语义、后台短事务、取消/预算/用户切换、遮挡拒绝后的恢复、窗口捕获、应用退出及 helper Epoch 隔离。当前为 RC 验收候选；跨平台确认仍需 [同提交 Mac 复验](rc-validation.md)。本轮生成本地验收包，不自动发布版本。

## 环境与证据

| 项目 | 实测值 |
| --- | --- |
| 系统 | Windows 11 家庭中文版，10.0.22631，amd64，普通用户交互桌面 |
| 显示器 | 单屏，2560 × 1440 物理像素 |
| 工具链 | Go 1.26.8、Node.js 26.2.0、Python 3.11.9；GOWORK=off |
| 浏览器 | Google Chrome 154.0.8037.93 |
| 日常应用 | Notepad 11.2607.14.0、Calculator 11.2607.0.0；本机提供英文 UIA 标签 |
| Electron | 44.5.1 / Chromium 152.0.7977.130，独立测试 profile |
| 稳定性源码基础 | `1e43d86161f06060db9d47cb053cce163d31515f` 上的工作树，dev / vcs.modified=true |
| 稳定性 helper SHA256 | `de58aca2d0103a0ad6f39ef72d9fc286389ced96675b4f50fae6237df31af2dd` |

首次适配的证据保留在 [Windows 初始记录](evidence/windows-20261005/README.md)，本轮补充见 [稳定性记录](evidence/windows-stability-20261005/README.md)。源码指纹及二进制哈希明确区分每轮构建；最终干净提交包的实际 `version`、manifest 和复验记录位于本机 `artifacts/release` 及新验收目录，不把旧 dev 二进制当成 RC 包。

## 日常真实任务

所有桌面效果经持久 dtw helper、Go SDK 或 JavaScript 入口完成。未使用 WebDriver/CDP、DOM 填值或剪贴板；页面监听器仅作事件 oracle。启动 fixture、准备文件、构建和读取业务日志由宿主完成。

| 场景 | 实际操作与独立验收 |
| --- | --- |
| Chrome，必选浏览器 | Ctrl+T → loopback URL → 原生 UIA 定位 → 全选/Unicode 输入 → Enter；DOM keydown/input/submit 均为 trusted，提交恰好一次，相同请求返回同一 RunID，get 可恢复原回执 |
| Chrome 稳定性 | 最新日常轮次 `20261005T112104Z-53b834`，20/20 短计划恢复原窗口、焦点与指针；12 轮初始证据另保留 |
| 记事本保存 | 两行中文和 emoji，值及修改状态 verify → Ctrl+S → 标题验证；独立读取磁盘精确一致，UTF-8 / CRLF，并保存 SHA256 |
| 计算器 | UIA invoke 计算 `128 + 256`；原生显示 `Display is 384`，PNG 显示 384；semantic / not_borrowed |
| Win32 键鼠与生命周期 | Unicode、组合键、Enter、点击、拖拽、滚轮；取消后继续操作；重建 EDIT 后旧 Ref 写入拒绝，新控件为空。独立消息日志判定 |
| F4 大树续扫 | 1,000 行后定位 Deep submit，64 节点/次；16 页 / 1013 节点、19 次 SDK 调用；dirty coverage 与独立提交次数核对 |
| F5 managed helper | 私有数据/控制管道授权 → 实际填写/提交 → EndTurn；后续写入拒绝，原回执保留；独立提交一次 |
| JavaScript 会话 | named pipe 两次 exec 沿用 state/Ref，同一 Epoch；原生读取 Chrome URI，局部 complete=true，stop 正常结束；全桌面 uia_partial 不冒充完整扫描 |

## 稳定性与 provider 专项

| 场景 | 独立结果 |
| --- | --- |
| 勾选及 no-op | 标准 checkbox 打开/关闭；再次设置已达状态为 verified / not_applicable，应用勾选回调不增加 |
| 多选 | Shanghai 加入/移除时保留 Beijing；应用用 LB_GETSEL 读取真实选择 |
| 树展开/收起、滚动 | Common Controls v6 的树状态和 Invoice 79 滚动 verified；TVM_GETITEM/GETITEMRECT 确认展开及目标在视口中 |
| 列表滚动 | Destination 79 的 ScrollItem verified；LB_GETITEMRECT 与视口相交，非从 UIA 回执推断结果 |
| 遮挡拒绝与恢复 | 自有覆盖层挡住 EDIT，target_not_hittable / delivery=none；独立 donor helper 观察到原顶层窗口和焦点，前后均一致、Seat ready；移除遮挡后的新样本实际收到一次鼠标消息 |
| Electron 键鼠 | 左/右/中键、双击、移动、250 ms 拖拽、双轴滚轮；DOM 回调验证，鼠标/拖拽/滚轮为 trusted |
| Electron Unicode、多行 | 原生 UIA 定位后短事务输入中文/emoji及 Enter 换行；值 verify 与 DOM 精确文本双重核对 |
| 菜单/弹窗 | 网页上下文菜单和 HTML dialog 在同计划内唯一 bind；Electron 原生菜单快捷键触发真实 native_menu，原生对话框 Confirm 触发 response=1 |
| 32 轮后台事务与去重 | 另一应用保持原前台，每轮填写并提交中文/emoji；32/32 restored，原窗口/焦点一致；相同 ID/正文保持同一 RunID，DOM 总提交次数精确为 33 |
| 预算和取消 | 258 UTF-16 文本及 501 ms 拖拽在激活/投递前拒绝；取消拖拽查询原 RunID 至清理完成，重复请求不重投；2500 ms 验证等待在一秒输入预算后停止并恢复，input_lease_expired |
| 用户切换 | 第二 helper 在事务内将第三应用置前台；原任务 user_interrupted / user_superseded，第三应用继续收到 THIRD-KEPT，未强行恢复旧应用 |
| 应用退出/旧对象 | 自有 Electron 退出后原任务终止、Seat ready；旧窗口的新写入 ref_gone / delivery=none |
| helper 重启 | 仅在无持有输入的终态后终止自有 helper；新进程 Epoch 不同，旧 Ref 拒绝。不宣称强杀能清理尚持有键鼠或恢复跨进程去重 |
| 窗口 PNG | Chrome、Notepad、Calculator、Win32 和 Electron 图像实际查看；Win32 调整尺寸更新 geometry_version，隐藏/最小化明确拒绝，应用恢复并完成重绘后重新捕获正确内容 |

固定单机的 32 轮测量为每轮约 300–338 ms，验证等待预算耗尽的一轮约 1064 ms；这不是通用实时上限或性能保证。原生调用与恢复可能超过输入预算。详细回执记录各轮实际时长。

## 收敛修复

- 前台 HWND 规范化到顶层窗口，修复 UIA SetFocus 后子 HWND 导致 Seat 归属未知、共享键盘误拒绝的问题。
- SetForegroundWindow 的实际窗口确认优先于返回值；必要时请求已验证对象的公开 UIA SetFocus，允许短暂空前台完成交接。保留 Windows 前台限制，不注入 Alt、AttachThreadInput 或修改全局设置。
- 借用前保留原 UIA 焦点，无法保留则预先拒绝；恢复先短暂等待自然焦点确认，再按需请求，并保留具体 restoration_reason。输出压缩仍保留恢复诊断。
- 一秒预算统一为 input_lease_expired，验证等待立即停止与清理；契约回归确认后续输入 skipped，而非等待整个较长验证期限。
- UIA 命中外增加实际 WindowFromPoint 顶层 HWND 检查，避免 Chromium provider 在其他窗口覆盖时仍返回自己的节点。
- 语义调用失败保留 native HRESULT；不将语义失败变为键鼠重试。测试日志使用互斥保护并发追加，避免 oracle 自身丢事件。
- 窗口捕获按目标窗口自身的 DPI 上下文渲染，再映射到物理窗口尺寸；本机 125% 缩放下 DPI-unaware Win32 图像不再出现因渲染尺寸不一致产生的多余黑边，保留几何与像素预算检查。
- 快速观察曾达到 128 个同步基线容量并中断持续任务。完成基线改为有界缓存，最旧游标明确要求 reset，新的观察可继续；回归验证 160 次快速 snapshot、旧游标 reset、新游标差量及未消费分页仍有效。Ref、授权和输入 RequestID 去重记录不因此淘汰。

## 兼容性边界

旧版 Common Controls v5 TreeView 曾在公开 ScrollItem 能力后返回超时（`native_action_failed` / `0x80131505`）。基线 helper 同样复现；增大 timeout 未解决，未保留该变更。fixture 通过本进程 manifest activation context 使用 v6 后，树滚动和独立状态检查通过。该设置只属于测试应用，不给 dtw 建立自动 provider 后备机制；旧 provider 的原失败回执保留。

Electron 原生应用菜单在本机 UIA 中没有 menu_item；菜单命令通过其实际快捷键验证，不能称为原生菜单项 UIA 点击通过。网页上下文菜单是另一项已验证场景。模态过渡可能重建 Chromium 节点，应结束后重新观察，再制定新的计划；脚本 bind 限制 enabled/offscreen 和唯一性。某轮旧节点导致 ambiguous_target，另一次连续任务复用旧 Ref 返回 ref_stale / delivery=none，均恢复前台并保留原回执，没有重放；后续独立任务每轮先重新观察。

初始阶段 Chrome 标签直点出现过 target_not_hittable 且前台恢复未确认，原 unknown/fenced 回执仍在本机 `artifacts/windows-owned-cleanup-2`。本轮受控遮挡专项验证新的恢复路径通过；任意浏览器标签几何、同一应用内部用户切换和 provider 树仍需逐任务观察，不能从恢复成功推导所有目标都可点击。

图像复核曾发现 fixture 从隐藏状态恢复后，过早发布恢复事件会得到仅标题栏的 PNG；应用现先完成 [RedrawWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-redrawwindow) 重绘，再发布该事件，复验图像内容正确。dtw 不根据任意黑色像素猜测是否为合法画布，也不替应用强制重绘。同步 [PrintWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow) 仍依赖 provider，不保证任意 GPU/保护窗口产生有效新帧。

## 复现和发布范围

```powershell
$env:GOWORK = 'off'
$env:PYTHONUTF8 = '1'
.\scripts\check.ps1
python scripts/accept-windows.py --browser-window '当前 Chrome 窗口标题 - Google Chrome' --rounds 20
python scripts/accept-features.py F4 F5
python scripts/accept-windows-stability.py --electron 'C:\path\to\electron.exe' --rounds 32
```

脚本默认构建当前源码，`--helper` 指定现有包内二进制。每轮使用新标题、文件、日志与 loopback 端口，不覆盖旧证据；完整 wire/桌面截图只保留本机忽略目录，仓库保存合成应用日志、回执摘要及自有 fixture 图像。日常脚本编辑新建文档、改变计算器计算并打开自己的表单标签；语义/稳定性脚本只结束本轮创建的 fixture/helper。失败轮次同样保留，不重放未知业务操作。

shared Win32 SDK fixture 的启动不保证获得 Windows 前台权限；脚本先通过 dtw 的公开 UIA focus 显式聚焦自有 EDIT，再运行独立 SDK 任务。一次初始窗口激活被系统拒绝的 needs_user_focus / delivery=none 回执已保留，后续步骤全部 skipped；新的任务样本在完成该前置准备后通过。此准备不注入 Alt 或改变 OS 前台规则。

`check.ps1` 已通过 Go race/vet/build、headless/embodied 示例、9 个 JSON 协议示例和 19 个 JavaScript 测试。发布范围限 Windows 11 amd64、普通权限、当前单屏及上述 provider；更多机型、多屏缩放、RDP/锁屏/用户切换、复杂 IME 和实际 Bot/Wails 联调未形成实机矩阵。Mac 必须基于最终同一 commit 复验，不能用历史成功直接确认跨平台 RC。最终打包、Mac 命令和验收条件见 [rc-validation.md](rc-validation.md)。
