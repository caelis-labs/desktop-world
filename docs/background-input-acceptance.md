# macOS 后台输入与会话历史验收

本轮基于 main `57fc54d6a711af44b4c82bda044a770a6131c57a`，保持标准 rmcp stdio 与 `exec(app, source)`。这是本地签名开发候选的验收，不是新的公证安装包、正式 release 或外部模型易用性复测。

## 阶段状态

2026-10-29 按用户要求阶段性收尾。当前为可审查的草稿实现，**尚未达到合并或发布条件**。后台输入、前台归还和浏览器能力分别记录，不把源代码/纯测试通过当作真实应用通过。下一阶段只先解决下方已复现阻塞，再补齐同一候选的 GUI 闭环。

## 范围与可核验目标

- 已完成执行按记录数、回执字节和 payload 字节滑动回收，不再以累计 512 次拒绝新工作。应用最新 state、存活引用和 owner 独立保留。
- 从固定 CUA SHA 搬入后台输入依赖闭包，内部选择目标投递；只有尚无准备/输入副作用的预检拒绝才允许转前台。效果不明时不重投。
- 同进程实例写操作串行，独立 app 的后台输入可以并行；前台借用及归还使用独立共享输入租约。归还时尊重外部 app/window 选择。
- Unicode、快捷键、坐标点击、默认滚动以 AppKit/Chrome 独立读回为验收目标。后台路由成功发布事件仍只表示 dispatched；本轮实际覆盖见下表。

## 保留的失败样本

原始证据保留于本机 `/private/tmp/dtw-background-probe-20261029-*`，不将含本机窗口库存的原始记录提交至仓库。

| 样本 | 结果与解释 |
| --- | --- |
| a / b / c | owned fixture 未能可靠成为前台，未开始 DTW 输入；修正 fixture 的 LaunchServices 激活，并确认签名已完成后再开始测试 |
| d | 后台 Unicode/Backspace 实际文本正确；点击回执 dispatched，但 AppKit click pad 未收到事件。原适配对全部 app 使用 Chromium recipe，随后按框架选择已有 generic AppKit recipe；保留失败，不以回执冒充点击成功 |
| e | 同一 PID 两个窗口的后台文字分别落入正确窗口，前台与指针采样保持不变 |
| f | 两会话各 8 次、约 200 字符的并发样本触发 30 秒 script_timeout。每事件重复完整 AX 校验耗时过高；保留原负载和期限，改为首次完整准入、后续精确单窗/身份/模态检查，等待期仅检查本地取消和期限 |
| g | 原候选的一次显式 focus 后归还通过；不推导用户离开再返回或同 app 切窗已覆盖 |
| h | 新候选后台文字和真实 pad down/up 均成功；旧探针将键盘到点击全程的指针位置不变作为统一判据，因约 0.4 点变化失败。用户确认当时在使用鼠标；这是验收判据过宽，不是已确认的输入缺陷。不再追查或用微调阈值掩盖，改为按动作/目标判定并保留无关桌面变化记录 |

原候选 SHA-256：`fd42bcc36553e61693415171a28f9f99842c84de7f816931c2bc41814f9a0358`。

## 最终候选与结果

最终本地候选 D：`0e7386ec48332bf73e3974cbc210cb9ecf106d8a9c180d9237ae97e48069729c`。Developer ID 签名及严格验证通过；源码 manifest SHA-256 为 `ad92f7efe7bc1964a64a59447a564d94355bbadbeaaa728a496f545c7c1a41ee`，复核无源码漂移。前一候选 C 为 `077bfb22680a02925bfee722ef055d6fe69cb3e9f55e67dab71a50f61b906929`；C 的结果不冒充 D 的全量验收。

自动检查通过：301 个 Rust 库测试；轻量 source pipeline（Core 63、脚本 21、macOS pure input 20、Windows pure input 5、Python 123）；Swift 严格编译与原生契约，包括新增 11 组首次点击就绪控制。既有 CLI 4、stdio 3 项在本轮较早候选通过。本轮不重建全量上游 CUA。

