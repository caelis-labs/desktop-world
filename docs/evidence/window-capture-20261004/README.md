# P2 独立窗口捕获 · macOS 实机证据

后续更新：2026-10-05 新增 Windows Chrome/Notepad/Calculator 窗口图像实测，见 [Windows 证据](../windows-20261005/README.md)。下文保留本轮 macOS 矩阵与当时状态。

2026-10-04，macOS 27.0.1 arm64 / Go 1.26.8。使用现有 Accessibility/input/screen_capture 授权，没有修改系统权限。此目录只包含受控测试应用的日志与像素。

最终独立全批 run 为 `20261004T074444Z-3d1d57`，F1/F2/F3/F6/F7/F8/F9 各使用新的应用进程、唯一窗口、独立 dtw 和应用自身日志。见 [summary](summary.json)、[原始测试输出](native-all.txt)、[完整检查](check.txt) 和 [85 项源码指纹](source-sha256.json)。构建记录的 base revision 为 `33729da`、modified=true；85 项源文件逐项 hash 与本次提交的实现/测试一致，文档/evidence 不参与源码指纹。合并后的分发包另从 clean exact HEAD 构建并验收。

F9 通过 Desktop World 完成实际电脑操作，背景应用日志为 [f9-background.jsonl](f9-background.jsonl)，前台独立输入日志为 [f9-human.jsonl](f9-human.jsonl)，[测试断言与结果](f9-test.txt)；10 张 PNG 的阶段、SHA256 和 bytes 见 [images.json](images.json)。

| 独立场景 | 结果 |
| --- | --- |
| 遮挡下更新 | 前台另一应用覆盖后台画布，原生捕获从红变绿；更新来自应用自身 callback，没有桌面截图裁切 |
| 人类输入 | 另一 dtw 模拟真实 Unicode 输入 `Human-🙂-capture`，独立事件日志完整；前台窗口和系统指针稳定，后台没有共享键鼠事件 |
| 移动/缩放 | 画布改变位置与尺寸后，PNG 与局部映射更新；没有桌面点击映射 |
| popup / sheet | 各自原生 Ref 捕获蓝色内容；父窗口绿色区域无蓝色 child pixels。Sheet 引起的父窗口变暗属系统视觉状态，不误判成旧帧 |
| 最小化/隐藏 | 拒绝捕获、不创建新 Asset，恢复后同一窗口重新生成有效绿图 |
| 同名窗口重建 | 旧 Ref 返回 ref_gone，新目录返回不同 Ref，新画布确为红色 |
| 捕获边界 | 普通 AX 窗口 Ref 拒绝；宿主未启用 capture 的 helper 拒绝；结束回合后捕获拒绝 |
| 授权与失败 | 契约另外覆盖窗口范围不扩大成桌面范围、交付后授权撤销、ReadAsset 权限失效、未知/超时/几何变化不交付 Asset |

F9 费用计量：后台 **41 次数据调用 / 39,654 projected text bytes**（包含拒绝路径），前台人类模拟器 **21 次 / 23,067 bytes**，另一个关闭 capture 的宿主做 1 次拒绝探测；BeginTurn/Grant/EndTurn/Close 的控制通道调用不计为模型数据调用。10 张图合计 **302,373 PNG bytes / 3,248,160 pixels**，每张均限制 640×640。这里的 bytes/pixels 不代表模型 token，未执行 LLM 推理或测量各 SCK getter 的运行次数。

渐进披露：`dtw schema` index 157 bytes、observe schema 2,581 bytes、capture schema 1,252 bytes，见 [测量](schemas.json)。应用 capture directory 只请求 name/role/app，未请求 value/URI/bounds/capabilities；截图才显式申请图像预算。普通 summary 不披露 capture directory。F9 是含正/负路径的验收全流程，正常单次取图无需重复这些 10 个场景。

原有可见区域截图与取消恢复在独立受控应用上另通过 [回归](visible-regression-test.txt)，应用日志与 PNG 同目录。完整检查含 race、vet、Windows build/vet、协议 examples 和 19 个 JS 测试；普通 CI 的 opt-in 桌面测试会跳过，不能替代本目录实机证据。

边界：本机只有一个显示器，没有锁屏、断开显示/会话、跨屏/混合缩放、最低 macOS 版本或更多 provider 的实机证据。相关错误由契约注入验证拒绝，未声称这些实机场景已通过。Windows 所有实机验收/适配后置，当前仅实现与编译/CI，不承诺 Windows 可用或新帧正确性。caelis-bot M0 更新和统一联调未进行。

复跑：

```sh
GOWORK=off python3 scripts/accept-features.py F1 F2 F3 F6 F7 F8 F9
./scripts/check.sh
# 分发包验收（不会重建或替换给定 helper）：
GOWORK=off python3 scripts/accept-features.py F9 --helper /absolute/path/bin/dtw
```

## alpha.4 收尾

alpha.3 的包内 7 项独立 macOS 场景和公开下载后的 F9 均通过。其 tag CI 又暴露了原有 Close 测试的启动竞态：20 ms 步骤期限可能在 native dispatch 前到期，随后 Close 合法返回 nil。alpha.4 用明确的 native-entry barrier 后再检查关闭，并验证解除阻塞后 cleanup 完成；不改 P1/P2 生产实现或原生场景。上述 85 项指纹固定描述 alpha.3/P2 基线；alpha.4 中仅 contract_test.go 的 Close 测试指纹变化，版本指引另行更新。alpha.3 已发布的 tag/assets 不覆写，alpha.4 分发包从新的 clean HEAD 重新验证。
