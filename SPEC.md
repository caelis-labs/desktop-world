# Desktop World v0.1 — Technical Specification

**状态：Proposed / 实现前设计草案**  
**研究日期：2026-09-29**  
**交付定位：独立 Go library；macOS / Windows 同等正式目标**  
**规范级别：文中的 MUST / MUST NOT 为 v0.1 正确性要求，SHOULD 为推荐实现，MAY 为可选能力。**

本文给出推荐路线，而不是将多种方案留给实现者重新选择。原生系统事实通过文末 [Sxx] 引用；其余标注为规范的内容是本项目的设计选择。默认预算、版本范围和测试门槛是拟定值，不是已有实测结果。

本交付包含 SPEC.md、可编译但没有引擎实现的 api.go、示例协议 JSON 和 README.md。没有执行 macOS / Windows 原生 POC、真实桌面测试或性能评测。Go 类型草案的编译检查不能替代这些验证。

## 0. 决策摘要

**Desktop World 是对真实桌面的持续、稀疏、带不确定性的可操作对象模型；不是完整桌面数字孪生，也不是自动化 Agent。**

建议架构：

```text
                    Agent / Bot / Wails / 其他宿主
                       |                |
                 Agent protocol     Go API / Anchor / Progress
                       +--------+-------+
                                |
               World: 观察投影 + 对象注册表 + 空间关系
                                |
                  带权限与前后条件的线性动作执行器
                                |
             Inventory / Semantics / Events / Input / Capture
                       |                         |
                 macOS backend             Windows backend
```

核心选择：

| 决策 | 推荐 | 不选择的方向与原因 |
|---|---|---|
| 世界建模 | 按需物化的对象图与空间模型 | 全桌面完整树持续镜像成本高，并制造完整性错觉 |
| 数据来源 | 原生窗口清单 + 原生语义接口 + 原生输入；视觉独立补充 | 单独依靠任何一种来源都不足以覆盖目标 |
| 对象定位 | 具体实例 Ref 与重新查询 Locator 分离 | 标题、路径、坐标不能充当永久身份 |
| 状态同步 | 本地 material revision + scoped snapshot/delta + 重建机制 | 不声称事件完整、OS 原子快照或全局事务 |
| 交互 | 对象语义动作与物理输入显式区分 | 不把 invoke、click、set_value、typing 混为一谈 |
| 连续操作 | 有界线性计划 + 局部校验 + 明确回执 | 不引入 DSL、分支、循环和自动修复 Agent |
| 具身 | 共享 Ref、Anchor、Geometry、Progress | 不将 3D 引擎、物理系统、角色状态机放入核心 |
| 部署 | 默认进程内 Go library，原生桥接边界独立 | 不先做常驻 daemon；为未来 helper 隔离保留边界 |

这种设计与传统 Computer Use 的差异，不是完全不用截图或鼠标，而是让截图、输入与语义接口都服务于同一个持续对象模型。

## 1. Goals / Non-goals

### 1.1 Goals

G1. 应用、窗口和基本 UI 对象可以被发现、局部观察、引用和操作。

G2. 对象具备状态、能力、生命周期与空间属性；未知、失效、截断不能伪装成确定事实。

G3. Agent 不必理解原始 AX / UIA 树，也不必每个低层步骤都进行一次模型推理。

G4. 稳定 Ref、局部增量与预算化读取能够避免反复发送完整上下文。

G5. 具身表现可以通过同一对象 Ref 得到几何锚点，真实交互通过同一 Ref 执行。

G6. 同一份核心 Go 代码、协议与正确性测试作用于 macOS 和 Windows；功能差异通过能力显式表达。

G7. 不依赖 Agent Runtime、LLM、Wails、渲染器、浏览器插件或网络服务。

### 1.2 Non-goals

v0.1 不提供任务规划、自主循环、LLM 总结、OCR、视觉 grounding、完整浏览器自动化、任意工作流脚本、跨机器远程桌面、恢复初始环境、桌面业务事务、永久对象 ID、物理碰撞、路径规划或角色动画。

不承诺支持所有应用、所有自绘控件、所有隐藏或虚拟化项目。不绕过锁屏、UAC、安全桌面、应用保护或系统权限。

## 2. 研究依据与路线比较

### 2.1 不需要发明另一套底层输入系统

Apple Accessibility 已具备对象属性、可设置属性、动作、层级和通知；Windows UIA 也提供对象属性、控制模式、事件与缓存。因此，“对象而非坐标”本身不是新发明。本项目新增的价值是跨来源的持续世界契约、上下文投影、执行回执，以及具身空间共享，而不是重新命名 AX / UIA。[S01][S02]

### 2.2 采用哪些成熟概念

| 来源 | 借鉴内容 | 不照搬内容 |
|---|---|---|
| Accessibility / GUI testing | 对象、属性、能力、原生动作、事件 | 完整树等同于用户可见世界 |
| Playwright | 定位器、执行前检查、局部等待、明确超时 | 浏览器中可执行的稳定性/遮挡检查在桌面同样可靠 |
| List / watch 系统 | snapshot + cursor + delta；历史过期需要重建 | 强一致资源存储、OS 事务版本 |
| Robotics / transform systems | 明确 frame、时刻、空间变换与传感器证据 | 导入机器人中间件、SLAM、全局统一物理世界 |
| Agent environment | 观察与动作的明确边界 | reset、reward、episode、确定性 step |
| 游戏世界 | 可识别对象、可用交互、角色指向同一对象 | ECS、游戏主循环、控制整个世界状态 |

Playwright 的 Locator 可在操作时重新解析并命中新 DOM 实例；这适合解释 Locator，却不能作为 Desktop World 悄悄替换旧 Ref 的理由。[S03][S04]

有限事件历史失效后重新获取 snapshot 是成熟模式；Desktop World 仅借鉴这一协议性质，不引入 Kubernetes 等运行依赖。[S05]

### 2.3 实现路线比较

A. **截图优先**：普适且贴近可见界面，但身份、文本、动作能力与变化原因需要额外推断。保留为视觉补充，不作为世界唯一真相。

B. **Accessibility-only**：语义与动作结构较强，但提供者质量、虚拟化和未发出的事件限制覆盖范围。作为重要来源，不作为完整桌面现实。[S06][S07]

C. **现成跨平台自动化库作为全部核心**：能缩短输入和截图 POC，但通常无法直接提供本文的身份、覆盖度、局部同步和不确定性契约。可以在后端内部复用 RobotGo 等项目的经验证功能；不能把其 API 原样作为 World API。RobotGo 当前还包含实验性的非 CGO 路径，不能简单概括成“只能 CGO”。[S08]

D. **浏览器 / 应用专属协议**：可提供高质量语义，但不是一般桌面基础。后续可作为可选 provider，不应成为 MVP 必需品。

E. **推荐组合**：窗口 inventory + AX/UIA 语义 + OS 输入 + 稀疏世界模型 + 可选视觉。采用窄原生桥接，Go 负责上层契约。没有必要为“纯 Go”牺牲线程、安全和错误信息。

## 3. 核心领域模型

### 3.1 World

一个 World 连接一个登录用户的交互式桌面会话。它维护的是“目前知道的桌面状态”，不是系统内所有 UI 的完整状态。

World 具有：

- Epoch：每次建立 World 的随机、不复用会话标识。
- Revision：已提交的可见模型变化序号。
- Object registry：稀疏对象与后端身份映射。
- Spatial topology：显示器、frame、变换、topology version。
- Seat state：共享的系统指针、前台窗口、键盘焦点与输入健康状态。
- Coverage / health：哪些范围已观察、哪些数据脏、哪些来源不可用。

World 不拥有应用状态，不阻止用户修改桌面，也不能把桌面重置到某个 revision。

### 3.2 Object

统一对象类别仅保留 `application`、`window`、`ui`。Window 是一等对象，但不需要独立维护第二套对象身份。

对象的核心字段：

```text
ref, kind, role
app?, window?, parent?, relations[]
name, value_preview, states, bounds
capabilities[]
lifecycle, object_version, geometry_version
sample_interval, provenance
```

role 使用小型标准词表：button、text_field、text、menu、menu_item、checkbox、list、list_item、tab、container、unknown 等。未知原生 role 不应被误猜为已知角色；可附诊断性 native role。

原生属性采用显式 `known / unknown / unsupported / redacted` 状态。`enabled=false`、`enabled=unknown` 和未请求 enabled 是不同情况。

### 3.3 图关系，而不是强制单棵严格树

主要关系为 `owned_by`、`contains`、`transient_for`、`labelled_by`。`app/window/parent` 是高频访问的便捷字段。

应用菜单、临时 popup 或全局 UI 不一定属于某个窗口。不得为了统一层级把它们伪装成错误窗口的子对象。Apple 的 Accessibility 模型本来就包含应用直属菜单栏等结构。[S01]

规范要求：投影层可以折叠没有信息价值的容器，但不得由此改变原生操作目标。模型需要保留投影对象与实际 native handle 的映射。

### 3.4 Pointer、Keyboard 与 Actor

Pointer / Keyboard 属于同一个真实输入 Seat，不是每个 Actor 独享的设备。系统前台窗口、键盘焦点、鼠标位置属于 World 观测状态。

Actor 是受宿主约束的参与者句柄：绑定授权范围、观察兴趣和执行身份。Actor 不必有视觉身体。多个 Actor 可以观察不同范围，但所有真实输入经过同一 Seat 调度器。

Actor 的“看向”“想去哪里”“当前动画”属于表现层；系统的“focus”“pointer position”属于真实桌面状态。两者 MUST NOT 自动互相覆盖。

## 4. 对象状态、能力与行为

### 4.1 状态模型

建议 v0.1 核心布尔状态：enabled、focused、selected、checked、expanded、offscreen、read_only、protected。不是每个平台或对象都支持全部状态。

