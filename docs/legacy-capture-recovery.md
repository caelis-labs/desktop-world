# 旧版应用捕获与浏览器能力回收

本次审计基于重写分支 `c57937b`，核对旧版 `26247c4`、`8fe6910` 与 RC.9 `79a097b` 的实际源码和验收记录。旧报告只证明对应旧候选；不作为当前二进制的 GUI 验收。

## 找回的实现及归属

| 能力 | 旧实现 | 当前状态与接入位置 |
| --- | --- | --- |
| 同一应用可见窗口、浮层和菜单候选 | `app_view/Scene.swift::discover` 枚举同 PID 的全部 WindowServer layer；额外通过系统 AX hit test 的 client PID、近似相等的可见 surface geometry 和点可见性关联 service-owned surface | 当前 `Native.swift::surfaceInventory` 原先只列 layer 0，丢失同进程 raised surface；在现有 `windows()` 补齐，继续发出运行时管理的 `window_ref` |
| 屏幕共享技术的单 surface 截图 | DTW 自有 `SCContentFilter(desktopIndependentWindow:)` + `SCScreenshotManager.captureImage`；关闭 child composition、cursor、shadow；校验 contentRect、输出尺寸和坐标映射 | 已移入当前 `Scene.swift`，由 `app.capture({window_ref})` 使用，不需要恢复旧 `operation:app` 入口 |
| 截图坐标点击 | 旧 `app_view.rs` 建立 image→screen 映射；`Scene.swift` 证明目标身份、bounds 与该点遮挡，再投递 CG 事件 | 已移入当前 `app.click()` 和 Scene bridge；当前按动作相关状态校验，不搬回旧版对整个 app 库存/全局堆栈严格相等的约束 |
| SCK 多 Runtime 生命周期 | `app_view/macos.rs` 的短生命周期同签名 helper、每用户 scene lock、父进程退出/超时清理 | 已由当前 `SceneOwner` 与 worker 接管；保留现有所有权，不增加第二套常驻 capture Runtime |
| 原生语义点击 | 旧普通 app 通过 CUA `app.click({name})` / AX 动作；popup 的像素路线与语义路线独立 | 当前 `observe()` 分页、`press()` 和 `setValue()` 已提供语义路线。新 surface 只有在真实 AX 对象及操作可证明时才开放语义动作；截图不证明 AX 可操作 |
| 浏览器 exact-tab 截图、`semantic_v2`、ref click/type/select | 旧 `native_tools.rs` 将 `get_browser_state`、`browser_*` 转发给同一 CUA SDK/worker。DTW 自有部分是参数呈现、结果压缩、回执和 profile 集成，并非独立浏览器渲染/语义引擎 | 当前浏览器只用原生窗口/AX；精确 tab provider 尚未迁入。恢复时应 port CUA browser 的依赖闭包，通过当前 app 脚本对象提供能力，不能恢复旧 worker 或旧 Host grant |
| 应用共享流自动包含新子窗口、浏览器 tab sharing | `poc/associated-capture/screen-sharing-research.md` 是技术调研，明确持续 application-inclusion stream 未实现；历史源码检索未找到 DTW 自有 `getDisplayMedia`、`tabCapture`、`Page.captureScreenshot` 实现 | 不将调研建议或 CUA tab provider 说成已完成的 DTW 自有分享流；不额外构造会议软件/浏览器扩展来替代既有捕获路线 |

## 当前架构的恢复边界

保持标准 MCP `exec({app, source})`。模型继续先用 `app.windows()` 选择窗口，再使用同一套 `capture`、`observe`、`press` 和 `click`。macOS provider 负责更多原生 surface 的归属和可用性；不增加公开 delivery、PID、审批或 request ID 参数。

同 PID 浮层复用现有 `NativeTarget`，其 owner 仍是已绑定的 app 实例。macOS 窗口覆盖声明为 `same_process_application_surfaces`，包括同 PID 的有效非零层 surface；Windows/Linux 维持原来的平台范围。全量 CG 库存可能保留已关闭或 ordered-out 的 backing 记录，原生 provider 逐个通过与动作相同的精确窗口查询确认当前可用 surface；明确不可用的记录计入 `excluded_unavailable_surface`。该范围不承诺列出全部最小化、其他 Space 或不可用文档窗口。`visible` 仍只是当前屏幕可见性，不等于对象存活、AX 可操作或共享帧可用。

