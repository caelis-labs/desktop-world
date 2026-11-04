# 统一 Computer Use 基准与后台输入验收（2026-11-04）

已实现一个可手工运行的多技术栈验收 APP，并用它发现和修复长文本搜索、深层目标续绑、虚拟光标及不必要的前台激活问题。**完整基准尚未通过。** 最新整套运行 a11 在拖拽期间失去有效窗口，严格真实光标审计也因采样间隙超限失败；后续独立验证只证明各自检查的合同，不能拼成整套通过。

任务入口、界面和运行命令见 [APP README](../tests/fixtures/computer-use-app/README.md)，固定目标见 [computer-use-v1.2](../tests/fixtures/computer-use-app/scenarios.json)，脱敏证据及原报告哈希见 [summary.json](evidence/computer-use-2026-11-04/summary.json)。所有输入通过当前源码候选的标准 `apps` / `exec({app,source})`；独立状态、事件日志和几何仅用于验收。当前安装的 0.3.4 没有替换，本报告不构成安装包或发行验收。

## APP 与退出条件

统一 shell 包含 AppKit 原生工作台、WKWebView 工作台、几乎没有单项 AX 的自绘消息工作台、真实系统文件面板和同进程独立窗口。Web 内容可单独运行，并复用原有 browser-workbench 的库存、输入可靠性和图片挑战入口。

| 技术栈 | 信息与动作 |
| --- | --- |
| AppKit | 190 个含重复名称的审计控件、长文本、Unicode 编辑、checkbox、slider、并列滚动、菜单与 peer 窗口 |
| 自绘 NSView | 30 房间、240 消息、时间与状态噪音、独立低 AX 弹出菜单、Pin/Edit/Reply 和 Unicode composer |
| WebKit / HTML / Canvas | 240 行、720 控件、深层 checkpoint、长文本、表单、异步 modal、嵌套滚动和低 AX 消息页 |
| 系统与运行时 | 真实 Open/Save、取消/覆盖目标、实例与几何边界、取消/EOF、真实和虚拟光标、后台焦点见证 |

42 项目标由 16 项 core 业务、P01/P02/P03 三项强制干扰检查和另 23 项 extended/model/boundary/system 目标组成。后 23 项仍未完成本轮验收；列入 APP 或目标文件不表示已经支持。Windows、Linux 和 Chromium 引擎分别验收，不能由 WKWebView 结果外推。

APP 默认后台启动。手工打开：

```sh
python3 tests/fixtures/computer-use-app/run.py \
  --output /tmp/dtw-computer-use-manual-001 --case manual-001
```

需要查看前台界面时显式加 `--foreground-preview`。标准自动入口为 `tests/acceptance/computer_use_acceptance.py`，同时强制真实光标、实际 overlay 和独立焦点观察；单独运行 core runner 不满足完整验收条件。

## 基准推动的产品修复

| 发现 | 根因及修改 | 验证 |
| --- | --- | --- |
| 无关长文本阻止普通 selector | 搜索复用了 4096 UTF-16 预览前缀；拆开预览和搜索，以每次观察共享的 1,048,576 UTF-16 预算遍历，未搜后缀仍报告不完整 | 晚出现的匹配、重复、预算耗尽和 Unicode 原生回归；a03 的业务进展 |
| Web 深层 checkpoint 不可达 | 筛选只读 4 页；显式 role/text 筛选提高到 32 页，保留截止时间、80 候选和 24 KiB 输出预算 | 首次修复后 a05 暴露续绑仍只读 4 页；统一查询和续绑预算，a10 找到观察 ID 并产生恰好一次业务效果 |
| move/drag 没有独立 overlay endpoint | 只有 click/scroll 初始化展示端点；补齐 move/drag | a05 四种动作实际窗口位置和像素通过；a13 当前候选 click/drag/scroll 通过，后台 move 拒绝 |
| 普通未知状态能授权激活 | 泛化键盘能力、菜单/内容准备、modal/AX 未知状态和物理指针限制混入前台授权 | 删除隐式 selector 聚焦；按实际框架与具体已验证键盘形状准入；a07 严格后台和 a12 默认 auto 焦点合约通过 |
| 光标豁免范围过宽 | 整段脚本豁免不能证明每个移动都合法 | 原生 begin/end trace 覆盖实际借用、配对释放和清理；每个采样转换必须完全落在允许区间内，结束记录在释放 HID lease 之前 |

