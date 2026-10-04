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

当前源码的 F4 / F5 独立验收使用 `dtw` managed helper。每项自动创建新的 Win32 fixture、标题和独立应用日志：

```powershell
$env:GOWORK = "off"
python scripts/accept-features.py F4 F5
```

F4 在 1,000 行控件之后通过 64 节点/次的续扫定位 Deep submit，填写并提交订单；检查访问进度、dirty coverage 和应用只提交一次。F5 通过实际私有管道填写/提交，再结束回合，检查禁止后续写入且能取得原收据。结果在 artifacts 的新目录中，包含 helper 版本、源码哈希和应用事件。没有成功实机记录之前，这两项保持待验收。
