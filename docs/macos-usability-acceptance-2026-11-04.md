# macOS 连续操作与干扰检查（2026-11-04）

本轮开发候选通过了当前 `apps` / `exec({app,source})` MCP 上的自有 AppKit 任务：首次调用直接执行已知 selector、右键打开独立弹层后连续操作并返回底层、只读菜单查询。光标审计的 5 个调用区间未检测到物理光标移动；各任务前后的前台应用相同。业务效果由 fixture 的独立计数器确认。

这是基于首批提交 `a968b29` 的 macOS 源码增量，沿用插件 → JS 编排 → 平台积木的架构。证据为开发二进制与直接脚本；当前安装的 MCP 包仍是 0.3.4。本轮没有安装、签名或发布，也没有完成真实模型任务、Telegram、Dock 或 Windows 验收。

## 行为变化

| 场景 | 本轮行为及边界 |
| --- | --- |
| 下一步已知 | selector、键盘、文本插入和默认滚动按需准备接收者，不要求每段脚本先 `look()`。显式语义 ID 仍需已有观察，坐标仍需已返回的 capture。纯 JS 不读取桌面。 |
| 右键弹层稍后出现 | app selector 的只读轮询跟随原进程实例当前最上层语义 surface；不再一直等待旧窗口。兄弟进程不能接管原实例的目标。 |
| 底层 → 弹层 → 底层 | 坐标绑定保留原截图、画布映射及该点原始进程实例，按当前最上层非透明像素选择接收者。画布锚点与映射须保持；语义 ID 仍绑定原对象；手势仍受单窗口限制。 |
| 捕获与弹层出现竞态 | 坐标派发前的瞬态捕获错误最多重读一次，并受原截止时间约束；没有重新发送输入。权限和取消错误不触发该重读，重读仍须保持原点的进程实例证明。 |
| 应用菜单 | `look({scope:'menu',role?,text?})` 复用精确窗口的原生菜单观察，不为读取准备焦点。仍需已证明归属的应用窗口；未增加无窗口根或 menu extras。 |
| 系统应用发现 | 合并已证明的运行 bundle，补上安装目录之外的无窗口 accessory 应用。发现不代表可捕获或可输入；helper/background-only 排除规则保留。 |

实现入口为 [scene.rs](../cmd/dtw/src/app_protocol/scene.rs)、[selection.rs](../cmd/dtw/src/app_protocol/selection.rs) 和 [Native.swift](../cmd/dtw/src/backends/macos/Native.swift)。公开合同见 [API](../skills/desktop-world/API.md) 与 [交互模式](../skills/desktop-world/PATTERNS.md)。

## 固定候选与复现

开发候选：`/tmp/dtw-macos-usability-candidate-a03/bin/dtw`。

```text
SHA-256 ed11b0a20b5ad4ca8d10955be401ba237e0761cdc2647a9b3765a787f8a333db
```

[仓库证据摘要](evidence/macos-usability-2026-11-04/summary.json) 保存生产源码、fixture、probe 与 Skill 的哈希、独立效果、审计和清理结果。验收后仅修正文档中的错误码说明：无授权像素返回 `outside_app`，画布或实例变化返回 `stale_scene`；该说明修正没有改变二进制。原始私有证据保存在下述临时目录，临时文件不保证长期留存。

最终运行通过普通 stdio MCP AppClient 连接冻结候选，没有调用旧 CUA 协议：

```sh
DTW_INPUT_TRACE=1 python3 tests/acceptance/cursor_audit.py --assert-independent \
  --observer /tmp/dtw-capability-polish.Pt5am9/cursor-observer \
  --output /tmp/dtw-macos-sequence-audit-20261104-a04 -- \
  python3 tests/acceptance/macos_scene_sequence_probe.py \
    --dtw /tmp/dtw-macos-usability-candidate-a03/bin/dtw \
    --output /tmp/dtw-macos-sequence-audit-20261104-a04/probe
```

