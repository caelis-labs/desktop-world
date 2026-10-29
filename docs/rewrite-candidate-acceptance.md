# 重写候选与最终安装验收

日期：2026-10-29。前两批核心实现在 #66 / `7a06c6b`，开发字节的原生与外部 Agent 六轮门槛已通过，见[核心验收](release-core-acceptance.md)。本记录从第三批开始：0.2.0 为候选源版本，尚未发布。

候选构建只在显式触发时运行，不恢复 CUA SDK/worker 或旧打包链路。Windows 最终支持须由用户独立验收；Linux 不制作正式候选或作支持承诺。编译、签名、公证、安装和 GUI 验证分别记录，不能互相替代。

| 层级 | 当前状态 |
| --- | --- |
| 固定来源与候选脚本 | 导出 clean Git commit/tree，固定 Rust 1.98.0/lockfile；目标依赖 notices、平台格式、版本、归档完整性均有检查 |
| macOS 候选 | `50dbec3` 已签名、公证；146 文件安装清单复核和六组原生 GUI 通过 |
| macOS 安装与 Agent | 同一安装包六组原生、Claude 3/3、agy 3/3 有效样本通过；另保留 1 次已解释的验收器误判失败 |
| Windows 原生构建 | `50dbec3` 原生编译、286 个库测试、3 个 CLI 测试及 C# 验收工具编译通过；当前 ZIP 已完整复核 |
| Windows 实机 | 用户独立安排，尚未通过；候选自带标准 MCP 双实例 smoke 与手工矩阵 |
| 公开 release/tag | 未创建 |

## 固定候选与安装验收：50dbec3

两个目标的源码均为 `50dbec36dbdcf9f85bbe36f2a092c95fac8fdb84`，冻结 Skill SHA-256 `47131aca267a20acb2de76d46b7568da0035b1caa9920d192264378d3fffb8b6`。后续验收记录更新不改变候选内的源码、运行时或 Skill；本候选尚未公开发布。

