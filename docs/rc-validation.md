# RC 发布范围与同提交验收

本轮目标是可供验收的 `v0.1.0-rc.1`，范围为 Windows 11 amd64 与 macOS arm64 的 CLI、JavaScript 持久会话、Go SDK 和 managed helper。Windows 实机验证已完成；macOS 必须在最终同一提交上复验，跨平台 RC 才能确认。历史 Mac 成功记录不代替本轮复验。当前不创建 tag、不上传发布包。

## 范围与完成条件

| 项目 | RC 要求 | 当前证据 |
| --- | --- | --- |
| 接口与构建 | race、vet、原生构建、协议及 JavaScript 检查 | Windows 自动检查；Mac 复跑待完成 |
| 日常任务 | 浏览器导航/填写/提交、编辑保存、计算器结果可独立核对 | Windows Chrome / Notepad / Calculator 实机通过 |
| 原生语义 | 设值、调用、勾选、选择、展开和滚动；已达状态不重复产生效果 | Windows 标准控件和 Common Controls v6；Mac 状态 fixture 待复跑 |
| 后台执行 | 读取和受支持的语义操作在后台；键鼠短事务后恢复，用户切换优先 | Windows Chrome 20 轮、Electron 32 轮及中断专项通过；Mac 待复跑 |
| 失败收敛 | 遮挡、取消、预算耗尽、对象退出与 Epoch 失效明确返回；不重放未知效果 | Windows 实机回执与应用日志；跨平台契约测试 |
| 捕获 | PNG 可查看；窗口变更更新几何版本，隐藏/最小化拒绝 | Windows Chrome / Notepad / Calculator / Win32 / Electron；Mac F9 待复跑 |
| 宿主 | 授权、回合撤销、原回执查询、有界观察及持久会话 | Windows F4/F5、JavaScript；Mac 对应 fixture 待复跑 |
| 分发 | 干净提交、manifest 版本/提交、校验和、对应源码及 MPL-2.0 | 两平台打包脚本；Mac 包待本机生成验收 |

实机配置为普通权限、已登录的单屏交互桌面。Windows arm64、Windows 10、Mac Intel/最低 macOS 14 实机、多屏混合缩放、RDP/锁屏/用户切换、复杂 IME、系统级独立输入、OCR 和实际 Bot/Wails 产品集成不在本次 RC 的已验收矩阵内。macOS 源码保留 amd64 构建能力，但分发脚本仅支持 arm64。不将 fixture 成功率当作任意应用或独立 LLM Agent 的成功率。

旧版 Common Controls TreeView 曾公开 ScrollItem 能力却在调用时超时；dtw 保留 native HRESULT 和原回执，不改用键鼠重发。现代 v6 TreeView 和 LISTBOX 滚动均有独立原生消息判定。Electron 原生应用菜单在本机未暴露 UIA menu_item；已验证其真实菜单快捷键及原生对话框，网页上下文菜单另通过原生树绑定验证。完整边界见 [Windows 报告](windows-validation.md) 和 [输入事务](cooperative-input.md)。

## Windows 复验与打包

打开 Chrome，使用当前精确窗口标题；Electron 本轮使用 44.5.1，通过现有 runtime 的 `electron.exe` 指定。fixture 只用于验收，不是产品运行依赖。

```powershell
$env:GOWORK = 'off'
$env:PYTHONUTF8 = '1'
.\scripts\check.ps1
python scripts/accept-windows.py --browser-window '当前 Chrome 窗口标题 - Google Chrome' --rounds 20
python scripts/accept-features.py F4 F5
python scripts/accept-windows-stability.py --electron 'C:\path\to\electron.exe' --rounds 32
```

从干净提交打包（PowerShell 7），核对 SHA256SUMS 后将 ZIP 解压到新的目录。真实验收须使用解压后的 helper：

```powershell
.\scripts\package-prerelease.ps1 -Version v0.1.0-rc.1
$rcHelper = 'D:\absolute\unpacked\desktop-world-v0.1.0-rc.1-windows-amd64\bin\dtw.exe'
& $rcHelper version
& $rcHelper doctor
python scripts/accept-windows.py --helper $rcHelper --browser-window '当前 Chrome 窗口标题 - Google Chrome' --rounds 20
python scripts/accept-features.py F4 F5 --helper $rcHelper
python scripts/accept-windows-stability.py --helper $rcHelper --electron 'C:\path\to\electron.exe' --rounds 32
```

## Mac 同提交验收

在 Mac 上检出最终交付的 commit，确认 `git status --porcelain` 为空、`git rev-parse HEAD` 与 Windows 包的 manifest 一致。使用已有 Accessibility、输入及屏幕捕获授权；`doctor` 只检查状态。需要 Xcode Command Line Tools、Go、Node.js、Python 3、Chrome，以及 Electron 44.5.1 runtime。

```sh
export GOWORK=off
./scripts/check.sh
python3 scripts/accept-features.py F1 F2 F3 F6 F7 F8 F9
python3 poc/background-input/accept-full.py --provider appkit
python3 poc/background-input/accept-full.py --provider chrome
python3 poc/background-input/accept-full.py --provider electron
python3 poc/background-input/accept-editor.py --provider textedit
```

F4/F5 是 Windows 专属真实管道与 UIA 测试。Mac 的短事务脚本验证原生菜单、弹窗、Unicode、键鼠、恢复和用户切换；TextEdit 脚本核对磁盘文本。原生 fixture 补充步骤见 [validation.md](validation.md)，Electron runtime 路径见 [POC 验收指南](../poc/background-input/FULL_ACCEPTANCE.md)。

再从同一干净提交生成 Mac 包，在新的目录解压并核对 SHA256SUMS 和 manifest。下列路径换成解压后的绝对路径；feature 脚本传 `--helper`，POC 脚本读取 `DTW_ACCEPT_HELPER`：

```sh
./scripts/package-prerelease.sh v0.1.0-rc.1
export DTW_ACCEPT_HELPER=/absolute/unpacked/desktop-world-v0.1.0-rc.1-darwin-arm64/bin/dtw
"$DTW_ACCEPT_HELPER" version
"$DTW_ACCEPT_HELPER" doctor
python3 scripts/accept-features.py F1 F2 F3 F6 F7 F8 F9 --helper "$DTW_ACCEPT_HELPER"
python3 poc/background-input/accept-full.py --provider appkit
python3 poc/background-input/accept-full.py --provider chrome
python3 poc/background-input/accept-full.py --provider electron
python3 poc/background-input/accept-editor.py --provider textedit
```

记录需包含 helper 版本/提交/哈希、应用 runtime、业务日志和回执。源码指纹核对该次工作树；Windows/Mac 换行策略可能改变原始文件哈希，因此跨平台版本以 commit 和源码归档为准。每轮创建新样本，保留失败回执；不换 ID 重试未知输入。helper 重启产生新 Epoch，不承诺跨进程恢复 Ref 或去重记录。只有上述范围内两平台全部通过、恢复失败已处理且无未解决的发布阻断问题后，才确认并发布 RC。Windows 包未签名；Mac 包 ad-hoc 签名且未经公证。
