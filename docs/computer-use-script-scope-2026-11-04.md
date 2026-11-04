# macOS 脚本级前台借用与定向补齐验收

本轮实现 `auto` 的整段 `exec` 前台借用，修正 AXPress 误用属性读取短超时的问题，并补齐 Chrome 独占实例与 WebKit 日期输入验收设施。**不是完整 42 项通过，也不是已安装 0.3.4 的验收。**

45 次 fresh 尝试全部保留，包含沙箱启动失败、旧验收器假设错误、真实输入效果不足、采样超标及未知回执；没有覆盖原输出、把未知改成成功或在同一次尝试里重发未知输入。汇总见 [summary.json](evidence/computer-use-script-scope-2026-11-04/summary.json)，原始目录为 `/tmp/dtw-script-scope-gui-a01/`。

## 当前行为与实现

- 每段脚本先走后台。只有可信能力预检证明当前动作干净 `not_dispatched`、尚无焦点准备或输入副作用时，才由 resident worker 建立一次前台 scope；原输入随后仅派发一次。
- 借用后，剩余动作一直受该 scope 的前台所有权约束；支持的指针投递仍可使用定向事件。逐动作清理只释放局部输入焦点，不消费脚本 scope。
- 每个 payload 都重新验证 resident 的单调失效证明。跨 worker 的私有 guard 使用有界认证 IPC，不缓存通过结果，也不在 Scene lease 内递归获取 Scene。
- 原前台保存 PID、kernel birth 与精确 AX 窗口对象/ID；目标保存精确逻辑根。证明属于根的 sheet/popup 后代可继续，同 app 兄弟窗口不算同一任务窗口。
- 外部切焦点使 scope 永久失效。焦点后来返回也不重新武装或激活。结算时仅在所有权与原身份/窗口仍有效时归还；恢复失败保持 sticky cleanup failure。
- 只有真实确认的 app 退出或锚点窗口关闭建立终态。终态清理只观察原前台是否自然返回，不激活第三个 app；保存提示、未知退出与按键关闭本身不证明终态。
- AXPress 单次 action reply 预算为 `min(2s, remaining deadline)`，区别于属性读取的 200ms 预算。只有真实 AX success 是正向派发回执；错误、超时与取消仍保留各自的不确定性，不改路重放。
- 定向输入的 C 回调保留 `focus_changed` 原因和已有的部分派发状态。脚本级预检拒绝另外输出私有 `refused / not_dispatched` 记录，供审计核对已尝试但未派发的动作；这类记录不授予任何光标移动许可。

实现入口是 `cmd/dtw/src/backends/macos/{mod.rs,worker.rs,Native.swift,input_route.rs,execution_guard.rs,scroll_guard.rs}`。公开工具仍是 `apps` / `exec`；没有新增 Agent 侧 focus/session API。

## 候选身份与验证边界

| 阶段 | 二进制 SHA256 | 用途 |
| --- | --- | --- |
| candidate 1 | `dfae1c9050ee136629c386146fc8e754a1ce085a7848f2a150996d24c71dacdc` | 首轮 AXPress、日期、Chrome、焦点与诊断 |
| candidate 2 | `5e530016f261f91ad4ad31a2f079fbe36284c4f100f198ec0cc9e5ed49bd38cf` | 增加焦点错误码透传；Chrome/日期/系统面板严格验收 |
| final | `efe99ff7fa8f73dc3d9eeef8ea5130fc47c5a558a6cd53437366cc5a09f592e8` | 在 candidate 2 上增加派发前拒绝的私有审计记录；最终焦点验收 |

三份源码与 helper 身份清单均保留在同目录的 `candidate-*-identity.json`。最终冻结目录为 `/var/folders/hn/r4ffst5510s89657cwrcjj6w0000gn/T/dtw-script-scope-final-yk1htkch/`，包含 `bin/dtw` 与配套 skills。生产源码、验收脚本、fixture 与 observer 均有 SHA256；开发基线是 `2adb269` 后的工作区。

业务表使用 candidate 2，最终焦点表使用 final，不能把不同候选拼成一次完整运行。最后增加的拒绝追踪没有更改业务投递策略。未替换用户安装、未发布、未 push。

## 最终脚本级焦点验收

每项使用独立 owned target 与 foreground witness。业务输入仅经当前 `apps/exec`；环境 actor 只按精确 PID/birth/bundle 注入一次激活。独立 observer 同时记录 activation 与连续前台采样，witness 记录真实键盘事件。

严格通过要求业务、焦点、输入释放、owned 清理及外层光标审计全部成立；焦点与真实光标采样均保留 **50ms** 上限。中断场景的合同成立与 `clean_run` 分开，缺失回执不生成替代成功。

