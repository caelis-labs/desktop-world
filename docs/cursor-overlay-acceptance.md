# Independent Agent cursor — checkpoint 2 acceptance

2026-10-30，检查点 1 提交 `e5525d8` 之上完成。签名开发候选 V 的 69 个源码/构建文件与验收时源码一致；完整哈希、探针源码、原始记录定位见 [机器可读证据](evidence/cursor-overlay-2026-10-30.json)。这不是发行、公证或外部 LLM Host 验收。

## 已落地

每个标准 MCP 会话共享一个延迟启动的 macOS 光标 helper；跨 App 保持标识、颜色和操作序号。只呈现实际成功投递的指针事件，跳过键盘、AX 语义动作、primer 和清理事件。输入仍由现有 CUA port 与前台路径执行，光标呈现失败不改变输入回执、不触发重投。

透明、不可交互、非激活的普通层级窗口跟随目标。Scene 只豁免经进程出生时间和 WindowServer 所有权复核的确切自有/同版 peer 窗口。窗口移动、关闭、布局变化使旧光标操作终止；取消隐藏对应操作。helper 退出后，下一独立动作只有在旧实例清理已确认后才能重建，会话身份不变，旧 hide 和旧 owner Drop 不影响新实例。实现与固定 CUA 来源见 [port 说明](cursor-overlay-port.md)。

## 验证结果

- 335 项 Rust library 测试通过；轻量源码检查通过，包含 160 项 Python 控制。Native Swift inventory/input/focus/scene/overlay 控制通过；没有完整编译上游 CUA。
- AppKit 真 GUI 全流程通过：实际点击与 marker 像素、跨 App 同会话、独立双会话、前台路径的确切窗口豁免、helper 退出后的新动作恢复、普通取消、stdin EOF、父进程退出、目标移动/关闭。
- 两个 marker 在截图前后均实际存在；`app.capture` 的静态 ClickPad 区域覆盖 marker，25,024 像素逐 RGBA 比较为零差异。此证据只覆盖该区域。
- 独立 Chrome profile 真实任务通过后台输入、点击、滚动，快捷键/Unicode 的前台回退，业务结果读回和自有资源清理。混合键盘流程不宣称全程后台；遮挡覆盖只在采样边界建立。
- 生命周期保留原 execution ID、回执、独立终止与窗口消失证据。进程消失或无失败日志不等于取得原生 worker 的零退出码。

## 保留的失败与限制

T/U 的探针判断、取消响应收集、生命周期字段和可见性前提失败全部保留，没有改写为通过。U 有一次在光标启动前发生 `AXFocusedApplication -25204`；独立 S/U/V Chrome 对照通过，但该次偶发根因未确认。

V 初轮及 S 对照在新窗口 AX 映射/前台准备阶段失败。fixture 采样前台 PID 为 603，随后只读核对为 loginwindow 且系统明确锁屏。用户解锁后的新全流程通过；锁屏操作不在当前支持承诺内，不以这些失败推导光标回归。验收不要求用户全局停用鼠标。

没有新增真实多显示器、Space 切换、锁屏、Windows/Linux 光标或 live LLM Host 验收。marker 独立窗口截图不证明它穿透其他不透明窗口可见。父进程骤停可能遗留无效的私有 registry 路径；这些路径不构成窗口豁免证据。