没有增加第二执行器或 Agent Session；仍是插件接入、JS 编排与平台 computer-use。已知输入不要求每段脚本先 `look()`；未知或已派发输入不重放。

## 默认后台与借用边界

物理指针需求和激活需求分别判断。后台应用的 hover、必要定向 ABI 缺失、未知 AX/modal 错误、菜单或内容不完整，都不能单独授权前台激活。原生准备和最终视觉焦点检查均执行这条约束；失去前台证明或读取失败不能当成许可。

自动键盘前台路线仅保留已有实测证据的四种形状：`chromium_command_a`、`chromium_supplementary_unicode`、`appkit_command_w`、`appkit_multiline_text`。不能把“不是 Chromium”推断成 AppKit；泛化 `native_keyboard_semantics` 不足以授权。sheet、关联进程键盘及无窗口快捷键在缺少所需前台条件时拒绝。合法的前台派发结束后保持目标前台，不恢复之前的应用。

P03 的目标是另一自有 witness 始终占据前台。它验证无初始 look 的语义点击、后台坐标点击、单独 Command+A 的实际 UTF-16 全选范围、准确 Unicode 替换和只读观察；最后验证 inactive hover 干净拒绝。见证者失去前台后立即停止，不重新激活它。焦点采样加激活通知覆盖每个原始请求和整个操作区间，最大实际采样间隙固定为 50ms；witness 必须保持 sentinel 且没有 key/flags 事件。标准 P03 使用默认 `auto` 连接，独立入口也可选择 `background-only`。

P02 策略为 `bounded-pointer-borrow-v3`，实际采样间隙同样固定不超过 50ms。仅有明确能力证据的 `native_hover_tracking`（move）和 `addressed_event_api_unavailable` 指针动作允许进入原生借用；未知理由、缺少/不配对 trace、未完整覆盖请求、采样停顿均失败。没有采到移动不等于完整的独立输入证明；位置变化本身也不能确定操作者。

## 原始尝试与结果

证据根目录均为 `/tmp/dtw-computer-use-<名称>`。目录保持独立，失败没有被后续通过覆盖。较早版本的目标清单包含 39/41 项，不能直接把总数与 v1.2 的 42 项混算。

| 尝试 | 结果与意义 |
| --- | --- |
| baseline-a01 | 4 项业务通过、12 失败；混有 runner 映射/参数问题和实际长文本搜索预算问题。22 exec 未采到位移，不能由此宣称全程无干扰 |
| baseline-a02 | 9 通过、3 失败、4 阻塞。Open 的 AXPress 返回未知但面板已打开；runner 后续改为仅读确认并取消，不重复打开 |
| baseline-a03 | 14 通过、1 失败、1 待图片复核；C01 经实际看图后追加复核通过，保留原始待复核状态；剩余 W02 筛选页数失败 |
| acceptance-a04 | overlay observer 编译参数错误，GUI 未运行；修复 `-parse-as-library` |
| acceptance-a05 | 15 项业务通过、W02 续绑失败、C01 待复核。P01 click/move/drag/scroll 真实 overlay 通过。46 exec 中 15 个区间采到位移，最大间隙 156.228ms，严格 P02 失败；位移操作者未确定 |
| focus-red-a06 | 旧候选的实验遇到几何和外部前台变化，不构成干净的焦点 red 因果证据；没有把变化归因于用户或 DTW |
| focus-green-a07 | 当前候选 `background-only` 8 步通过，最大焦点间隙 19.771ms，witness 无按键/修饰键，清理通过。此运行没有独立物理光标审计 |
| dense-a08 | observer 可执行文件路径不存在，GUI 未运行；没有 report.json |
| dense-a09 | 沙箱中 fixture 启动以 -6 退出，0 exec，不能算产品 GUI 结果；保留启动及清理记录 |
| dense-a10 | 当前候选后台 W02 通过：观察 ID 可续绑，checkpoint 日志恰好增加一次。4 exec、0 位移、最大间隙 34.125ms，严格物理审计通过 |
| acceptance-a11 | 最新整套：5 通过、10 失败、4 阻塞、23 未执行。N06 拖拽中 hit stack 改变，窗口几何变更且不可见，后续失去操作界面；来源未确定。18 exec、0 位移，但最大间隙 203.692ms，严格 P02 失败。P03 未到 witness 操作阶段。资源清理通过 |
| focus-auto-a12 | 当前候选默认 `auto` 的 8 步通过；最大焦点间隙 30.846ms。独立物理审计 8 exec、0 位移、最大间隙 30.212ms，通过。inactive hover 为 `foreground_required/not_dispatched`，无焦点准备；witness 无按键/修饰键，清理通过 |
| sparse-a13 | 当前候选 C01 实际截图及完整 AX absence 复核通过，C02 pin 和 C03 Unicode reply 业务通过。click/drag/scroll 的实际 overlay 位置、热点和渲染像素通过；inactive move 干净拒绝，P01 整体记 unsupported。22 exec、0 位移，但最大间隙 72.683ms，严格 P02 失败。资源清理通过 |