Name、value、text、bounds 不属于同一种信息。读标签不意味着支持读文档，读 value 不意味着支持写 value；屏幕有矩形不意味着该对象可点击。

### 4.2 能力模型

每个 capability 有两层：

```json
{
  "name": "set_value",
  "support": "supported",
  "availability": "blocked",
  "reason": "read_only"
}
```

Support：supported / unsupported / unknown。Availability：available / blocked / unknown。

角色与能力分离：button 可能没有可靠 invoke，container 可能可接收焦点。不得用 role 直接推导保证存在的行为。

权限是额外条件；“原生支持某动作”不代表当前 Actor 获准执行。提供给 Agent 的动作列表 SHOULD 显示支持性与当前限制，而不是静默省略所有不可用能力导致误解。

### 4.3 语义操作与物理操作

v0.1 对象语义操作：`focus`、`invoke`、`set_value`。

v0.1 物理输入：`pointer.move`、`pointer.click`、`pointer.drag`、`pointer.scroll`、`keyboard.type_text`、`keyboard.press`。

`invoke` 请求对象原生动作；`pointer.click` 请求鼠标事件。`set_value` 请求修改值；`type_text` 在已验证焦点处注入文本。它们不具有相同副作用。

MUST NOT 因原生 invoke 不支持就偷偷点击；MUST NOT 因 set_value 不支持就偷偷清空剪贴板粘贴。上层可以明确选择另一条计划，但回执必须忠实反映实际执行机制。

## 5. Identity 与 Lifecycle

### 5.1 Ref 的保证

Ref 是 opaque token，至少包含或关联 Epoch、单调分配的注册表编号与后端 generation。外部调用方不得解析其内部结构。

一个 Ref 在其有效期内绑定具体观察到的提供者实例。**库不得把旧 Ref 静默重新分配给另一个实例。** Ref 不保证跨 World、跨进程重启、跨应用重启或跨 provider 重建有效。

Windows UIA RuntimeId 仅在某一时刻的桌面范围内唯一，未来可能被复用；官方要求按不透明标识处理。[S09]

推荐后端匹配范围：

- Windows：后端 generation + 进程实例 + 原生 RuntimeId 比较 / Element 比较；HWND、PID 只作辅助。遇到明确销毁或 provider reset 即切断连续性。
- macOS：后端 generation + 进程实例 + 存活 AXUIElement 的 equality；保留 native 引用生命周期。AXSwift 的实现也使用 CFEqual，而不是靠标题生成身份。[S10]
- 名称、树路径、AutomationId、控件索引、坐标均只是定位线索，不得独立构成 durable identity。

如果提供者在没有可观测销毁边界时复用了同一原生身份，本库不能可靠证明实例连续性。Ref 的保证是本库不主动偷换绑定，不是提供者拥有无歧义的全局身份；一旦检测到身份矛盾应保守失效。

提供者自身可能复用同一 UI 实例代表不同列表项。此时 Ref 仍只能标识 provider 实例，不能自动获得“同一业务记录”的保证。涉及敏感动作时应增加名称、角色、所属窗口与关键值前置条件。

### 5.2 Lifecycle

| 状态 | 含义 | 可否直接写 |
|---|---|---|
| live | 最近有效读取成功 | 仍需执行前验证 |
| stale | 存在脏通知或信息超龄 | 必须先刷新 |
| unavailable | 暂时不能访问，身份死亡未确认 | 不允许 |
| gone | 已确认生命周期终止 | 不允许；必须新发现 |
| expired | 本库已回收引用资源 | 不允许；不表示 UI 对象已销毁 |

只在完整且足够相关的验证、明确销毁通知或进程生命周期证据下认定 gone。一次有预算的扫描没有看到对象，不构成 gone。

UIA 虚拟化元素可能不可见于当前树，placeholder 的属性读取也可能返回 `UIA_E_ELEMENTNOTAVAILABLE`；这不应统一映射成永久删除。[S07]

### 5.3 Locator 与重新发现

Locator 是受限查询：Within、kind、role、name_equals / name_contains、required_states、capability、max_depth。

Ref 说的是“这个实例”；Locator 说的是“在这个范围内寻找符合条件的对象”。重新查询可以产生新 Ref，但必须让上层知道发生了新绑定。

执行器的 bind 步骤必须唯一命中。只有一个已返回候选，但搜索因为预算或 provider 失败而未完成，不等于已经证明唯一；返回 `search_incomplete`，不得选择第一个候选继续执行。

### 5.4 回收与资源边界

初始建议：未被关注的 UI 对象空闲 5 分钟后可回收；执行计划绑定和活跃观察对象被 pin；每 World 的 UI 注册表软上限 10,000。达到资源限制时拒绝或报告 partial，不通过隐式复用 Ref 腾位置。

引用资源回收、已知对象删除、退出观察范围必须以不同原因表达。进程或后端重建后，可批量 invalidation；不要让上层误以为桌面全部对象逐个被用户删除。

## 6. 空间模型与具身边界

### 6.1 Frame 必须显式

每个 Point / Bounds 都携带 FrameID、TopologyVersion、单位和采样时刻；所有公开矩形统一左上原点、x 向右、y 向下，允许负坐标。

不能承诺“所有屏幕共享一个全局 DIP 平面”。v0.1 桌面原生 frame 在 macOS 使用转换后的 Quartz screen points，在 Windows 使用 physical screen pixels。Windows UIA 的边界矩形、clickable point 与 ElementFromPoint 使用物理坐标。[S11]

应用的本地 logical points、Wails WebView 的 CSS pixels、截图的 image pixels 必须显式转换。多屏不同缩放采用逐显示器变换；跨屏矩形按片段处理，不能套一个统一 scale。

### 6.2 Anchor

```text
Anchor { target: Ref, u: 0..1, v: 0..1 }
ResolveAnchor(anchor) -> Point + geometry_version + validity
```

Anchor 表示相对对象矩形的位置，例如顶端中央 `(0.5,0)`。它用于跟随、视线和指向，也可以成为需要额外验证的物理动作目标。

Anchor 不是“可点击点”证明；控件中央可能被遮挡、不可交互或包含其他对象。真实点击需要单独的 hit-testing / 前台 / 可用性检查。

显示器排列、分辨率、缩放变化时递增 topology version。旧图片坐标和旧 absolute point 默认失效；基于 Ref 的 anchor 可以重新解析。

### 6.3 Actor 的集成契约

上层渲染器使用同一 Ref 获取 Anchor 与几何变化。动画系统持有自己的位置、方向和动作状态，不写入 World 的 OS 事实。

```text
Agent 选择目标 Ref
      ├─ 表现层：解析 Anchor → 走近 / 看向 / 指向
      └─ 执行器：验证相同 Ref → 原生动作 / 输入 → 回执
```

靠近对象不应成为核心库执行动作的强制条件。是否先走过去、是否等待动画完成由宿主决定。动画期间目标移动，表现层重新跟随；执行前再刷新目标。目标失效则中止交互，而不是点击角色最后指向的旧坐标。

高频动画位置不会进入 Agent 的普通 world delta。渲染器可使用专门的 geometry Watch / ResolveAnchor；CPU 和输入座席不被 60Hz 角色动画牵制。

## 7. Observation 与上下文预算

### 7.1 三层观察

| 投影 | 返回内容 | 默认不返回 |
|---|---|---|
| summary | 显示器、应用、窗口、焦点、健康概览 | 全部控件、长文本、图像 |
| outline | 一个范围内有意义的 UI 对象与能力摘要 | 实现型容器噪声、全文 |
| detail | 指定对象的请求字段、限制与来源 | 无关子树 |

ReadText 是独立、受限的正文读取操作。Capture 是独立视觉操作。不能为了请求窗口列表而自动附上整屏图片。

### 7.2 观察无副作用

观察 MUST NOT 自动 focus、scroll、展开菜单或 realize 虚拟化项目。必要时可返回“可通过显式动作继续发现”，但不能为了补全树改变用户桌面。

### 7.3 初始预算

以下为待测默认值，宿主可收紧，放宽需受全局资源上限约束：

| 项目 | 初始值 |
|---|---:|
| summary 最大对象数 | 32 |
| outline 最大对象数 | 64 |
| 默认 outline 深度 | 3 |
| 单次查询最大访问节点 | 512 |
| 单次 observation 输出 | 16 KiB UTF-8，包含 envelope |
| 每个摘要文本 | 192 个 Unicode scalar values |
| 单次 ReadText 输出 | 4,096 个 Unicode scalar values |
| 一次读取逻辑预算 | 500 ms |
| 单计划最大步骤 / 总期限 | 16 / 10 s |
| 单步默认期限 | 2 s，受计划剩余期限约束 |

读预算不是杀死原生阻塞调用的能力。引擎可以停止等待并返回 partial / provider_unavailable，但不能声称被超时的原生调用已停止。详见执行与后端隔离。

### 7.4 Coverage 是协议的一部分

每次返回必须说明：声明范围、已请求字段、最大深度、访问节点数、采样区间、是否完整、是否截断、不可用来源、dirty 状态与 continuation。

`complete=true` 只对本次声明的 scope/filter/depth/sample 成立，不代表所有桌面 UI 或所有虚拟化项目已发现。

当结果超预算，应停止或返回 continuation，不能先构建巨大响应再静默裁掉关键错误。连最小 envelope 都容纳不了，返回 `budget_too_small`。

### 7.5 Token 策略

使用短 opaque Ref、字段投影、限制文本、局部范围、稳定排序与显式 delta。摘要通过确定性逻辑生成，不依赖 LLM。Token 数依赖 tokenizer，核心库承诺 byte / rune / item budget，而不是虚假的精确 token 上限。

