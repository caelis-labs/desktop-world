# DTW 实施检查点

本文件把 [架构基线](next-architecture-baseline.md) 转成可评审、可验证的实施步骤。日期：2026 年 10 月 27 日。方向已确认；C1、C2-macOS 和 C3-macOS 的限定切片已通过本地验证与独立审阅，后两项包含真实交互桌面 fixture。C4-macOS.1 已完成显式选择的单 app 协议、可信启动 grant 与实际 Caelis 预授权闭环；C4-macOS.2 的生产 Harness 本地授权接线和 C5-macOS.1 的前台 fixture 最小脚本闭环已通过；Caelis 配套按用户要求仅本地提交。下一步为 C5-macOS.2 空窗口枚举可靠性检查点，Windows C2/C3 独立待办。详细证据见 [原生验收记录](native-checkpoint-acceptance.md)、[C4 验收记录](app-protocol-acceptance.md)和 [C5 验收记录](app-script-acceptance.md)。

当前行为回归参照为 DTW `a1833a1fbc80de6e14fc86a0972782a00706de33`，首批 CUA 来源固定为 Driver 0.38.0 / `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`。检查点编号只用于记录实施与验收，不引入新运行框架或新的授权状态机。

## 执行与证据规则

每个检查点按“限定变化 → 实现 → 针对性验证 → 审阅差异和失败 → 记录结论”的循环完成。遇到失败先解释并修复当前步骤，不能用新增 fallback、放宽断言或扩大范围把失败盖过去。通过后依据下一步准入继续；未取得必需证据时，只暂停依赖该证据的能力，保留其他独立工作。

| 证据类别 | 能证明什么 | 不能替代什么 |
| --- | --- | --- |
| 源码与契约审阅 | 依赖方向、状态所有权、来源记录和边界实现 | 执行结果和实际平台行为 |
| 独立内核与测试后端 | 类型独立性、准入、引用失效、结果与取消状态转移 | OS 身份证据、AX/COM、截图、输入、阻塞和进程终止 |
| 自动化集成与 CI | 被执行环境中的编译、协议、进程与回归场景 | 未运行的平台或交互桌面；CI 成功不等于 GUI 成功 |
| 真实平台与 GUI | 指定机器、OS、权限和应用下的原生行为及独立效果读回 | 其他平台、其他 DPI/权限组合或安装包行为 |
| 分发包验收 | 安装后依赖、签名/TCC、子进程身份和运行行为 | 未覆盖的应用工作流或平台组合 |

每次记录代码版本、环境、命令或操作步骤、预期与实际结果、证据路径以及未覆盖范围。新增能力是否完成，按实际通过的平台和能力组判断，不从一个平台推断另一个平台。GUI 验收按物理桌面串行；CI、测试后端及真实 GUI 结果分开记录。

| 检查点 | 架构阶段 | 状态 | 评审对象 |
| --- | --- | --- | --- |
| C0 | B0 | 已确认方向 | 分层、内部不变量、固定来源与 app 脚本边界 |
| C1 | B1a | 本地、CI 和审阅通过 | std-only 类型与执行内核，测试后端闭环 |
| C2-macOS.1 / .2 | B1b + B2a | 限定内部切片实机通过 | 原生进程、discovery/身份、SCK capture/click |
| C2-Windows.1 | B1b + B2a | 未实施 | 进程/HWND discovery 与实例身份 |
| C3-macOS | B2b | 限定内部切片实机通过 | AX snapshot/advertised AXPress、迟到效果读回 |
| C3-Windows | B2b | 未实施 | UIA 与选定 WGC/像素路径，需 Windows GUI |
| C4-macOS.1 | B3a | 本地、实机与预授权 Harness 闭环通过 | v1 request/receipt、固定 Host grant、cancel/result、单 exec |
| C4-macOS.2 | B3b | 本地产品代码与真实 GUI 通过，Caelis 未发布 | 生产 Harness app/能力审批、上下文建立与撤销 |
| C5-macOS.1 | B4a | 最小前台 fixture 切片通过 | 五方法、有界 VM、逐次 Core、state/步骤回执、原生超时/取消 |
| C5-macOS.2 | B4b | 待定位与复验 | 偶发空窗口列表的冷启动/后台/前台支持边界与诊断 |
| C6-能力组-平台 | B5 | 未开始，逐组验收 | 选定能力批量扩展及过渡路径收敛 |

## C0：架构方向与来源基线

