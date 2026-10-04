# 语义选择、勾选与滚动到目标

本批对应补全计划 P1，helper CLI 为 `dtw`。每项保留 scope、Operations、Authorizer、完整写前刷新、Ref 身份、原收据和 unknown 边界，不主动聚焦、移动鼠标或发送共享键盘。`set_value` 继续写入文本/标量值；它不能统一表达这些布尔状态。

| 动作 | 参数 | 自动验证 | macOS | Windows 实现（不承诺可用） |
| --- | --- | --- | --- | --- |
| `set_selected` | `set_selected:{selected:true/false}` | selected 达到指定值 | 可写 AXSelected | UIA SelectionItem AddToSelection / RemoveFromSelection |
| `set_checked` | `set_checked:{checked:true/false}` | checked 达到指定值 | checkbox 可写 AXValue；否则从 freshly known 状态执行一次 AXPress | TogglePattern 从 On/Off 状态最多 Toggle 一次 |
| `scroll_into_view` | 仅 target | offscreen=false | provider 广告 AXScrollToVisible，并能建立 fresh 视口位置 | ScrollItemPattern ScrollIntoView |

布尔值不能省略，false 不丢失。即使 completion=dispatch，仍自动验证；已有状态满足时不调用 provider，收据为 semantic / not_applicable / verified。写前无能力时 delivery=none；已进入 provider 后的原生错误保守记为 unknown，即使 provider 回复未支持，也不假设副作用未发生。验证失败与 unknown dispatch 均不自动重试，不把“发送完成”等同于“状态或业务完成”。额外 after 条件仍须满足。

选择动作只写请求项，不主动清空其他项。Windows 不调用清空其他选择的 Select；容器只能单选、要求至少一个选择等约束可能拒绝 Add/Remove。macOS 的单/多选联动由 provider 决定，可能改变其他项；需要保留其他项的业务必须额外校验。F6 的独立多选场景验证 Order B 保留。

mixed/indeterminate checked 保持 unknown，不转换为 false。可写原生 setter 能指定期望值时可使用 setter；只能 toggle 且无法证明当前 On/Off 状态时拒绝。不会在原生调用超时或验证失败后再 toggle，也不会转成 Space、click 或 wheel。

滚动只证明目标与原生视口有非空交集，不保证完整显示、未被其他窗口遮挡、窗口捕获新鲜或共享桌面可点击。macOS 用当前 AXPosition/AXSize 与 parent 链上的 AXScrollArea/AXWindow 交集推导；链不完整、超过 32 层、几何不可读均保持 unknown 并阻止该能力。后台被遮挡不会被当成 offscreen；这个状态不授予物理输入权限。

Apple 的公开 [scrollToVisibleAction](https://developer.apple.com/documentation/appkit/nsaccessibility-swift.struct/action/scrolltovisibleaction) 常量从 macOS 26 提供；库保持 macOS 14 部署目标，通过文档对应的 AXScrollToVisible wire 名调用，仅以 provider 的广告与当前状态决定能力。实机成功记录使用系统 WebKit provider，不能推导所有 AppKit 控件或所有 macOS 14 应用均支持。Windows 的依据是 [SelectionItem AddToSelection](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationselectionitempattern-addtoselection)、[TogglePattern](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomationtogglepattern) 和 [ScrollIntoView](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationscrollitempattern-scrollintoview)。编译/API 依据不替代实机适配。

## Agent 使用与成本

先 summary → 已有窗口 Ref → name/role 的有界查询；只对目标请求 states/capabilities。标准控件和异步 WebKit AX 树都使用原路径，小范围重复只读查询有次数上限，不用输入初始化或全桌面大树。

```javascript
const item = dw.one(await dw.find(state.window, {name_equals:'Approve order'}), {role:'checkbox'});
await dw.check(item.ref, true);
// 观察当前提交控件后，以支持的语义动作完成业务并确认结果。
```

`dw.check`、`dw.select`、`dw.scrollIntoView` 是窄参数入口。目录与逐动作 schema、32 次脚本调用/60 秒/8 KiB print、24 KiB outline 页系列预算保持原有上限。错误、coverage、channel、delivery、verification、原收据不因 compact 而消失。SDK 次数与文本 bytes 仅说明接口体积，没有运行 LLM，不能换算成实际 token 节约率。

实机证据与调用量见 [P1 独立验收](evidence/semantic-actions-20261004/README.md)。每项新应用/新 helper/唯一标题，前台持续 Unicode 输入，后台实际业务回调、时间重叠、no-op、false、未支持拒绝、原收据均独立检查。Windows 全部实机验收后置，不承诺当前 Windows 可用；Bot M0 与联调继续后置。