不要默认输出 native handle、巨型属性 map、全量 capability schema 或每个字段重复长说明。协议握手告知一次词表；对象只返回本次相关字段。

图片和长附件走 AssetID，不塞 base64；用户文本标记 provenance=ui_content，保持原文语言，不混进系统指令。

### 7.6 Freshness

`cached`：返回现有已知状态和年龄。

`max_age`：对超龄或 dirty 字段尝试刷新。

`refresh`：发起新的指定范围读取，不承诺“同一瞬间”的原子快照。

执行前必须 fresh-read 必要事实，不允许因为观察 cursor 看起来较新就跳过对象生存、权限、焦点和能力检查。

### 7.7 文本分页

ReadText 的 offset / limit 使用返回规范化文本的 Unicode scalar-value 索引，而不是原生 UTF-16 字节偏移。Continuation 绑定 Ref、text version 与读取来源；文本变化后旧 continuation 返回 `text_changed`。

后端优先使用原生有界读取或范围接口。原生 API 必须整体返回 value 的情况，应报告该限制：输出限制不代表跨进程传输成本也受同样限制。v0.1 不提供任意富文本选区与格式编辑，不自动 OCR，也不把整棵后代标签拼接成假定顺序的文档。

## 8. World state synchronization

### 8.1 Revision 表达“知识变化”

World Revision 是本库提交顺序，不是 OS 版本、墙钟时间或真实桌面的事务 ID。它只对当前 Epoch 有意义。

能够推进 revision 的变化包括：已知对象 material 字段变化、对象生命周期变化、topology / seat 状态变化，以及需要让订阅者知道的范围失效。

仅仅刷新了 sampled_at，而属性值未变化，不应制造新的 material revision。比较与重建测试应剔除非 material 的读取时间、调用耗时等元数据。

`ObjectVersion` 反映该对象 material 状态变化；`GeometryVersion` 专门反映其几何变化。全局 revision 变化不能自动使所有对象操作失效：另一个窗口闪烁、鼠标移动或无关文本更新不应打断当前计划。

### 8.2 事件只是失效提示

推荐流水线：

```text
native event → dirty scope → bounded refresh → normalize
                                          → diff → commit(rev)
                                          → scoped projections / delta
```

UIA 官方文档明确并非每个 property 的变化都会有事件，移除事件处理器后仍可能有已排队的晚到事件。因此不能把收到的事件当作完整、精确的事实日志。[S06]

v0.1 SHOULD 混合事件与低频校对：应用/窗口 inventory 初始可每 2 秒校对；活跃观察目标初始可每 500 ms 校对；执行时按必要前后条件更积极刷新。以上是调优起点，不是实测 SLA。

### 8.3 首次扫描与并发变化

先注册事件和建立 dirty-generation，再扫描。扫描期间发生事件，则完成本轮后重新读取相关范围。只能清除不晚于本轮开始时的 dirty generation，不能覆盖更新事件。

每次异步读取携带 backend epoch 与请求 generation。过期响应不能覆盖更新的已提交样本。引擎 reducer 单点串行提交，而不是让平台回调直接修改公开 World map。

事件队列溢出时，将受影响范围标记 dirty / invalidated 并重扫；不能直接丢事件后继续声称状态健康。

### 8.4 Snapshot 与 cursor

Observation 必须返回与其**本地 material 投影**一致的 cursor。这里保证的是本库 store 的 snapshot/cursor 一致性，不是其原生字段在同一时刻取样。

Cursor 绑定 Epoch、Actor/授权 generation、scope、projection、字段集合、过滤参数与 revision。它不是权限令牌，不能转交另一个 Actor 取得数据。

不同 scope 或字段集合必须建立新视图。客户端不得把一个窗口的 delta cursor 用来请求另一个窗口。

### 8.5 Delta

v0.1 使用可选字段投影的 `upserts`、`removed` 和 `invalidated_scopes`，不引入复杂 JSON Patch。

Upsert 是该视图中该对象的完整选定投影，而不是让模型猜缺失字段是否被清空。

Removed 的 reason 至少区分：destroyed、out_of_view、expired、backend_reset。退出过滤结果或 top-N 视图不等于系统对象已被销毁。

初始可保留最近 4,096 个 material change batches 或 60 秒历史，先到上限者淘汰。历史不足、订阅溢出、权限变化或投影无法完整编码时返回 `reset_required`。

MUST NOT 截掉 delta 尾部却推进到最终 cursor。v0.1 在单个 delta 超预算时直接要求新 snapshot；后续再引入按提交边界分页。

### 8.6 不承诺的因果关系

Receipt 的 start/end revision 与期间相关变化可以帮助排障，但“发生在动作之后”不证明“由该动作造成”。用户或其他进程也可能修改 UI。

本地 delta 连续，只说明客户端没有丢失已提交的模型变化，不代表原生提供者没有漏发事件或世界没有未知部分。

## 9. Interaction 与线性计划

### 9.1 基本执行单位

Plan 是有总期限与步骤上限的有序列表，不是工作流语言。允许以下控制步骤：

- bind：在当前执行时刻局部查询并唯一绑定别名。
- wait：在范围内等待受限属性谓词成立，只有读操作。

其余步骤为显式语义或输入动作。v0.1 不提供分支、循环、任意表达式、脚本调用、自动错误恢复或模型调用。

### 9.2 绑定语义

`bind("field", Locator)` 在该步骤执行时查询。若唯一性已证明，就把别名固定到一个 Ref。之后所有 `bound: field` 都引用该实例。

目标在后续步骤被替换时，别名 MUST NOT 自动重绑。要针对新实例执行，必须出现新的显式 bind 步骤或由上层提交新计划。

不允许把一个计划中的所有 Locator 在开始时提前解析，因为前面的步骤可能才会创建后面的对象。

### 9.3 每步执行流程

```text
1. 检查计划剩余预算、取消、Seat health
2. 解析 Ref / bound alias / Point
3. 校验 Actor 当前权限与允许操作，生成已解析 Intent
4. 刷新生存、能力与动作所需状态
5. 校验目标名称/角色/ownership 等显式前置条件
6. 执行至多一次副作用调用或明确的一组物理输入事件
7. 如需验证，局部轮询声明的后置条件
8. 记录 delivery、verification、evidence、变化范围与错误
9. 只有该步骤满足自己的 completion 规则才继续
```

只读查询/前置条件可在步骤预算内重试。发送过副作用的步骤不得因为超时、模态 UI 或观察失败而自动重新发送。

### 9.4 可执行性检查

focus：请求前台窗口 / 控件焦点，并读取实际前台和焦点确认。不能把原生调用返回当成焦点已获得。

invoke：检查实例有效、原生能力、enabled 等可用信息；不能套用鼠标遮挡检查代替语义能力。App 仍可能产生自身的焦点/弹窗副作用，库不假设 native action 完全无其他影响。

set_value：检查原生可写能力、read_only / protected 状态，修改后读取可读 value 验证；读回值可见但不同，报告 not_met，不伪装成功。

pointer.click / drag：执行时重新解析目标几何，检查显示拓扑、目标前台条件、可用性与 hit-test 证据。无法证明目标命中时默认拒绝 ref-target 的受保护点击。宿主明确授予 raw desktop input 后才允许使用 Point；回执说明这是坐标输入，不是假定对象语义命中。

keyboard.type_text / press：在发送前验证期望窗口确实前台、期望对象拥有焦点。默认不提供“发给当前不知是谁的焦点”的 Agent 写入口。

这些检查缩短但不能消除校验与注入之间的 OS 竞态。它们不是对共享桌面的硬隔离。

### 9.5 文本、按键、鼠标的规范

TypeText 表示向当前验证过的输入目标插入 Unicode 文本，不自动选择原文本，也不保证完整复现用户 IME 组合过程。替换整个 value 应明确使用 set_value 或上层显式 select-all 组合操作。

Press 表示完整 key chord，v0.1 仅暴露配对的按下/释放操作。Key 与 Unicode 字符串不同。Windows Unicode 输入和扫描码输入具有不同原生语义，不能互相替代。[S12]

Modifiers 明确命名 `control / alt / shift / meta`；可另提供 `primary`，仅表示宿主快捷键约定（macOS meta、Windows control），不是对所有应用快捷键含义的保证。

Click 支持 left/right/middle、count=1/2。Drag 是配对的 down→move→up；取消路径也要 best-effort 释放库自身按下的按钮。

Scroll v0.1 使用 `wheel_step`，DX/DY 正号分别表示请求视口向右/向下。后端转换原生方向；不同应用和用户设置可能导致不同实际距离，不能宣称一个 step 等于固定像素。Windows 的 wheel delta 与绝对坐标有明确原生规则，后端不可直接照搬公开坐标值。[S13]

MUST 验证坐标有限、数值范围、topology、对象范围；拒绝 NaN、Inf、过大持续时间和无限长文本。

### 9.6 Completion 规则

`completion=dispatch`：所请求的 native operation 被接受，或整组输入事件已插入输入流；不要求已看到 UI 效果。

`completion=verify`：除了 delivery，还必须观察到 After 谓词。focus 默认自带实际焦点验证；set_value 默认带 value 读回验证。invoke / click / press 等没有通用业务效果，调用者可以明确选择 dispatch，或提供 After。

如果宿主对某类动作要求验证，Agent 不能用 dispatch 绕过。必须由授权策略决定是否允许不带结果证据的输入。

After 谓词只支持固定属性相等、对象是否存在、焦点/值/状态等小型集合，不引入任意代码。完整搜索或重新绑定通过显式 bind 步骤，不把复杂 query 隐藏进通用表达式。

### 9.7 Seat 调度与人类优先

