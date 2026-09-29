# Windows native fixture

在 Windows 11 amd64 的已登录桌面中，用标准 Go 工具链构建：

```powershell
New-Item -ItemType Directory -Force bin, artifacts
$fixtureTitle = "Desktop World Native Fixture " + [guid]::NewGuid().ToString()
$fixtureLog = Join-Path (Get-Location) "artifacts\native-fixture.jsonl"
go build -o bin\DWNativeFixture.exe .\tests\native-fixtures\windows
Start-Process .\bin\DWNativeFixture.exe -ArgumentList @("-title", "`"$fixtureTitle`"", "-log", "`"$fixtureLog`"")
go test -c -o bin\native-acceptance.test.exe .\tests\acceptance
$env:DW_NATIVE_FIXTURE_TITLE = $fixtureTitle
$env:DW_NATIVE_FIXTURE_LOG = $fixtureLog
$env:DW_NATIVE_CAPTURE_PATH = Join-Path (Get-Location) "artifacts\native-fixture.png"
$env:DW_NATIVE_CANCEL_TEST = "1"
.\bin\native-acceptance.test.exe -test.v -test.run TestNativeFixture
```

每轮使用新的日志文件或独立工作目录。程序是普通 Win32 EDIT / BUTTON / STATIC 控件，记录控件实际收到的消息与提交次数，不使用 UI Automation 做自己的判定。

不以管理员身份绕过 UIPI。若前台切换被系统拒绝，先由用户聚焦 fixture，再执行一个新的验收轮次。`needs_user_focus` 是系统约束，不允许通过模拟 Alt 等方式绕过。

当前 Windows 代码已在 macOS 上通过 amd64 交叉编译和 `go vet`，尚未在 Windows 主机执行。CI 的核心测试也不能替代这项桌面验收。