| final 尝试 | 业务/焦点合同 | 严格光标 | clean_run | 焦点最大间隔 / 光标最大间隔 |
| --- | --- | --- | --- | --- |
| `final-hover-a01` | 通过：后台两次增量 → 借用一次 → 精确多行文本与后续操作 → 归还一次 | 通过 | true | 44.914 / 27.868 ms |
| `final-keyboard-a01` | 通过：向已聚焦的多行编辑器追加精确文本触发借用；后续保持前台，结束归还 | 通过 | true | 24.338 / 21.646 ms |
| `final-external-a01` | 通过：真实 partial drag 后外切；拖拽及 semantic/point/keyboard/text 共五项拒绝，counter 仅 +3、精确文本不变，不抢回 | 通过 | false，保留 partial input 的 unknown | 20.391 / 17.307 ms |
| `final-cancel-a01` | 通过：held drag 中取消，配对释放，无 suffix；恢复读与清理成立 | 通过 | false，原调用无回复，仍是 unknown | 20.244 / 26.708 ms |
| `final-return-a01`、`a02`、`a03` | **严格未通过**，详见下文 | 三次均通过 | false | `a02` 焦点 61.226ms 超标 |

外切返回的三个 final 尝试都观察到后续四个输入为 `focus_changed`，没有重新激活或执行 suffix，`effects.dispatch.not_dispatched = 4`，已有 partial drag 仍为 unknown。actor 返回目标的时间早于同一 exec 的 suffix 时间。

`final-return-a01/a03` 的拖拽在切换瞬间返回 `focus_unproven`，未满足验收器要求的五个 `focus_changed`；没有把证明不可读改成已证明焦点变化。`a02` 的五个错误码和 return-before-suffix 断言成立，最终仍因连续焦点采样 61.226ms > 50ms 判失败。因此保留“返回后不重获”的业务证据，**不宣称该子项严格通过或竞态已完全解决**。

早期没有设置 `DTW_INPUT_TRACE=1` 的运行缺少路由证据，不能算严格光标通过。另有多个 >50ms 的采样失败；全部列在汇总里，不归因用户、不放宽阈值。新 auditor 只接受 runtime 在 `send()` 前生成的明确 `refused + execution_scope_preflight + not_dispatched`，unknown/无分类拒绝仍失败，任何无合法借用区间的光标位移仍失败。

## 业务补齐与未知回执

下表每项是单独 fresh 尝试，均使用 candidate 2，不能替代完整 42 项运行。

| 目标 | 尝试 | 业务与真实回执 | 严格光标最大间隔 |
| --- | --- | --- | --- |
| N11 Save 面板 | `N11-a03` | 精确文件字节、save journal 恰好一次；打开/保存均 dispatched，无工具错误 | 通过，28.448ms |
| N15 未保存退出/重启 | `N15-a02` | 取消退出保留内容、真实退出、新 PID/birth 与观察重置；本次无 unknown | 通过，45.959ms |
| W04 HTML 日期表单 | `W04-a03` | 空表单真实拒绝；真实键盘 `2027 TAB 1 TAB 2 TAB`；独立 oracle 精确 `2027-01-02` 后保存一次并重开核对 | 通过，30.781ms |
| B01 Chrome | `B01-a04` | 独占复制 bundle/fresh profile；精确 Unicode 保存一次、checkpoint 137 恰好一次 | 通过，44.602ms |
| B04 Chrome | `B04-a05` | 普通与 trusted-pointer 两种模式各提交一次；原事件/独立业务 oracle 验证，trusted 模式要求真实 down/up/click | 通过，17.282ms |

Chrome 并未使用已安装 Chrome 的 bundle ID 或用户 profile。每次 exec/capture 前复核 copied executable、argv 中的 profile/URL、PID birth、真实 bundle URL 与唯一实例；清理仅信号通知经核验的 owned PID，保留复制文件与启动/清理证据。本次没有 forced Chrome cleanup。

两个实机发现的 harness 假设已修正：LaunchServices 的 `/tmp` 别名须与 `/private/tmp` 解析后比较；公开 view 的 coverage 描述 surface，不能凭空要求 `instance_count` 字段。真实翻译弹窗通过已观察到的唯一 Close/关闭按钮关闭。无 AX press 能力或字段语义缺失时，由父线程查看实际 PNG，写入 hash-bound 像素点；输入前再次核对新 capture 几何。B01 字段与 B04 trusted 按钮均保留该交接证据。

W04 首轮截图已显示年份 2027，但 AX 查询没有返回 year 子节点。新流程去掉了不成立的每分段 AX 必须可读假设，保留最终精确独立日期、保存和重开断言；没有写 DOM、预置日期或改变控件类型。

**N12 尚有真实未知回执。** `N12-a02` 打开 Save、取消覆盖与取消面板的业务 oracle 成立，seed 文件 hash 不变、保存增量为 0；点击 Save 已显示覆盖确认，但 AXPress 在约 0.38s 返回 `-25205`。该操作保持 `dispatch.unknown = 1`、工具错误及 `clean_run=false`，不能用业务结果改写其原回执，也不能靠增加等待时间证明 AX success。延长 action timeout 没有消除这种错误。更早的 `N12-a01` 是输入前 `look_required`；它同样保留，不替换成后来效果。

