# Windows tooltip/shadow 上游调研与实机边界

本轮按用户追加要求，对照了 trycua/cua 与 vercel-labs/agent-desktop 的 Windows 实现，并用真实 Chrome 原生文件选择器复测其可用路线。结论限定于本机 Windows 11 23H2、build 22631，以及 DTW 的应用隔离、独立 alpha 合成合同；**不声称 Windows 在所有版本和捕获模式下都无法截取 tooltip/shadow**。

完整含 `SysShadow` 截图在本轮保留为技术/合同限制，按用户“如确实属于技术限制，可先跳过”的指示跳过；S03 的文件选择、接收者与恢复截图继续单独验证。没有把 unsupported、失败候选或桌面截图改记为应用隔离截图通过。

## 固定的上游源码

| 项目与版本 | 读到的实际路线 | 对本问题的结论 |
| --- | --- | --- |
| [trycua/cua `ecaf10b`](https://github.com/trycua/cua/tree/ecaf10b1973f304e2e6088adc063069031a5e6ab) | [capture.rs](https://github.com/trycua/cua/blob/ecaf10b1973f304e2e6088adc063069031a5e6ab/libs/cua-driver/rust/crates/platform-windows/src/capture.rs) 使用 PrintWindow/GDI，黑帧或遮挡时尝试 WGC；失败后可用 GetDC(NULL) 的 screen-region BitBlt，并报告遮挡。[wgc.rs](https://github.com/trycua/cua/blob/ecaf10b1973f304e2e6088adc063069031a5e6ab/libs/cua-driver/rust/crates/platform-windows/src/wgc.rs) 使用 CreateForWindow、free-threaded frame pool、D3D11 staging readback | WGC 的 HWND 准入与本轮实测相同；桌面区域备选能记录可见的最终画面，但会扁平化背景，不提供独立窗口 alpha/无遮挡应用像素证明，不能直接移植为 DTW 的成功路线 |
| [vercel-labs/agent-desktop `6660fba`](https://github.com/vercel-labs/agent-desktop/tree/6660fbac768dc7e7332837b90ed465684859afa3) | [capture_backend.rs](https://github.com/vercel-labs/agent-desktop/blob/6660fbac768dc7e7332837b90ed465684859afa3/crates/windows/src/system/capture/capture_backend.rs) 优先 Modern，失败转 Legacy；[capture_modern.rs](https://github.com/vercel-labs/agent-desktop/blob/6660fbac768dc7e7332837b90ed465684859afa3/crates/windows/src/system/capture/capture_modern.rs) 同样使用 CreateForWindow；[capture_window.rs](https://github.com/vercel-labs/agent-desktop/blob/6660fbac768dc7e7332837b90ed465684859afa3/crates/windows/src/system/capture/capture_window.rs) 使用兼容位图和 PrintWindow，空图时从 flags=2 改为 flags=0 | 发现了可实测的不同 GDI 路线，已在本机复测；它没有给本次真实 SysShadow 返回可用阴影 alpha。所读代码与原生 pattern 测试不能当作该系统浮层已通过的上游证据 |

上述文件的上游 Git blob SHA 分别为：CUA capture `453b0b715f06e00d81a360231902d436385caf9e`、WGC `5533190ecb8b4812dedbc729505a91cf09e036e1`；agent-desktop backend `f33a96f1841b86ea4e456e41a2bc5d014cb63fb4`、modern `246fda5a66f6f9534ffed7c2ea55ffe2fc4c146d`、legacy `5a619c5fffec0290bb8902c5e0273fc0265c2cbb`。本轮没有导入上游运行时、代码或资产。

## 本机真实验证

原始输出位于 `artifacts/pr107/`，其中 `chrome-a01`、`chrome-a02`、`chrome-a03` 均保留原失败、wire frames、观察和清理记录。

- `chrome-a01`：鼠标经过真实截图复核后停在原生 chooser 的文件行，观察到 `tooltips_class32`，原应用合成返回明确 `windows_surface_unsupported`；`chrome-a03` 的独立 Win32 诊断进一步确认了 tooltip 与 `SysShadow` 两个实际窗口。
- `chrome-a02`：实验性精确 HWND WGC 在同类真实窗口的 `CreateForWindow` 返回 `0x80070057`；没有发生帧池或 GPU 帧读取，延长帧等待不解决这次准入拒绝。实验候选仅留诊断，相关代码和依赖未加入产品。
- `chrome-a03/native-surfaces.json`：同一个真实 tooltip/shadow，比较 DIB section / CreateCompatibleBitmap、PrintWindow flags 2 / 0、黑/白初始底色，共 16 组读取。tooltip 为 489×125；shadow 为 494×130。
- tooltip 的 flags=2 返回 61125 个全不透明像素；flags=0 返回 61121 个不透明像素和四个透明像素。它能提供控件图像，并不能证明 SysShadow 的半透明像素。
- SysShadow 在所有 flags=2 变体中均为 64220 个不透明黑像素；flags=0 均为 64220 个透明黑像素。初始黑/白底色及两类位图均未给出真实半透明阴影。原始 alpha 直方图、RGB 诊断 PNG 与独立实际桌面 PNG 分别留存，未将不透明 RGB 诊断图冒充有效 alpha。

两套上游的普通 WGC 窗口路线不会绕过这个实际 HWND 拒绝。Microsoft 的 [IncludeSecondaryWindows 合同](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.includesecondarywindows) 提供把相交 secondary windows 画入主窗口纹理的另一种路线，但要求 Windows 11 24H2 / build 26100，默认关闭，并在主窗口边界裁切。本机 build 22631 不具备该 API；它还需要重新验证整个 owner/secondary-window 合成、几何、取消和资源生命周期，不能当作本轮已经实现的修复。

这里的“限制”来自实测拒绝、GDI 结果和已声明的应用隔离合同，并非以缺少证据推导平台永久不可能。后续可在 24H2+ 专门环境评估 secondary-window 捕获，或另行设计带明确授权与遮挡证明的桌面复合模式。

## 本轮处理

保留完整窗口排序，仅已知 `tooltips_class32` / `SysShadow` 不参与语义接收者选择；未知 popup/menu 没有获得新豁免。per-pixel alpha 不可证明时，截图仍明确失败且不建立坐标绑定。文件流程可通过 fresh 语义读取证明真实文件对话框接收者，再执行新的明确键盘请求；输入使提示自然消失后检查准确文件内容与新 PNG。没有预停放鼠标、隐藏原失败或重发未知输入。
