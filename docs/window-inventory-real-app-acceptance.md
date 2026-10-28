# C5-macOS.2b.1：TextEdit 自然窗口状态与边界取证

本文保留 #59 的历史现场与当时返回格式；当前 typed coverage、稳定错误及 C5 完整复验见 [C5 完整验收](c5-completion-acceptance.md)。

日期：2026-10-27。实现提交 `9aad23cc941470e8419142f98df4350e99068979`，
运行环境与二进制 hash 见 [D1 验收](rewrite-baseline-acceptance.md)。
**本切片取得了真实应用的自然空清单、闭窗与 Space 边界证据；
没有确证历史 PID41684 的偶发根因，也没有放宽当前拒绝规则。**

## 范围、方法和成功标准

新建一个 TextEdit 进程（PID15140、kernel generation `1793109845779864`），
只打开已保存的自有 `dtw-inventory.txt`，不操作其他 TextEdit 进程。
诊断控制以 PID、bundle ID 和 launch timestamp 三者确认对象。
同一原生文档窗口 ID 为 `225480`；辅助表面不按标题、尺寸或相似位置推断身份。
未调用 setAccessibilityWindows 或覆盖 AXWindows 返回值，没有向测试文档输入内容。

`real_app_inventory_probe.py` 仅发 observe grant，分别以原子或脚本为首个请求，
每个阶段各做三次新上下文（合计六个独立 DTW provider）。
每个上下文分别读取一次原子 windows 和一次 app.windows；失败原样保留，不重试，
不自动激活，不提交动作。观察器在两次请求前后读取该 PID 的 CG/AX，
每个结果保留时间、角色、精确 ID、最小化/全屏和 app hidden/active 状态。

真正的应用首次 AX 读取只有 cold 阶段第一个 script-first 样本：
其前置观察只读 CG。其余样本是已被 AX 接触过的应用上的新 provider/context，
不能称为六次应用冷启动，也不能排除其他应用的首次接触问题。
CG 和 AX 是先后读取，不是系统原子快照；稳定阶段的往返对照
可以支撑以下结论，瞬时差异不能直接判为 provider 缺陷。

验收前先约定：跟踪同一窗口 ID，不把 offscreen 当 absent；
支持状态必须稳定可读，未证明状态必须明确拒绝；
每种状态保留至少三个 script-first 和三个 atomic-first 新上下文。
只对所列应用/OS/状态作结论，不在此切片扩展所有窗口类型或所有应用。

## 实测矩阵

每行均为六个新上下文、十二次读取；原子和脚本结果一致。

| 阶段 | 独立 AX/CG 证据 | DTW 结果 |
| --- | --- | --- |
| cold-visible | AX1，StandardWindow；ID225480 在屏 | 1 个窗口 |
| background | 未隐藏、非 active；同 ID/StandardWindow 仍在屏 | 1 个窗口 |
| hidden | app hidden；AX1、同 ID，但 subrole 为 AXDialog | 成功空支持清单 |
| restored | 同 ID 恢复 StandardWindow | 1 个窗口 |
| minimized | AXMinimized=true；AX1、同 ID、AXDialog | 成功空支持清单 |
| unminimized | AXMinimized=false；同 ID 恢复 StandardWindow | 1 个窗口 |
| fullscreen-current-space | AXFullScreen=true；同 ID 在屏 | 1 个窗口 |
| fullscreen-other-space | AX success/空数组，CG7，ID225480 仍存但不在屏 | 明确 window_inventory_unproven |
| fullscreen-return-space | 返回后同 ID、FullScreen=true、StandardWindow 在屏 | 1 个窗口 |
| fullscreen-other-space-instrumented | 补强观察器后重复：CG查询成功、AX数组类型有效，AX0/CG7 | 明确 window_inventory_unproven |
| windowed-restored | 退出全屏，仍同 ID/StandardWindow | 1 个窗口 |
| closed | AX success/合法空数组；文档 ID 消失，但 CG5 仍存 | 明确 window_inventory_unproven |
| closed-settled | 等待5秒后再开始六上下文；仍 AX0/CG5、无文档 ID | 明确 window_inventory_unproven |

总计 **78 上下文 / 156 次读取**：84 次返回1窗口，24 次成功空支持清单，
48 次明确未证明。这里的48次拒绝是所验证的保守边界，不记为窗口支持成功。
原子外层错误为 `backend_failed`，脚本为 `script_exception`，
二者均保留原生 `window_inventory_unproven` 和有界计数；
脚本有失败步骤且不提交 state。

隐藏和最小化的空清单有明确来源：同一窗口的 AX subrole 从
`AXStandardWindow` 变为 `AXDialog`，当前 provider 明确排除后者。
**这表示支持范围内没有窗口，不表示应用没有窗口。**
两次恢复均回到同 ID/StandardWindow；没有为支持这些状态扩大角色白名单。