另一个诊断发现是后台 AppKit NSTextView 的坐标点击与 `Command+A`：输入进入了目标事件流，但未改变多行编辑器的 responder/selection；普通文本框收到文本。hover 先借用再点击的路径成立。保留 `scope-keyboard-a01/a02/a03` 失败，不把它们称为产品修复。最终键盘 scope 验收明确从已聚焦多行编辑器开始，测试一次追加；它证明键盘触发的借用生命周期，不证明所有后台控件的点选或全选都有效。

## 七个原 blocked 项逐项结论

没有一项已被证明是平台永久不可能。技术限制、禁止的绕过方式与应补齐范围分别记录：

| 项目 | 现有技术/环境边界 | 不应支持的绕过 | 本轮结论与后续完成条件 |
| --- | --- | --- | --- |
| S01 Dock | 当前 provider 语义/动作根依赖普通窗口，缺 application AX root 合同 | 把 Dock 全屏 surface 当任意桌面授权；仅靠同名图标猜应用归属 | **应补齐产品合同，本轮未实现**。需证明 Dock item 与 owned app 的关系，支持真实 root action 与过期/歧义拒绝 |
| S02 windowless accessory | 当前 no-window 合同没有 status/menu root 输入通路；系统托管 surface 还需 provenance | 造窗口骗合同，或跨系统服务仅按 label 输入 | **应补齐产品合同，本轮未实现**。现有真实 status-item/counter fixture 可复用，需 root/popup ownership 与恰好一次效果验收 |
| W04 date | 原 recipe 与 AX 必须可读假设不足 | 预填日期、写 DOM、取消 required、改成普通 text 控件 | **本轮补齐真实日期输入验收**，上述 fresh 尝试严格通过 |
| B01 Chrome | fresh profile 单独不能隔离同路径实例；显式 copied `.app` 路径已有可用基础 | 用户 profile、全局 bundle-ID 授权、无法证明隔离仍输入、killall | **本轮补齐独占实例 harness 并严格通过**；不是必须先增加公共 PID selector |
| B04 Chrome pointer | 同 B01 的隔离先决条件；trusted-pointer 必须独立验证 | 用普通 AXPress 提交冒充 trusted pointer，unknown 后重发 | **本轮两种真实模式均严格通过**；B01 结果未替代本项 |
| S03 Windows tooltip/shadow | 当前是 Darwin 主机，没有 Windows 原生 GUI 证据 | 用 macOS/合成记录冒充 Windows，或预停放光标掩盖 tooltip | **应补齐 Windows 原生验收**。当前不执行、不宣称 Windows GUI 通过 |
| S04 mixed-DPI/desktop | 当前两屏均 2x；缺实际 mixed-DPI 与隔离 desktop 条件 | 改用户显示/锁屏条件，绕过不可用/安全桌面，恢复后重放未知输入 | **应补齐专门环境和分项验收**。`desktop_unavailable` 应保持拒绝；本轮未改变用户显示环境 |

S01/S02 是后续 provider roots 工作，不能靠局部 `no_window` 豁免补成支持。S03/S04 是原生验收环境缺口，不能写成平台永远不支持。

## 代码与非 GUI 检查

最终产品代码完成 release all-targets：**600 个库测试、5 个 CLI 测试、3 个 MCP 测试通过**。`scripts/check-source.sh` 完整通过，其中 computer-use focused 集合 150 项、cursor audit 27 项通过。Native inventory 已在本轮以 warnings-as-errors 通过；最终 owned fixture 也以 warnings-as-errors 编译。

源码检查中的一次 rustfmt 失败只涉及新增测试的换行，已格式化并重跑完整检查；未调整行为断言。相关日志 SHA256 见 summary。最终 delta、生产源码身份和证据链接另行核对。

复验模板如下；每次必须使用新目录，多个 GUI 运行必须串行：

```sh
DTW_INPUT_TRACE=1 python3 tests/acceptance/cursor_audit.py \
  --assert-independent --observer /absolute/path/to/cursor-observer \
  --output /tmp/new-cursor-run -- \
  python3 tests/acceptance/computer_use_scope.py \
  --dtw /absolute/path/to/frozen/bin/dtw \
  --observer /absolute/path/to/focus-observer \
  --fixture-binary /absolute/path/to/ComputerUse \
  --output /tmp/new-cursor-run/probe --case normal
```

`--trigger keyboard` 使用明确的初始多行编辑器条件；`--case external --actor ... [--external-return]` 验证外切；`--case cancel` 保留无回复的未知结果。Chrome 图像交接有有限等待，必须查看请求中的真实 PNG 并绑定其 hash，不从私有业务状态反推输入点。
