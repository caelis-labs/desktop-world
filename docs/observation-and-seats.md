# 按需观察与后台输入的边界

本文区分当前实现和后续设计，不将接口提案视为已实现能力。

## 时间与版本

时间回答“何时看到”，版本回答“相对先前样本是否变化”，Ref 回答“哪个原生实例”。这些信息不能相互替代：时间很新不证明对象没被替换；相同版本也不证明当前仍有焦点。

| 数据 | 用途 | Agent 默认呈现 |
| --- | --- | --- |
| observation coverage 的 sample_start/end | 一次遍历不是原子截图；显示整个采样区间 | 保留一次 |
| Object sample_start/end、Fact sampled_at | 精确诊断、跨源样本时差、审计 | helper compact 省略；完整 wire 保留 |
| Version / GeometryVersion | 区分内容和几何变化，做前置谓词 | 保留 |
| Ref / Lifecycle | 身份、过期、失效 | 保留 |
| coverage complete/dirty/truncated/continuation | 没看到不等于不存在；继续分页和验证完整性 | 始终保留 |
| receipt delivery/verification/outcome/fault | 已发送、已验证和未知必须区分 | 始终保留 |

当前不使用跨机器 wall clock 判断动作先后。执行期限应由单调时钟控制；后台隔离后还需记录时钟域。所有写入仍 fresh-read 目标、权限、焦点和能力，不能因为省略展示时间而省略执行校验。

## 当前已有的选择能力

`observe` 的 `fields` 支持：kind、role、name、value_preview、states、bounds、capabilities、app、window、parent、relations、lifecycle。`summary / outline / detail` 控制对象层级，scope 限定范围，match 做有限定位，budget 限制深度、节点、结果数、文本和输出 bytes；长文本通过 `read` 分页，变化通过 `sync` 获取。

```json
{"id":"inspect","op":"observe","args":{"scope":{"refs":["OBSERVED_WINDOW_REF"]},"projection":"outline","fields":["name","role","value_preview"],"budget":{"max_depth":5,"max_results":40,"max_output_bytes":12000}}}
```

这是有界字段选择，不是 GraphQL。当前 states/capabilities/bounds 是整组选择，基础身份/版本/覆盖元数据仍固定存在。**原生后端目前仍可能读取未返回的属性，减少输出不等于等量减少 AX/UIA 调用。** 已将 summary 默认字段收紧为 role/name/app/window。helper 的 compact 是呈现层：已知 Fact 变为 `{known: value}`，未知/不可用/被遮盖状态明确保留；`--full-output` 可返回原格式。

后续适合增加受限的嵌套选择，例如 `states.enabled`、`states.focused`、`capabilities.invoke`、`bounds.rect`、`diagnostics.sample_time`，并将选择下推到原生读取/缓存请求。无需先引入完整 GraphQL 服务、任意递归或任意表达式。内部存储必须区分未请求和未知，保留每字段新鲜度，不能让一次部分读取覆盖为“所有字段都已刷新”。支持省略 diagnostics，并不允许省略身份、分页、错误、未知结果或权限边界。

## 不打扰前台有三个层次

| 层次 | 可以做什么 | 代价与限制 |
| --- | --- | --- |
| 语义后台操作 | observe/read、受支持的 set_value/invoke；未来扩展 select/toggle/expand/semantic scroll | 依赖应用 provider，应用自身可能弹窗或激活；不是任意键鼠 |
| 应用定向事件 | 研究进程/窗口定向输入、应用内逻辑位置 | 与真实 HID 语义不同，不能普遍处理菜单、拖拽、IME、全局快捷键；单独报告能力 |
| 隔离交互座席 | 独立焦点、键盘状态、光标和显示内容 | 独立 OS 会话/VM/远程桌面 backend；应用必须运行在那里 |

当前 `pointer.*` / `keyboard.*` 使用用户桌面的共享 Seat。多开 Actor 或 helper 不会产生第二套系统焦点和指针，初版也没有跨进程 Seat 锁。语义调用不会主动偷偷退化成物理输入，但应用自己的副作用仍可能改变前台。

Windows 的同一交互 window station 同时只有一个 input desktop，系统 cursor 是共享资源；仅 CreateDesktop 不能承诺用户与 Agent 同时各有一套可用的 SendInput 桌面。[Desktops](https://learn.microsoft.com/en-us/windows/win32/winstation/desktops)、[SetCursorPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcursorpos)。macOS 的 combined session input state 汇集同一登录会话的事件源；私有事件源状态或向进程 post event，不能据此宣称拥有另一套 WindowServer 焦点和光标。[Apple event state](https://developer.apple.com/documentation/coregraphics/cgeventsourcestateid/combinedsessionstate)。

UIA Invoke 是调用 provider 的动作，不等同于合成点击，其副作用和阻塞行为依赖 provider。[Invoke guidelines](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-implementinginvoke)。因此 `background_only` 即便拒绝 focus/物理输入，也只能承诺“不主动使用共享输入”，不能单靠此开关保证应用永不抢前台。

## 若实现隔离，需要改变什么

建议保留同一 SDK 内核，新增 Session/Seat backend 和薄 helper：

1. **能力与策略**：区分 semantic/background、targeted events、shared physical、isolated physical；策略禁止静默退化。`background_only` 对不支持的任务明确返回需要交互座席。
2. **会话命名空间**：World 绑定 session/seat/epoch，Ref、坐标 frame、display topology、缓存、权限、receipt 不得跨座席复用。独立 helper 负责进程崩溃与生命周期隔离。
3. **独立执行环境**：应用启动在选定会话/VM 内；输入、捕获、焦点和取消均在那个环境中执行。窗口内容捕获应独立验证遮挡、最小化和后台渲染，不用用户屏幕截图冒充。
4. **恢复与传输**：断线查询原 run、断连停发后续输入、清理持有键鼠、禁止新 epoch 下静默重放。远程应用文件、凭据、剪贴板和下载需要明确交接。
5. **并发验收**：用户前台持续打字/移动指针，Agent 在另一座席完成真实编辑、拖拽、对话框和浏览器任务。记录前台切换、指针偏移、串键、用户输入丢失与任务正确率；主动崩溃/断线后重复验证。

建议先完成语义后台能力分级与无干扰测量，再以隔离 backend 提供真正独立光标/焦点。虚拟显示器、隐藏窗口或画一个 Agent 光标都不足以证明输入隔离。Windows/macOS 隔离会话的实际可运行性、系统版本限制和发行条件需各自实机验证，当前没有实现或验收。

## 本轮对焦点模型的修正

macOS 的系统级 AX 焦点属性在本机出现 `AXCannotComplete`，前台应用的 AX 属性却成功。现从 NSWorkspace 获得前台进程，读取该应用焦点，并检查采样前后前台 PID 一致；焦点引用同时注册其原生关系。

Finder 的内联改名框直接挂在应用下，AXWindow 和 AXFocusedWindow 都返回无值。实现保留“窗口未知”，在 **键盘目标 Ref 等于实时焦点、目标 App 等于实时前台 App、Actor 确实获授权** 时支持该无窗口编辑器；窗口范围不会因此扩大。窗口不明确的鼠标输入仍被拒绝。此为对原 SPEC 假设“所有键盘目标都有窗口”的有证据修正，不是独立后台焦点。
