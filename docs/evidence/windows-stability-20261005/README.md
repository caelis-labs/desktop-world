# Windows 稳定性收敛证据 · 2026-10-05

最终源码工作树轮次 `windows-stability-20261005T114825Z-abee05` 在 Windows 11 amd64、普通权限、2560×1440 单屏、125% 缩放上通过。所有 UI 效果使用真实持久 dtw；自有应用的原生消息、DOM 回调、文件及图像独立判定业务结果。未运行 LLM，不将脚本通过率解释为通用 Agent 成功率。

本目录只保存合成应用的证据，不包含全桌面 inventory、用户浏览器截图或完整 wire。完整成功及失败日志在本机独立 artifacts 目录中。构建为 `1e43d86161f06060db9d47cb053cce163d31515f` 上的 dev 工作树（modified=true），helper SHA256 `de58aca2d0103a0ad6f39ef72d9fc286389ced96675b4f50fae6237df31af2dd`。归档时已逐项核对 [程序源码指纹](source-sha256.json) 与工作树一致；最终干净提交的 RC 包另记录精确 revision 和二进制哈希，不能混用这两种构建标识。

| 证据 | 内容 |
| --- | --- |
| [summary.json](summary.json) | 原生状态与 no-op、列表/树滚动、遮挡拒绝与恢复、窗口捕获；Electron 各项原回执、前后 Seat 与 runtime |
| [native-events.jsonl](native-events.jsonl) | BM_GETCHECK / LB_GETSEL / TVM_GETITEM/GETITEMRECT 独立状态、鼠标消息、覆盖层与窗口恢复 |
| [electron-events.jsonl](electron-events.jsonl) | 中文/emoji、多行、真实鼠标/拖拽/滚轮、网页上下文菜单、HTML 弹窗、33 次精确提交；trusted 事件核对 |
| [electron-native.jsonl](electron-native.jsonl) | Electron 44.5.1 / Chromium 152.0.7977.130、真实原生菜单命令与对话框 response=1 |
| [菜单树覆盖](electron-native-menu-coverage.json) | 本机原生应用菜单未暴露 menu_item；验证的是其实际快捷键，网页菜单另有语义绑定 |
| [第三应用日志](third-events.jsonl) | 事务内用户切换后第三应用继续收到 THIRD-KEPT，旧任务 user_superseded |
| [check.txt](check.txt) | Go race/vet/build、示例、9 个协议样例及 19 个 JavaScript 测试通过，含新基线容量与预算清理回归 |
| [F4/F5 元数据](f4f5-summary.json)、[F4](f4-test.txt)、[F5](f5-test.txt) | 本轮较早 dev 构建的 UIA 续扫与私有管道复验，保留该轮自己的构建版本；最终包还需使用解压 helper 复跑 |

32 轮均先重新观察字段和提交按钮，然后独立填写/提交，回执 restored、原前台窗口和焦点一致，时长 300–338 ms；相同 ID/正文返回同一 RunID，33 次 submit 包含一轮初始 Unicode 示例。取消拖拽通过原 RunID 查询至清理结束；预算等待约 1064 ms 后停止、恢复并 ready。258 UTF-16 文本及 501 ms 拖拽在激活前拒绝。应用退出后旧窗口新写入 ref_gone / delivery=none；终态后 helper 重启的新 Epoch 拒绝旧 Ref。没有强杀持有输入的 helper。

图像已实际查看：

- [正常 Win32](native-normal.png)、[调整尺寸](native-resized.png)、[隐藏/最小化恢复后重绘](native-restored.png)：按目标 DPI 上下文渲染并映射物理 frame，修复额外黑边。
- [Electron](electron.png)：最后一轮中文/emoji、多行内容和自有 Canvas 窗口。

探索轮次保留了旧 Common Controls v5 的 ScrollItem timeout、模态过渡的 ambiguous_target、复用 Chromium Ref 的 ref_stale、快速观察的基线容量错误，以及过早发布重绘事件的仅标题栏 PNG。未重放不确定业务输入；分别采用 v6 fixture、明确唯一的可见 bind、每任务重新观察、有界同步基线缓存和应用自身重绘确认后复验。不能把这些调整解释为任意 provider 的透明自动恢复。

复现、修复边界及发布范围见 [Windows 验收报告](../../windows-validation.md)；[同提交 Mac 复验](../../rc-validation.md) 仍是跨平台 RC 门槛。