同一个用户桌面的库实例共享进程内 Seat 协调器；所有写计划串行 admission / execution，读操作按后端并发限制运行。多个 Actor 不可各自启动一条并发拖拽/键盘输入链。

跨进程的第三方自动化和真人输入不受本库锁控制。MUST NOT 声称拥有系统级独占输入事务。

检测到用户干预时取消剩余步骤，释放库持有的输入状态，返回 interrupted。干预检测根据权限与平台标记 supported / best_effort / unavailable；没有检测到事件不代表没有人类干预。

禁止 BlockInput、强制重置用户物理按键、反复抢回焦点或隐藏式输入劫持。Windows 已经按住的键可能影响 SendInput；后端必须处理这一条件，而不是假定输入状态为空。[S14]

## 10. Error 与 uncertainty semantics

### 10.1 两个独立结果轴

Delivery：

| 值 | 意义 |
|---|---|
| not_applicable | bind / wait 等只读步骤 |
| none | 能确认尚未发送任何副作用 |
| complete | 原生请求被接受，或全部输入事件被插入；不是业务成功 |
| partial | 明确只发送了部分输入或部分动作 |
| unknown | 无法确认是否或完整发送 |

Verification：not_requested / verified / not_met / unknown。

`verified` 仅表示声明的后置条件在动作后被观察到，不证明因果关系，也不证明外部系统已持久保存。`not_met` 仅表示在限定观察期内未满足，不能据此推断副作用完全没有发生，或未来不会出现迟到效果。

Windows SendInput 返回插入输入流的事件数量，并受 UIPI 限制；其结果不能证明目标应用已经处理，更不能证明表单已提交。UIPI 拦截原因也不能仅凭返回值精确识别。[S14]

### 10.2 Step 与 Plan 结果

Step state：skipped / satisfied / dispatched / failed / unknown。

- satisfied：bind/wait 成功或动作所需后置条件已验证。
- dispatched：dispatch completion 已满足，但没有验证业务效果。
- failed：能确定所需条件未满足；仍须查看 delivery 是否已经产生副作用。
- unknown：副作用或效果无法确定。
- skipped：从未开始该步骤。

Plan outcome：

- completed：所有步骤满足各自 completion，可能仍包含 dispatched 而非 verified。
- stopped：在没有已知或可能副作用时停止。
- partial：存在已知已执行前缀，后续已知未完成，且没有更高优先级未知结果。
- unknown：任何关键步骤存在未消除的不确定副作用/结果；保留完整已知前缀与 skipped 尾部。

不能把 `completed` 翻译成“用户任务一定成功”。上层若需要这一保证，必须为最终操作提供可验证的业务状态。

### 10.3 典型故障码

`permission_denied`、`capability_unavailable`、`ref_stale`、`ref_gone`、`ref_expired`、`ambiguous_target`、`search_incomplete`、`precondition_failed`、`needs_user_focus`、`input_rejected`、`partial_delivery`、`verification_timeout`、`provider_unavailable`、`seat_fenced`、`user_interrupted`、`reset_required`、`receipt_expired`、`request_conflict`、`epoch_mismatch`。

错误必须附 retry_class：read_only / reobserve / never_automatically。它是恢复建议，不是自动重试命令。平台错误保留可诊断 code，但不得泄露原始敏感 payload。

### 10.4 超时与无法取消的 native call

Go context 取消只能控制本库等待与后续调度，不能保证中断 COM / AX 调用或撤回已经注入的输入。

UIA Invoke 的及时返回依赖提供者实现；AX 调用超时同样可能发生在动作已经产生作用之后。应把这些情况列为 uncertain，而不是“确定未执行”。[S15][S10]

如果一个写调用仍可能在原生线程中执行：

1. 停止后续步骤，返回 unknown receipt。
2. 将受影响 Seat / native action lane 标记 fenced。
3. 不让新计划与该迟到动作重叠。
4. 直到原生调用确实退出、线程/连接安全隔离并确认 quiescence 后，才允许解除 fence。
5. 刷新当前世界；不得自动重发旧动作。

只读 provider 阻塞只隔离相应工作资源；没有在途写时，不需要无理由把所有其他应用输入永久封死。线程池必须固定有界，不得靠无限新建 goroutine 逃避 native hang。

默认嵌入式库不承诺硬取消。后续 helper 可限制故障扩散，但终止 helper 也无法撤回已发送到 OS 的输入。

Terminal receipt 记录在返回时作出的终止决定；后续 Observe 可以得到新事实。原生调用晚到并结束可解除 fence，但不能偷偷把“超时未知”历史改写为此前确定成功。

### 10.5 重复请求与 crash 边界

每个 Plan 包含带当前 Epoch 的 RequestID。引擎在副作用 admission 前记录 Actor、规范化 plan digest 与 RequestID。Digest 包含 Epoch、步骤、目标、参数与预算，但不包含 RequestID 本身；相同 canonical body 的含义不能随请求改变。

同 RequestID + 同 Actor + 同 body：返回已有 running / terminal receipt，不再次执行。同 ID 不同 body：request_conflict。

完整 receipt 可按预算过期，但已用 RequestID tombstone 在 World 生命周期内保留。初始最多 65,536 条；上限触发 resource_exhausted，不通过忘记旧 ID 来继续提供虚假去重。

未决请求不得从 ledger 回收。旧 Epoch 的请求在重启后被拒绝。`GetReceipt` 查不到不代表动作从未执行；接收方不能用“没找到”作为重放依据。

这仅防止当前 World 中工具重发造成的重复执行，不提供跨崩溃的 durable exactly-once。

### 10.6 Go 返回约定

Execute 一旦 admission 成功，遇错也必须返回可用 Receipt 与 RunID。调用方 MUST 先保存并检查 receipt，再处理 error，不能因为 error != nil 丢掉已经执行的前缀。

Cancel 只承诺阻止尚未派发的步骤并启动清理。Close(ctx) 可能在 native call 未静止时返回 close_incomplete；不得释放仍被原生回调引用的资源并造成 use-after-free。

## 11. Public Go API

附带 api.go 是本节的完整类型草案，可独立编译，但没有真实引擎。建议将 v0.x 原生 Backend SPI 留在 internal，先稳定 World / Actor 语义，而不是过早承诺第三方 ABI。

核心接口摘录：

```go
type World interface {
    Environment(context.Context) (Environment, error)
    NewActor(context.Context, ActorConfig) (Actor, error)
    RequestPermissions(context.Context, PermissionRequest) ([]Permission, error)
    Close(context.Context) error
}

type Actor interface {
    ID() ActorID
    Observe(context.Context, ObserveRequest) (Observation, error)
    ReadText(context.Context, TextRequest) (TextResult, error)
    Changes(context.Context, ChangeRequest) (ChangeSet, error)
    Watch(context.Context, WatchRequest) (Stream, error)
    ResolveAnchor(context.Context, Anchor) (ResolvedAnchor, error)
    Capture(context.Context, CaptureRequest) (CaptureResult, error)
    ReadAsset(context.Context, AssetID) (Asset, error)
    Execute(context.Context, Plan) (Receipt, error)
    GetReceipt(context.Context, RunID) (Receipt, error)
    Cancel(context.Context, RunID) (Receipt, error)
    Progress(context.Context, RunID) (<-chan Progress, error)
    Close() error
}
```

World 是可信宿主入口；Actor 是已绑定权限的业务入口。Agent JSON 参数不能选择任意 ActorID 来冒充其他参与者。

Options / constructor 建议由 `local.Open(ctx, Options)` 提供，配置 native driver、预算、缓存和宿主 dispatcher。根包只放类型和接口，避免根包 importing backend 而 backend 又 importing 根类型的循环。

公开结构里的 Target / Step 是受限 tagged union。实现必须校验恰好一个分支、op 与参数匹配、作用域和预算；零值或不合法组合不得猜测修复。

## 12. Agent-facing protocol

### 12.1 主要操作与控制面

Agent 常用操作：`world.observe`、`world.read`、`world.sync`、`world.act`、`world.capture`。

控制面另有 `world.run.get`、`world.run.cancel`，用于超时恢复与宿主中止。Go Watch、ResolveAnchor、Progress 不必全部暴露给模型。Progress 是 best-effort 表现通知，慢订阅者可以合并/丢弃中间阶段，不得阻塞执行器；Receipt 与带 reset 语义的 Changes 才是恢复依据。

协议不要求 MCP / ACP / HTTP。默认是可序列化请求与响应；宿主自行封装为工具或本地函数。核心库不启动网络端口。

### 12.2 Envelope

```json
{
  "protocol": "desktop-world/0.1",
  "world": "e-demo",
  "op": "world.observe",
  "args": {
    "scope": {"desktop": true},
    "projection": "summary",
    "budget": {"max_results": 32, "max_output_bytes": 16384}
  }
}
```

Revision / object version 在 wire 中使用十进制字符串，避免 JavaScript 数字精度问题。时间是 UTC RFC3339；时长使用明确的整数毫秒。Go API 中的 time.Duration / uint64 不能直接按默认 JSON 编码当作协议。

未知 op / enum / tagged-union 分支默认拒绝。协议版本做能力协商；不得把任意原生属性名或函数名作为模型可调用的逃生口。

### 12.3 默认安全行为

Unknown fact 必须有 status，不用空字符串假装“已知为空”。截断必须显式给出 truncated / continuation。权限拒绝与 provider 不支持分开表达。

Tool schema 将所有 UI 文本描述为不可信数据，字段长度受限。Observation 不是附加 system prompt。

### 12.4 兼容性

v0.1 固定 schema 与操作语义；新增 capability 可通过协商扩展。保持 root Go 类型与 wire DTO 分离，使 Go 改动不直接破坏已有模型工具 schema。

运行时适配器可把 receipt 转成简短模型提示，但 MUST 保留 unknown、partial、未执行尾部和新的 Ref 绑定，不得为了美观只返回 success=true。

