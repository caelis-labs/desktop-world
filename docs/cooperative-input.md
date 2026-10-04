# 同一桌面的短前台事务

`cooperative` 的目标是减少 Agent 占用用户电脑的总时间。读取、捕获和受支持的语义动作沿用后台通道；明确需要键鼠的已知步骤，在一份短 Plan 内借用前台，随后归还。模型推理、网络等待和下一次工具调用之间不保留前台。它共享实际系统键鼠，不提供第二套物理设备或独立 OS 会话。

## 宿主选择

普通构建可用，无需 POC build tag：

```sh
GOWORK=off go build -o bin/dtw ./cmd/dtw
dtw serve --input-mode cooperative --write-app-window '精确的已观察窗口标题'
```

直接嵌入使用 `local.Options{InputMode: desktopworld.InputModeCooperative}`；独立宿主使用 `host.Options{InputMode: desktopworld.InputModeCooperative}`。默认 `shared` 保留原来的前台输入契约。模式只由可信宿主配置，Hello 同时校验实际 World 和宿主选择；Agent 参数、UI 文本和授权管道不能改变模式。Windows 选择 cooperative 会明确失败；Windows 实机验收整体后置。

`InputPolicy` 是独立权限上限。`no_shared_input` 仍在任何原生效果前拒绝整份含 focus/键鼠的计划。合作模式不会增加写范围、原始 Point 权限或操作白名单，也不会把失败的语义/定向投递自动改成另一通道。

## 最小操作链

1. 保持一个 helper。先用 summary 和 `name,role` 找窗口，再在该窗口内用名称/角色找控件。按动作读取 `dtw schema act <action>`。
2. 优先用受支持的语义动作。需要键盘时，把点击输入框、组合键、短文本与已知提交动作放在一份短 Plan；Chromium 通常需要先实际点击输入框建立焦点。
3. 新菜单或弹窗必须使用当前 Ref。可在同一短 Plan 内以精确、唯一 Locator `bind` 一个**已知**的新控件，再使用 bound alias；未知页面应结束事务、观察，再生成下一份计划。将 Locator 限在窗口/子树内，避免扫描整个浏览器应用。
4. 查看完整 Receipt，并独立验证业务结果。未知/partial 不换 ID 重放；查询原请求或 Run。恢复完成后再开始下一份任务。

坐标操作只接受已观察 UI/Window 的 Ref 或相对 Anchor，不接受绝对 Point。拖拽两端必须属于同一原生窗口与应用；原生输入前再次核对实际命中目标的祖先关系。焦点确认与 compositor/AX 命中可能不同步；输入前最多等待 120 ms，让同一 retained 目标命中稳定，仍受原一秒预算约束，不重投事件、不换目标、不再次激活。截图的 capture-window Ref 与 AX Ref 仍独立，截图坐标不授予输入权限。

## 时间、回执与清理

首次输入设定一秒预算；每次读取/操作和输入事件边界检查预算与前台应用。预算耗尽停止后续输入并恢复。已进入 provider 的有界调用、事件消费和清理可能跨过预算，因此一秒不是硬实时上限。单次文本最多 256 个 UTF-16 单元，拖拽最多 500 ms，超过上限在激活/投递前拒绝。长任务拆成多份已知短计划。多行文本的换行/Tab 对应真实 Enter/Tab 键，可能触发控件自己的提交或焦点行为。

输入步骤的 `channel=foreground_transaction`；语义步骤继续报告 `semantic`。Receipt 的 `input` 包含：

| 字段 | 含义 |
| --- | --- |
| `mode` | 宿主选定的 `cooperative` |
| `foreground_ms` | 本事务借用前台至清理结束的时长；未借用时为零/省略 |
| `restoration` | `not_borrowed`、`restored`、`user_superseded` 或 `failed` |

`focus` 的自动验证发生在事务内，事务结束后原用户窗口会恢复。恢复失败返回 unknown 并 fence；晚到的原生结果必须先清理再释放执行令牌。取消会释放本库尚持有的键/按钮。用户切换到另一应用时停止新输入，保留其新前台；只在鼠标仍位于库的最后位置时恢复原指针。已进入系统事件流的输入不可撤回。

## 实现边界与证据

原生 key-focus 与前台采样使用动态探测的 SkyLight 私有 SPI，精确绑定 AX 窗口和进程生命周期；只接受当前桌面上实际可见的 WindowServer 窗口，不自动跨 Space、恢复最小化窗口或改写系统权限。缺少 SPI/权限时明确不可用。最低 macOS 版本、其他机器、复杂 IME、系统级快捷键及任意应用的兼容性不能从单机通过推导。用户输入干预检测是 best effort，尤其不能完整区分同一应用内部的用户/应用窗口变化；不承诺无干扰。

POC 的 `public_pid` 已证明有限 AppKit 点击/短文本可不借用前台，但 WebKit 和不同 provider 不具备同等语义；该路线继续保留在 build-tagged 实验中。正式路径先交付经过多种 provider 验证的完整短事务，不建立未经验证的自动 provider 白名单。没有 VM、第二个登录会话、常驻桌面占有锁或后台输入服务。

逐项业务日志、占用时间、取消/超时/用户切换和复现命令见 [实机验收](../poc/background-input/FULL_ACCEPTANCE.md)。第三方来源及 MIT 通知见 [THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES.md)。
