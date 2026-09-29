# Desktop World

独立 Go library：把桌面作为一个按需观察、带生命周期与不确定性的对象世界。根包不依赖 Agent Runtime、LLM、Wails、浏览器插件或网络服务。通用 Agent 接入见 [stdio helper](docs/helper.md)，真实任务成本见 [可用性评估](docs/usability-evaluation.md)，字段选择和后台隔离见 [设计说明](docs/observation-and-seats.md)。

MVP 优先验证 Token 效率与易用性。通用 Agent 推荐 [JavaScript 调用链](docs/scripting.md)：持久会话中组合观察与动作、中间数据本地保留、按需 print、强制呈现覆盖范围和失败；后台输入隔离不属于首版验收范围。

**当前状态：实验性实现，尚未达到 SPEC 的双平台正式发布门槛。** macOS 原生键鼠、语义操作、截图和生命周期路径已在 AppKit fixture 上运行，并通过 Chrome 原生输入与 Canvas 负面验收；Windows 后端已有实现、交叉编译与静态检查，仍需 Windows 交互式桌面验收。原始设计保留在 [SPEC.md](SPEC.md)，实现边界见 [docs/implementation.md](docs/implementation.md)，实际运行证据见 [docs/validation.md](docs/validation.md)。

## 快速运行

需要 Go 1.23+；当前验证工具链为 Go 1.26.8。

```sh
# 完整的五步表单计划，使用内存 fixture，不发送系统输入。
go run ./examples/headless

# 共享 Ref / Anchor 的具身集成示例，不移动真实鼠标。
go run ./examples/embodied

# 只读原生探测，不自动申请系统权限。
go run ./cmd/dw-inspect -environment
go run ./cmd/dw-inspect

# 契约、race、vet、Windows 交叉构建、协议示例检查。
./scripts/check.sh
```

原生平台：

- macOS 14+，arm64 / amd64，开启 CGO，安装 Xcode Command Line Tools。桥接使用 AX、CGEvent、ScreenCaptureKit，不创建 NSApplication，不接管宿主主线程。
- Windows 11 amd64，纯 Go 原生绑定，无 CGO 依赖。COM 在固定 MTA 工作线程中初始化和释放；不会修改进程全局 DPI 模式。
- 其他平台可运行协议和 fixture；`local.Open` 明确返回 `platform_unsupported`。

模块路径：`github.com/caelis-labs/desktop-world`。当前只是本地仓库，未发布远程版本。

## 接入

```go
ctx := context.Background()
world, err := local.Open(ctx, local.Options{})
if err != nil { return err }
defer world.Close(ctx)

actor, err := world.NewActor(ctx, desktopworld.ActorConfig{
    ID: "inspector",
    ReadScopes: []desktopworld.Scope{{Desktop: true}},
    Operations: []string{"observe", "read", "sync", "resolve_anchor"},
})
if err != nil { return err }
observation, err := actor.Observe(ctx, desktopworld.ObserveRequest{
    Scope: desktopworld.Scope{Desktop: true},
    Projection: desktopworld.ProjectionSummary,
})
```

空权限不代表全部允许。发现窗口后，可创建另一个仅绑定该窗口 Ref 的 Actor，并授予需要的写操作。每一步都会重新检查 Ref、生存状态、所属范围、能力和必要焦点。`Authorizer` 可进一步收紧字段、参数和目标；它必须有界且非交互，审批 UI 由宿主负责。

Actor 的操作名为 `observe`、`read`、`sync`、`resolve_anchor`、`capture`、`read_asset`、`bind`、`wait` 以及公开的动作名。绝对 Point 输入另需 `raw_input` 和 Desktop 写范围。`visible_region` 截图必须有 Desktop 读范围，即使裁切目标是一个已授权窗口。

宿主可显式调用 `World.RequestPermissions` 申请 `accessibility`、`input`、`screen_capture`。`Open` 不弹权限请求，Go library 本身不能替用户授予操作系统权限。

## 执行与恢复

