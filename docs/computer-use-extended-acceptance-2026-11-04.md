# Computer Use 剩余目标单项验收（2026-11-04）

首轮实现已提交为 `0fd00c1`。随后补齐原先剩余 23 项的独立执行入口，并保留 **47 次单项尝试**。最新结果为：13 项业务或独立中断效果成立，3 项 unsupported，7 项 blocked；其中 **8 项通过单项严格验收，7 项在后台，1 项在显式前台诊断条件下**。完整 42 项基准仍未通过，独立运行不能拼成一次整套成功。

原始首轮记录见 [首轮报告](computer-use-benchmark-2026-11-04.md)。本轮每次尝试的候选、脚本/fixture 哈希、原始报告 SHA256、真实光标间隙、未知派发与清理结果见 [脱敏证据](evidence/computer-use-extended-2026-11-04/summary.json)。原始证据目录为 `/tmp/dtw-computer-use-targets-b01`，后续重跑没有覆盖之前失败。

## 执行方式与证据边界

每项使用 fresh fixture、独立输出目录和当前标准 `apps` / `exec`。输入没有通过 DOM、CDP 或私有 oracle；文件预置仅限覆盖取消测试的 owned seed，状态和几何仅用于独立验收。输入未知时只观察实际效果，未重放原始输入。

扩展标准入口：

```sh
python3 tests/acceptance/computer_use_acceptance.py \
  --dtw /absolute/path/to/frozen/dtw \
  --scenario C04 --case C04-a01 \
  --output /tmp/dtw-computer-use-C04-a01 \
  --source-revision <source-identity>
```

每次重跑使用新目录。`--fixture-binary` 可复用已编译的自有 fixture，报告记录其实际 SHA256；`--observer`、`--overlay-observer` 和 `--focus-observer` 可复用已编译的只读观察器。单项结果保留全部 42 个固定目标，未选目标仍为 `not_run`；本报告的汇总只用于跟踪覆盖，不代表一次运行通过。

`targeted_automation_passed` 核对该项业务/独立效果及清理，`targeted_passed` 进一步核对原始工具错误；仅精确 execution ID 对应的已知、纯 `not_dispatched` 拒绝可用于负向合同，未知、已派发、focus preparation 或 interrupted 都不能使用该豁免。`clean_run` 保留原始工具错误，不因负向合同成立改成 true。`targeted_acceptance_passed` 还要求严格 P02 光标证据；完整 `acceptance_passed` 和 `target_baseline_passed` 不因此变为 true。

## 最新结果

下表“效果成立”仅指固定业务或独立中断效果。严格通过还要求符合工具错误及真实光标条件。后台是默认测试条件。