- **目标**：确认 DTW 持有核心类型、执行语义及授权边界，CUA 是固定来源和可替换实现；确定按能力组迁移、单 app 脚本、Harness 授权与逐次调用校验的方向。
- **通过条件**：分层、状态所有权、身份/引用/效果不变量及首批范围可解释同名应用、越权、重启、陈旧引用和取消；用户已授权按此方向实施。
- **证据**：架构文档、当前源码核查和用户方向确认。此处没有运行验证结论。
- **停止条件**：实现需要第二份授权权威、无约束的跨 app 入口或动作自动重放，须先修正设计；不能以接入方便削弱已确认边界。
- **下一步准入**：只选最小可执行 C1，暂不新增平台、公开协议或脚本依赖。

## C1：独立类型与执行内核（B1a）

**目标**：实现 DTW 自有身份、grant、上下文、引用和结果类型，以及单执行准入和必要生命周期状态。以确定性测试后端完成 observe → press → reobserve，让边界规则成为可执行证据。内核只依赖 Rust 标准库，可脱离 CUA SDK、平台实现和 MCP 单独编译。

| 必须验证的行为 | 通过条件 |
| --- | --- |
| 独立编译与依赖方向 | 独立 `rustc` 编译及测试入口通过；核心不导入 CUA、MCP 或原生平台类型 |
| 成功闭环 | observe 生成引用，press 使用该引用，独立 reobserve 验证测试后端状态变化；仅动作返回成功不足以证明效果 |
| 身份与授权 | 错 app/实例、缺少能力、过期 grant 均在后端动作派发前拒绝；请求不能自行扩大 grant |
| 引用边界 | 错 context/window/provider/generation、过期或已失效引用拒绝；不能把旧引用转给另一后端重试 |
| 实例与观察变化 | provider/window 代际变化需新观察；app 实例变化需可信方重新建立上下文，旧 grant/ref 不自动继承 |
| 效果语义 | `not_dispatched / dispatched / unknown` 与 `confirmed / unverified / mismatch` 分开；效果确认必须对应明确的读回条件 |
| 取消与停止 | 准入前取消不调用后端；准入后中断时保留上下文未知状态和后端原始结果，明确未派发仍保持未派发；不重放，取消/关闭后不接纳新调用 |
| 单执行准入 | 同一 Runtime 忙时拒绝新的执行；不暗中新增 FIFO 队列或并发输入 |

**证据要求**：独立编译命令和结果、上述行为的测试结果、影响范围内既有检查结果及差异审阅。可使用可控时间、测试后端和取消信号验证状态转换；这不证明真实 OS 时序或原生 worker 已被终止。具体结果记录如下。

| C1 证据记录 | 当前结果 |
| --- | --- |
| 实现版本与文件范围 | `0fe87f584dedb98cb24094af201d3c3b84a85950`；`cmd/dtw/src/lib.rs`、`computer/mod.rs`、`runtime/mod.rs`、`runtime/tests.rs`、独立 runner 与 source-check 接线 |
| 独立 rustc 编译与测试命令、结果 | `python3 tests/ci/run_core_checkpoint.py`；Rust 1.98.0，`-D warnings`，25/25 通过，不加载 CUA/Cargo/平台库 |
| 身份、授权、引用、效果与取消行为验证 | 25 项受控后端测试覆盖上表，独立读回实际测试状态；屏障与可控时钟安排交错，不使用 sleep。额外验证 binding 清理期间 busy/取消、直接销毁的析构顺序、后端 panic 关闭和原始诊断保留 |
| 既有回归检查及差异审阅 | Cargo 测试：新内核 25 + 现有执行器 26 通过，1 项需原生环境的既有测试 ignored；source-check：25 内核 + 8 Python + 70 固定 CUA prelude 检查及 TypeScript、rustfmt 通过；版本检查通过；独立审阅问题已修复并补回归 |
| 已知限制与未验证项 | 本地 macOS arm64；新库未接生产 MCP。未验证 OS 身份、原生进程终止、GUI 或分发包，也未完成 CUA 源码迁移；远端 CI 状态见对应 PR，不能代替这些证据 |
| 检查点结论 | C1/B1a 本地门槛通过；B1b 仍待 C2 的真实平台验证 |

本地复现命令（仓库根目录；`CUA_SOURCE_ROOT` 指向核验过的固定源码 checkout）：