| 真实验证 | 结果与证据边界 |
| --- | --- |
| AppKit 后台 Unicode、Backspace、坐标左击 | D / m 通过实际文本与 down/up；目标未出现在采样前台，用户指针变化不作为失败 |
| 两个独立 MCP 会话并发 | D / final-concurrent 两 app 各 8 次、192 字符，分别 4.384 / 4.363 秒；原生事件区间重叠，完整文本路由正确；此前 C / j 为 4.373 / 4.411 秒，没有削减原超时负载 |
| 取消、EOF、父进程退出 | C / lifecycle-a 三项通过；原始部分前缀、各 down/up、未执行后缀、无重放及 exact worker 清理分别验证；断管不伪造完整终态账本 |
| 自动前台回退 | C / k 独立 right double-click 通过；先后台输入再回退的 C / i、l 与 D / m 失败，不能据 k 宣称问题已解决 |
| 显式/重复 focus 归还 | D / final-focus-return 的一次、两次及同脚本 primary→secondary→primary 三组均通过；C 的重复 focus 失败样本保留，不宣称主动外部留返已覆盖 |
| 闭窗后 owner / 库存 | D / final-close-window 已见真实 close 通知和释放配对，原 unknown 保留；随后窗口库存仍含旧窗口，整项未通过，尚不能区分关闭过程延迟与库存边界缺陷 |
| Chrome 冷 AX / 深层发现 | C / browser-c 最初三轮仅 53 节点空结果；第 4 轮、总计约 2.442 秒找到深层按钮，后续 12 页到 visited 1213 的定位、筛选和开表单成功；只重复读取，未重放写入 |
| Chrome 后台键盘 | C / browser-c 首字符 `s` 已到达真实 textarea，随后 `background_input_failed` / unknown；保存次数为 0，坐标保存和滚动尚未进入 |

## 合并前必须补齐

1. **后台输入后的物理前台回退。** D 已移除“AX focus 成立即可跳过 raise”的逻辑，并在首个 pointer/down 之前给原目标点最多 750 ms 的只读就绪窗口；第二击仍严格单次验证。D / m trace 证明 raise/focus 已请求，但原 owned observer 仍以 layer 0、alpha 255 遮挡目标点，最终 `target_occluded / not_dispatched`。没有绕过遮挡证明，也没有重投点击；根因仍待定位。
2. **闭窗后的库存收敛。** D 已观测原窗口关闭，后续 `windows()` 仍含旧引用。需核对原生关闭生命周期及有界只读收敛，不通过挑选其他窗口或直接忽略旧引用掩盖。重复 focus 归还已在 D 修复并通过三组验证。
3. **Chrome 后台键盘目标连续性。** 查明首字符后哪个真实目标/原生焦点条件变化；保留 original unknown，不通过取消检查或自动重放掩盖。修复后再进入保存、滚动及外部 Agent 任务复测。
4. 同一最终候选补齐后台同 PID 多窗、主动 app/window 留返、闭窗及 lifecycle 回归。新的 agy/Claude 场景、正式安装包、公证和发布仍未执行。

## 验证边界

- fixture 以 20 ms 采样前台/指针，并记录激活和本地输入事件。用户可操作鼠标与其他 app；无关指针/窗口变化不判操作失败，也不从采样差异推定 DTW 因果。不主动归位/移动真实光标；目标投递路线的代码审查与目标实际效果分别记录。
- Chrome 使用独立临时 profile 与 240 条记录的本地业务页面；AX 查找、编辑、真实事件、唯一保存和滚动由网页独立日志检查。坐标来自只读页面几何，不代表模型视觉定位验收。
- 本轮没有运行新的 agy/Claude 模型任务。既有六个模型任务仅证明其原始安装候选。
- 未承诺锁屏、任意 app/系统版本、复杂菜单/系统对话框、远程 MCP、Windows 后台输入或 Linux 桌面支持。
- scroll 的现有公开合同仍以窗口中心为锚点；面向嵌套区域的坐标/元素锚点是后续能力组，不用“先点击”掩盖这一限制。


## 复现与证据位置

签名候选与 source manifest 位于本机 `/private/tmp/dtw-background-gui-20261029-d/`。原始证据分别位于上述 `dtw-background-probe-20261029-*`、`dtw-background-final-*`、`dtw-background-browser-20261029-*`、`dtw-background-lifecycle-20261029-a`。它们保留原始失败与清理信息，但可能包含本机窗口库存，所以只提交[脱敏摘要](evidence/background-input-2026-10-29.json)。

使用新的输出目录运行，禁止复用失败目录或自动重放写入：

```sh
DTW_AX_READINESS_TRACE=1 DTW_SCENE_TRACE=1 python3 tests/acceptance/background_input_probe.py --dtw <signed-binary> --output <fresh-directory>
DTW_AX_READINESS_TRACE=1 python3 tests/acceptance/background_browser_probe.py --dtw <signed-binary> --output <fresh-directory>
DTW_AX_READINESS_TRACE=1 python3 tests/acceptance/background_lifecycle_probe.py --dtw <signed-binary> --output <fresh-directory> --case all
```

后台 probe 不主动移动鼠标或要求无关桌面静止。foreground-fallback 用例会实际借用前台和物理指针；其他 app 的无关事件只记录，当前动作目标的冲突仍保留原错误。
