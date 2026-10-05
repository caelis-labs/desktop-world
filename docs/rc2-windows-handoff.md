# rc.2 Windows 实机验收交接

指定主机：**DESKTOP-90677U0**，Windows 11 amd64、已登录交互桌面。该机器无公网 SSH；由本机操作者执行。交接 Issue 的 `commit` 是唯一验收基线，先检出其完整 SHA；后续代码变更必须重新交接/验收，不能沿用 rc.1 历史结论。

## 构建与自动检查

预装 Go 1.26、Node 24、Python 3.11+、Rust stable、PowerShell 7；Chrome 已登录且可正常操作，Electron 验收 runtime 路径明确。普通用户、单显示器，UIA 与目标 APP 同等或更低完整性级别。清空无关测试窗口，保留工作文件。

```powershell
git fetch origin
git checkout --detach <交接Issue中的完整SHA>
git status --porcelain  # 应为空
$env:GOWORK = 'off'
pwsh -File scripts/check.ps1
pwsh -File scripts/package-prerelease.ps1 -Version v0.1.0-rc.2
```

打包仅生成本地候选，不创建 tag 或发布。验证 `manifest.json` 的 vcs.revision 与交接 SHA 相同、vcs.modified=false、windows/amd64，记录 dtw.exe SHA256、SHA256SUMS、Windows build、工具版本、屏幕/缩放、APP 版本和完整性级别。Windows 包未签名。

## 原生回归与新增 SDK 流程

```powershell
python scripts/accept-windows.py --browser-window '<当前Chrome精确窗口标题>' --rounds 20
python scripts/accept-windows-stability.py --electron '<可信electron.exe完整路径>' --rounds 32
python scripts/accept-features.py F4 F5
python scripts/accept-rc2.py --helper '<解压候选包的bin\dtw.exe>'
```

前两项会构建当前 SHA 的 helper；第三项同样必须在该 checkout。最后一项必须用解压包原生 helper，检查包与源码行为一致。脚本只管理自身 fixture，原生操作统一经 dtw，不通过剪贴板、WebDriver/CDP 或另一个 UIA sidecar 注入。

rc.2 新流程要求全部通过：

- 原生 session 在 APP 未启动时成功，pending 不允许写；APP 启动后完整唯一发现使声明 active。
- 同名多实例保持 ambiguous；精确窗口标题或观察到的 application Ref 能消除歧义。不完整发现不授权。
- `dtw auth add` 在同一 session 动态追加第二个 APP，epoch/旧 Ref/原回执不变；Windows 当前用户 named pipe 正常读写，不使用半关闭 framing。
- 撤销 APP A 后 A 拒绝输入，APP B 仍可写；撤销别名后同 APP 的授权状态一致。检查取消在途动作与原 run_id，不能用新 ID 重发未知效果。
- APP 退出/重启新实例不继承授权，重新授权需可信宿主显式操作。
- Go 与 TS/Python/Rust 各一次有真实业务结果的任务：发现窗口/控件，Unicode 设值或键入，提交，APP 自身日志/最终文本独立确认；原请求查询无重复提交。源码脚本与包内 SDK 同时复验。
- focus + bind_focus + press/type 是单份 cooperative 计划，焦点范围外拒绝，`primary` 对应 Control；原窗口/焦点/指针恢复和人工输入不中断。需要核对 receipt 的 input/restoration，而不只看 completed。
- 默认元数据不包含键入文本；audit 碰撞轮转、append 排他与不完整末行拒绝；JS 重启生成新 runs 目录，debug 未启用时无 wire/script-code。
- 新回合权限清空；stdin/宿主控制断开撤权；取消/close 后不再产生迟到输入。Forced termination 或 unknown/fenced 不判通过。

## 包内接入与证据

在与源码无关的临时目录解压候选，执行 bin/dtw.exe version/doctor/schema。将 package 的 clients/typescript 做本地 npm install，clients/python 做本地 pip install，Rust 以 path dependency 接入；helper 固定为同一包内 exe。Python/Rust 不安装 Node 才能运行客户端（仓库全套验收仍需要 Node 做 TS 检查）。按三个 SDK README 完成真实 fixture任务，记录独立 APP submit 事件，检查不会引用开发目录或本地缓存。

在交接 Issue 回填：完整 SHA、所有命令/退出码、每场景 PASS/FAIL、summary.json、必要的脱敏原回执、helper/包哈希和实机环境。不要公开 full wire、script-code、截图、owner 文件及用户文档内容。

**发布退出条件：** 三平台当前 SHA CI 通过；Mac 与 Windows 同一 SHA 的原生回归和新增 SDK/动态授权通过；源码与解压包接入通过；没有未解决的 unknown/fenced/restoration failure、权限升级或重复效果。只有这些条件完成后，才进入正式 rc.2 tag/Release/包发布步骤。交接状态本身不构成发布授权或验收通过。