## 13. Platform abstraction

内部 Driver 由五个小型 facet 组成：Inventory、Semantics、Notifications、Input、Capture。它们可以由同一后端对象实现，不要求拆成五个公开 package。

```go
// 说明性 SPI；不承诺 v0.x 第三方插件兼容。
type Driver interface {
    Inventory(ctx context.Context, req InventoryRequest) (NativeInventory, error)
    Query(ctx context.Context, req NativeQuery) (NativePage, error)
    Read(ctx context.Context, key NativeKey, fields FieldMask) (NativeSample, error)
    Perform(ctx context.Context, req NativeOperation) (NativeOutcome, error)
    Capture(ctx context.Context, req NativeCapture) (NativeImage, error)
    Events() <-chan Invalidation
    Health(ctx context.Context) DriverHealth
    Close(ctx context.Context) error
}
```

NativeKey 是后端私有身份，不能直接作为公开 Ref。Backend 负责原生对象 retain/release、线程与来源；Registry 负责 Ref 分配、生命周期与预算；Engine 负责授权、投影、计划和结果语义。

Backend 必须报告 capability、partial coverage、原生 delivery 证据和错误来源。不得在内部做不可见的点击回退、重试副作用、OCR 或模型推断。

一次 Backend read 可以批量获取多个属性。Windows UIA CacheRequest 就是避免大量跨进程逐属性读取的成熟能力；cache-only element 不能被当成可执行的 live element。[S16]

### 13.1 不强行统一的内容

保持不同：原生 role / pattern、provider 覆盖率、焦点限制、权限原因、文本范围能力、坐标单位、window-content capture 可用性、滚动方式、用户干预检测与输入失败证据。

统一的重点是“怎样知道”和“怎样失败”，不是把两端压成看起来一样的 success bool。

## 14. macOS backend

### 14.1 建议基线

建议 v0.1 最低 macOS 14，首发验证 arm64 和 amd64；这是项目的工程选择，不代表已经认证全部后续系统版本。macOS 14 基线便于使用 ScreenCaptureKit 的截图路径；若需要更低系统版本，应另行承担 capture fallback 维护成本。ScreenCaptureKit 的 stream 与 screenshot 有独立内容过滤和图片配置能力。[S17]

### 14.2 Inventory 与窗口关联

用 NSWorkspace / 进程信息发现应用实例，用公开窗口 inventory 与 AX application/window 信息建立候选窗口。

语义窗口以 AX 可操作对象为主要身份；CG 窗口清单可提供显示与 capture 相关线索。不得依赖私有 AX→CGWindow ID 接口。

PID + 标题 + bounds 只用于建立候选关联。两个同名、同尺寸窗口无法消歧时，不得给 AX 对象附上可能属于另一窗口的截图或动作 facet。v0.1 可返回 correlation=ambiguous，或只提供桌面可见区域截图。

没有 Accessibility 授权时允许权限范围内的退化 inventory；授权变化导致模型来源重建时，宁可显式 invalidation/new Ref，也不悄悄融合不确定的旧对象。

### 14.3 AX bridge

使用 ApplicationServices AXUIElement / AXObserver 的窄 C / Objective-C bridge：按需读取角色、名称、值、bounds、children、action names 与 settable 属性。[S18]

能批量读取时批量；children 分页；不在后台递归整个桌面树。每进程按兴趣注册有限 AXObserver，回调仅入队 invalidation。

Observer 与 AX 引用显式管理 retain/release；使用有持续 CFRunLoop 的 native worker。AX 操作不在 Wails UI 主线程等待；需要 AppKit 主线程的少量调用由宿主 dispatcher 桥接。

库的 init/Open 不得夺取进程主线程、不创建第二个 NSApplication 主循环、不调用阻塞式应用启动。CLI 示例可自己管理运行循环；GUI 宿主仍是应用生命周期唯一所有者。

为 native AX 请求设置适当 messaging timeout；注意它是调用边界，不是事务撤销机制。[S19]

### 14.4 输入与截图

采用 CGEvent 系列发送鼠标、键盘、滚动与 Unicode 文本；高层执行器承担焦点与权限前置检查。涉及听取用户物理输入的可选监控能力必须独立报告权限，不将“可注入”与“可监听”当成同一授权。

视觉通过 ScreenCaptureKit screenshot 路径获取；v0.1 规范共同能力仅要求显式授权的 visible_region / display snapshot。窗口独立内容图可在身份关联可信且 backend 能力支持时提供，不作为两端最低共同保证。[S17]

AX 屏幕坐标、AppKit 局部坐标与 capture pixels 统一通过专门的 space converter，禁止在动作实现散落 y=screenHeight-y 等不考虑多屏的公式。

### 14.5 必测边界

已签名宿主与开发 CLI 的权限体验、权限撤销、AppKit/Wails 主循环共存、不同缩放多屏、完全同名窗口、菜单与 popup、AX provider 卡住、输入后 native timeout。

App Sandbox / App Store 分发不作为 v0.1 保证。首轮使用明确签名的桌面宿主验证 TCC 与 capture 行为；其他分发模式单独评估，不由 library 声称自己可以独立获得宿主权限。

## 15. Windows backend

### 15.1 建议基线

建议 Windows 11 amd64 作为 v0.1 正式基线；arm64 作为明确后续构建目标，不通过修改 domain model 才能适配。正式发布必须和 macOS 一起完成相同契约的验收，不允许只有 macOS 通过就把 Windows 标记正式支持。

### 15.2 Inventory 与 UIA

EnumWindows / 进程实例识别用于 inventory，结合可用窗口状态与 UIA 元素；HWND 不作为永久 Ref。

语义默认采用 UIA ControlView 和受限 scope；RawView 仅在明确请求和预算下用于补充。优先 CacheRequest 批量获取本次所需属性和 pattern。

InvokePattern → invoke；ValuePattern 且可写 → set_value；TextPattern → 必要的文本读取。不能声称 TextPattern 通用支持写文本，也不能把每个 Window / Element 都当成有 ValuePattern。

### 15.3 COM 与线程

UIA 调用由固定 native worker 承担；客户端采用非 UI 的 MTA 线程，CoInitializeEx(COINIT_MULTITHREADED)。事件注册/移除在规定线程串行处理，晚到回调靠 COM 生命周期正确收尾。[S20]

Go goroutine 不等于固定 OS 线程。若使用 Go 管理执行线程，需要 LockOSThread 并遵守 native apartment / callback 生命周期；也可由窄 C++ bridge 完全拥有 COM 工作线程。不得把裸 COM pointer 交给任意 Go goroutine 并自行猜测线程安全。[S21]

建议第一版采用小型 native C ABI 封装，输出有界 POD 数据/owned buffer。Go 负责释放约定。纯 Go COM binding 可以先 POC，但不应牺牲错误映射和线程正确性。

UIA2 提供 AutoSetFocus、ConnectionTimeout、TransactionTimeout 等控制；可以用于限制意外焦点行为和 provider 等待，但不把它们视为可中断所有动作的保证。[S22]

### 15.4 Foreground 与输入

SetForegroundWindow 受到系统前台切换规则限制。即使调用条件看似满足，也需要验证实际前台；失败返回 needs_user_focus，不使用模拟 Alt 或反复抢焦点来绕过。[S23]

SendInput 负责物理输入。检查事件插入数量，区分 none / partial / complete / unknown。UIPI / integrity 限制原样暴露，不请求默认管理员权限或 uiAccess 绕过。[S14][S24]

键盘注入前确认目标前台/焦点，处理 UTF-16 surrogate pair、非 ASCII、emoji、修饰键清理。完整 IME composition 不属于 v0.1。[S12]

### 15.5 DPI、capture 与宿主

采用 UIA 的 physical screen coordinates 为 Windows desktop frame。设置线程 DPI awareness 时保存/恢复上下文；库不能在宿主已启动后全局修改进程 DPI 模式并破坏 Wails 布局。[S11][S25]

绝对鼠标事件按 virtual desktop 范围换算到原生归一化坐标；不能默认 primary monitor。[S13]

v0.1 推荐 GDI visible-region screenshot 作为简单的共同视觉能力，明确其可见屏幕语义；不是“指定窗口独立内容”的假保证。Windows Graphics Capture 可作为后续或可选的独立窗口能力；DXGI 高帧率采集和 dirty pixel 优化推迟。[S26][S27][S28]

锁屏、用户切换或桌面不可交互时暂停写操作并反映 health。RDP 断开/显示拓扑改变必须触发能力与空间重新校对，不能继续沿用旧坐标。

## 16. Visual information

视觉是独立证据，不是对象身份主键。Object bounds 能连接 capture 区域与对象；CaptureResult 同时给出 image frame、desktop frame、像素尺寸、逐 tile 变换、捕获时刻、topology version 和 related revision。

相关 revision 只说明图像与某段模型观察相关，不保证 screenshot 与 AX/UIA 属性原子同步。

两种 capture kind 必须区分：

- visible_region：此刻屏幕区域的可见内容，包含遮挡它的其他应用。
- window_content：后端明确保证的指定窗口内容采集，依赖平台能力与可靠窗口身份关联。

从窗口 bounds 裁切整屏图片，不得标记成 window_content。只授权读取某个窗口的 Actor，不应因此自动获得可能含其他应用内容的桌面裁切图。

Asset 以本地 ID、TTL 和 Actor 作用域取回；不自动上传，不在日志里编码图片。v0.1 可以给上层视觉模型提供图片，但不内置 OCR / grounding，也不把模型推断的“按钮”自动升级为可信原生对象。

如果未来加入视觉对象，应显式区分 native / inferred / fused，带来源和不确定性；它们不能悄悄继承原生语义动作的保证。