| 目标 | 最新尝试 | 效果/覆盖状态 | 单项严格结果与限制 |
| --- | --- | --- | --- |
| N11 真实保存 | N11-a02 | 效果成立 | 精确文件字节、一次保存通过；AXPress 未知回执仍在，且该次采样间隙 78.606ms，严格失败 |
| N12 取消覆盖 | N12-a02 | 效果成立 | 真实覆盖确认与 Save 取消、seed hash 不变；AXPress 未知回执使运行不干净 |
| N13 hover/双击 | N13-a02 | unsupported | 后台 hover 为 `foreground_required/not_dispatched`；没有激活或重放，未继续双击 |
| N14 旧截图几何 | N14-a01 | unsupported | 真实标题栏移动成功，但动作自动刷新观察；公共 x/y 无旧图身份，不能把后续合法新图输入假称旧图拒绝 |
| N15 取消退出/重启 | N15-a02 | 效果成立 | 真实取消退出、内容保留、提交、退出原进程、同连接重启及新 baseline 成立；取消按钮 AXPress 未知回执使严格失败 |
| C04 低 AX 编辑 | C04-a02 | 效果成立 | **通过**；只编辑 message-137、邻居不变、一次日志；光标 0 位移，最大间隙 38.943ms |
| C05 旧消息滚动 | C05-a01 | 效果成立 | **通过**；实际滚动至 message-132 并操作精确目标，无最近消息替代；间隙 38.108ms |
| W04 表单校验 | W04-a03 | blocked | 真实非法表单无保存，note/quantity/checkbox 已输入；日期控件的键盘方案未生成合法值，未完成有效保存与重开 |
| W05 嵌套滚动 | W05-a04 | 效果成立 | **通过**；inner 纵向与 outer 横向分别变化，邻区和另一轴不变；间隙 22.629ms |
| W06 Web Canvas | W06-a03 | 效果成立 | **前台诊断通过**；真实低 AX Pin/Reply、准确 Unicode 与邻居不变；间隙 43.889ms。后台尝试未产生所需动作，仍不宣称后台通过 |
| B01 Chromium | B01-a01 | blocked | 当前 bundle/path 授权无法隔离既有 Chrome 中的 owned profile/PID；未操作用户浏览器 |
| B02 库存跨页 | B02-a02 | 效果成立 | **通过**；复用原 Inventory，编辑第 1/3 页两条记录，各保存一次并 GUI 重开验证，全邻居不变；间隙 21.146ms |
| B03 重复标签 | B03-a03 | 效果成立 | **通过**；先证明真实重复 Reviewed 的歧义无派发，再搜索并只审核 ledger 137；间隙 25.268ms。预期错误仍保留 |
| B04 Chromium trusted pointer | B04-a01 | blocked | 与 B01 相同的实例隔离限制，未用历史 window API 或接触用户 profile 绕过 |
| B05 实际模型图片 | B05-a03 | 效果成立 | **通过**；实际 DTW PNG 经 ViewImage 读图后作答，公开字段读回一致，单次提交后才读取私有 judge；间隙 27.242ms |
| R01 标准取消 | R01-a04 | 独立效果成立 | **前台诊断**证明部分路径、未到终点、一次配对释放和 fresh read；异步原请求未知/错误保留，严格不干净 |
| R02 EOF | R02-a03 | 独立效果成立 | **前台诊断**证明部分路径、配对释放、exact MCP/worker 退出及新连接恢复；原请求无回执记 unknown，严格不干净 |
| R03 错误实例 | R03-a02 | unsupported | 同 executable sibling 进入旧画布后 `look_required`，双方业务不变；缺少明确派发证据，foreign-occlusion 子目标未完成，不能全项通过 |
| R04 后台干扰 | R04-a02 | 效果成立 | **通过**；默认 auto、连续 witness 焦点/按键目的地与真实光标均通过，间隙 47.829ms，清理通过 |
| S01 Dock 根 | S01-a02 | blocked | 实际观察没有建立可操作的精确 Dock item 与独立业务 oracle，未转任意桌面点击 |
| S02 无窗口菜单根 | S02-a02 | blocked | 自有 accessory/status-item 实际运行与观察完成，但精确应用根菜单 action/归属尚未验证 |
| S03 Windows tooltip | S03-a02 | blocked | 需要 Windows 原生桌面、真实 tooltip/shadow 及文件选择结果；没有用 macOS 结果替代 |
| S04 桌面/显示条件 | S04-a02 | blocked | 已保存实际显示清单；未改变用户当前桌面来伪造独立的 mixed-DPI、搬移和不可用桌面验证 |

## 本轮修正与失败归因

本轮产品代码保持 `0fd00c1`；修改集中于验收设施和自有 fixture，没有新增产品自动激活例外。

