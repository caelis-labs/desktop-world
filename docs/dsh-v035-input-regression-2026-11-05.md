# DSH v0.3.5 输入问题修复与实机验收

实现基调：优先保证 Agent 功能与明确结果，其次改善后台操作、真实光标与焦点体验；
协议保持 `apps` / `exec` 和少量清晰的 `app` 方法。路由、必要等待及清理由 DTW 内部处理。

本次工作基于 v0.3.5 `3911173b2161bb2eff8eb2e52373df336b836599`，在独立工作树
`/private/tmp/dtw-dsh-v035-fixes-20261105` 的 `codex/dsh-v035-input-fixes-20261105` 分支完成。
原始 DSH 调用、失败尝试、冻结二进制与 GUI 原始记录位于
`/private/tmp/dtw-dsh-v035-audit`。本报告区分源码检查、真实 GUI 效果与观察质量。

## 已确认的问题

| 问题 | 根因和修复 |
| --- | --- |
| DSH 把键盘及坐标点击普遍解释成必须前台、无遮挡 | 实际默认策略是 `auto`。原版普通 AppKit 的遮挡点击和后台中文输入已经可用；不能把失败泛化为 macOS 的普遍限制。 |
| 无窗口快捷键被 `foreground_required` 挡住 | 应用级快捷键绕开了自动前台规划。现在完整清单证明无窗口后，由同一执行 scope 自动借用；新窗口就绪由内部只读等待处理，不重放快捷键。 |
| 后台没有可保留的 key window 时不能正确切换路线 | 将明确的 AX noValue 识别为干净的能力拒绝，允许自动借用；任意 AX 错误仍不能授权重放。 |
| TextEdit / NSRemoteView 的“新建文稿”坐标点击无效果 | 到达宿主的定向事件不等于远程控件生效。标准 Open 面板按角色和 `AXIdentifier=open-panel` 在派发前选物理路线；resident 原先忽略 `visual`，错误注入 make-key records，现使用适当的视觉激活序列。普通窗口仍保留后台路线。 |
| `⌘.` 已取消面板却返回 `unknown` | CoreGraphics 对 Command 等修饰键两条边都生成 `FlagsChanged`。旧管线漏掉配对释放，把松开当成新输入，访问已销毁的 AXParent。现在按 keycode/flag 区分方向，保留正确释放类型，仅向原进程释放。 |
| 前台 `⌘W` 已关闭文稿却报焦点不可证明 | 完成检查再次要求旧 AX 焦点存活。实际输入完成后只保留本地取消/截止时间与释放检查；输入前的目标证明不变，`focus()` 继续验证焦点后置条件。 |
| 新面板冷态导致归还时 AXFocusedWindow 短暂 cannotComplete | 在原清理预算内只重读完整所有权证明；不重发激活或输入，不忽略进程变化及外部焦点选择。 |
| 面板换成新文稿后误认为外部抢焦点，不归还前台 | worker 阻塞 stdin 时停住 Cocoa 主事件循环，旧归还通知积压到下一轮才送达。改用有界输入线程，主线程空闲和接纳请求前处理原生回调；不忽略真实外部 leave/return。 |
| 光标审计可能放过永久偏移，并把边界精度误差算成异常 | 增加请求结束归位检查；实际归还必须读回确认，不能只发出 warp；内部路线与区间时间戳保留亚毫秒精度。 |
| 50ms 空档与 NSWorkspace 缓存造成验收误报 | 取消固定空档失败门槛，记录实际最大间隔；观察器改用即时 WindowServer PSN/PID/PSN，历史激活通知仅作辅助。 |

新增 `await app.focus()`：明确确保当前执行中的授权应用/窗口在前台；在同一脚本内进行相关输入，
结算时归还原前台所有者。正常输入自动选路线，不需要先手动 focus。
窗口关闭后的特殊处理仅服务归还，不授权后续输入；外部焦点选择永久终止当前借用。
错误指导也改用真实存在的 `app.look()`，不再提示不存在的公开 `waitForWindow`。

## 验收标准

功能先检查实际 UI 后置条件及工具错误/派发结果；观察单列可用性和体验，采样空档只是诊断。
`observed-pointer-borrow-v5` 连续读取真实系统光标：

- 未观察位移与“借用后归还”分别报告，临时移动也不能称为全程独立。
- 位移必须对应当前动作的有效借用区间，结束必须归位；缺失证据不能当作成功。
- MCP 调用以外的位移单列，标为 actor undetermined，不只凭位置归因于 DTW。
- 采样不能证明无限时间分辨率下绝无变化；原始间隔和边界不确定性保留。