## 17. Permission / security boundary

### 17.1 两层权限

OS capability：宿主进程是否具备 accessibility、screen capture、input injection、user-input observation 等权限或系统条件。分别报告 granted / denied / not_requested / restricted / unknown。

Actor authorization：可信宿主允许其观察哪些应用/窗口、读取哪些字段、执行哪些动作、获得哪类图片。NewActor 静态 scope 与 Authorizer 的决策取交集；空授权不代表全部允许。Open 不自动弹出权限请求。

World.RequestPermissions 仅供宿主显式调用；不同平台可以返回已有状态、需要用户操作或 unsupported。它不承诺能程序化授予 OS 权限。

### 17.2 授权时机

授权不是只在 Plan 入队时检查一次。读取、文本、图像、绑定后的目标和每次副作用前都需要当前授权。

Authorizer 接收已解析 Intent、实际 Ref、必要参数与 plan digest，可由宿主关联已批准计划；必须是有期限、非交互 callback。需要人工审批时返回 approval_required，由上层展示并重新提交新 RequestID，不能在核心执行线程等待审批 UI。

审批不跳过执行前重新验证；标题相同或历史 Ref 有效不表示目标未变化。Agent 不能修改 ActorID / force 参数增加权限。

### 17.3 隐私与 prompt injection

所有 UI 文本是数据而非指令。库不执行其中出现的命令，不让原始内容改写工具 schema。

已识别的 password / protected 字段默认不读、不保留 value，不通过 native diagnostics 或 Capture shortcut 绕过策略。应用未标记的敏感内容无法由基础库自动全面识别；不得宣称自动防止所有隐私泄漏。

本文没有解决所有 prompt injection。LLM 仍可能被界面文字诱导，在已有授权范围内执行坏决策；高层需做意图/审批控制。

### 17.4 缓存、资产与日志

权限收窄/撤销时立即阻止新的导出和写操作，清理相关文本/图片缓存与 view cursor，通知 reset_required。已经发送给外部模型的数据无法撤回，必须明确这一限制。

默认日志只含 request、scope、步骤、状态、耗时与 native code；不含键入全文、截图或完整 UI 文本。低熵敏感值的哈希也可能泄漏，不能把默认 hash 当成匿名化。

### 17.5 不提供的安全承诺

ReadOnly 和 Actor scope 是本库路径的约束，不是同进程插件的 OS sandbox。真实鼠标/键盘作用于共享桌面，存在检查与注入竞态；不能保证它是强隔离的按窗口能力系统。

默认拒绝已识别的受保护认证区域、锁屏、安全桌面和高权限跨界操作；不通过管理员启动、uiAccess 或权限绕过补齐功能缺口。[S24]

高风险应用操作是否需要确认由宿主决定。基础库不能只看 label 就准确判定一个按钮会删除文件、发消息还是提交付款。

## 18. Package / module 组织

建议先用一个 module，只有少量公开包。目录中的 example.invalid 是刻意不可解析的占位路径，创建仓库时替换为真实 module path。

```text
desktopworld/
  go.mod
  model.go                 # 根包：公共 domain types
  world.go                 # World / Actor interfaces
  action.go                # Plan / Receipt / Fault
  observation.go           # Snapshot / Delta / Coverage
  space.go                 # Frame / Bounds / Anchor
  local/
    open.go                # 宿主构造入口；imports internal engine
    options.go             # 预算、native/宿主 dispatcher 选项
  internal/
    engine/                # reducer、投影、registry、Seat scheduler
    backend/
      driver.go            # 私有 facet / native sample contract
      darwin/              # build tags，C/ObjC bridge，AX/CG/SCK
      windows/             # build tags，COM/Win32 bridge
    safety/                # 授权与数据导出控制，可先与 engine 同包
  protocol/
    dto.go                 # wire 版本、tagged unions
    codec.go               # 严格解析，revision string，ms durations
    tools.go               # 与特定 Agent SDK 无关的 schema
  dwtest/
    fixture.go             # 假驱动、虚拟时钟、故障注入
  cmd/dw-inspect/           # 可选的只读诊断程序
  examples/
    headless/              # 没有角色 UI 的宿主，不表示无桌面会话
    embodied/              # Anchor + Progress 消费者示例
    wails/                 # 独立 example module，避免污染核心依赖
  tests/
    contract/
    native-fixtures/macos/
    native-fixtures/windows/
    acceptance/
```

依赖方向为 `local → engine → 根包类型`；`engine → 私有 backend interface`；`protocol → 根包`。根包不 imports local / engine / Wails，避免循环依赖。

跨原生边界只传受控结构和所有权明确的 buffer / handle。CGO 指针与回调必须遵守 Go 约束，不在原生长期保存不合法的 Go 指针。[S29]

v0.1 不拆成十几个独立 module，不引入外部 broker、数据库、消息总线或游戏引擎。

## 19. 上层接入

### 19.1 Agent Runtime

Runtime 负责模型、提示词、工具登记、tokenizer、task memory 与决策。Desktop World 提供可被任何 Runtime 包装的观察/动作协议。

推荐 runtime 只把对任务相关的 summary/delta 送给模型。每收到 native event 就唤醒 LLM 是错误用法；局部维护和计划内等待由库承担。

### 19.2 Bot

Bot 创建一个 Actor，将选定对象 Ref 分别交给动画系统与执行器。Bot 可以在成功、失败、unknown 时选择不同反馈动作，但不能把“动画播放完了”当成真实操作成功。

### 19.3 Wails

Wails 应用导入 local 与根包，拥有主线程/窗口生命周期，向 native backend 提供必要的主线程 dispatcher。World 不知道 Wails，也不输出必须由某种前端渲染的组件。

frontend 接收的是有限 DTO 或 geometry event；原生 handle、Asset 的无限制访问、Actor 授权修改不暴露给任意 WebView JavaScript。

把 desktop frame 转成 WebView CSS 坐标应由集成层显式处理窗口位置、显示器缩放和 WebView zoom，不直接把 UIA pixel / AX point 填进 CSS left/top。

### 19.4 审批

审批 UI 与等待策略在宿主。通过 Authorizer 对已解析动作核对批准范围、参数与目标。模型建议的“这是安全动作”不是授权证据。

审批通过后仍可能发生 UI 变化；重新执行必须重新校验，不能持有旧坐标跳过检查。

## 20. MVP scope 与演进路线

### 20.1 v0.0：先跑通契约，不先堆功能

建立 fake backend、Ref registry、revision/cursor、最小 Plan executor 与错误语义。在 macOS 和 Windows 同时建立 native POC，每端都要完成窗口读取、单个输入控件读取/写入和一次物理输入。

这不是“先完成 macOS，再翻译 Windows”，而是尽早找出两个平台共同契约能否成立。

### 20.2 v0.1：正式 MVP

必须具备：

1. 两端应用/窗口 inventory、局部基本 UI 观察、bounded text、状态与 capability。
2. Ref lifecycle、带覆盖度的 snapshot/delta、明确 reset_required。
3. focus / invoke / set_value，以及移动、点击、配对拖动、wheel scroll、Unicode text 和完整 key chord。
4. 有界 bind/wait/action 计划，局部前后检查，结构化 partial / unknown receipt，当前 World 请求去重。
5. 带 frame/topology 的 bounds/anchor；授权的单显示器或区域静态截图与 Asset。
6. 权限探测、Actor scope、手动终止、输入清理、基本用户干预处理与诊断健康状态。
7. 两端同一契约测试与真实交互式桌面验收通过。

不要求每个对象支持每个动作；要求平台不能用泛化 success 掩盖不支持。Windows 默认可见区域图与 macOS 可选独立窗口图的差异通过 capability 表达。

### 20.3 v0.2

优先选择依据 POC 结果确实需要的能力：native helper 隔离、更多文本范围、显式 virtualization/scroll-into-view、Windows window-content capture、更好的 geometry watch、provider-specific capability。

Helper 值得优先做的触发条件：真实应用 provider hang 导致宿主无法安全恢复，或可信宿主要求硬故障隔离。不要仅为了架构“完整”提前服务化。

### 20.4 v0.3 及以后

可选 browser / application provider，显式 native / inferred visual objects，跨 provider 关联、更多输入设备、选择性的持久业务 locator、远程 transport。

每项扩展仍须满足 Ref 不偷换、来源可辨、输入有回执、观察有边界的核心契约。

### 20.5 明确延期

完整 OCR/grounding、LLM planner、任意 workflow DSL、自动语义修复、复杂 IME、富文本编辑、完整虚拟化列表探索、自动剪贴板粘贴、高帧率视频、远程无人桌面、跨重启 Ref、持久 exactly-once、ECS / 渲染 / 物理 / 3D 动画、安全桌面与提权。

## 21. Testing strategy

### 21.1 Unit / property / fuzz

纯 Go 测试覆盖 reducer、scope、projection、query、budget、Ref generation、version、cursor、Plan 校验与回执归类。

关键性质：

- 同一 Ref 不被本库重新分配；旧 generation 永远不能操作新对象。
- 完整 snapshot + 连续 delta，重建得到相同 material projection；忽略读取时间等非 material 元数据。
- delta 丢失/溢出时必须 reset，不能伪造无变化。
- 不完整搜索不产生唯一执行绑定。
- 已发送或可能发送的写操作不被自动重试。
- 终止后的步骤保持 skipped；清理只释放本库拥有的输入状态。
- 相同 RequestID 并发提交只 admission 一次；改 body 冲突；旧 Epoch 被拒绝。
- unrelated global revision 不会无条件终止目标仍有效的计划。
- 坐标往返覆盖负原点、多缩放、旋转与跨屏；拒绝非法浮点值。
- 文本覆盖 UTF-16 surrogate、Unicode scalar 分页、emoji 与中途变化。