- **独立目标与错误分级：** 新增四组扩展用例及 fresh 单项 runner。所有目标有明确 passed/failed/unsupported/blocked，预期负向拒绝按精确 execution ID、派发、focus preparation 与 interruption 核对，原始错误仍在。
- **Web 输入和定位：** a01 暴露 roleless selector 同时匹配 label/field、原生按钮与网页链接。a02 改显式角色和实际 PNG 几何；B05/B03 再根据已观察到的字段 `press` 能力先聚焦，未在未知后换路。滚动锚点限于实际可见区域，水平滚动显式带 `delta_y:0`。各原失败保留。
- **真实退出与清理：** fixture 新增真实 NSApp 退出确认/取消。R04-a01 的 dirty note 让 owned cleanup command 打开保存确认并被强制终止；a02 仅对私有收尾 command 允许正常退出，标准 `app.quit()` 仍保留确认语义。N15 重启进程由 PID + 微秒 kernel birth 观察，未知 exit code 保持 None，只有匹配身份的真实 quit journal 加已观察退出才能通过清理；强制信号每次重查 birth，且强制清理始终失败评分。
- **中断不能冒充自然完成：** R01/R02 不只检查 down/up，还要求部分路径和未到终点的独立事件坐标。异步响应及无响应均参与错误/unknown 指标，不能被同步 metrics 漏掉；取消后的 busy 仅轮询新读取，不重发输入。
- **EOF 光标边界：** 跨连接恢复 sidecar 必须与两个真实 transcript、请求/响应 ID、只读 source、execution ID 和时间边界吻合。无回执的原请求只得到物理观察终点，不伪造输入 receipt；50ms 和逐借用区间检查不变。

47 次尝试中，R04-a01 是一次明确的自有资源清理失败，后续批次按设计停止；该进程与 MCP/worker 已结束，修正后才继续新目标。其他清理结果和所有位移、采样超限保留在摘要。外部任务可能干扰桌面，但未用这个可能性替某次失败作确定归因。

## 前台与图片证据

默认后台。R01/R02 首次后台尝试超过路径采样上限，不能作为后台能力证据；第二次合法路径已派发但没有实际手势回调。W06 的有效后台尝试未产生预期 Canvas 动作。随后只为这些具体目标建立显式前台诊断条件，通过 `--fixture-foreground --background-failure <原始同项目报告>` 记录准备条件与原报告哈希。这不是产品自动 fallback，也不证明它们的后台路线通过。后续判断应继续区分“当前可用定向路径没有该效果”和“平台在技术上不可能后台实现”。

B05 的每次图片都来自 `AppClient.capture()` 真实 PNG。主线程实际调用 ViewImage，再提供绑定 SHA256 的答案和图像像素输入点；用例先公开读回输入值，单次提交后才访问隐藏 judge。a01 输入未写入而失败；a02 图片与业务通过但采样间隙 50.800ms，严格失败；a03 全部通过。三个尝试均保留，新图片没有沿用前次答案。

## 最终检查与后续

- `sh scripts/check-source.sh` 通过，包含 **110 项 Computer Use 单测、26 项真实光标审计单测、14 项 fixture HTTP 单测**及项目原有检查。首个源码检查因 Web helper 新增观察能力而暴露旧 Mock 契约失败；补齐非 press/已观察 press/未知不重放测试后通过，旧日志保留。
- 全部 fixture Swift 文件 typecheck 通过，当前编译 fixture 已用于实机；生产源码没有变化，继续使用首轮已通过 590 库测试、5 CLI、3 MCP 的固定候选。
- 二进制 SHA256 仍为 `8cf5a7b28847b57fa25a1a153bd66c88d1c4078ab3cb3db27d93011e48e0df47`。没有替换安装的 0.3.4，也没有发行验收声明。

后续产品工作应由这些保留失败驱动：系统根/无窗口菜单 action、Chrome 的安全实例绑定、日期输入方案、AXPress 未知效果的调用体验，以及默认后台自绘手势/Canvas 的实际能力。Windows GUI 继续在 Windows 机器验证，Linux X11 顺序保持。当前已完成的是剩余目标的逐项评估与可复现入口，未完成的支持范围没有被改记为通过。
