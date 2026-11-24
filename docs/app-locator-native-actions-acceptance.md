# App 定位器与原生动作：候选验收

基线是 DTW 0.6.0（`6e10d17`）。[对比报告](cua-dtw-atomic-comparison-2026-11-24.md)讨论为什么优先改这几处；[随包 API](../skills/dtw/API.md#methods)记录实际调用合同。公开 MCP 工具仍只有 `apps` 和 `exec`。这里的测试不安装软件包，不修改用户文档，也不运行 CUA 驱动。

## 实机方法

在 macOS 27.2.1 arm64 上，用编译的 release 二进制通过 stdio MCP 操作一次性 AppKit 应用。应用包含 WKWebView、同名和近似名称控件、两个独立滚动区，以及原生菜单和弹出按钮。[验收脚本](../tests/acceptance/macos_owned_gui_probe.py)记录二进制 SHA-256、每步回执、应用端事件和退出结果。输入次数与业务结果分别断言；不以“脚本成功”代替应用端证据。[运行方法](building.md#external-client-gui-probes-and-historical-assertions)。

首个候选 `e7f7cb9b5ef5f595deb7681eecc0a269397f215c0e74071a65b9beca2ed519e0` 在真实窗口中通过了精确名称点击、原生字段写入和状态等待、WKWebView 备注提交、嵌套菜单命令及上下文菜单检查；**另外三组失败**，不能计作该候选通过：

- 按元素滚动返回 `invalid_observation`，两个区域均未滚动。原生锚点应答缺少 Rust 校验器必需的 `surface_owner`，导致 DTW 主动退役工作进程，并非应用收到了滚轮却没有滚动。
- 原生下拉选项返回 `unsupported_capability`，未记录选项业务事件。
- 中键坐标点击被仅支持左右键的旧投递路径拒绝，目标未收到事件。仅测试 CGEvent 的构造和配对无法发现这个路由错误。

第二个候选 `59fc667d0d43f69a50483fb524b61f9ef5d85b8bae6ddbe71fb66d482a33b423` 修复了上述三项的生产路径。定向复测中，只有左侧区域收到一次滚轮（偏移 `0 → 48`，右侧不变）；原生菜单关闭后，`Finance` 在索引 1 触发恰好一次业务回调；普通中键送达一次 down/up。该候选**仍未通过修饰手势验收**：带四个修饰键的中键事件送达，但在鼠标释放后，AppKit 的会话修饰键读数仍为 `1966080` 而非 `0`。因此不发布这条派发路径，所有非空 `modifiers` 在输入前明确拒绝，而不是用附加按键或鼠标事件尝试恢复。

## 最终冻结候选

**macOS 27.2.1 arm64，SHA-256 `de3030f70b3e110fa6245835a623050e05e3411aa9de56a0b0b5daf6318e49c6`。** 使用同一份 release 二进制、全新夹具和私有证据目录，一次运行全部 **13/13** 组（含夹具就绪检查），无重试。业务事件来自应用进程，不是以 MCP 回执代替。

| 验收合同 | 应用端或协议端证据 |
| --- | --- |
| 精确定位与等待 | `Save` 和 `Save As` 各触发自己的按钮一次；重名、错误大小写或前缀不产生点击。原生字段从禁用变为启用；写入中文和 emoji，再按值读回。提交回调一次，相邻字段未改。移除控件后才满足 `detached`。 |
| WKWebView | 备注字段写入、读回后显式提交，业务回调一次，另一字段保持原值。Web 下拉控件的原生选项请求被拒绝。 |
| 滚动锚点 | 左侧收到一个滚轮事件，偏移 `0 → 48`，右侧保持 `0`。过期 ID 或同时指定 ID、坐标时，两区均不动。 |
| 原生菜单 | 精确路径只触发目标分支的 `Record`（tag 11）；错名、歧义及禁用命令均无业务回调。`showMenu` 在声明该能力的控件上触发一次请求，夹具记录上下文菜单打开、关闭；普通按钮拒绝。 |
| 原生下拉选项 | `Owned Options` 打开并关闭，随后恰好一次 `Finance` 回调（索引 1），读回选中值。错名、重复或禁用项不改变选择，且未留下打开的菜单。 |
| 坐标手势 | 前台可见点的普通中键和随后的左键各收一对 down/up。合法非空修饰键返回 `unsupported_capability` / `not_dispatched`；重复键返回 `invalid_arguments`。两种拒绝都未产生按键或点击，AppKit、HID 和会话修饰标志及按下的鼠标按钮均为 0。 |
| 新窗等待 | 一次按钮请求后，首次等待返回 `wait_timeout`；夹具放行新窗后，只读续等匹配同一 PID 的 `DTW Owned Second Document`。再等返回 `input_required`。日志只有一次请求、一次开窗，没有重放。 |

夹具和 MCP 均正常退出（退出码 0，无强制终止），两个原生工作进程均被回收。另在同一二进制上运行 8 项无桌面 worker 关闭控制（两个角色，各覆盖 TERM、EOF、坏输入和父进程退出）。原始日志、截图、上游浅克隆不随 PR 分发。

## 非 GUI 验证

- 最终源码运行 `cargo +1.98.0 test --release --locked --manifest-path cmd/dtw/Cargo.toml --lib --bins --tests -- --test-threads=1`：701 项库测试、6 项 CLI、3 项 MCP stdio 全部通过。受控后端覆盖模态路由、兄弟进程隔离、未知派发不重放、截断或不可读的定位证据以及失败回执归属。
- `sh scripts/check-source.sh`、两组 Swift 原生合同（inventory、input）均通过；Windows/Linux provider 单独做过交叉类型检查。它们不替代对应平台的桌面实测。

## 结论适用范围

这是开发候选的直接 MCP 验收，不是已发布插件、模型成功率 A/B 或跨平台 GUI 认证。原生 setter 的读回不证明 DOM `input` 或保存；菜单命令回执不证明业务完成；元素滚动仍是有符号步数的滚轮操作，不是 AX 翻页或 scroll-into-view。非美式键盘布局、混合 DPI、跨应用拖放、锁屏及未测试应用不作支持承诺。
