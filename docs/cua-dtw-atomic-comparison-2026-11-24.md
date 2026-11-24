# DTW 原子能力对比：CUA 主干、模型接口与 macOS（2026-11-24）

**结论：保留 `apps + exec`。** 现有 14 个 `app` 操作足以完成常见的单应用任务：找控件、编辑、提交、处理文件面板、读回文档，以及在低 AX 界面看图操作。最影响可用性的不是少一个 `doubleClick()` 别名，而是**同一个定位条件在不同动作中可能指向不同窗口；动作报错后，不总能辨认哪一步已经派发；一个动作发出后，缺少直接等待目标状态的表达式**。

建议顺序：先修这三个合同，再加轻量的 Playwright 式定位语法；macOS 首批做**菜单路径、按元素打开上下文菜单／选择下拉选项、按元素滚动**。这些都能不扩大授权范围地减少截图、坐标换算和焦点切换，且上游有可对照的 GUI 用例。不建议重新公开 atomic MCP，也不建议为了追平 CUA 工具数引入全桌面操作。

本文记录的是[主干 `6e10d17` 时的实现](https://github.com/caelis-labs/desktop-world/tree/6e10d17c5846ed710512252f04b6ff058093ad44)，不是候选版本的使用说明。后续改动的实机结果见[候选验收](app-locator-native-actions-acceptance.md)。

## 比的是哪两个对象，什么叫“验证过”

| 对象 | 本轮基线 |
| --- | --- |
| DTW | `main` / 0.6.0 / `6e10d17c5846ed710512252f04b6ff058093ad44`。以[公开操作表][ops]和[随包 API][api]为准。`--atomic` 已在 [0.6.0](../CHANGELOG.md) 移除；源码里的 `Method::ALL`、`window_ref` 和内部 atomic JS 测试不是模型接口。 |
| CUA | `https://github.com/trycua/cua.git` 的默认分支 `main`，本地 `artifacts/upstream/cua/`，commit [`8c56dd3820f0ec2b7271513783627423b6d905bc`][cua]。已核对 `origin/HEAD → origin/main`、`is-shallow-repository=true`、`rev-list --count HEAD=1`、工作树干净。目录由 DTW 既有的 `/artifacts/` 忽略，不是子模块或产品依赖。 |
| 对比范围 | CUA 中与 DTW 同层的是 `libs/cua-driver`：对照其 [脚本 API][cua-script]、[portable SDK 合同][cua-contract]、macOS 原生工具和 E2E。SDK manifest 明确只是子集，不以其条目数代替 macOS 能力。DTW 自己就从较早的 CUA 移植过 AX、键鼠及 SkyLight；见[来源清单](native-source-provenance.md)和[后台输入移植记录](background-input-port.md)。未把 VM、agent 模型、远程控制平台列作 DTW 缺陷。 |

下文区分三种上游证据：

- **实跑**：查到上游 [macOS hosted run 45291988663][run] 的 [native][native-run] 和 [shared][shared-run] 两个 job 及执行步骤均成功。实际跑的是 `3311e1d27b15e4644df9e6fdb559869a442c4387`，**不是**本次克隆的 HEAD。逐字节比对两个提交的 workflow、两个 macOS runner、[AppKit cell 表][cells]、AppKit/SwiftUI/shared 三个测试文件，共 7 个文件相同。[runner][runner] 按 cell 执行 `--ignored --exact` 的 GUI 用例；这次是两条 lane，shared 为 Electron、WKWebView，不含 Tauri、capture、browser。依据是 job 结果与实际执行清单，未把它当成本机复跑、每条路径均首轮成功或对当前 HEAD 的认证。
- **旧实跑**：[上游支持台账][ledger]把 macOS 的有效 145/145 追溯到 `0b9b5d94`、`64e82449` 及一个单独补跑的 cell，其中含精确拒绝，不是 145 个成功动作。它和今天的测试合同有差异：例如旧表记 Electron 后台 scroll 拒绝，当前 [shared 矩阵][shared-code]要求滚动生效；反过来，当前 Electron 后台 AX `type_text/type_submit/editor_save` 预期是明确拒绝。不要把旧表或一次工具成功响应当作新版本实测。
- **用例 / 源码**：只找到测试断言或实现，或者对应 lane 未在本轮核实。写明级别，不升级为实机结论。DTW 的[原生／WKWebView 业务效果记录][dtw-live]也只证明其冻结候选，和本轮协议测试分开。

## 一、协议够不够，模型用起来卡在哪里

| 用户意图 | DTW 0.6.0 现在怎么做 | 对比后的判断 |
| --- | --- | --- |
| 找应用、进入窗口 | `apps` 拿授权标识；`exec` 固定应用，`look` 按需启动／选窗；`focus`、`quit` 显式操作 | 普通单应用流程比传 PID、窗口号和 session 简单。CUA 可以固定一个明确窗口；DTW 没有公开的任意 sibling 窗口动作。不要为单窗任务引入窗口句柄。 |
| 找元素、看画面、读文档 | `look({role,text,scope/target})`、`capture()`、`readText({target,offset})` | 已有筛选、菜单／子树、同一文本版本的 UTF-16 分页和快照增量。默认 80 项不等于全树只有 80 项。CUA `app.query`、状态查询和可选视觉裁剪更便于定向读取；DTW 的 `text` 不等于 Playwright 的可访问名称。 |
| 指针 | `click` 支持 ID、selector、坐标；`move`、`drag`、`scroll` | 坐标已有左右／双击、悬停、双轴滚轮及配对拖拽；**目标形式不对称**：右键／双击仅坐标，滚动只能按坐标或中心，不能把刚找到的控件作为接收者。这是功能差异，不是缺少同义词。 |
| 编辑与快捷键 | `setValue` 替换原生文本值，`typeText` 键入，`keypress` 发送一个 chord | 三者不可混称 `fill`。`typeText({target,text})` 的共用 schema 有 `target`，但原生能力目前仅 Windows 实现（[准入](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/runtime/mod.rs#L1224)、[后端](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/backends/native_client.rs#L765)）；macOS 已能对编辑控件做经验证的聚焦，再向当前接收者键入。 |
| 等待和验证 | `wait({title,role,text})` 等可见窗口中的匹配项，`window:'stable'` 看窗口元数据，`waitForNewWindow()` 等新 surface；输入返回 receipt | 动作 selector 已检查唯一性；公开 `wait({role,text})` 只要求样本中有匹配，不保证唯一、启用或值正确。[内部测试](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/wait_tests.rs)覆盖部分状态，但公开 JS 缺少等待 **enabled、value、selected 或消失** 的条件，以及“观察不充分，不能下结论”的状态结果。CUA 的 [SwiftUI 实跑用例][verify-case] 对同一窗口组合三个谓词，要求连续两次满足；这是比再加一个 sleep 更值得借的原子。 |

### 三处应先修或明确的合同

1. **`click(selector)` 与 `setValue(selector)` 在弹窗出现时不采用同一选窗策略。** [scene.rs:236–248](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/scene.rs#L236) 只让窗口范围的 `PressSelected` 进入按当前场景重查的分支；[selection.rs:229–262](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/selection.rs#L229) 允许派发前跟随一次同实例顶层变化。setter 走起始窗口，而[执行后的状态处理](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/scene.rs#L573)又将两者都视为跟随场景。本轮在同一受控夹具复现：窗口 101 第一次读取时没有字段，新顶层窗口 102 出现后才有；相同 `{role:'text_field',text:'Popup action'}` 用于 click，向 102 派发一次，用于 setter 则 `selector_not_found`、零派发。证明的是漏选，不是用户应用中已经发生误写。**修法：**两个按 selector 的动作共用「当前实例、已证明的模态／顶层接收窗、最终唯一性」判定。显式 `{target:id}` 仍绑定原对象；模态/兄弟进程不明时拒绝，已派发不再重新解析。
2. **批量脚本失败的定位信息与动作回执分离过度。** 同一脚本先成功一次，再遇到“动作已派发、后置读回失败”，模型能看到失败位置，但聚合计数不总能告诉它哪一步能再做。[JS 桥](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/app_execution.rs#L424)先抛 error 而不返回该步 receipt；[未捕获异常投影](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/scripts.rs#L206)去掉 `details`；[公共结果](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/projection.rs#L37)主要是 dispatch 计数和可选的逐步确认。歧义错误建议看候选 ID，而[候选摘要](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/selection.rs#L406)本身没有 ID。**修法：**保留聚合及 `failure.call/operation`，在失败调用上附有界 dispatch/证据与可执行的只读恢复建议。不能安全发布 ID 就建议带条件的 `look`，不要泄漏内部 ref 或建议重试输入。
3. **新窗等待的锚点不是文档所说的任意“最近已派发输入”。** 常规窗口路径只在 press/click/keypress 回执不是 `not_dispatched` 时设置基线（含 `unknown`）；无窗快捷键、quit 另有路径。等待开始就 `take()` 基线，超时也消耗它。受控复现：新上下文中 scroll 后 wait 得到 `input_required`；随后 `keypress('META+N')` 没产生窗口，第一次 wait 是 `wait_timeout`，再等则是 `input_required`，两次 wait 都不重发键。[设置处](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/scene.rs#L580) · [消费处](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/scene.rs#L743) · [当前文案][api]。**修法：**明确哪些动作产生基线、何时失效，并防止等待错归因到更早的输入；允许继续只读等待也应绑定原动作，不能重发触发窗口的快捷键。

### 借 Playwright 的语法，不借浏览器独有的承诺

[Playwright Locator](https://playwright.dev/docs/locators) 最值得借的是「先描述目标，每次动作前才解析」和唯一匹配；[actionability](https://playwright.dev/docs/actionability) 值得借的是有预算的派发前等待。CUA `run_script` 已有 `app.click({role,name})`、`waitFor`、`verify`、按目标 `pressKey/scroll`，不只是蛇形命名的 MCP 方法。DTW 不必再造一个 `page` 或导入 Playwright。

```js
// 今天能运行的写法（示例界面）：等待同窗状态，不把 click 回执当保存成功
await app.click({selector:{role:'button', text:'保存'}});
await app.wait({role:'static_text', text:'保存成功', timeout_ms:3000});
```

```js
// 建议的脚本语法草图；不是 DTW 0.6.0 API
const save = app.getByRole('button', {name:'保存', exact:true});
await save.click();
await app.getByRole('static_text', {name:'保存状态', exact:true})
         .waitFor({value:'保存成功', timeout:3000});
```

- **新增 `name/exact`，不重定义 `text`。** 当前 macOS `text` 在标题、描述、值等原生字段做宽匹配且忽略大小写和变音符号（[匹配与遍历](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/backends/macos/Native.swift#L4420)）。例如 `Save` 也可能命中 `Save As`；不能把 `name` 简单翻译为 `text`。先定义 DTW 的原生名称及角色映射，使用真实 `text_field/static_text` 等角色，不冒充完整的 ARIA 名称算法。支持祖先／子树限定比遇重名默认 `.first()` 更可靠。
- **locator 只持查询，不持特权。** 查询消歧和状态等待由同一个执行／准入层负责，可以重做只读观察，不可重做输入。传入已观察 ID 则仍须证明原生对象连续性；不允许因同名替换或出现另一个实例而悄悄改投。正向匹配与不存在不同：树不完整、值不可读、状态变化时要能表示 `unknown`，不能宣称已消失。
- **不要提供错误的兼容别名。** [Playwright `fill`](https://playwright.dev/docs/api/class-locator#locator-fill) 会触发 `input`；DTW 原生 `setValue` 不作此承诺。DTW 已实测 Go to Folder 的值设对但没触发路径选择，键入才生效（[记录](dtw-next-optimization-plan-2026-11-17.md#实机结果)）。同理，locator 的 `press('Enter')` 应表示向经证明的键盘接收者送键，而非对 AX 元素做 `AXPress`。不能实现该合同时宁可暂不提供。保留现有脚本和回执；不要引入 `force` 绕过所有权检查或用并行 Promise 包装需要串行的输入。

## 二、macOS：已实现什么，上游还多什么

AX 表示按辅助功能元素寻址，PX 表示按像素寻址，均不自动说明实际派发方式。下表的「实跑」限定为上述有记录的宿主和用例。源码位置的数字是该固定提交中的行号。

| 任务／证据 | CUA 已有的可区分行为 | DTW 的现状与最小切入点 |
| --- | --- | --- |
| **多级菜单 · 实跑** | [AppKit][appkit] 1207、1343：`Window → Arrange → Left` 由 fixture 的 `menu_action` 确认；缺叶项或后台禁用时没有业务动作，失败后紧接的有效路径仍可执行。 | 已有 `look({scope:'menu'})`、保留子树、按项 click 和快捷键；[菜单观察](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/backends/macos/Native.swift#L4788)不展开逐级子菜单。补按路径逐段精确解析、校验 enabled、只清理本次打开的层级；业务命令至多一次，无需让模型管理原生菜单句柄。 |
| **上下文菜单、下拉框、自定义 AX 动作 · 实跑** | [AppKit][appkit] 3185、3216、1511：后台按值选 `Finance` 后读回；按元素打开上下文菜单后 `Copy` 确实触发；调用声明的 `Pin Row` 使专用计数增加且未触发普通 press。1453 另测带 `AXHelp` 的分组。 | DTW 的像素右键已可用；语义动作主要是 `press`（含聚焦、集合选择）与可编辑**字符串**的 `setValue`，见 [Native.swift:4244–4284](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/backends/macos/Native.swift#L4244)。先补按元素 `showMenu`、按选项 `selectOption`；自定义动作只允许同一元素现场重新声明的动作，不能把任意字符串当原生调用。普通点击／文本 setter 不改义。 |
| **按元素或页滚动 · 实跑** | [AppKit][appkit] 1961、2009：前／后台以滚动 offset 变化验证。其[原生适配](https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/rust/crates/platform-macos/src/tools/scroll.rs#L782)限 `AXTextArea`、父 `AXScrollArea` 及可用滚动按钮。Electron [shared 用例][shared-code] 1545 的效果断言看真实 `scrollTop/clientHeight`，不只看事件送达。 | [公开 scroll](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/scene.rs#L532)只接中心或截图点；[macOS 内部已有](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/backends/macos/mod.rs#L1370)「保留元素 → 校验可见几何 → 定点 wheel」。先把这个锚点接到公开 `target`，不用重写 wheel。CUA 的 AX 翻页／reveal 是不同的原生能力，不能静默改变 delta 单位。验收目标 pane 改变、邻区不变，不因未见变化再投一次。 |
| **快捷键和精确文本接收者 · 实跑；键位布局仅源码／单测** | [AppKit][appkit] 1577 针对编辑元素送键并确认提交；[TextEdit](https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/rust/crates/cua-driver-e2e/tests/installed_app_textedit_macos_test.rs#L306)按 token 后台插入、独立刷新读回，另测兄弟文档不串写；这不证明磁盘已保存。[上游键位](https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/rust/crates/platform-macos/src/input/keyboard.rs#L1100)会读取当前布局，含 Dvorak 等测试。 | DTW 已有经验证的 editable click、Unicode 键入、菜单等价键；缺少 macOS 上**单调用**的目标字段键入／送键。命名快捷键两条路径仍有固定键码表（[Rust](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/backends/macos/cua_input/keyboard.rs#L165)、[Swift](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/backends/macos/Input.swift#L287)）。先复用 receiver 证明；键位另用非 US 布局及切换验收，不与 Unicode 文本混为一谈。不要用 AX 插入静默取代需要输入事件的 `typeText`。 |
| **修饰点击、中键、拖拽 · 实跑与旧实跑** | [AppKit][appkit] 562、661、1821：一个有序 down/up、中键到指定窗口、`Cmd+click` 前台保留原选择且后台拒绝不改值。上游另有 slider 拖拽送达与启动 AppKit DnD 会话后拒绝的[旧记录][ledger]。 | 已有 PX 左右／双击、hover、同窗路径拖拽和标题栏移动；缺中键与一次手势内的 modifiers。值得先补**自包含的修饰点击**，断言前后选择集合、取消后按键／按钮释放。滑块成功不等于拖文件成功；不引入跨调用 `down/up` 或跨应用拖放。Chrome 特定 `AXWebArea` 的定向 hover 已在[当前路由][route]，不能照旧报告列为缺失。 |
| **状态验证与诚实回执 · 实跑** | [SwiftUI][verify-case] 190–251 验 enabled、selected、连续样本；[AppKit][appkit] 2102 区分“AXPress 回错但 checkbox 已改变”和“回错且没变”。 | `look` 能读值、`readText` 能分页；成功回执与后置确认已分离。但[普通 AXPress](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/backends/macos/Native.swift#L1541) 遇错误保留 unknown，缺通用可等待状态条件。先做只读状态核验；可把 toggle 变化附作后置证据，**不把它追认为派发成功，也不再次切换**。 |
| **窗口、sheet、生命周期 · 实跑／源码** | [Open panel 用例](https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/rust/crates/cua-driver-e2e/tests/bring_to_front_macos_test.rs#L503)在长文件清单中仍找 Cancel，并从 WindowServer 验证 sheet 消失；[AppKit][appkit] 3283 起用 `set_window_frame` 移动窗口并核对截图，**这条用例保留尺寸，不证明 resize 已实跑**。CUA 另有显式 reopen 已隐藏应用的[用例](https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/rust/crates/cua-driver-e2e/tests/installed_app_launch_macos_test.rs#L233)。 | DTW 已支持安装发现、非激活启动、聚合多实例、可证明的 sheet/helper、真实 Open/Save 和正常 quit；标题栏可搬窗，不等于显式设置窗口尺寸或寻址最小化的 sibling。[实机边界](agent-capability-followup-acceptance.md)。先借大文件面板及同应用双文档负例；窗口布局命令仅在有任务需求时补。 |
| **像素观察与过期截图 · 用例／源码** | CUA 有 `zoom`（截图裁剪，不是捏合手势），可选一次性 `capture_id`。其 [capture→parse→click 测试](https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/rust/crates/cua-driver-e2e/tests/perception_capture_loop_test.rs#L725)要求已消费的图拒绝第二次点击；发布扩展另验 OCR 结果、过期图。此次核验的 CI 没有 capture lane。 | DTW 已有 app 合成 PNG、透明像素命中、几何刷新、画布改变提示和 fraction 坐标；输入绑定**最近的**截图，不能显式指定“我看的那张图”。[既有 N14 记录](computer-use-extended-acceptance-2026-11-04.md#最新结果)把此项记为未覆盖。优先评估可选图像版本校验／裁剪坐标回映；不必为此引入默认 OCR、全屏截图或把原生 ref 暴露给模型。 |

**哪些不应当算欠账：**DTW 的单位是一次授权的 app，前台／后台由 provider 按实际能力选择，确需前台时不承诺归还焦点或光标；CUA 的显式 `delivery_mode`、桌面输入、全局剪贴板、跨 Space、虚拟显示器、浏览器 DOM/CDP 和 force-kill 不是免费补全。历史记录中自绘 Pin 和 Web 相邻字段误写确曾失败，后来的[直接输入验收][dtw-live]已分别证明只 Pin 一次、中文和 emoji 写到正确备注且数量不变、只保存一次；不要重复立项。后台证据只外推到测过的 AppKit、Electron、WKWebView 等宿主；多屏混合缩放、锁屏及任意应用无干扰都未由本轮证明。

**权限与显示的证据更窄。** 上游[权限探测](https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/rust/crates/platform-macos/src/permissions/gate.rs#L548)用新进程规避 Screen Recording 的进程内缓存；DTW [当前实现](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/backends/macos/Scene.swift#L13)做 preflight、限制重复提示并报告 `permission_required`。晚授权探测、轮询节奏和多屏坐标的模拟测试不等于真实 TCC 变更／混合 DPI 验收。优先借诊断和测试，不为此扩大授权。

**其他平台只作合同校验。** Windows 走前台／UIA，已支持带目标的 `typeText`；Linux 是本地原生 X11，单个可证明窗口才有 AT-SPI，菜单／子树、正常 quit 不可用，Wayland 不在合同内。建议在模型可见的方法说明中紧挨签名标注这些差异，未支持要在 payload 前拒绝。不要把 macOS 的 AX 动作名塞给其他后端，或为对齐签名伪造成功。详见[平台矩阵](product-matrix.md)。

## 建议的首轮验收

1. **先修协议，不加 MCP 工具。** 在现有延迟弹窗夹具中给父窗和新窗放同名可写字段；分别用 selector、保留 ID、另一进程实例操作，核对确切接收者及派发次数。对批处理的“前一步已发、失败步未发”和“失败步已发但读回失败”分别检查 MCP 可见错误。新窗等待覆盖首个 scroll、等待超时后继续观察、前一步 unknown；均不得重放。
2. **再做一条完整的定位—动作—验证链。** 给 `Save`／`Save As`、同名双按钮、字段值恰含按钮名、尚未启用的按钮、截断树、已替换对象各做一例。验收 `getByRole(...).click()` 至多派发一次；`waitFor` 的超时、`unknown` 和明确不满足可分辨。用同一组本地任务对比新旧写法的首试成功率、工具往返、截图次数、误投次数，而不只比较代码字符数。本轮未做模型 A/B。
3. **macOS 先验菜单／选项／滚动，再验修饰手势。** 复用 DTW 自有工作台，增加两处同名菜单叶、禁用项、选项当前值、两个独立滚动 pane、受控多选列表。用业务日志／真实值及邻接对象不变判定，不用“已派发”打分；前台、光标、配对释放和用户切走中止分别记录。非 US 键盘、混合 DPI、跨窗拖放应是独立验收，不阻塞前两批。

### 本轮复核

本地对 DTW 0.6.0 跑公开方法枚举、场景选择、新窗等待、重复／不完整 selector、未知输入不重放等 19 项定向 Rust 测试，19 项通过（受控 backend，非 GUI）：

```sh
cargo test --manifest-path cmd/dtw/Cargo.toml --locked --offline --lib -- \
  scene_lazy_tests:: sdk_public_method_enumeration_matches_app_contract \
  selection_waits_for_complete_unique_discovery selection_does_not_hide_nonpressable_duplicate \
  selection_unknown_action_is_not_rediscovered --test-threads=1
```

另从同一 DTW 提交临时导出源码，在[现有场景夹具](https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/scene_lazy_tests.rs#L34)中增加两项诊断，实际经公开 JS 对比「晚出现的文本框：click vs setter」及「scroll／keypress 后连续等新窗」；2/2 均得到上文列出的结果。临时目录已清理。`python3 tests/ci/run_native_input_tests.py` 与 `python3 tests/ci/run_native_inventory_tests.py` 也均通过；后者在沙箱内被 Swift SDK 宏编译权限阻断，获准后原命令通过。两组都是不操作 GUI 的合同测试。

以上是基线调研：当时未在用户桌面运行 CUA 或向真实应用投递输入，也未更改 DTW 产品代码／Skill。PR 后续实现和实机结果单列于[候选验收](app-locator-native-actions-acceptance.md)，不改变本文的历史结论。

[ops]: https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/cmd/dtw/src/app_protocol/operations.rs#L35
[api]: https://github.com/caelis-labs/desktop-world/blob/6e10d17c5846ed710512252f04b6ff058093ad44/skills/dtw/API.md#methods
[cua]: https://github.com/trycua/cua/tree/8c56dd3820f0ec2b7271513783627423b6d905bc
[cua-script]: https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/rust/crates/cua-driver-core/src/script_tool.rs#L164-L198
[cua-contract]: https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/contract/README.md#scope-and-compatibility
[run]: https://github.com/trycua/cua/actions/runs/45291988663
[native-run]: https://github.com/trycua/cua/actions/runs/45291988663/job/138103319471
[shared-run]: https://github.com/trycua/cua/actions/runs/45291988663/job/138103319643
[cells]: https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/scripts/ci/macos/appkit-cells.tsv
[runner]: https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/scripts/ci/macos/run-rust-e2e.sh#L158-L169
[appkit]: https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/rust/crates/cua-driver-e2e/tests/harness_appkit_test.rs
[verify-case]: https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/rust/crates/cua-driver-e2e/tests/harness_swiftui_test.rs#L190-L254
[shared-code]: https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/rust/crates/cua-driver-e2e/tests/cross_platform_behavior_test.rs#L1830-L1842
[ledger]: https://github.com/trycua/cua/blob/8c56dd3820f0ec2b7271513783627423b6d905bc/libs/cua-driver/docs/action-support.md
[dtw-live]: direct-input-policy-acceptance-2026-11-17.md#实机证据
[route]: macos-input-routing.md#路线选择