全屏 Space 往返是离屏窗口仍然存在的正对照：切到自有 fixture 所在桌面后
AX0，而原 CG ID 仍在；回到 TextEdit 的全屏 Space 后同 ID 恢复。
不凭一张离屏 CG 列表推断 Space，也不凭 AX0 推断窗口已关闭。
该对照不代表全部 Mission Control、Stage Manager、多显示器或 Space 设置组合。

闭窗通过独立观察器向该精确文档窗口的 AXCloseButton 发送一次 press，
这是测试状态设置，不是新迁入的 DTW close 能力。截图在关闭前确认是自有文档。
以 close 控制记录时间为参考，约221ms的首次观察仍含原文档 CG ID；
closed 阶段约463ms时该 ID 才从 CG 消失，5个同 PID layer-0 表面仍存在；
没有用尺寸把它们猜成可忽略对象，所以全局库存保持“未证明”。
保留上下文中的旧窗口 observe 返回 `target_changed`；
随后旧 capture 返回 `stale_window`，无图片，不重用旧截图。

## 历史偶发调查结论

早前 `dtw-c5-native-harness-20261027-r2` 第一次 windows Host Call 抛
“ambiguous fixture window”，但未保存当时清单，不能推断一定是空。
`dtw-c5-native-semantic-harness-20261027-r1` 的第一次 Host Call 明确记录
`inventory=[]`，只有一个 windows 步骤、没有新动作，state 未提交。
两次及更早的 combined-r1 指向同一长寿命 PID41684、
generation `1793087588813856`，不是应用冷启动的证据。

旧日志缺少同时刻 AX/CG、role/subrole、hidden/minimized/Space 信息，
旧实现又会静默过滤归属不明项；现有材料不足以区分角色排除、
首次 AX 接触、其他 Space、精确 ID 或 CG 映射失败。
本次已复现的两类自然空清单是可验证原因类别，不能回填成旧事件的确定根因。

另发现旧闭窗 fixture 证据的干预因素：true-empty-fixed 与 unproven-fixed
使用同一 PID87831，先执行 ax_empty，close_all 不重置 AXWindows；
released-empty 的 release_windows 也显式调用 setAccessibilityWindows([])。
它们证明 CG 表面可残留，不能独立证明真实应用自然 AX0。
本次 TextEdit 未做该干预，补上了真实闭窗对照；旧记录保留并加说明。

## 复核材料与复现

原始记录：`/private/tmp/dtw-real-app-20261027`。
每阶段 `result.json` 和每上下文 MCP JSONL 保留成功与拒绝；
`close-retained-context/result.json` 保留旧引用和目标窗口截图；
`cleanup.json` 确认本次 TextEdit 及两个自有 fixture 均已退出。
仓库内的[汇总证据](window-inventory-real-app-evidence.json)保留各阶段原始结果 hash。

手动复现前编译 `tests/fixtures/real-app-inventory.swift`，并新建自有 TextEdit
实例与已保存的临时文档。用 observer 的 list 前后差集记录新 PID/launch，
先用 cg 观察模式确认自有窗口，第一次 probe 使用 --cold；
之后 observer 的 inspect、hide/unhide、minimize/unminimize、
fullscreen/windowed、activate、close 控制仅用于设置这个自有应用的状态。
跨 Space 对照通过激活另一个自有 fixture，再返回原 app，
必须确认同一 native ID 离屏和返回。每次 probe 使用新的输出目录：

```sh
python3 tests/acceptance/real_app_inventory_probe.py /absolute/dtw /absolute/observer /new/evidence \
  --pid OWNED_PID --launch OWNED_LAUNCH --stage owned-state
```

初版观察器可能把失败的 CG 查询或异常 AX 类型降级为空数组；
独立审阅后已收紧为 CG失败退出，记录 AX array-valid，
并在 probe 中记录二进制/观察器/脚本来源 hash。
早期阶段观察到了非空 CG；跨 Space 在补强后的 instrumented 阶段再次确认，
闭窗两阶段使用补强版。没有用早期观察器的隐式空值证明真实无窗口。

## 下一检查点：C5-macOS.2b.2

先明确支持范围空、窗口不存在和库存未证明的可读合同，再实施最小变化。
目标是 Host/Agent 能识别本轮观察到的状态限制，并保留下一次自然异常首次请求的
同 PID/代际、AX/CG、角色及状态证据。任何闭窗后空成功的放宽，
必须保住同 ID 跨 Space 仍存在的负控，不能按 offscreen、表面尺寸或重试次数猜测。
每种声明支持的状态至少三个新上下文对照，失败保留，独立审阅后更新产品矩阵。
历史事件无法追溯确证时明确记录证据不足；不以恢复旧运行链解决覆盖缺口。