- macOS：[构建 40821230632](https://github.com/caelis-labs/desktop-world/actions/runs/40821230632)，[候选 artifact](https://github.com/caelis-labs/desktop-world/actions/runs/40821230632/artifacts/12874787735)。277 个库测试、4 个 CLI 测试、3 个 stdio 测试通过。ZIP SHA-256 `cad422a0020e883ef7cef15bb8c6180cf6da9207c81691927fa78b961a311adb`；二进制 `2ef33daf84aedceea71b69a4e10712f0b291282ae0df81473c8939937ea7bc6b`；BUILD `12ec84cfe92781c80a557f4594cefea8ba5ed3d1fddfc12af5c0dda6aed97453`。
- 签名为 Developer ID、Team `64KZ67PM5J`、标识 `dev.caelis.dtw`；公证 Accepted，原提交 ID `076ee1aa-025b-4542-9ea3-29415fb6e9ef`。解包后 strict 签名有效；首次 `=notarized` 检查失败，`spctl` 在线评估后同一字节通过该要求。原始两份记录都保留。`spctl` 自身仍返回“有效代码但不是 app”，不把它写为 `.app` Gatekeeper 或全新系统安装通过；未重签、未改包。
- 安装目录 `/private/tmp/DTW 安装验收 0.2.0 参数归一版/desktop-world-0.2.0-aarch64-apple-darwin`；146 文件的完整清单与哈希通过。本机 macOS 27.0.1，最低编译目标 14.2 不代表旧系统实测。GUI 期间仅以进程绑定的 `caffeinate -di` 防止空闲熄屏，没有改变锁屏设置。
- Windows：[构建 40821230287](https://github.com/caelis-labs/desktop-world/actions/runs/40821230287)，[候选 artifact](https://github.com/caelis-labs/desktop-world/actions/runs/40821230287/artifacts/12874553873)。286 个库测试、3 个 CLI 测试及验收 C# 编译通过；完整 ZIP 清单与哈希已下载复核。ZIP SHA-256 `bd99733bf50b9d7bb021add97f21da6373ade2af8b6536c87d37f5faea457e41`；二进制 `f98c953c8d54bfbd302dbfd854855231b3550006abf283323ee04ea775e09a29`；BUILD `27bc252dbbf0d938f785da4fc1d85e6c11072b05c1af714c2208f1b19e5cc07b`。该 Windows 候选未签名，真实 GUI 待用户独立验收。

六组 macOS 安装字节原生验收全部通过：`/private/tmp/dtw-installed-host-arguments-{native-all,native-focus-intercept,native-eof,native-parent-exit,multi-instance,web-discovery}-20261029/evidence.json`。覆盖常用输入与释放、EOF/父退出、多实例精确路由和浏览器发现；窗口截图已视觉核对。

受控失焦操作 `execution_e05b6d2a-6d6a-4218-802a-88b1ba4838d9` 在输入 18 字符后返回 `focus_changed/unknown`，提示刷新并确认部分效果，保存后缀未执行；另一窗口保留原 seed，两边保存数 0。此样本没有新增误投字符，但前一签名样本曾有一个已排队的 `q` 落到另一窗口。因此这里只证明检测与停止后续输入，不承诺撤回已投递事件、零误投或 OS 级原子性。同一窗口中的任意业务内容变化也不具备完整版本锁。

Windows 用户应使用上述当前 artifact 中的 `desktop-world-0.2.0-x86_64-pc-windows-msvc.zip`。包内 `acceptance/windows/README.md`、`smoke.ps1` 和手工矩阵通过标准 MCP 验证两个同应用实例、文本/快捷键、截图、释放和可选指针组。Rust 1.98.0、MSVC 19.51.36260、Windows SDK 10.0.26100.0；静态 CRT 和原生导入检查通过。CI 编译不替代 Windows 桌面、DPI、权限、并发和取消验收。

开发预检 `/private/tmp/dtw-host-arguments-{claude,agy}-20261029-16/evidence.json` 均通过：Claude 5 exec / 6 MCP、8,934 JSON bytes、57.976 秒；agy 7 exec / 8 MCP、10,480 JSON bytes、175.273 秒。开发二进制 `dbff5998708938cbe18938934e6066f5a996eb64ba29f78a2c8e15e6525df7bd` 与冻结 Skill 组合仅作预检，不计入安装六轮。

### 安装包的六个有效 Agent 样本

场景是自有的 240 行 Chrome 工作台和真实 TextEdit：只编辑两条指定记录、每条恰好保存一次、其余 238 条不变，再填写/保存/读回报告。业务动作仅走标准 DTW MCP，未使用 DOM、HTTP 或文件编辑代做。六个有效样本均通过完整业务、易用性和清理检查；没有 DTW errors、unknown、未恢复读取或元数据查找失败。末尾再次核对整个 146 文件安装清单，字节未变。该证据不等于生产账号、冷启动或全新系统验收。

| 样本 | Agent 工具总次数 | exec / MCP | JSON bytes | 图片 bytes | Agent 总耗时（秒） |
| --- | ---: | ---: | ---: | ---: | ---: |
| claude-01 | 8 | 5 / 6 | 8,324 | 65,555 | 51.924 |
| agy-01 | 10 | 6 / 7 | 13,133 | 65,245 | 162.299 |
| claude-02 | 8 | 5 / 6 | 7,860 | 65,744 | 51.219 |
| agy-02b | 12 | 6 / 7 | 29,740 | 65,114 | 152.022 |
| claude-03 | 8 | 5 / 6 | 9,426 | 65,316 | 59.279 |
| agy-03 | 12 | 7 / 8 | 12,815 | 65,010 | 209.306 |

Claude 三轮中位数为 5 exec / 6 MCP、8 次 Agent 工具、8,324 JSON bytes、51.924 秒；agy 为 6 exec / 7 MCP、12 次 Agent 工具、13,133 JSON bytes、162.299 秒。JSON 按紧凑 DTW payload 计一次、图片另计，不代表完整模型 token 成本；耗时包含模型和客户端等待。Claude 显式指定 codex/gpt-6.1-terra，agy 沿用本机客户端配置；这些观测不能作为同模型下的公平速度排名，也不是 SLA。

路径为 `/private/tmp/dtw-installed-host-arguments-<样本>-20261029/evidence.json`，汇总 `/private/tmp/dtw-final-installed-summary.json`。前三轮是在相同候选上取得的通过记录，之后又以最终验收器复核路由和计数；原 agy-02 的元数据误判失败保留在下文，未更名或改判，后续三轮均为新任务。最终验收器的改动与后续文档提交没有改变 `50dbec3` 的运行时、Skill 或打包代码。GUI 已结束，测试资源已清理，桌面已释放。

安装序列的原第四轮 `/private/tmp/dtw-installed-host-arguments-agy-02-20261029/evidence.json` 保持失败：6 exec / 7 MCP、11,964 JSON bytes、226.667 秒，DTW issues 为空。原始 Agent 记录显示它先读本次插件根目录 `SKILL.md`，明确返回 `file does not exist` 且无数据，随后读取实际 `skills/<name>/SKILL.md` 成功；验收器错误地把这个普通元数据查找失败归为读取任务数据。它未通过文件读取业务数据，业务操作均走 DTW。原验收因此未进入初始业务断言，不能将原记录直接改为通过。

保留的完整业务快照经独立核对符合两次保存和报告内容；自有 TextEdit 报告又通过新的完整 GUI 读回与磁盘比对后单次关闭，另存 `reconciliation.json`，原失败文件未改写。验收器仅针对本次精确插件根路径、明确不存在且无输出的元数据查找修正分类，并独立记录该额外步骤；仍要求实际正确 Skill 被加载。任务文件、成功读取别名、其他错误或任何返回数据都不能由此放行；DTW 错误、unknown、业务和调用阈值不变。修正仅在外部验收器，未改安装包或 Skill；余下三轮使用新的自有任务样本。验收器基线为 `c46667fdbc75beea5bf2d378a77ebf0545286175`，探针 SHA-256 `14c48a641ab497e6e9f5a3c5f4249e0314ae08afdda7923338a6abacb0035c01`；47 项纯测试及最终 8 项路由复核通过。前三轮原始调用经新规则复核，其已有计数、字节和通过结论不变；原第四轮另存 `route-review.json` 解释 1 次元数据查找失败，不重判原业务/清理状态。

## 历史 Windows 交付（已由上方候选替代）

成功构建：[40796800484](https://github.com/caelis-labs/desktop-world/actions/runs/40796800484)，源提交 `a07d1eb48ca391d3fe20a37e6beda4f9a584b847`。当时的 artifact 为 `dtw-windows-candidate-a07d1eb48ca391d3fe20a37e6beda4f9a584b847`；该包和更早失败包仅保留历史记录，当前交付使用上方 `50dbec3`。

- ZIP：`desktop-world-0.2.0-x86_64-pc-windows-msvc.zip`，SHA-256 `c1471679ca9e70b29413148eec9eb0decca0b4dba7400be12188b0806ead55b1`。
- `bin/dtw.exe` SHA-256：`940d21d60cf750c04f41fb5e118952261269ef8bcf21eccf6db742907805cef5`。
- 工具链：Rust 1.98.0、MSVC 19.51.36260 x64；该包的 SDK 环境字段为 null，不推测其版本。PE 导入检查通过，不依赖动态 VC CRT。最低 Windows 系统版本仍须实机验证。
- 包内 `acceptance/windows/README.md` 和 `smoke.ps1` 用普通 stdio MCP 操作两个同可执行文件实例，检查分别编辑/保存、Unicode/快捷键、窗口截图和释放状态；另有可选指针组。仅需 Windows PowerShell 5.1/.NET，不需 Rust/CUA/VS。
- 手工矩阵仍包含真实浏览器/Agent、DPI、并发会话、取消/超时、进程退出重启、EOF/父退出、权限与安全桌面，以及截图视觉核验。CI 的 C# 编译不计 GUI smoke 通过。

## 前一轮固定候选复核

候选 [40813820899](https://github.com/caelis-labs/desktop-world/actions/runs/40813820899)，源 `3ba108ef98dd97d467b2cb89c72b17b0cb1da4c3`，268 个库测试、4 个 CLI 测试、3 个 macOS stdio 测试通过。Developer ID 签名与公证 Accepted，原公证 ID `8bfcc230-0b81-4455-9d0a-d5bc0ee55506`。

- ZIP SHA-256 `d5f6ba6fb020a2a02f49835e1551af74e5534c62c14c048a98a6a7c11e34ba4f`；二进制 `3be785cccee20e7ba83774b975114c4fe70cc467eeb629fea89ad3c0ffc4393e`；Skill `7aeba65bf92c6cbb718cae2582b35ba70dd6b118a9c34ee827598739ee1382b1`。
- 在 `/private/tmp/DTW 安装验收 0.2.0 观察恢复版/desktop-world-0.2.0-aarch64-apple-darwin` 核对 146 文件清单。首次 `codesign -R '=notarized'` 未通过；执行 `spctl` 评估后同一字节通过该公证要求。`spctl` 本身返回“有效代码但不是 app”，不将它记为 `.app` Gatekeeper 或全新系统安装通过；原校验结果分别保留，未重签或更改归档。
- `/private/tmp/dtw-installed-refresh-{native-all,native-focus-intercept,native-parent-exit,multi-instance,web-discovery}-20261029-01/evidence.json` 及 `...-native-eof-20261029-02/evidence.json` 六组通过，截图已视觉核验。EOF 的第一次启动缺少探针要求的 trace 环境，在 GUI 创建前即停止，保留原日志，修正启动配置后才执行独立样本。
- 受控失焦目标收到 19 字符，另一窗口收到一个 `q`，两边保存数都是 0；原操作 `execution_a521c93e-15db-469f-a7ac-f7ec7cbe8b7a` 返回 `focus_changed/unknown`。保留已排队事件无法撤回的限制。

安装模型序列首轮 `/private/tmp/dtw-installed-refresh-final-claude-20261029-01/evidence.json` 未通过：6 exec / 7 MCP、8,756 JSON bytes、88.028 秒，业务初始正确且恰好两次保存，但脚本 `execution_7a25b469-dbab-491f-a82a-30f39fc6b6f3` 在打开表单后把 `text: ""` 传给 observe，被参数校验拒绝。不是窗口变化或原生查询失败。余下五轮未运行；已保存文档经独立完整读回与磁盘比对后单次关闭，`reconciliation.json` 不改变原失败。

该缺口改为：可选空关键词规范化为无关键词过滤，公开 schema 同步；非空空白、角色/引用、长度、cursor 互斥仍按原合同校验。`invalid_args` 现在保留具体字段原因，拒绝仍发生在派发前，不撤销脚本里更早的动作。77 项定向 Rust 控制通过，包括真实 VM 分页绑定与真实 MCP 的空关键词/省略等价及严格拒绝控制。

同时审计共用 Skill 与 Windows 原生角色：规范复选框应是 `checkbox`；可用 TogglePattern 的 Off/On/Indeterminate 分别读为 `"0"`/`"1"`/`"2"`，未知状态保留 null，原生读取失败不当作 false。新增 3 项 Windows 控制，需在最终 Windows CI 编译执行。多行输入的实际角色可能为 `text_field` 或 `text_area`，Skill 从定向查询取真实角色并复用，保留静态标签排除、禁用控件、歧义与新引用检查，不增加扫描。8 组 JS 边界控制、完整两记录批处理、4 段语法与 Skill 校验通过；随后只排除明确静态标签，避免丢弃同名禁用的 unknown/combo_box 控件；相应 8 组控制通过。冻结 Skill `7cc535aec6ad64705a6c6f14a631d820c36e5254436e43f1820251b83b099b6e`，新模型/安装验收进行中。

开发二进制 `3e8c2134c2935c651bceeb45a23324611ba2a25efaee12a9ddc642654a7845cb` 配合中间 Skill `64724a8884ff4e9b57f846d390ec7821e99927569774b7c1fbd7c7fc74f540fe` 的 `/private/tmp/dtw-empty-filter-claude-20261029-14/evidence.json` 未通过，agy 未启动：12:21:56–12:22:36 的两个 exec 分别遇到 `semantic_window_unavailable` 与 `capture_failed`，任务没有写入；清理按键明确 `not_dispatched`。`pmset` 原始日志证明 12:06:03 熄屏、12:24:48 恢复显示，覆盖该样本。恢复显示后，开发版和签名版对同一自有窗口的独立只读查询均成功；未推断锁屏或其他用户操作。新查询的首个分页仍有 next，不将其当作完整发现。

该样本 0 次业务保存、报告仍为初始 `Pending review`；两扇窗口经独立核验后单次关闭，另存 `reconciliation.json`，不改写原失败。后续 GUI 序列使用 `caffeinate -di <验收命令>`，断言随该进程结束释放，不修改锁屏设置、不唤醒或解锁会话；这是验收环境控制，不是产品的熄屏/锁屏使用能力。

在显示保持开启的 `/private/tmp/dtw-field-contract-claude-20261029-15/evidence.json` 中，开发二进制 `3e8c2134c2935c651bceeb45a23324611ba2a25efaee12a9ddc642654a7845cb` / Skill `7cc535aec6ad64705a6c6f14a631d820c36e5254436e43f1820251b83b099b6e` 仍未过门槛：10 exec / 11 MCP、11,296 JSON bytes、155.741 秒。通用函数把可选 role 留为 undefined，`execution_978fa705-8593-4c32-abc0-b105e6a95389` 在打开表单后触发 Host 参数非 JSON 错误；恢复后业务正确且恰好两次保存，但该轮保持失败，agy 未启动。已保存报告另行完整读回后单次关闭，不重判原记录。

进一步按 JavaScript 常见参数习惯，仅对 Host 参数普通对象省略自有、可枚举、值为 undefined 的数据属性；共享 schema、必填参数、数组和严格 state/result 边界不变，不执行 getter/toJSON，不修改调用方对象，省略属性仍计遍历预算。新增 5 项有意义控制，相关 82 项测试通过（真实 VM、分页 Core、MCP 与非法参数/状态边界）。

`d356f8e` 的 [macOS 构建](https://github.com/caelis-labs/desktop-world/actions/runs/40819297711) 已通过 272+4+3 测试及签名/公证，原公证 ID `899b7107-91e3-4f2c-8381-cfecb3a78650`；[Windows 构建](https://github.com/caelis-labs/desktop-world/actions/runs/40819298277) 通过 281+3 测试，包括新增复选框控制。它们尚不包含上述 Host undefined 参数修正，不能作为该修正的最终安装包证据。

前一开发组合的 `/private/tmp/dtw-discovery-refresh-{claude,agy}-20261029-13/evidence.json` 都通过：Claude 6 exec / 7 MCP、8,757 JSON bytes、53.425 秒；agy 6 exec / 8 MCP、21,232 JSON bytes、142.838 秒，业务与清理通过。这些样本只证明其开发二进制与 Skill，不能替代上面的安装失败或后续新版本验证。

Windows 同源 [40815204943](https://github.com/caelis-labs/desktop-world/actions/runs/40815204943) 编译与 274+3 测试通过；此时尚不包含空关键词与复选框修正，保留为中间构建记录，不宣称最新 Windows GUI 或最终交付通过。

## macOS 安装字节与模型反馈

原生验收候选：[40804590000](https://github.com/caelis-labs/desktop-world/actions/runs/40804590000)，源 `df7beabdeb007e1256fab00ae0f515713f1f5a31`。259 个库测试、4 个 CLI 测试、3 个 macOS stdio 测试通过；Developer ID 签名、时间戳、Team、临时钥匙串清理及公证 Accepted。原公证 ID 为 `5b37217e-caa2-4078-86de-5bf06509adef`。

- ZIP SHA-256：`180b5517811a27895f3b1c152aac4263ce34014b213ee301d7dc59d8308cc49f`。
- 二进制 SHA-256：`8d2e493722264cfa89a8c4a7366a38b45c07d7df62f41765b318dbfe06d8e2d7`；包内 Skill SHA-256：`22558b17933ff4ae53303230e32c42033ad0d3dde3b9895203754006f94982cc`。
- 安装到 `/private/tmp/DTW 安装验收 0.2.0 最终候选/desktop-world-0.2.0-aarch64-apple-darwin`，解包后再次核对归档、完整安装清单、签名和在线公证要求。这是当前开发机的新目录安装，不是全新系统/TCC 或最低系统版本验收。
- 六组原生验收均通过：`/private/tmp/dtw-installed-final-{native-all,native-focus-intercept,native-eof,native-parent-exit,multi-instance,web-discovery}-20261029-01/evidence.json`。覆盖 Unicode/快捷键/保存、语义动作、滚轮、右键/双击、坐标归属与遮挡、取消、EOF/父退出、双实例重启以及真实 Chrome 原生 AX 发现；截图已视觉核验。
- 受控输入失焦时，目标收到 20 个字符后返回 `focus_changed/unknown`，后续保存未执行，未抢回焦点。另一窗口仍收到一个已排队的 `q`，两侧均未保存并正常清理。因此保护能及时停止后续输入，但不能撤回已投递的系统事件，也不承诺跨应用零误投或 OS 原子性。

`/private/tmp/dtw-installed-workbench-claude-20261029-01/evidence.json` 的首次安装模型任务未通过：Agent 将未发现的按钮猜为 `Save`，实际为 `Save changes`；原操作 `execution_0c4e52ed-3b2f-43e3-b7ed-3c499c6d3f28` 在五个成功语义动作后出现脚本错误，未执行保存。Agent 随后发现真实标签并完成业务，但该轮仍按零错误门槛判失败：6 exec / 7 MCP、8,135 JSON bytes、55.447 秒。原失败不重跑、不改判；留开的自有文档经独立读回与磁盘比对后单次关闭，另存 `reconciliation.json`。

Skill 修正要求首次改表单值前确认实际保存/关闭动作名，任务给出的字段名不能推导按钮标签；每种表单发现一次，随后批处理并刷新引用。使用同一签名运行时及修订源码 Skill（SHA-256 `6bb1272cdc709d197cf6f9ccac6949db33a9f217ee19886074972f2775e64e75`）的独立样本 `/private/tmp/dtw-skill-tuning-{claude,agy}-20261029-12/evidence.json` 均通过：Claude 为 5 exec / 6 MCP、9,299 JSON bytes、42.911 秒；agy 为 5 exec / 6 MCP、14,540 JSON bytes、228.210 秒。业务、易用性和清理均通过；这是不同模型/客户端的观测，不作客户端速度因果比较。

复核又修正了示例丢弃禁用保存按钮的问题：发现时保留其名称、enabled 与 actions；实际按压前重新确认可用和 advertised press。冻结 Skill SHA-256 为 `46982533d8c5a17b48efaf7723e4a5ec4c4935654e718ed509e46bfb279b01b3`，Skill 校验、4 段脚本语法与禁用按钮纯控制、40 个验收解析控制通过。前述源码 Skill 样本不计为这个最终版本或新包的安装验收；固定候选验证继续进行。

该 Skill 的候选 [40808079382](https://github.com/caelis-labs/desktop-world/actions/runs/40808079382)，源 `44c839eafa6c5fb3fca0b1f5263f36c9007b854f`，构建、259+4+3 源码测试、签名和公证通过，原公证 ID `1369b6e6-18ad-449b-8997-3195f4300a98`。ZIP SHA-256 `e89450175876e5451ee2a1ec3e23041b9c24f957029c90baac8c0cc3d5d083ba`，二进制 `8f84b4f75527df46fb3d7ca96b350e777e5615ba3f6abaaa8b826a2c01c09ae9`。新目录 `/private/tmp/DTW 安装验收 0.2.0 Skill 修正版/desktop-world-0.2.0-aarch64-apple-darwin` 的 146 文件清单、签名与在线公证核验通过。它与前一包的 CodeDirectory hash 同为 `7b49855ddea3928578bc90bdc67e3ffd9cd56047`，`cmd/dtw` 源树同为 `ab5557eefda4d9eb55c42e3d3097a6f1221e1482`；没有把前包六组原生记录改标成新二进制重跑结果。

最终六轮序列在首轮 `/private/tmp/dtw-installed-skill-final-claude-20261029-01/evidence.json` 停止：11 exec / 12 MCP、13,963 JSON bytes、74.775 秒，两次脚本异常，门槛未通过，余下五轮未执行。业务初始核验正确、恰好两次保存；清理按保守规则留开的 TextEdit 后经独立读回和磁盘比对单次关闭，另存 `reconciliation.json`，原失败保持不变。

- `execution_a0482349-98d9-49f8-ae3d-1a7412ee13dd` 在搜索 setter 后、Edit 动作前遇终页覆盖不完整；原脚本丢弃了 coverage.reason，旧账本没有保存它，原因不可恢复。
- `execution_bc12e111-4d1b-4ab3-bc8f-a9117d3adc24` 保存后查找 Edit，脚本将“覆盖不完整”和“零匹配”合并报错，不能确定是哪一种；稍后的新查询完整并找到目标，不足以证明原失败必由异步页面更新造成。

修正分为观察证据与脚本恢复：账本保留每次查询的覆盖诊断，只有同窗新 observation 完整且动作之前的只读刷新才单独计为恢复；脚本在已知界面转换后有界地重建查询，不复用旧页或重放写入。验收继续阻断脚本/原生错误、unknown、未恢复读取和不完整窗口库存；仅允许 DTW 账本证明已恢复的只读语义查询，并在 `metrics.read_refreshes` 单独保留原诊断。业务正确、恰好两次保存、调用/输出上限和清理门槛不变。后续修改尚待新的固定来源验收，不以此修改重判以上失败。

这轮修改的纯验证已通过：39 项 Rust 诊断/投影/MCP 控制、43 项验收解析控制、源码快速检查，以及 Skill 校验、4 段脚本语法和 9 组有界查询控制。开发二进制 SHA-256 `232402cdcde5ca39f2b7e38204308480d5c4c368fda27f2e1e076c57e2d0dafb`，冻结 Skill `7aeba65bf92c6cbb718cae2582b35ba70dd6b118a9c34ee827598739ee1382b1`。`/private/tmp/dtw-discovery-refresh-{native-all,web-discovery}-20261029-01/evidence.json` 均通过，截图已视觉核验；Native.swift 未改变（SHA-256 `cfb2bfff19cc42cab9af2de89a728b07cefc37ed2eb9d04435b2c3fd2bad9235`）。先验证此固定开发组合的模型任务，再制作新签名候选。

## 原始失败及修复

- Windows [40764663815](https://github.com/caelis-labs/desktop-world/actions/runs/40764663815)，来源 `6bcd88b`：Git archive 受到 `core.autocrlf` 影响，导出的 lockfile 与提交 blob 不同；在 Rust 编译前停止。`632065c` 显式导出 LF 源码，真实临时 Git 回归同时保护 `.gitattributes -text` 文件的原始字节。
- Windows [40766203496](https://github.com/caelis-labs/desktop-world/actions/runs/40766203496)，来源 `632065c`：已生成 ZIP，但库测试发现 generic Windows 子进程的强制终止分支漏记实际 wait 状态。原始 EOF 主错误不变，后续清理查询却可能错误返回成功。`a07d1eb` 无条件保留实际退出状态，覆盖自然退出与 kill 的交错、exit 1、强杀不确定性及重复查询；native cooperative worker 的既有语义继续保留。
- 本机 macOS `/private/tmp/dtw-candidate-macos-20261029-01`，来源 `6bcd88b`：原 codesign 等待系统私钥；用户确认后返回 `timestamps differ by 10104 seconds`。保留 `FAILURE.json`、`commands.json`、`logs/017.err` 和采样，没有改时间、关闭时间戳或将失败签名标为通过。后续使用 CI 的现有认证重新生成候选。
- macOS [40798307879](https://github.com/caelis-labs/desktop-world/actions/runs/40798307879)，来源 `fb1045e`：259 个库测试、4 个 CLI 测试、3 个 macOS stdio 测试通过；Developer ID/时间戳/Team 校验及公证 Accepted，原提交 ID `78e22d32-0e2d-4234-91f0-98d092485785`，临时钥匙串清理与原搜索列表恢复通过。ZIP SHA-256 `832bcb3eea3aa3c7c98d8b8552ed212f0b59915259fb51b67454412976520599`，二进制 `4d8c05e8bda7a03efab3ac1177e25ee9c082c7842abdd3435c154de90faa0a9a`。安装路径 `/private/tmp/DTW 安装验收 0.2.0/desktop-world-0.2.0-aarch64-apple-darwin`；原归档和安装字节未修改。多实例 `/private/tmp/dtw-installed-multi-instance-20261029-01/evidence.json` 通过；native all `/private/tmp/dtw-installed-native-all-20261029-01/evidence.json` 中文本只到 `DTW `、保存 0 次，回执却无错误。原样本的失焦来源未归因。
- 同一旧安装二进制的独立受控负例 `/private/tmp/dtw-focus-intercept-original-20261029-01/evidence.json`：输入期间由另一个自有 AppKit 测试应用接管焦点，原目标收到 18/1006 字符，另一窗口收到 988 字符并保存 1 次，原回执仍无错误。修复将同步 worker 的 AppKit 前台快照改为系统级 AX 当前应用核验，精确窗口查询前后都检查 PID。修复开发字节 `38c327624264aef8ba72ea3822148e8adebaef0ac1f0a105fde8a6ad339cc640` 的同一控制 `/private/tmp/dtw-focus-intercept-20261029-01/evidence.json` 通过：19 字符后停止，`focus_changed`、`unknown`，保存后缀未执行，另一窗口文本不变，不抢回焦点，两窗口及 native worker 均正常清理。纯焦点控制 11 项及验收回执负例通过；开发字节的通过不替代新签名候选验收。
- macOS [40801105989](https://github.com/caelis-labs/desktop-world/actions/runs/40801105989)，来源 `0289557`：259+4+3 源码测试、签名及公证通过，原公证 ID `3b51f127-57f9-441b-bfa1-a1be039b5bac`。ZIP `5a7b9fecdb5ba023629b98635d70dc79738c63495b462a055f4156698b1c8e64`，二进制 `0551724dc059b68ebcd57b0b3d4f7bfa8d9fb9db7bd04c7bededf238a07d7652`；新安装目录完整清单与公证核验通过，最小 PATH 的标准 MCP 四工具/两参数发现通过。`/private/tmp/dtw-installed-fixed-native-all-20261029-01/evidence.json` 在首次 focus 已激活窗口后读 AXWindows 返回 `CannotComplete (-25204)`；回执明确 `unknown/ax_unavailable`，文本仍为 seed、0 输入事件、0 保存，正常清理。该原始操作不重放，候选仍未通过 GUI 门槛。
- 激活就绪的本地中间字节 `d7446cb007338892fce9bb8ba7fe707a17215d78c7fd6422aeb59565a42cb6d7` 在 `/private/tmp/dtw-focus-readiness-native-all-20261029-01/evidence.json` 暴露初始阶段过严：没有当前 focused app 时尚未尝试聚焦即返回 `not_dispatched/focus_unproven`，0 输入、0 保存。修正将初始聚焦、激活后只读就绪等待、输入中途核验分开；仅前两阶段处理私有类型的焦点缺失，公共错误结构不变。新增阶段控制 13 条、就绪控制 49 条通过。
- 最终本地字节 `ecc41a1c90fa0bd3affd1a9e1a03a822622ecb75ad2def9a99d3ba26c7d9a7ef` 的 `/private/tmp/dtw-focus-phases-native-all-20261029-01/evidence.json` 全组通过，截图已人工查看；`/private/tmp/dtw-focus-phases-intercept-20261029-02/evidence.json` 受控失焦通过，正常清理。前一个 intercept `...-01` 有脚本以外的 Space keycode 49 在输入前加入空格，来源未归因，样本判无效并保留；其原操作在清理时取消，未重放。用户为桌面留出时间后才创建新的独立样本，未将无效样本算为通过。

Windows 第二次失败包 ZIP SHA-256 为 `26ea4131aee920d82608fec7faedb1f04d159726533480fb5c0c0de7595935f8`，二进制 SHA-256 为 `3f70fd10c91816480a427aedd5d9b675c698db6f59765f6bc6d812690c2f40cf`；仅留作原始证据，不计入验收。上述失败均保留原 workflow run，修正后按新源提交触发，不以同一次操作重放掩盖原因。

最终二进制、归档、源提交、工具链、签名、公证与验收日志分别绑定。安装模式直接启动包内二进制并读取包内 Skill，验收前后复核全部清单；不以旧开发字节的通过证明新候选。发布时使用验收过的相同字节。