跨 PID helper/menu 有两个不同身份：用户选择的逻辑 app 与 WindowServer 的实际 surface owner。旧版 public AX client + geometry 只提供 best-effort 关联，不能直接把 helper PID 伪装为 app PID，也不能按矩形重叠猜归属。应在原生层证明并保留关联之后，再扩展 `WindowTarget` 的内部 owner 映射。该部分不能通过放宽当前 owner 检查实现。

浏览器 exact-tab provider 应保留同一 app 参数，tab ref 为 provider 内部对象引用；把 DOM refs、snapshot generation、navigation invalidation、页面截图的 CSS 坐标映射和 tab 生命周期一起迁移。禁止把 native-window PNG 坐标当作 tab viewport CSS 坐标。旧 `native_tools.rs` 的 Host grant/profile 审批逻辑已与当前产品方向冲突，不应取回。

## 验证要求

- 新增 surface 的正向证据：独立 owned fixture 有普通窗口与 raised popup；`windows()` 同时列出，分别截图，目标图不含其他 app；语义 action 或截图绑定点击有独立计数读回。
- 同 app 非目标窗口开关、其他 app 窗口/指针变化不应消耗目标绑定。目标本身关闭、移动、owner 改变应返回可刷新错误，并证明未重放输入。
- 后台点击与物理 fallback 分别验证；截图可用不等于能前台点击，AX 成功也不等于 UI 实际效果。
- 跨 PID helper 与 exact-tab provider 尚未完成前明确列为未支持，不借旧 RC.9 或旧浏览器验收宣称当前通过。
- Windows、Linux、锁屏、多显示器与系统菜单的覆盖各自记录；macOS 单机验收不外推。

## 本轮验证入口

`tests/acceptance/application_surface_probe.py` 复用现有 owned AppKit fixture 和标准 MCP 客户端，创建唯一标题的 `.floating` 窗口，并用独立 CG 元数据确认其非零 layer。它只执行一次有效点击，检查该 surface 收到的一对 down/up、普通窗无误投，以及关闭后旧引用拒绝；全程不要求真实鼠标静止。Fixture 保留关闭的 NSWindow 对象，覆盖全量库存仍有 backing 记录的原始情形。

```sh
DTW_AX_READINESS_TRACE=1 DTW_SCENE_TRACE=1 python3 tests/acceptance/application_surface_probe.py --dtw <signed-binary> --output <fresh-directory>
```

四项浮层 oracle、二十项后台 oracle、Swift fixture 的 warnings-as-errors 编译和 Python 语法检查通过。真实候选 G（SHA-256 `3c69c2d9e840b02cf7c70ae762be84e0c4786f57ccc5d9125e1d185022451b1f`）的浮层闭环通过：独立 CG layer 为 3，截图只含目标 surface，一对 left down/up，普通窗计数未变，关闭后旧 ref 拒绝。本地证据 `/private/tmp/dtw-application-surface-g/evidence.json`；这不推导跨 PID 菜单、锁屏或 exact-tab 支持。

## 历史源码

- [RC.9 app observation](https://github.com/caelis-labs/desktop-world/tree/79a097b3b479fbf65926fdd6a58c296d8d1e1e1e/cmd/dtw/src/app_view)
- [旧版 app_view 调用与绑定](https://github.com/caelis-labs/desktop-world/blob/26247c4/cmd/dtw/src/app_view.rs)
- [旧版浏览器薄 wrapper](https://github.com/caelis-labs/desktop-world/blob/8fe6910/cmd/dtw/src/native_tools.rs)
- [旧版浏览器接口与坐标说明](https://github.com/caelis-labs/desktop-world/blob/8fe6910/packaging/plugin/skills/desktop-world/BROWSER.md)
- [历史 screen-sharing 调研](../poc/associated-capture/screen-sharing-research.md)

最终候选 L（`0f6d9ffee25bf3c90965e64cae81669ff225801388bdd8d7c5ea4d6343ca8c2b`）在 `/private/tmp/dtw-final-l-surface/evidence.json` 再次通过同一浮层闭环；其范围仍仅同 PID surface。
