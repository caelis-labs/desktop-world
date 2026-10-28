# 重写版产品矩阵

日期：2026-10-28。本轮接续 #63 的默认执行摘要切片（`16c5afd1732e27e38407b601abe26d2012ccd1b8`），沿用官方 rmcp 3.8.0 stdio，交付第一批通用基础与发现能力中的窗口引用隔离和原生筛选/分页。#63 尚未合并时，本切片以其分支为 PR 基线。C5 报告证明当时共享骨架和 macOS 五方法的限定场景。#61 已把外部接入改为标准 MCP + Agent Skill：DTW 不承担审批授权，每次执行显式携带平台稳定 app ID。历史 grant/定制 Host 验收不能代替新入口的验证。

下一 release 仅包含重写版。表中“历史通过”只指报告中指定源码、二进制、平台和场景；不会继承旧版本 Windows、Browser 或安装支持。

| 能力 / 工作流 | 当前实现与支持边界 | 验证与后续工作 |
| --- | --- | --- |
| 标准 Agent MCP 接入 | `dtw mcp`；`apps` / 同步等待的 `exec` / 只读 `result`；exec 只需 app/source，执行编号内部生成；result 默认按 app 查最近接受执行，标准取消与断连清理；无定制 Host 前置条件 | [当前 SDK/接口验收](rmcp-acceptance.md)：app/source、原始回执、取消恢复及自有原生 GUI；[先前骨架验收](multi-app-foundation-acceptance.md)；[合同](app-protocol-v1.md) |
| App 身份、运行实例与窗口发现 | macOS typed coverage/diagnostics；语义/脚本同一 Core；Windows 原生未实现 | TextEdit 12 状态矩阵[历史通过](c5-completion-acceptance.md)；支持范围内为空与归属未证明分开，历史偶发仍缺原始现场 |
| AX 观察 → AXPress → 独立读回 | macOS 窄范围 AXPress；结构化 role/name/value/enabled/actions；attached sheet 等边界受限 | [本轮原生验收](rmcp-acceptance.md)：按钮字段与独立计数器读回通过；更广边界见 [C3 历史验收](native-checkpoint-acceptance.md) |
| 截图 → 像素点击 → 独立读回 | macOS SCK 精确窗口、自有前台 fixture；短命 scene owner，切 app 保留原逻辑上下文与 AX 引用；基础路径仍依赖 AX 窗口校验；遮挡等拒绝 | [本轮验收](rmcp-acceptance.md)：PNG、一次像素点击读回及同连接 A→B→A 截图通过；其他场景保留 [C5.1 历史证据](app-script-acceptance.md) |
| 按 app 的多上下文脚本组合 | 五个实际 Promise 方法；同会话多个 app 的独立 state/refs；公开输入仅 app/source，内部 deadline、无调用次数配额；VM/回执字节受限 | 本轮 Rust 回归验证 Promise、未处理 rejection、schema/类型声明一致性与结构化错误；[脚本合同](app-script-v1.md)，C6 沿同一入口扩展 |
| 取消、超时与未知效果 | 执行期限、标准 MCP 取消、原生 owner 重建而已提交 state 保留、EOF/父退出回收；未知不重放 | [本轮 GUI](rmcp-acceptance.md)验证取消、已提交 state 保留与原生 refs 重建和 EOF 后全部原生 worker 回收；迟到效果/输入中断见 [C5 历史证据](c5-completion-acceptance.md)，强杀仍不能保证输入释放 |
| Agent Skill 与配置样板 | 标准 `SKILL.md`、普通 MCP JSON、Codex TOML、agy MCP CLI、Claude 官方插件 layout 和本地 stage helper | 当前 Claude Code + gpt-6.1-terra 通过标准 Skill→apps→两次 exec 完成 420 控件查询、一次 press、独立读回及截图，无额外 result 查询；[当前记录](paged-discovery-acceptance.md)。agy 保留 [#61 历史结果](standard-agent-acceptance.md)；[安装说明](install.md) |
| 脚本文件与远程 MCP | 当前仅接收脚本文本，传输仅 stdio；文件便利入口与 HTTP 均未实现 | JSON/JS 内容保真回归已覆盖。文件由可选客户端入口读取后发送内容；远端不依赖调用方路径。首版聚焦 stdio，已复用官方 rmcp 3.8.0；其他 transport 与远程验收后置；[边界](next-architecture-baseline.md#脚本内容文件输入与远程执行) |
| 不依赖语义树的基础窗口路线 | 现有 list/window/capture/click 已接入，但仍有 AX 归属/模态 guard | WindowSurface 与 SemanticBinding 分离合同已明确；替代身份/窗口复用/坐标证明尚需实现，不能直接删 guard |
| 简洁模型输出 | exec 默认提供步骤/动作摘要和最后注意项，用户 value/console/图像不变；result 保留完整原始账本；被捕获的未知动作、中断及恢复标记仍可见 | [摘要切片验收](compact-exec-acceptance.md)；这是默认账本输出的精简，不是大树分页或所有响应的固定字节上限 |
| 复杂 AX 树按需发现 | macOS 原生 role/text 筛选、保留 frontier 的单次游标续页；每窗语义/像素引用独立，已访问页可操作；明确 partial coverage | 420 个 AppKit 控件与就绪后的 WKWebView 大树[真实通过](paged-discovery-acceptance.md)；WebKit 冷启动就绪仍有缺口，区域/子树过滤后置 |
| 多会话/后台/鼠标独立/锁屏 | MCP 会话状态独立；app worker 不持有闲置 scene 锁；同连接执行仍串行 | [本轮验收](rmcp-acceptance.md)通过独立会话状态/并发截图/退出清理；完整输入仲裁、后台新路线、不动指针与锁屏均未实现，不从后台 AXPress 推导 |
| 文本、键盘、滚动、拖拽、菜单、focus、启动/退出应用 | 未迁入 | C6 按能力组迁入；先确定资源 owner、清理和回执。启动需另定运行实例建立前的入口与绑定语义 |
| Browser/profile/DOM | 未迁入 | 独立资源身份、生命周期与真实流程验证，不能继承桌面 app 的范围猜测 |
| Windows 原生 | 共享模型可复用；原生 provider 尚未实现 | 独立完成 UIA/WGC/输入和真实 GUI 验收后才宣称支持 |
| 依赖与构建 | rewrite-only；无 CUA SDK、上游 worker 或 legacy feature | [D1 历史证据](rewrite-baseline-acceptance.md)，持续按当前源码检查来源边界 |
| 签名、TCC 与公开分发 | 本地开发 stage 已提供；新版发行包合同未完成 | 候选打包、最终产物安装、签名、权限与目标 Agent 实际加载仍需独立验证 |

## 推进循环

1. #61 的标准 MCP、五方法、app ID、生命周期和两个普通 Agent 的模型工作流[均已记录](standard-agent-acceptance.md)；结论限于记录中的自有 fixture 场景。
2. 多 app/多会话骨架及[官方 rmcp 入口重新验证](rmcp-acceptance.md)已通过；已落地公共合同同步、简洁默认结果、按窗/类型隔离引用、原生筛选和分页；下一步完成 AX 无关基础窗口路径、浏览器语义就绪与按需 API 发现。C6 按[架构基线的能力组清单](next-architecture-baseline.md)批量扩展。每组完成“限定场景 → 迁入 → 调用 → 独立读回 → 记录边界”，输入状态和浏览器等资源必须明确 owner 与清理路径。
3. 两项轻量 CI 继续保护已确定的源码/Core/IPC/Swift 纯逻辑。先验证功能，再为实际回归和稳定合同按需增加 CI，不重新构建完整 CUA。
4. 根据已验证的平台与工作流选择 release 范围，再执行[发行准入](releasing.md)。不用填满整个矩阵后才交付首版。