对 JSON tagged union、未知枚举、过大数值、超长文本、重复 step ID、别名使用顺序和 cursor 混用做 fuzz。

### 21.2 Fixture

Fake backend 使用虚拟时钟，脚本化以下变化：对象替换、标题变化、provider 节点复用、disabled、焦点转移、窗口关闭、部分输入、超时但实际成功、事件丢失/乱序/重复、late callback、权限撤销、delta history overflow。

Golden fixtures 保存规范化输入/输出与期望回执，不依赖真实用户的私密截图。任何录制数据必须经授权并去敏。

### 21.3 Native integration

制作两个很小的被控 fixture app：macOS AppKit/SwiftUI 和 Windows Win32/WPF，各含文本框、按钮、状态标签、滚动列表、popup、同名窗口与延迟行为。

Fixture 需要自己的 ground-truth 事件日志，记录实际收到的输入、值变化与业务 stub 执行次数。不能只拿本库自己的 AX/UIA 读回结果证明本库没有误操作。

故意替换对象、让 provider 迟到、改变焦点并切换显示拓扑，验证 native semantics 和抽象契约一致。

### 21.4 Real desktop acceptance

必须在有登录用户、允许输入和已明确授权的交互式桌面测试。不要把无图形会话的 CI 编译通过当成 Desktop World 可用。

每端至少覆盖：原生应用、一个 Electron 应用、浏览器中能通过平台接口访问的基本控件、自绘/Canvas 的负面案例。记录具体 OS/app 版本，不宣称全部应用兼容。

覆盖开发 CLI 与打包宿主、权限拒绝与撤销、多显示器不同缩放、负原点、窗口重叠、用户移动鼠标/切换焦点、锁屏恢复。真实输入测试同 Seat 串行，不并行抢键盘。

### 21.5 性能与 Agent 效率

采集：native read 次数、已访问节点、返回 bytes / text runes、对象缓存大小、dirty queue、native 超时、P50/P95 延迟、一次任务模型 round trip 数、实际收到的副作用次数、错误 verified 比例。

使用同一批任务比较“完整树每步观察”与“summary/outline/delta + 有界计划”，而不是用不同任务得出 token 节省数字。Tokenizer 统计放在 Agent 测试适配层。

阶段目标可以是：简单 5–6 步表单操作只需一次 act 调用，不产生中间 LLM round trip；正常 scope 响应不超声明预算；所有注入故障在 fixture 中得到正确的 partial/unknown 分类。具体耗时和节省百分比必须实测后再写入文档。

## 22. 关键风险与验证 POC

| POC | 必须验证的问题 | 通过条件 / 失败后的动作 |
|---|---|---|
| P1 双平台 native 生命周期 | GUI 主循环、COM/CFRunLoop、线程回调和宿主关闭 | 两端 import 后不抢 UI 主线程，关闭无悬空回调；失败先缩小 bridge |
| P2 Identity | 重读、销毁重建、同名窗口、虚拟化复用 | 不偷换 Ref；无法确定时 invalidation，不追求假稳定 |
| P3 同步 | 漏事件、重扫并发、过期响应、cursor 溢出 | 明确 dirty/reset；旧结果不覆盖新状态 |
| P4 Batch | focus→bind→focus→set/type→Enter，途中替换/抢焦点 | 不向已知错误目标输入；失败保留前缀、不自动重发 |
| P5 空间 | mac point / Windows pixel / capture / Wails CSS 多屏变换 | fixture anchor 与 hit target 对应；旧 topology 被拒绝 |
| P6 权限与隐私 | capture 遮挡泄漏、受保护字段、权限收窄、高权限窗口 | 不把 desktop crop 当独立窗口授权；拒绝绕过、清理缓存 |
| P7 不可取消调用 | native write 超时后是否仍会发生副作用 | unknown + fence + 有界资源；无法安全恢复则引入 helper |

P2、P4、P5、P7 是最容易被“demo 能点按钮”掩盖的风险。先完成这些 POC，再扩大控件覆盖。

不能以一次 happy-path 演示证明 ref 稳定、无焦点竞态或动作不会重复。共享桌面的剩余风险需要被协议表达，而不是仅靠更多重试隐藏。

## 23. 完整交互示例

以下全部是受控 fixture 中的示意请求；`e-demo`、`r-window` 等不是任何真实桌面句柄。小对象名用于说明协议，不构成应用适配器或永久 locator。

### 23.1 找到窗口并读信息

第一步只获取窗口摘要：

```json
{
  "protocol": "desktop-world/0.1",
  "world": "e-demo",
  "op": "world.observe",
  "args": {
    "scope": {"desktop": true},
    "projection": "summary",
    "budget": {"max_results": 32, "max_output_bytes": 16384}
  }
}
```

响应只返回已授权的应用/窗口概况，例如 `r-window`，name=“Desktop World Fixture”，capability focus=supported。它同时带 revision、cursor、coverage，不附整棵控件树。

第二步局部展开：

```json
{
  "protocol": "desktop-world/0.1",
  "world": "e-demo",
  "op": "world.observe",
  "args": {
    "scope": {"refs": ["r-window"]},
    "projection": "outline",
    "fields": ["role", "name", "states", "capabilities"],
    "budget": {
      "max_depth": 3,
      "max_results": 64,
      "max_visited_nodes": 512,
      "max_output_bytes": 16384
    }
  }
}
```

假设发现 `r-body` 是文本区域，随后调用：

```json
{
  "protocol": "desktop-world/0.1",
  "world": "e-demo",
  "op": "world.read",
  "args": {
    "target": "r-body",
    "limit_runes": 4096,
    "freshness": {"mode": "refresh"}
  }
}
```

ReadText 返回真实支持的 text/value 来源、text version、截断状态与 continuation，不为文本阅读自动 focus 或滚动。

### 23.2 定位输入区域并输入

Agent 已知 `r-window`，不必先拿到全部控件。提交三步即可局部查询并写值：

```text
bind field := within(r-window), role=text_field, name_equals="内容"
focus bound(field)
set_value bound(field), text="Hello Desktop World", verify value_equals(text)
```

此处 focus 是显式请求真实焦点；set_value 仍通过语义路径。若 Value 写能力缺失，步骤返回 capability_unavailable，不能悄悄替换成键盘输入。

另一条明确的物理路径可以是：bind → focus → keyboard.press(primary+A) → keyboard.type_text → value 验证。它的副作用、平台快捷键假设与输入回执必须独立说明。

### 23.3 一次连续操作

假设 outline 已发现持久的状态标签 `r-status`，fixture 初始状态不是“已提交”。此计划焦点确认、写入、Enter 与最终局部状态验证均在库内完成：

```json
{
  "protocol": "desktop-world/0.1",
  "world": "e-demo",
  "op": "world.act",
  "args": {
    "epoch": "e-demo",
    "request_id": "e-demo:request-001",
    "timeout_ms": 10000,
    "steps": [
      {
        "id": "focus-window",
        "op": "focus",
        "target": {"ref": "r-window"},
        "completion": "verify",
        "timeout_ms": 2000
      },
      {
        "id": "find-field",
        "op": "bind",
        "bind": {
          "name": "field",
          "require_unique": true,
          "locator": {
            "within": "r-window",
            "role": "text_field",
            "name_equals": "内容",
            "required_capability": "set_value",
            "max_depth": 3
          }
        },
        "timeout_ms": 2000
      },
      {
        "id": "focus-field",
        "op": "focus",
        "target": {"bound": "field"},
        "completion": "verify",
        "timeout_ms": 2000
      },
      {
        "id": "fill",
        "op": "set_value",
        "target": {"bound": "field"},
        "set_value": {"text": "Hello Desktop World"},
        "after": [
          {
            "target": {"bound": "field"},
            "property": "value",
            "equals_string": "Hello Desktop World"
          }
        ],
        "completion": "verify",
        "timeout_ms": 2000
      },
      {
        "id": "submit",
        "op": "keyboard.press",
        "target": {"bound": "field"},
        "press": {"key": "Enter", "modifiers": []},
        "after": [
          {
            "target": {"ref": "r-status"},
            "property": "value",
            "equals_string": "已提交"
          }
        ],
        "completion": "verify",
        "timeout_ms": 2000
      }
    ]
  }
}
```

窗口变为前台、定位字段、确认焦点、原生写值、发送 Enter、等待状态，全都不需要中间 LLM 推理。计划总期限优先于每个步骤期限相加；实际可用时间随步骤消耗递减。

这是假定 fixture 的 status value 可读且能代表本地提交 stub 的协议示例。真实网页/应用需要选择真实可观察的完成条件，不能通用地猜“按 Enter 就提交成功”。

### 23.4 UI 在执行中替换对象

假设完成 focus-field 后，应用重建输入区域，旧 `r-input-old` 失效。fill 步骤执行前检查失败：

```json
{
  "run_id": "run-001",
  "request_id": "e-demo:request-001",
  "epoch": "e-demo",
  "state": "terminal",
  "outcome": "partial",
  "bindings": {"field": "r-input-old"},
  "steps": [
    {"id": "focus-window", "state": "satisfied", "delivery": "complete", "verification": "verified"},
    {"id": "find-field", "state": "satisfied", "delivery": "not_applicable", "verification": "verified"},
    {"id": "focus-field", "state": "satisfied", "delivery": "complete", "verification": "verified"},
    {
      "id": "fill",
      "state": "failed",
      "delivery": "none",
      "verification": "not_requested",
      "fault": {"code": "ref_gone", "retry_class": "reobserve"}
    },
    {"id": "submit", "state": "skipped", "delivery": "none", "verification": "not_requested"}
  ],
  "seat_health": "ready"
}
```

库没有把别名自动指向同名新输入框，也没有按 Enter。Agent 读取局部 delta / outline，看到新 Ref 后可决定提交新 RequestID 的计划。