## 最终验证

最终冻结候选为 `candidate-14/bin/dtw`，SHA-256：
`ecc870c6ee2306a6878eb0dfdd74362ac07accc2b5d76c4437c5e5299c541f6b`。
`candidate-14/identity.json` 保存二进制和完整 `cmd/dtw` 源文件摘要；验收后逐项匹配。
下面的证据目录均相对 `/private/tmp/dtw-dsh-v035-audit`。

| 最终候选实机场景 | 功能结果 | 观察结果与证据目录 |
| --- | --- | --- |
| 真实 TextEdit：无窗口打开、新建空白文稿、关闭、再次打开、取消 | 5/5；没有工具错误、unknown 或清理失败；原 PID 89003，结束时零窗口，未保存或退出用户应用 | 焦点全部符合预期。7 次 MCP 调用中 1 次受控光标借用，归还距离 0，0 次异常，0 段调用外位移。`audited-real-textedit-a06` |
| 完全遮挡的普通 AppKit 点击、后台中文、公开 focus、focus 后输入、无窗口新建后输入及严格策略拒绝 | 10/10；12 次 MCP 调用，清理通过 | 调用内光标位移为 0；准备阶段 7 段调用外位移单列。一次 PSN 快照无法确定前台，观察质量为 inconclusive，不能将整轮称为全程独立。`final-owned-candidate14-a01` |
| 借用中外部切走再返回 | 通过；后续各类输入拒绝，不重新获得输入权限；保持外部最后选择 | 实际 PID 轨迹为 witness → target → witness → target；2 次 MCP 调用内无光标位移，8 段调用外位移单列。`final-external-candidate14-a01` |
| 无 key-window 对照的最终候选尝试 | 未完成关键对照，不算通过；在多行点击步骤观察到第三个应用成为前台，静态 witness 前提失效 | 6 次调用中 4 次捕获到未获允许的位移，另有 84 段调用外位移；包括只读截图及纯脚本查询，无输入派发也出现位移。actor 未确定，不把它们直接归因于 DTW。原始 failed 记录保留。`final-unkeyed-candidate14-a01` |

TextEdit、普通窗口、外部选择三轮的最大焦点采样间隔分别约为 149.06、78.42、65.98ms。
50ms 只是旧测试的观察完整性阈值，并不能保证阈值以内没有短暂抢焦点或移动光标。
现在不以间隔大小决定功能成败；真实未知 PID 仍单列为证据不足，不伪造观察成功。

较早冻结候选的补充组合验证也保留，不能冒充最终二进制验收：
`audited-unkeyed-a01`（candidate-07）验证新窗口未获键盘焦点时只派发一次 Cmd+N、
拒绝后续文字，并在下一 exec 用 focus 正常恢复；11 个功能场景及观察通过，零光标位移。
`audited-panel-focus-a05`（candidate-08）在真实 NSDocumentController/NSRemoteView 面板中，
以 `app.focus(); app.click(...)` 完成一次新建回调及文稿窗口替换，归还前台和光标；
借用被识别，交接期间 14 段调用外位移保留待归因。

源码验证：`cargo +1.98.0 test --release --manifest-path cmd/dtw/Cargo.toml --locked --all-targets`、
`python3 tests/ci/run_native_inventory_tests.py` 和 `sh scripts/check-source.sh`。
Rust 的 608 个库测试、5 个 CLI 测试、3 个 MCP 测试全部通过；原生纯合同及仓库源码检查通过。
日志分别为 `/private/tmp/dtw-dsh-v035-final-rust-tests-a10.log`、
`/private/tmp/dtw-dsh-v035-final-native-tests-a09.log`、
`/private/tmp/dtw-dsh-v035-final-source-check-a06.log`。
原生纯合同不冒充 GUI 验收；GUI 使用冻结副本，按顺序执行。

## 保留的边界

原始失败没有删除。截图交接期间曾观察到调用外位移和前台切换；这些记录不能直接归因于 DTW。
一次测试准备助手失败，以及扩大面板键盘前台路线后“已派发但未取消”的对照也保留；
最终修复公共修饰键释放管线，保留真正可用的后台能力。

本次没有发布版本、替换 DSH 安装包或更改其他 Agent 注册、模型及 Skill 配置。
没有进行 Windows 真实 GUI 验收；不把 macOS 的结果推广到其他系统。