- Plan 最多 16 步、10 秒；默认每步 2 秒。支持 bind / wait、focus / invoke / set_value、pointer move / click / drag / scroll、Unicode type_text、完整 key chord。
- Ref 固定指向一次 provider 实例。绑定失效会停止；不会换成同名对象，也不会静默把 invoke 改成 click。
- 同 Epoch / Actor / RequestID / 规范化内容只执行一次。使用同 ID 查询或恢复，不换新 ID 重放未知动作。
- **先保存 Receipt，再处理 Execute 的 error。** `delivery` 与 `verification` 是独立结果；`completed` 只表示声明的完成条件满足。
- native 写调用仍在执行时，返回 unknown 并 fence 共享输入通道，直到原生调用退出。确认不了输入清理的通道保持 fenced，需重新启动宿主；不能用另一 World 绕过同进程 fence。
- 拖拽与文本注入在事件边界检查取消；只释放库自己按下的键/按钮。已经进入系统输入流的事件无法撤回。

完整例子在 [examples/headless/main.go](examples/headless/main.go)。角色渲染器可以使用 [examples/embodied/main.go](examples/embodied/main.go) 的 Anchor；解析 Anchor 不会自动聚焦或移动真实鼠标。

## Agent 协议

`protocol.Handler{Actor: actor, Epoch: env.Epoch}` 绑定可信宿主选定的 Actor。调用 `Handle(ctx, requestJSON)` 即可接入任意工具框架，不启动网络端口。

协议固定为 `desktop-world/0.1`；版本和 revision 是十进制字符串，时长字段为 `*_ms`。拒绝重复 JSON key、未知字段、未知操作、非法 target 联合类型和超限参数。UI 文本始终是不可信数据。

[examples/protocol](examples/protocol) 包含设计中原始请求。`protocol.Tools()` 提供工具描述、envelope 和完整参数 schema；最终由 Handler 严格验证。`desktop-world schema act` 可查看参数、大小写、取值和上限。

观察输出计入完整成功 envelope 的 UTF-8 字节预算，默认 16 KiB。多页观察固定在同一采样批次；每页 cursor 只描述这一页，需分别同步或重新获取完整观察。`Changes` 重新读取声明范围并比较物化视图，超预算或历史/权限/拓扑失效返回 `reset_required`。`Watch.Next` 是轮询式消费，不依赖原生事件无遗漏。

## 真实桌面验收

macOS：

```sh
# 启动专用 fixture，每轮使用新的标题和日志文件。
DW_FIXTURE_TITLE='Desktop World Native Fixture demo' \
DW_FIXTURE_LOG="$PWD/artifacts/demo.jsonl" \
./script/build_and_run.sh --verify

go test -c -o bin/native-acceptance.test ./tests/acceptance
DW_NATIVE_FIXTURE_TITLE='Desktop World Native Fixture demo' \
DW_NATIVE_FIXTURE_LOG="$PWD/artifacts/demo.jsonl" \
DW_NATIVE_CAPTURE_PATH="$PWD/artifacts/demo.png" \
DW_NATIVE_CANCEL_TEST=1 \
./bin/native-acceptance.test -test.v -test.run TestNativeFixture
```

必须在已登录的交互式桌面、具有现有 OS 授权的宿主环境中运行；沙箱可能隐藏显示器和 TCC 状态。测试仅写入明确命名的 fixture 窗口。普通 `go test ./...` 会跳过真实输入测试。Codex 的 Run 按钮也指向这个 fixture 构建脚本。

Windows 的构建和运行步骤见 [tests/native-fixtures/windows/README.md](tests/native-fixtures/windows/README.md)。验收检查应用自己收到的文本、按键、提交次数和替换事件；不只依赖本库自己的 UIA/AX 读回。

浏览器原生接口和 Canvas 负面用例见 [tests/native-fixtures/browser/README.md](tests/native-fixtures/browser/README.md)。本地网页记录真实 DOM 输入与提交；表单操作由 AX / CGEvent 驱动，不依赖浏览器插件或 DOM 自动化。

## 当前限制

- 原生变化同步使用显式刷新与 Watch 轮询，尚未接入 AXObserver / UIA 事件加速；普通 provider 漏事件不会被误当成完整变化日志。
- 用户干预检测为 `best_effort`：执行前焦点、持有按键/按钮和目标命中检查，不是系统级输入隔离。
- 仅实现 `visible_region`，不把屏幕裁图声称为独立 `window_content`。Windows 暂不包含鼠标光标。
- 无 OCR、视觉定位、工作流 DSL、自动重绑、剪贴板后备、持久 exactly-once、角色动画或高帧率捕获。
- 双屏混合缩放、Windows 真实桌面、更多浏览器 / Electron 兼容性和打包 Wails 宿主仍需验证；不能从当前 AppKit / Chrome fixture 推导全部应用兼容性。