### 23.5 已发出 Enter，但结果未知

如果 Enter 事件全部插入输入流，而状态 provider 超时：

```json
{
  "id": "submit",
  "state": "unknown",
  "delivery": "complete",
  "verification": "unknown",
  "fault": {
    "code": "verification_timeout",
    "retry_class": "never_automatically"
  }
}
```

Plan outcome=unknown。不得自动再按一次 Enter。下一步应读取当前状态、检查宿主可用业务证据或交回用户。

这里注入本身已经返回且已知完整，单纯读回超时不一定需要 fence。只有仍可能存在迟到原生写动作或输入状态不确定时，才保留 Seat fence。

### 23.6 世界增量

```json
{
  "from": "410",
  "to": "414",
  "cursor": "opaque-cursor-next",
  "reset_required": false,
  "upserts": [
    {
      "ref": "r-status",
      "role": "text",
      "value_preview": {"status": "known", "value": "已提交"},
      "version": "7"
    }
  ],
  "removed": [
    {"ref": "r-input-old", "reason": "destroyed"}
  ],
  "invalidated_scopes": []
}
```

这是该 view 的选定字段投影，实际 envelope 还应带 coverage。若 cursor 已过期，返回 reset_required=true，重新取局部 snapshot；不能猜测缺失的 revision。

### 23.7 具身 Bot 使用同一对象

下面是概念性宿主代码。character / renderer 是上层对象，不属于本库；World / Actor 的声明见 api.go。

```go
anchor := desktopworld.Anchor{Target: buttonRef, U: 0.5, V: 0}
resolved, err := actor.ResolveAnchor(ctx, anchor)
if err != nil || !resolved.Valid {
    // 不沿用旧点执行真实输入。
    return
}

// 宿主将 desktop frame 转为自己的渲染坐标。
character.LookAt(renderer.FromDesktop(resolved.Point))
character.WalkNear(renderer.FromDesktop(resolved.Point))

// 执行器仍在真正执行时重新检查 buttonRef；动画不是授权或有效性证据。
receipt, execErr := actor.Execute(ctx, desktopworld.Plan{
    Epoch: epoch,
    RequestID: newRequestID(epoch),
    Timeout: 3 * time.Second,
    Steps: []desktopworld.Step{{
        ID: "activate",
        Op: "invoke",
        Target: desktopworld.Target{Ref: buttonRef},
        Completion: "dispatch", // 明确只请求 native action，不假定业务完成。
    }},
})
// 即使 execErr != nil，也先处理/保存 receipt，再由上层选择反馈动画。
character.ReactTo(receipt, execErr)
```

真正把角色与 Computer Use 合成一个产品的关键不是让角色跟着鼠标跑，而是它看向、走近、选择和操作的对象始终使用同一套可验证的引用与状态。

## 24. 实现顺序与验收清单

建议按依赖排序实现，而不是按平台堆功能：

1. 冻结最小 Ref / Fact / Coverage / Receipt 契约，完成 API/JSON 校验与 fake backend。
2. 双平台 inventory / basic semantic read / native lifecycle POC。
3. Registry、局部 snapshot、订阅失效与 bounded refresh；完成 identity POC。
4. focus、set_value、完整 key chord 与输入回执；先把 unknown/fence 做正确。
5. Plan bind / wait / precondition / postcondition；完成 UI replacement 与用户干预测试。
6. pointer move/click/drag/scroll、多屏转换、截图 Asset、Anchor 消费者。
7. 两端 fixture、打包宿主/Wails smoke、真实桌面 acceptance；之后再扩大控件覆盖。

v0.1 release checklist：

- macOS 和 Windows 都通过同一规范性契约；不支持项明确出现在 capability。
- 同名/重建/虚拟化对象不偷换引用；搜索不完整不能执行唯一绑定。
- 在明确模拟的故障中，没有把未知效果报告成 verified。
- 输入终止有清理，取消不声称撤回副作用，native hang 不靠无限线程恢复。
- 截图、文本、日志和 delta 都经过当前授权。
- 示例应用没有包含具体 Agent Runtime / LLM / Wails 在根 module 的强制依赖。
- 使用说明明确 native build、权限、线程、平台版本与剩余竞态边界。

## 25. 最终推荐

Desktop World 的核心不应是一棵树、一张截图或一个 Actor 引擎，而应是三份相互一致的契约：

**对象契约：这个 Ref 指向什么、现在知道什么、还是否有效。**

**变化契约：从我上次观察到现在，本库知道哪些变化，哪些信息需要重建。**

**行动契约：针对这个对象请求了什么，发送到哪一步，验证了什么，还剩什么未知。**

空间模型把这三份契约与具身表现连接起来。这样，即使 MVP 只会读窗口、写输入框和点击按钮，上层已经可以构造一个“生活在桌面世界中”的角色，而不是两个互不相干的模块。

---

## Appendix A. 主要研究来源

以下为原始技术文档或项目一手资料。访问日期为 2026-09-29。引用证明的是对应系统事实或设计灵感，不证明本方案已经在真实桌面验证。

[S01] Apple — OS X Accessibility Model（架构性归档文档）。  
`https://developer.apple.com/library/archive/documentation/Accessibility/Conceptual/AccessibilityMacOSX/OSXAXmodel.html`

[S02] Microsoft — UI Automation Overview。  
`https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-uiautomationoverview`

[S03] Playwright — Auto-waiting / Actionability。  
`https://playwright.dev/docs/actionability`

[S04] Playwright — Locators。  
`https://playwright.dev/docs/locators`

[S05] Kubernetes — API Concepts（resource versions、watch 与历史过期）。  
`https://kubernetes.io/docs/reference/using-api/api-concepts/`

[S06] Microsoft — Subscribing to UI Automation Events。  
`https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-eventsforclients`

[S07] Microsoft — Working with Virtualized Items。  
`https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-workingwithvirtualizeditems`

[S08] RobotGo — 项目 README / 能力与构建说明。  
`https://github.com/go-vgo/robotgo`

[S09] Microsoft — IUIAutomationElement::GetRuntimeId。  
`https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationelement-getruntimeid`

[S10] AXSwift — UIElement.swift（项目一手实现，CFEqual 与 timeout 处理说明；不替代 OS 正式契约）。  
`https://github.com/tmandry/AXSwift/blob/main/Sources/UIElement.swift`

[S11] Microsoft — UI Automation and Screen Scaling。  
`https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-screenscaling`

[S12] Microsoft — KEYBDINPUT。  
`https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-keybdinput`

[S13] Microsoft — MOUSEINPUT。  
`https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-mouseinput`

[S14] Microsoft — SendInput。  
`https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput`

[S15] Microsoft — IUIAutomationInvokePattern::Invoke。  
`https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationinvokepattern-invoke`

[S16] Microsoft — Caching UI Automation Properties and Control Patterns。  
`https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-cachingforclients`

[S17] Apple — What's new in ScreenCaptureKit, WWDC23。  
`https://developer.apple.com/videos/play/wwdc2023/10136/`

[S18] Apple — AXUIElement / ApplicationServices reference。  
`https://developer.apple.com/documentation/applicationservices/axuielement`  
`https://developer.apple.com/documentation/applicationservices/axuielement_h`

[S19] Apple — AXUIElementSetMessagingTimeout。  
`https://developer.apple.com/documentation/applicationservices/1459345-axuielementsetmessagingtimeout`

[S20] Microsoft — Understanding Threading Issues。  
`https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading`

[S21] Go — runtime.LockOSThread。  
`https://pkg.go.dev/runtime#LockOSThread`

[S22] Microsoft — IUIAutomation2。  
`https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomation2`

[S23] Microsoft — SetForegroundWindow。  
`https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setforegroundwindow`

[S24] Microsoft — Security Considerations for Assistive Technologies。  
`https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-securityoverview`

[S25] Microsoft — SetThreadDpiAwarenessContext。  
`https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setthreaddpiawarenesscontext`

[S26] Microsoft — Capturing an Image（GDI）。  
`https://learn.microsoft.com/en-us/windows/win32/gdi/capturing-an-image`

[S27] Microsoft — Screen capture（Windows Graphics Capture）。  
`https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture`

[S28] Microsoft — Desktop Duplication API。  
`https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api`

[S29] Go — cgo command / pointer passing。  
`https://pkg.go.dev/cmd/cgo`

补充概念参考：

[S30] Gymnasium — Env API（观察/动作边界；不引入 reset / reward 假设）。  
`https://gymnasium.farama.org/api/env/`

[S31] ROS 2 geometry2 / tf2（空间 frame 与 transform 思路；不引入 ROS 依赖）。  
`https://github.com/ros2/geometry2`

[S32] W3C WebDriver（对象引用、stale 与输入协议的比较参考）。  
`https://www.w3.org/TR/webdriver/`

## Appendix B. 仍需用 POC 收敛的实现点

以下不影响本 RFC 对外契约，但必须在对应能力上线前落实：

- macOS AX 窗口与 capture 身份的公开接口关联覆盖率；无法唯一关联时保持降级，不采用私有 API。
- 真实 provider 对失效、虚拟化复用和超时的行为；允许后端报告更保守的不确定性。
- Wails 宿主主线程 dispatch 与实际 DPI/缩放变换；核心不依赖某个 Wails 版本。
- 两端用户干预监测的实际授权要求、误判率与可用性；没有数据时标记 best_effort / unavailable。
- 混合 DPI、显示器旋转和跨屏窗口的 anchor / input / capture 变换。
- native call 阻塞是否需要首发 helper；根据 P7 决定，不假定 context 能解决。
- 本文预算的真实延迟、资源与 token 效果；没有测试结果之前不发布节省百分比或成功率。