```sh
python3 tests/ci/run_core_checkpoint.py
CARGO_TARGET_DIR=/private/tmp/dtw49-target cargo +1.98.0 test --manifest-path cmd/dtw/Cargo.toml --locked --offline
NPM_CONFIG_USERCONFIG=/dev/null NPM_CONFIG_REGISTRY=https://registry.npmjs.org CUA_SOURCE_ROOT=/private/tmp/dtw-cua-0380-source sh scripts/check-source.sh
python3 scripts/release-artifacts.py check
```

首次 source-check 在本机 npm 镜像取得 TypeScript 5.9.3 时返回 404；通过命令级公共 registry 覆盖后完整检查通过，未修改用户 npm 配置。C1 最初未运行既有的 `recovery_tests::idle_and_cancel_recovery_preserve_runtime_and_native_expiry`；C2/C3 本轮已配置真实 binary/固定 worker 并单独运行通过。

C1 后续修复了新增 library target 后 macOS CLI 丢失 Swift 链接元数据的问题（`c72672e455cdfdc3c9d2d9e1b6b6c9dd21c32ba6`）。[该提交的 CI](https://github.com/caelis-labs/desktop-world/actions/runs/40448827415) 四项成功：版本、源码契约、macOS 包/MCP、Windows 包/MCP。这不补足任何原生 GUI 证据。

C1 原始切片只有语义观察和 press；C2 已加入像素观察的 kind/几何隔离。取消对本次执行上下文是终态，忙标记覆盖后端调用和绑定清理；不能据此推断 OS API 已终止。同步 Backend 是内部检查点接口，C2 接入生产前必须加入有界 IPC、原生派发准入和进程生命周期验证；没有默认将阻塞原生调用放到 MCP 线程。Rust 类型与不透明进程内引用尚不是稳定 wire protocol。

**停止条件**：独立编译依赖 CUA 类型，越权请求到达后端，旧引用被接受，取消后的未知结果被改成成功或触发重发，均阻止 C1 通过。测试后端模拟不了的真实平台事实留到 C2，不扩充虚构的“已验证”覆盖。

**下一步准入**：C1 的必要测试和审阅通过、限制已记录后，进入 C2 的一个平台切片。C1 通过仅表示 B1a 完成；B1 还需要目标平台的原生进程边界证据。C1 不接管现有生产入口，也不声称已迁入 CUA 原子源码。

## C2：原生边界与首批来源迁移（B1b + B2a）

**共同目标**：把内核接到真实平台，落实原生 worker 的创建、准入、取消、超时、关闭和资源回收，并完成窄范围 discovery/身份代码迁移。每组记录来源 commit、原始路径/hash、目标路径、依赖闭包及 notices；类型包装不算源码迁移。

| 平台切片 | 通过条件与必需证据 |
| --- | --- |
| C2-macOS | 运行应用/窗口 discovery 与实例证据不依赖 CUA worker；声明采用的公共 API、隔离的私有窗口映射及覆盖限制。现有 SCK capture/click 成组进入统一授权和引用边界；真实 GUI 覆盖多窗口、隐藏/关闭、移动、遮挡、旧截图拒绝及输入后的独立读回，并复用相关 #49 场景 |
| C2-Windows | 进程/HWND 基础 discovery 和可核验实例身份完成窄迁移；在真实 Windows 上核验多实例、窗口归属、关闭与部分覆盖。先完成基础只读组；本切片通过不授予 UIA/WGC 动作组完成状态 |
| 每个平台的进程边界 | 自动化进程测试及该平台实机证据证明停止接收、终止、等待和回收；覆盖启动失败、阻塞、取消、超时与父进程退出。派发后未知保留原状态，不自动恢复后重放；终止 worker 不被解释为撤销已发送的 OS 输入 |

C2 进一步按独立可评审的小切片推进，不把表中全部能力放进一个变更：

1. **C2-macOS.1 原生只读通路**：选定 supervised worker/IPC 边界，窄迁入应用/窗口 discovery 与身份核验，验证启动、关闭、超时和 read-only 实机结果。新库通过内部测试入口接入，暂不发布新协议或动作能力。
2. **C2-macOS.2 SCK 成组接入**：在 .1 的原生生命周期证据上接入已有 capture/click，逐次 grant、坐标绑定和像素动作的真实 GUI 验收通过后，才标记 C2-macOS 完成。
3. **C2-Windows.1 基础只读通路**：以同样的契约迁入进程/HWND 子集，独立取得 Windows 进程和 GUI 证据；不以 macOS 通过作为前置，也不混入 UIA/WGC 动作扩展。

每个切片都有自己的代码版本、证据与未覆盖范围；失败停在当前切片修复，不为提前扩大可用项跳过门槛。

**停止条件**：需要根据标题/尺寸猜测授权归属，不能回收阻塞 worker，关闭后仍接收执行，或将未知效果当作未发生，均阻止对应平台通过。没有实机证据的项目保留未验证或受限支持，不能用其他平台的成功替代。

**下一步准入**：共同内核规则不变，目标平台 C2 的证据完整后，该平台可进入 C3。另一平台可以继续独立补齐 C2，但不能借用已通过平台的状态；共享规则发生变化须重验相关 C1/C2 场景。

## C3：最小真实语义闭环（B2b）

**目标**：迁入一个语义观察/动作状态组，使 app scope → exact window → observe → 一次 press/invoke → 独立读回在真实平台成立。与同平台像素能力共存，但各自保留 provider、代际、几何和有效期。

| 平台切片 | 通过条件与必需证据 |
| --- | --- |
| C3-macOS | AX snapshot 与 advertised AXPress 成组；element 到 exact window 的派发前核验、原生对象保活、mutation admission、调用期限和失效测试通过。窗口映射优先公共证据；保留私有调用时隔离并声明支持范围。真实 GUI 覆盖多窗口/浮层、AX 延迟、旧引用、取消不重放及至少一个应用闭环 |
| C3-Windows | UIA snapshot/invoke 与选定的 WGC/像素路径分别验证；COM apartment、对象保活、provider 阻塞、single-flight 和进程边界成立。真实 GUI 覆盖所声明的 DPI 范围、modal picker、阻塞取消/重启和迟到效果；未覆盖范围受限，不能推断为已支持 |

**证据要求**：来源与依赖闭包审阅、内核和原生集成测试、每个平台的真实动作及独立读回记录。固定 CUA backend 可以作为架构联调证据，但只能标为“接入”，不能标为“源码迁移完成”。

**停止条件**：snapshot 与 action 的状态所有者不一致、精确窗口关联无法证明、采用未经授权的自动 focus/selection/pixel fallback、或原生调用失败掩盖部分效果，均阻止该能力通过。

**下一步准入**：至少一个目标平台的最小真实闭环完成并明确能力覆盖后，才以其实际语义制定 C4。未完成迁移的组保持独立状态，不能进入对应 C6 批量扩展。

## C2/C3 本轮结论与下一次循环

代码版本 `a606d33`，完整 SHA、环境、命令、结果与原始证据 hash 见 [验收记录](native-checkpoint-acceptance.md)。

- C2-macOS.1：真实应用/窗口身份、同名多窗口、隐藏/恢复，以及 close/cancel/EOF/父进程退出后的 worker 回收通过。
- C2-macOS.2：SCK 截图后一次点击由 fixture 独立计数确认；重放、移动、遮挡、隐藏及关闭均拒绝派发。
- C3-macOS：AX 观察 → 一次 AXPress → 新观察读回；错误窗口、旧引用和 attached sheet 拒绝；普通非模态浮窗共存、AX 延迟和内容不可用时像素读取通过。
- 派发中取消：约 159 ms 返回 Unknown + Cancelled，worker 已回收；目标应用约 2 秒后完成一次动作，独立新上下文读回。取消没有撤销已经进入目标应用的工作，也没有重放。
- Windows C2/C3、新路径分发包、公开协议和 app 脚本均未完成；生产 MCP 仍使用原有执行器。

后续已选择并完成 **C4-macOS.1 协议契约切片**：冻结 request/receipt、能力与目标声明、可信 grant 入口和 cancel/result 语义；让 Harness 适配器能展示并批准单 app 范围，再沿已验收的 macOS 路径运行一次端到端闭环。确定性协议测试、实际 Caelis 预授权流程及 GUI 读回三类证据已记录在 [C4 验收记录](app-protocol-acceptance.md)。生产动态审批现已在 C4-macOS.2 本地产品代码中通过；后续 C5-macOS.1 验收记录见下文。Windows C2.1 可以独立推进，必须另取 Windows 原生进程及桌面证据。

## C4：稳定外部协议（B3）

**目标**：依据真实闭环形成版本、能力声明、request、receipt、errors、cancel 和 result 契约；Harness 在派发前能够识别目标 app、能力及附加资源范围。

- **通过条件**：公共类型由 DTW 拥有；原生 schema/token 不成为公开契约；错误和回执保留未派发、已派发及未知的区别。可信 grant 与请求分开，固定 grant 的变更通过新上下文生效；低权限 catalog 不隐含观察或输入授权。
- **证据**：协议契约和自动化集成测试、真实 Harness 适配流程，以及已通过平台的端到端验证。逐平台报告支持能力，不能统一标记未验证平台可用。
- **停止条件**：协议接受模型自证授权、compact 结果改变效果语义、或丢失原生未知状态，先修正契约再扩展入口。
- **下一步准入**：Harness 的审批与执行范围一致、回执可追溯、兼容/版本边界明确后进入 C5；不为脚本临时创建第二套协议或授权权威。

### C4 分阶段准入

1. **C4-macOS.1（已通过）**：显式选择 `app-protocol` feature；单 `exec` 的版本化 typed requests、opaque refs、单活动请求、原 ID 查询/去重及原始 receipt；可信启动 grant 与请求隔离，实际 Caelis 预授权 + 原生 GUI 闭环。见 [协议 v1](app-protocol-v1.md)。既有发布包未切换。
2. **C4-macOS.2（本地通过，配套未发布）**：生产 Harness 在执行前展示完整 app 实例和能力，由自己的审批政策建立/撤销上下文；覆盖拒绝、到期、app 重启、并发审批、取消和上下文更换，审计记录与实际 grant 一致。模型字段始终不能构造审批事实。需要真实集成入口及 GUI 证据，不以参考 Host 文件替代。
3. 只有上述生产授权闭环和本轮协议都稳定后，才选择最小 C5 app 脚本切片；Windows 与新入口分发验收独立记录。

## C5：App 级脚本（B4）

**目标**：复用 QuickJS/rquickjs 引擎和必要封装，提供只绑定一个 app 的脚本对象；所有 Host Call 都进入同一 DTW Core。

- **通过条件**：脚本不能通过任意 PID、全局 launch、原始 call、文件/网络/进程访问或旧 unrestricted 路径逃逸；逐次执行 grant、实例、window 和 binding 校验。时间、内存、栈、调用次数、取消和关闭有界。脚本错误不抹去先前真实效果，JSON state 提交与桌面副作用分开。
- **证据**：脚本与 Host Call 的边界测试、限额和取消集成测试、在已通过平台上的单 app 语义/像素组合 GUI 流程。真实桌面与测试后端分别记录，跨 app 流程由 Harness 编排。
- **停止条件**：静态解析被当作授权，Host Call 绕过 Core，预算不能终止等待，或脚本失败导致动作自动重放，均阻止通过。
- **下一步准入**：脚本执行与原子入口具有同等边界，并完成所声明平台的真实验证后，再按需求进入 C6。

### C5 本轮结论与下一次循环

C5-macOS.1 的五个既有原子方法已接入同一 Core，137 项库测试及 26 项既有执行器测试通过；实际生产 Harness 完成同脚本语义/像素组合、图片输入、动作后异常、state 保留、原 ID 去重和撤销，另有真实超时/取消与迟到效果读回。来源、限额、失败和未覆盖项见 [协议](app-script-v1.md)与[验收记录](app-script-acceptance.md)。Caelis 的 `96b1ffe9`、`e63a0fc7` 均只保留本地。

C5-macOS.2 的明确目标是解释偶发 `inventory=[]`，建立冷启动/后台/前台的支持边界和有界诊断，再按每种受支持状态至少三个新上下文复验。当前前台 fixture 通过不替代这项可靠性证据；零窗口必须不派发、不猜测、不自动 focus。完成前不切换默认入口，不进入 C6 批量扩展；这是一条限定的验证循环，不追加全部原子能力。

## C6：按能力组扩展（B5）

**目标**：逐组增加选定的键盘、文本、滚动、拖拽、菜单、focus、Browser 和 Windows 深层能力。每次只扩展一个有完整状态所有权的能力组，记录为 `C6-能力组-平台`。

- **通过条件**：来源、依赖闭包和版权清晰；目标、焦点、修饰键释放、资源授权及结果语义符合内核契约；迁移完成后收敛旧路径，不留下两份可自动互相 fallback 的状态域。Browser 额外验证 profile/target/frame/origin/文件约束。
- **证据**：该能力组的契约与回归测试、目标平台原生集成及真实 GUI；作为分发能力发布前，补齐对应安装包、权限、依赖和子进程身份验收。CI 与其他平台结果不可代替。
- **停止条件**：依赖闭包尚不清楚、需要未经确认的共享资源、过渡实现不能解释状态归属，或缺少所声明平台的真实效果证据，均停止该组扩展。
- **下一步准入**：当前组的必要证据完整、差异已审阅、限制已公开，再选择下一组。范围由实际需求决定，不以迁入 CUA 全目录为完成目标。
