# Bot 宿主接入

`v0.1.0-alpha.1` 提供 `host` Go 包和 macOS arm64 helper 开发包。推荐 Bot 通过这个 Go client 管理一个持久子进程，不在 Wails 主进程直接执行原生输入。根包仍可嵌入，但使用方需自己承担原生调用阻塞、进程生命周期与权限隔离。

以下示例使用当前源码的 `dtw` 命令。Windows managed transport 和 InputPolicy 已有原生任务验收，见 [F4/F5](features.md) 与 [Windows 报告](windows-validation.md)。本次 Windows 适配未发布新包，也未同步进入 caelis-bot 的 M0；宿主应使用对应源码构建，历史包以其发布说明为准。

本仓库提供接入边界与可运行示例，尚未修改或验收 caelis-bot 的实际 Runtime/Wails 集成。不要同时启用两个会竞争同一桌面的写后端，也不要在错误后自动切换后端重放动作。

宿主后续可显式设置 `host.Options.InputMode=desktopworld.InputModeCooperative`；Hello 会核对模式，Agent 不能更改。见 [短前台事务](cooperative-input.md)。当前没有更新 caelis-bot 的固定版本或联调。

## 最小顺序

```go
client, err := host.Start(ctx, host.Options{
    Executable: trustedAbsoluteHelperPath,
    InputPolicy: desktopworld.InputNoShared, // 后台语义任务；共享输入任务使用 InputShared。
    Stderr: diagnosticWriter,
})
if err != nil { return err }
defer client.Close()
if err := client.BeginTurn(ctx, runtimeTurnID); err != nil { return err }

// observe 得到 Ref；Runtime 展示并完成现有应用授权流程后：
if err := client.Grant(ctx, runtimeTurnID, approvedApplicationRef); err != nil {
    return err
}
reply, err := client.Call(ctx, runtimeTurnID, stableToolCallID, "act", plan)
// 即使 reply.Error != nil，也先保存 reply.Result 中的原收据。
// err != nil 表示传输/等待不确定；禁止用新 ID 重试。
if err != nil { return err }
modelResult := host.Content(reply)
_ = modelResult

// 用独立的、仍有效的短期限 context 停止，不能传已取消的工具 context。
stopCtx, cancel := context.WithTimeout(context.Background(), time.Second)
defer cancel()
if err := client.EndTurn(stopCtx, runtimeTurnID); err != nil { return err }
```

完整且能编译运行的入口为 `examples/bot-host`，默认只读。显式开发验收：

```sh
DW_FIXTURE_TITLE='Desktop World Bot Alpha unique-run' \
DW_FIXTURE_LOG="$PWD/artifacts/bot-alpha-unique.jsonl" \
./script/build_and_run.sh --verify
go run ./examples/bot-host --helper "$PWD/bin/dtw" \
  --fixture-title 'Desktop World Bot Alpha unique-run' --text '联调 🌍'
```

这个 CLI 的 `--fixture-title` 是开发者明确指定的测试授权；生产 Bot 必须替换为 Runtime 已完成的批准，不能从模型请求、网页或窗口名称推断授权。

## 权限与生命周期

- `Start` 仅接受可信绝对路径；不自动下载、更新或重启 helper。握手最长 15 秒，不自动申请系统权限。进程只继承 HOME/PATH/TMPDIR/LANG/LC_CTYPE，以及 Windows 启动所需的 SystemRoot/WINDIR/USERPROFILE/TEMP/TMP，不转发模型 API 凭证。InputPolicy 是可信启动上限，Grant 不能放宽 no_shared_input。
- `BeginTurn` 创建空写授权。回合 ID 为 1–64 位 ASCII 字母、数字、连字符或下划线；由 Runtime 生成，每次唯一，一个 helper 同时只接纳一个活动回合。结束的 ID 不能重用。
- `Grant` 只接受刚观察到的存活 application Ref，最多 32 个。不接受应用名、窗口、原生 PID 或模糊匹配。授权限同一应用实例、同一回合，每个输入步骤重新检查。
- 数据请求的 `turn` 在工具参数之外由 host 注入。模型只获得 observe/read/sync/act/capture/get/cancel，不获得 BeginTurn/Grant/EndTurn。数据流不能扩权，managed 模式禁用启动时写授权和 raw Point 输入。
- `EndTurn` 经独立控制通道撤销应用授权并取消执行，不等待输入数据队列腾空。已经进入 OS 的事件不可撤回；返回确认不证明此前无副作用。
- `Call` 等待取消或传输失败后，用控制通道结束回合；若控制无法确认，关闭 helper。`Close` 先关闭授权通道，再关闭数据入口，给进程 2 秒退出机会，必要时终止。强杀不证明键/按钮释放完成。
- 同一 `(turn, id)`、相同操作/参数只发送一次；不同内容返回冲突。`Reconcile` 仅等候原响应，结束回合后仍可使用。新进程没有旧收据或跨进程 exactly-once；unknown/fenced 必须交给宿主核对，不能自动重启绕过。
- 每会话最多保留 4096 条数据请求、4096 条控制请求/4096 个回合。接近上限由宿主在没有未决副作用时正常结束会话；不静默丢失历史或重建权限。