a13 的临时 probe 原报告将有 `unavailable:['move']` 的 P01 投影为 passed，这是临时 helper 的状态分类缺陷；保留该原报告，派生 `reviewed-report.json` 按标准 runner 规则改记 unsupported。图片 review 绑定原 PNG 的 SHA256，未用 review 改写原始执行或物理审计结果。标准 runner 遇到该情况本来就记 unsupported；目前后台 move 没有通过 P01 全部动作条件。

a13 经 `ViewImage` 实际查看的 C01 图像中，`Release check 137` 清晰位于 item 136 与 item 138 之间，完整 AX 查询未出现消息文本。截图 SHA256 为 `ca19c19beba32a4954d790f872fa6b495571601652618cb4947a21ebf8136fcd`。业务坐标来自私有 supervisor，因此这是实际截图审查与直接脚本证据，不是自主模型视觉寻址或 B05 图片挑战通过。

## 候选身份与自动检查

最新候选的源基线为 `f76cdf9` 加本轮工作区修改，二进制 SHA256：

```text
8cf5a7b28847b57fa25a1a153bd66c88d1c4078ab3cb3db27d93011e48e0df47
```

冻结候选的 8 个生产源码文件哈希已逐个核对，与最终工作区对应文件一致。较早 a05 候选为 `1f130952969149e23c5756e78d771c6c8e69377e0d001a7ac63e77f1643791a3`，不包含最新焦点策略，不能用其结果证明新策略。

- 最终源码 Rust release/all-targets：590 库测试、5 CLI、3 MCP 通过。
- Swift 原生 inventory/surface 纯检查通过；宏插件在沙箱被拒绝后按原命令在 Host 完成。
- 新增 Computer Use 验收 Python 单测 41 项通过；真实光标审计单测 20 项、Web HTTP 单测 11 项通过。
- `sh scripts/check-source.sh` 通过，包含新验收单测和 fixture Web 检查；`git diff --check` 通过。

## 仍未完成的条件

本轮未取得一个同时满足 core 业务、图片复核、干净运行、P01/P02/P03 的完整标准运行。a11 的窗口变化与 a05 的真实光标变化尚未建立操作者归因；a11/a13 的采样间隙超限也没有豁免。受控后台合同通过只覆盖其具体操作，不能宣称任意应用、任意输入都不占前台或光标。

其余 23 项仍待推进，包括实际保存/覆盖、更多表单与滚动、Chromium 独立引擎、真实模型图片任务、中断/错误实例矩阵、Dock/无窗口菜单根及 Windows 系统目标。自绘消息 APP 不替代真实 Telegram；Windows GUI 留在 Windows 机器，Linux X11 在参考基线之后继续。后续按这些固定目标的失败逐项修复，不通过缩小目标、放宽 50ms 或重放未知输入制造通过。
