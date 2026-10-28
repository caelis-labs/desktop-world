# 重写版产品矩阵

日期：2026-10-28。#60 合并基线为 `d0d8279130b74a58496e13b7f6a4c4565f6f00cf`。C5 报告证明当时共享骨架和 macOS 五方法的限定场景。本轮按用户决定把外部接入改为标准 MCP + Agent Skill：DTW 不承担审批授权，每次执行显式携带平台稳定 app ID。历史 grant/定制 Host 验收不能代替新入口的验证。

下一 release 仅包含重写版。表中“历史通过”只指报告中指定源码、二进制、平台和场景；不会继承旧版本 Windows、Browser 或安装支持。

| 能力 / 工作流 | 当前实现与支持边界 | 验证与后续工作 |
| --- | --- | --- |
| 标准 Agent MCP 接入 | `dtw mcp`；`apps` / 同步等待的 `exec` / 只读 `result`；每次显式 app ID，标准取消与断连清理；无定制 Host 前置条件 | [本轮验收通过](standard-agent-acceptance.md)：协议、原始回执、生命周期及自有原生 GUI；[合同](app-protocol-v1.md) |
| App 身份、运行实例与窗口发现 | macOS typed coverage/diagnostics；语义/脚本同一 Core；Windows 原生未实现 | TextEdit 12 状态矩阵[历史通过](c5-completion-acceptance.md)；支持范围内为空与归属未证明分开，历史偶发仍缺原始现场 |
| AX 观察 → AXPress → 独立读回 | macOS 窄范围 AXPress；结构化 role/name/value/enabled/actions；attached sheet 等边界受限 | [本轮原生验收](standard-agent-acceptance.md)：按钮字段与独立计数器读回通过；更广边界见 [C3 历史验收](native-checkpoint-acceptance.md) |
| 截图 → 像素点击 → 独立读回 | macOS SCK 精确窗口、自有前台 fixture；切 app 前关闭旧 owner；遮挡等拒绝 | [本轮验收](standard-agent-acceptance.md)：PNG、一次像素点击读回及同连接 A→B→A 截图通过；其他场景保留 [C5.1 历史证据](app-script-acceptance.md) |
| 单 app 脚本组合 | 五个实际 Promise 方法、可发现 schema、类型声明、有界 VM、JSON state 和动作回执 | 本轮 Rust 回归验证 Promise、未处理 rejection、schema/类型声明一致性与结构化错误；[脚本合同](app-script-v1.md)，C6 沿同一入口扩展 |
| 取消、超时与未知效果 | 执行期限、标准 MCP 取消、上下文重建、EOF/父退出回收；未知不重放 | [本轮 GUI](standard-agent-acceptance.md)验证取消、状态重建和 EOF 后全部原生 worker 回收；迟到效果/输入中断见 [C5 历史证据](c5-completion-acceptance.md)，强杀仍不能保证输入释放 |
| Agent Skill 与配置样板 | 标准 `SKILL.md`、普通 MCP JSON、Codex TOML、agy MCP CLI、Claude 官方插件 layout 和本地 stage helper | agy 与 Claude Code 2.1.312 的真实模型均完成 Skill→apps→exec→一次 press→独立读回→截图；[记录](standard-agent-acceptance.md)、[安装说明](install.md) |
| 文本、键盘、滚动、拖拽、菜单、focus、启动/退出应用 | 未迁入 | C6 按能力组迁入；先确定资源 owner、清理和回执。启动需另定运行实例建立前的入口与绑定语义 |
| Browser/profile/DOM | 未迁入 | 独立资源身份、生命周期与真实流程验证，不能继承桌面 app 的范围猜测 |
| Windows 原生 | 共享模型可复用；原生 provider 尚未实现 | 独立完成 UIA/WGC/输入和真实 GUI 验收后才宣称支持 |
| 依赖与构建 | rewrite-only；无 CUA SDK、上游 worker 或 legacy feature | [D1 历史证据](rewrite-baseline-acceptance.md)，持续按当前源码检查来源边界 |
| 签名、TCC 与公开分发 | 本地开发 stage 已提供；新版发行包合同未完成 | 候选打包、最终产物安装、签名、权限与目标 Agent 实际加载仍需独立验证 |

## 推进循环

1. 本轮标准 MCP、五方法、app ID、生命周期和两个普通 Agent 的模型工作流[均已记录](standard-agent-acceptance.md)；结论限于记录中的自有 fixture 场景。
2. C6 按[架构基线的能力组清单](next-architecture-baseline.md)批量扩展。每组完成“限定场景 → 迁入 → 调用 → 独立读回 → 记录边界”，输入状态和浏览器等资源必须明确 owner 与清理路径。
3. 两项轻量 CI 继续保护已确定的源码/Core/IPC/Swift 纯逻辑。先验证功能，再为实际回归和稳定合同按需增加 CI，不重新构建完整 CUA。
4. 根据已验证的平台与工作流选择 release 范围，再执行[发行准入](releasing.md)。不用填满整个矩阵后才交付首版。