数据和控制是同一可信宿主拥有的进程管道，**不构成同用户恶意本地进程的安全沙箱**。一次只运行一个拥有写权限的桌面宿主，MVP 不提供跨进程全局互斥。

## 返回预算和脚本组合

Go client 启动 helper 时指定 `--full-output`，保留类型化 Fact、精确时间和完整原始结果。需要 typed Observation/Receipt 时使用 `protocol.Decode(reply.Result, &value)`。Go error 与 `Reply.Error` 分开：后者可能伴随 partial/unknown 收据。

面向模型使用 `host.Content(reply)`：紧凑 Fact、省略逐对象 sample 时间，保留 coverage、分页、身份、收据和未知状态。计入 text 与 structuredContent 两份 UTF-8 JSON 总量，预留 1 KiB 给 Runtime 元数据，总计不超过 32 KiB。超限返回明确 `model_output_budget` 与原 run_id/outcome，不静默截断，也不把动作称为未发生；完整回复仍可用 Reconcile 读取。宿主追加超过预留空间的字段时需自行重新计算总预算。截图保持本地 asset，不在此函数内注入 base64。

观察优先使用 scope + fields + budget；需要分页就保留同一 observation/cursor。不要每个动作都输出全树。时间与 Fact 的原始格式仍能从 host 保留结果读取。

`clients/javascript/desktop.mjs` 提供 `createSession(transport)`、局部计算、多次 await、选择性 print 和失败时停止后续调用。当前 JS managed 路径已使用 `dtw session`；正式 Plugin 候选在其上提供标准 Skill + MCP、独立 worker 与 supervisor watchdog，见 [通用宿主契约](agent-plugin-host-contract.md)。Bot 等宿主应消费同一固定 release payload，不再维护独立 JS transport 桥。真实 Bot turn、权限和数据目录生命周期仍由 Bot 自己承担。Node vm 不是任意本机 JS 安全沙箱。

## 底层控制协议

`serve --host-control --full-output`：stdin/stdout 是数据 NDJSON。Unix 继承 FD 3 为 host→helper 控制请求，FD 4 为 helper→host 回应。Windows 当前源码由 host 复制两个私有匿名 pipe 端点，并通过 Go 的 AdditionalInheritedHandles 限定句柄继承列表；句柄编号仅经可信启动环境传递，helper 验证为不同 pipe、删除元数据并关闭继续继承标志。模型 schema 不含句柄、turn 或授权入口。控制 EOF 撤销全部授权并停止数据服务。Windows 11 F5 实机已验证授权、填写/提交、回合撤销、拒绝新写入和原回执恢复。

```json
{"id":"host-1","op":"begin_turn","turn":"turn-unique"}
{"id":"host-2","op":"grant","turn":"turn-unique","application":"observed-application-ref"}
{"id":"host-3","op":"end_turn","turn":"turn-unique"}
```

控制响应协议 `desktop-world/host-control-v0.1`；数据为 `desktop-world/helper-v0.1`。get/cancel 可在活动回合结束后查询原 run_id。所有通道都拒绝未知字段/重复 key。控制操作不出现在 Agent schema。

## 发行边界

目前公开 v0.1.0-rc.3 已有 macOS arm64 与 Windows amd64 同提交预编译包。该版本的 macOS GUI 仅局部复测，Windows rc.3 未做本轮原生 GUI 复测；较早 rc.2 结果不能代替。macOS rc.3 为 ad-hoc 签名、未公证，Windows 未签名。正式 Plugin 候选也需各自归档的原生 GUI 和签名/分发证据，不能以源码构建或另一平台交叉编译代替。

固定公开 tag 与模块版本，校验 SHA256SUMS 和 `manifest.json` 的 revision。打包脚本 `scripts/package-prerelease.sh` 拒绝脏工作区，打包干净 HEAD，包含同 revision 的 Go 源码。见 [HANDOFF](../HANDOFF.md) 和 [NOTICE](../NOTICE)。

## rc.2 动态声明

Go `host.Client` 增加 `Declare(ctx,turn,name,windowTitle)`、`Grants(ctx,turn)`、`Revoke(ctx,turn,applicationRef)` 和 `RevokeGrant(ctx,turn,grantID)`。声明 name/windowTitle 二选一，APP 未运行时 pending。模型仍只有数据入口；授权控制不进入七操作 schema。`desktopworld.NewPlan().Focus(fieldRef).BindFocus("input",windowRef).Press(desktopworld.Target{Bound:"input"},"O","primary")` 构造单份计划。TS/Python/Rust 的对应 HostSession/Plan 用法见 [Agent 接入](agent-integration.md)。
