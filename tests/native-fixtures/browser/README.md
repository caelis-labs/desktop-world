# Browser native accessibility fixture

网页只使用标准 HTML 表单与一个没有语义子控件的 Canvas。独立日志来自 DOM 的 keydown / input / submit 回调，验收要求这些输入具有 `isTrusted=true`，不使用 DOM 脚本填值。Desktop World 通过操作系统 AX / UIA 读取和操作浏览器。

终端一（每轮选新的标题与日志文件；服务仅允许 IPv4 loopback）：

```sh
mkdir -p artifacts bin
go run ./tests/native-fixtures/browser \
  -title 'Desktop World Browser Fixture demo' \
  -log artifacts/browser-demo.jsonl
```

在要测试的浏览器中打开输出的本地 URL，并让该页成为窗口的当前标签页。页面显示 `submitted:0` 后，在具有操作系统权限的第二个终端运行：

```sh
go test -c -o bin/native-acceptance.test ./tests/acceptance
DW_BROWSER_FIXTURE_TITLE='Desktop World Browser Fixture demo' \
DW_BROWSER_FIXTURE_LOG="$PWD/artifacts/browser-demo.jsonl" \
DW_BROWSER_CAPTURE_PATH="$PWD/artifacts/browser-demo.png" \
./bin/native-acceptance.test -test.v -test.run TestBrowserFixture
```

测试先按唯一窗口标题限定作用域，再通过原生接口查找输入框。五步计划执行聚焦、全选、Unicode 输入和 Enter；重复请求不增加提交次数。Canvas 中的像素文字不能绑定为语义按钮。日志文件必须不存在，服务拒绝覆盖上一轮记录。

测试结束后关闭测试标签页，在终端一按 Ctrl+C 停止服务。默认的 `go test ./...` 跳过这项真实输入测试。当前仅在 macOS / Chrome 上执行通过，其他浏览器与 Windows 需要各自运行验收。