重跑时使用新的输出目录，并在具有 Accessibility/Screen Recording 权限的 macOS 测试桌面串行执行。通用命令和光标 observer 构建见 [构建与验收](building.md#current-external-client-gui-probe)。

## 实际任务结果

| 用例 | 独立效果 | MCP 图片输出 |
| --- | --- | --- |
| 新 MCP 上下文直接点击已知 `peer` selector，未先 look | peer 从 0 到 1，其他按钮不变 | 0 |
| capture → 右键 → selector 操作弹层 → 原坐标点击底层 | right/opened/action/base 各增加 1；弹层关闭、底层几何保持、无其他按钮输入 | 1 |
| capture → 右键 → 原坐标操作弹层 → 原坐标点击底层 | right/opened/action/base 再各增加 1；弹层关闭、底层几何保持、无残留鼠标按下 | 1 |
| `look({scope:'menu',text:'File'})` | 返回 File 菜单项；业务计数、key window、active 和前台应用的前后样本保持 | 未单独记录 |

两条三步输入序列均在一个脚本内执行，中间没有额外的显式 look/capture。内部仍做接收者证明与只读刷新。各 1 张图片指工具返回数量，不能推导为内部只捕获一次；本轮未测量总体耗时、token 或真实模型效率收益。

连续光标审计共覆盖 5 个 exec 区间，检测移动为 0、独立性违规为 0。原生 trace 对应 2 次 AX press 和 5 次定向点击。请求采样间隔为 2ms，实际最大采样间隙为 79.755ms，短于间隙的移动可能漏检。前台应用仅有任务前后样本，本轮不能排除中途短暂焦点变化，也不对任意动作或应用承诺零干扰。

两个 MCP 连接都以 0 退出，未强制清理，原生 worker 已回收；fixture 正常响应 quit，未使用 SIGTERM。原始报告：

- `/tmp/dtw-macos-sequence-audit-20261104-a04/probe/report.json`
- `/tmp/dtw-macos-sequence-audit-20261104-a04/report.json`

## 回归与失败保留

Rust 1.98.0 release 全目标测试通过：579 个库测试、5 个 CLI 测试、3 个 MCP stdio 测试。源码检查通过；原生 inventory 套件通过（158 个 inventory 断言及配套控制），probe 清理控制 3 项通过。新增回归覆盖首次及重复脚本、纯 JS、菜单 scope 续绑、延迟弹层、原实例消失、透明空洞、未点击点被兄弟进程接管、弹层移动/消失、画布扩张，以及瞬态捕获一次恢复和两次失败时的输入次数。

保留了全部开发尝试，没有将失败改名为通过：

| 尝试 | 结果及处置 |
| --- | --- |
| `dtw-macos-sequence-20261104-a01-sandbox-failure` | 沙箱中启动未确认，`ps` 清理查询被拒；随后 Host 精确进程查询确认 fixture 未运行。 |
| `dtw-macos-sequence-20261104-a01` | selector 仍轮询旧窗口而超时；修复 app selector 在只读期间跟随当前同实例 surface。 |
| `dtw-macos-sequence-audit-20261104-a02` | selector 通过，pixel 在弹层加入 inventory/capture 间隙时得到 `scene_changed`；修复输入前的一次有界重读。该次另有一个区间检测到光标移动，来源无法归因，记录保留。 |
| `dtw-macos-sequence-audit-20261104-a03` | 同一最终生产候选的三个用例通过；4 个 exec 区间未检测到光标移动。 |
| `dtw-macos-sequence-audit-20261104-a04` | 加入独立新 MCP 上下文的无 look 用例；四个用例通过，5 个 exec 区间未检测到光标移动。 |

上述目录均位于 `/tmp`；具体原始路径见证据摘要。a03/a04 的通过不抹除 a02 的移动，也不能据此给 a02 指定原因。

## 后续边界

- Dock 已可被发现，但当前窗口入口仍返回 `no_window`；应用根、Dock 项和无窗口菜单栏项需要自己的原生归属及命中证明。不能把发现修复当作这些操作已支持。
- 自有独立 popup 证明当前连续操作契约，不替代真实 Telegram 右键菜单、Chrome 原生弹层或更多系统应用验收。
- 安装包与配套 Skill、真实模型任务、持续焦点/用户键盘目的地检查仍需独立证据。
- Windows tooltip/shadow、输入桌面切换及其真实任务留到 Windows 机器；Linux X11 在 macOS 参考边界稳定后推进。
