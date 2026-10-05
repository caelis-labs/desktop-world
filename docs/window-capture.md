# P2 独立窗口内容捕获

`window_content` 从原生窗口取像素，不从共享桌面截图裁出窗口。macOS 使用 ScreenCaptureKit 的 [desktopIndependentWindow](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/init(desktopindependentwindow:)) filter 和一次性 screenshot；不激活应用、不移动系统鼠标、不发送键盘输入。

## 按需发现与捕获

先用 summary 找到应用 Ref；在该应用范围显式查询 `capture_windows`，默认只披露名称、角色与归属，32 条 / 8 KiB，不生成图像。再单独请求有像素预算的捕获：

```js
const windows = await dw.captureWindows(appRef);
print(dw.list(windows, ['ref', 'name']));
// 依据完整目录选择精确 Ref；同名窗口须进一步区分，不能取第一项。
const image = await dw.capture({kind:'window_content', target:windowRef,
  max_pixel_width:1024, max_pixel_height:768});
print(image);
```

Go 对应 `ProjectionCaptureWindows` 和 `Actor.Capture`。窗口目录只接受 fresh 观察；有访问/输出预算，截断保留 coverage。完整目录仅描述这次采样时可分享且在屏幕上的原生窗口，不保证窗口随后仍存在。隐藏/最小化窗口不列为当前可捕获对象。图片只在宿主启用 capture 并设置 assets directory 后保存；图片文件由宿主管理生命周期，结束回合不删除宿主文件。

## 原生身份与坐标

macOS 公共 AX API 不能可靠提供用于 ScreenCaptureKit 的窗口 ID。因此目录返回专用 `role:capture_window` Ref，直接绑定 [SCWindow.windowID](https://developer.apple.com/documentation/screencapturekit/scwindow/windowid)、拥有进程及进程启动时间；不按标题/几何关联 AX 窗口。其 App 归属保留原生进程身份。普通 AX 窗口请求 window_content 返回 `capability_unavailable`；在 AX 窗口范围查询目录返回 `capture_identity_unavailable`，不偷偷扩大到整个应用。专用 Ref 可取 detail 和图像，不提供 AX 子树、focus 或语义控件动作。

每次捕获前后均重新枚举 shareable content，并校对 WindowServer 的当前窗口身份/可见性；窗口关闭或进程实例更换后旧 Ref 不重绑。同名窗口重建得到新 Ref。原生窗口 ID 的跨采样复用和系统/provider 状态的瞬时变化仍受公共接口的可观察性限制；不是持久对象 ID 或像素/AX 的同步快照。

窗口图像的 `Target`、`ImageFrame`、`ImageToTarget` 描述窗口局部坐标；`DesktopFrame` 为空、`ImageToDesktop` 为零，不授予共享桌面点击坐标。移动/缩放后重新捕获，不能沿用旧图像推导桌面位置。完整窗口捕获不接受 Region 或 IncludeCursor。macOS 14.2+ 排除 child windows，sheet/弹窗需使用各自原生 Ref；14.0/14.1 的 child-window 组合行为未实机验证。

## 失败、权限与预算

- 屏幕捕获权限不足返回 `permission_denied`；宿主可以按 `Intent.CaptureKind` 限制窗口/桌面捕获。窗口读取范围不等于桌面读取范围，交付后和 ReadAsset 前仍重新检查授权。
- 已知隐藏/最小化返回 `window_not_visible`，失去原生身份返回 `ref_gone`，无法证明可分享性返回 `window_unavailable`；不恢复旧 PNG。SCK 明确报告 blank/idle/suspended/stopped 等非 complete 状态时拒绝交付；未带 stream 元数据的一次性截图只按该次请求返回，不缓存复用。
- 异步枚举和 screenshot 有取消/期限；回调不持有已释放的 C 取消指针。超时/无有效 sample buffer/几何变化分别返回失败，不发布 Asset。`CapturedAt` 是交付相关时间，不能声称是显示内容的精确生成时间。
- 默认像素上限为 1920×1080，可显式调小；单边最多 8192，原生单幅 surface 限 16 Mi pixels，最多 16 tiles / 32 MiB PNG。模型文字输出预算与图片成本分别记录，不把 bytes/pixels 当作 token。

## 平台与验收范围

macOS arm64 上独立 F9 通过 dtw 实际完成：后台画布被另一应用覆盖期间红→绿更新，前台持续 Unicode 输入，指针/焦点稳定，移动缩放、蓝色弹窗/Sheet 分别捕获，父窗口排除 child pixels，最小化/隐藏拒绝并恢复，同名窗口重建拒绝旧 Ref，宿主关闭 capture 和结束回合拒绝请求。见 [实机证据](evidence/window-capture-20261004/README.md)。

本机只有一个显示器。本轮没有锁屏、断开显示/登录会话或跨屏操作证据；错误/状态拒绝路径由契约注入覆盖，不能冒充这些实机场景通过。更多应用、最低 macOS 版本、混合缩放与上述场景仍待专项验证。

Windows 实现绑定已有 UIA HWND / 进程实例，以 [PrintWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow) 请求 provider 渲染完整窗口，检查 interactive desktop、隐藏、最小化、[cloaked](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)、销毁和几何变化，渲染后按预算缩小。不使用桌面 StretchBlt 作为窗口捕获后备。2026-10-05 在 Windows 11 上修复 Chrome 黑图：使用 `PW_RENDERFULLCONTENT`，Chrome、记事本和计算器的实际图像均已查看确认，见 [Windows 验收报告](windows-validation.md)。PrintWindow 由应用处理且同步执行，可能阻塞或对其他 GPU/UWP 返回无效内容；managed helper 可隔离并停止阻塞进程，直接嵌入调用可能 `close_incomplete`。availability 保留 unknown，单机成功不能证明任意 provider 都会产生正确新帧。

Windows 窗口渲染临时采用 [目标窗口的 DPI awareness](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowdpiawarenesscontext)，退出渲染时恢复 worker 的物理像素上下文；完整 surface 映射至之前校验的物理窗口 frame。2026-10-05 在本机 125% 缩放下验证 DPI-unaware Win32 正常、调整尺寸及隐藏后重绘恢复的图像，修复原先因逻辑/物理尺寸混用产生的额外黑边；另查看 Electron 图像。隐藏/最小化仍拒绝，不强制显示或重绘应用，不用像素启发式猜测合法黑色画布。
