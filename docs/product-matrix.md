# 重写版产品矩阵

日期：2026-10-28；基于 #59 合并提交 `579727df5386c708ca4074e5df033aed5f0f7494`，C5 已完整落地共享骨架和 macOS 五方法闭环，见 [C5 验收](c5-completion-acceptance.md)。下一 release 只包含重写版，不包含旧脚本兼容或上游 CUA worker。表中“通过”仅限所列环境与场景；历史旧版本的 Windows、Browser 和安装证据不继承为重写版支持。

| 能力 / 工作流 | macOS 重写路径 | Windows 重写路径 | 证据与下一检查点 |
| --- | --- | --- | --- |
| App 身份、运行实例与窗口发现 | typed coverage/diagnostics；原子、JS 与账本一致；TextEdit 12 状态矩阵通过 | 未实施 | [C5 完整验收](c5-completion-acceptance.md)；支持范围内为空与未证明明确区分，历史偶发仍缺原始证据 |
| AX 观察 → AXPress → 独立读回 | 窄范围 advertised AXPress 已通过；attached sheet 等边界受限 | 未实施 UIA | [C3](native-checkpoint-acceptance.md)；补选定真实应用流程，按支持范围扩展 |
| 截图 → 像素点击 → 独立读回 | SCK 精确窗口、自有前台 fixture 已通过；遮挡等拒绝 | 未实施 WGC/输入 | [C3](native-checkpoint-acceptance.md)、[C5.1](app-script-acceptance.md)；记录真实应用与权限/窗口状态 |
| 单 app MCP、可信 grant 与 Harness | 协议、预授权与实际 Caelis 本地接线已验收 | 未完成平台接线 | [C4](app-protocol-acceptance.md)；Caelis 配套仍按要求仅本地提交，未发布 |
| 单 app 脚本组合观察/动作 | 五方法、可发现 schemas、类型声明、有界 VM、JSON state 和回执已验收 | 未完成原生闭环 | [C5 完整验收](c5-completion-acceptance.md)、[脚本合同](app-script-v1.md)；C6 按同一入口扩展 |
| 取消、超时与未知效果 | 迟到效果、idle grant 到期、输入中途 cancel/close/父死已验收；异常退出保留清理诊断 | 仅共享逻辑，原生未验收 | [C5 完整验收](c5-completion-acceptance.md)；Unknown 不重放，强杀不能保证输入释放 |
| 文本、键盘、滚动、拖拽、菜单、focus | 未迁入 | 未迁入 | C6 按工作流所需逐组迁入，不以全量复制为本次目标 |
| Browser/profile/DOM | 未迁入 | 未迁入 | 后置，需独立资源约束与真实流程验证 |
| 默认入口与构建依赖 | 仅新版 app 入口；默认编译协议/脚本，无 CUA SDK/worker 或 legacy feature | 共享入口已收敛；原生执行 unavailable，未做 Windows 构建/GUI 验收 | [D1](rewrite-baseline-acceptance.md)：离线构建、CLI、fixture 与实际 Caelis 本地 Harness 通过 |
| 安装、签名、TCC 与分发 | 新版包合同未完成 | 新版包合同未完成 | 选定发布范围后建立新版候选构建，验收最终产物；旧发行自动化已退出 |

## 分阶段推进

1. **开发反馈**：轻量 CI 检查格式、版本、Core/IPC、验收负控与 Swift 库存/输入纯逻辑；仍是两项作业，无完整 Cargo/CUA 编译。CI 绿色不等于以上产品工作流通过。
2. **单一新版基线**：D1 已删除旧入口、依赖及包路径，并验证默认 MCP/脚本构建和实际启动；没有 legacy feature 或兼容发行物。新版分发合同仍待实施。
3. **C6 能力组**：C5 核心已收口。按选定工作流批量迁入能力，沿现有身份、授权、引用、预算和回执合同扩展；完成实现 → 操作 → 独立读回 → 保留失败与证据 → 更新矩阵。Windows 完成原生与 GUI 验收前不宣称支持。
4. **按需保护与发行**：对稳定且需保护的行为增加针对性 CI；按实际已验证的平台和能力定义 release 范围，再做安装包、签名、权限和真实 Harness 验收。无须先把整个矩阵填满。

新增条目先写清任务与成功标准，再实施和验收。矩阵不等同于测试数量清单，也不要求为每一格建立永久 CI。详细执行规则见[实施检查点](implementation-checkpoints.md)，发行准入见[releasing](releasing.md)。
