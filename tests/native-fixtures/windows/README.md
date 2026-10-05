# Windows native fixture

`-semantics` 另创建勾选框、多选列表、TreeView、输入遮挡层及窗口状态按钮。此模式通过本进程 activation context 启用系统 Common Controls v6，不修改系统设置。应用用 BM_GETCHECK、LB_GETSEL、TVM_GETITEM/GETITEMRECT 等原生消息记录真实状态，不使用 UIA 作为业务判定。

```powershell
python scripts/accept-windows-stability.py --electron 'C:\path\to\electron.exe' --rounds 32
```

脚本检查状态操作与 no-op、保留其他选择项、列表/树滚动、遮挡拒绝后的前台恢复、窗口尺寸/最小化/隐藏与 PNG；Electron 检查真实键鼠、表单、菜单、弹窗、取消、预算、用户切换、32 轮恢复、原回执去重、应用退出与 helper Epoch 隔离。日志及完整 wire 写入新的本机 artifacts 目录。未指定 `--electron` 时只验证原生部分，不能据此认定完整专项通过。旧版 Common Controls 的 TreeView provider 曾发生滚动超时，保留在兼容性报告中。

在 Windows 11 amd64 的已登录桌面中，用标准 Go 工具链构建：

```powershell
New-Item -ItemType Directory -Force bin, artifacts
$env:GOWORK = "off"
$fixtureTitle = "Desktop World Native Fixture " + [guid]::NewGuid().ToString()
$fixtureLog = Join-Path (Get-Location) "artifacts\native-fixture.jsonl"
go build -o bin\DWNativeFixture.exe .\tests\native-fixtures\windows
Start-Process .\bin\DWNativeFixture.exe -ArgumentList @("-title", "`"$fixtureTitle`"", "-log", "`"$fixtureLog`"")
go test -c -o bin\native-acceptance.test.exe .\tests\acceptance
$env:DW_NATIVE_FIXTURE_TITLE = $fixtureTitle
$env:DW_NATIVE_FIXTURE_LOG = $fixtureLog
$env:DW_NATIVE_CAPTURE_PATH = Join-Path (Get-Location) "artifacts\native-fixture.png"
$env:DW_NATIVE_CANCEL_TEST = "1"
.\bin\native-acceptance.test.exe '-test.v' '-test.run' '^TestNativeFixture$'
```

每轮使用新的日志文件或独立工作目录。程序是普通 Win32 EDIT / BUTTON / STATIC 控件，记录控件实际收到的消息与提交次数，不使用 UI Automation 做自己的判定。

不以管理员身份绕过 UIPI。若前台切换被系统拒绝，先由用户聚焦 fixture，再执行一个新的验收轮次。`needs_user_focus` 是系统约束，不允许通过模拟 Alt 等方式绕过。

2026-10-05 已在 Windows 11 amd64 实际执行通过，包括 Unicode/组合键、点击/拖拽/滚轮、拖拽取消后的清理和控件替换后的旧 Ref 拒绝。fixture 明确实现 Ctrl+A，并在重建 EDIT 后保留静态标签关联。证据见 [Windows 验收报告](../../../docs/windows-validation.md)。CI 的核心测试不能替代桌面验收。

当前源码的 F4 / F5 独立验收使用 `dtw` managed helper。每项自动创建新的 Win32 fixture、标题和独立应用日志：

```powershell
$env:GOWORK = "off"
python scripts/accept-features.py F4 F5
```

F4 在 1,000 行控件之后通过 64 节点/次的续扫定位 Deep submit，填写并提交订单；检查访问进度、dirty coverage 和应用只提交一次。F5 通过实际私有管道填写/提交，再结束回合，检查禁止后续写入且能取得原收据。两项均已在 Windows 11 实机通过；结果在 artifacts 的新目录中，包含 helper 版本、源码哈希和应用事件。
