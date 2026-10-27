# 重写版产品矩阵

日期：2026-10-27；功能基线为 #57 合并提交 `882f9f1eb76e2303e2bdc0370d73984e0196e876`。下一 release 只包含重写版，不包含旧脚本兼容或上游 CUA worker。表中“通过”仅限所列环境与场景；历史旧版本的 Windows、Browser 和安装证据不继承为重写版支持。

| 能力 / 工作流 | macOS 重写路径 | Windows 重写路径 | 证据与下一检查点 |
| --- | --- | --- | --- |
| App 身份、运行实例与窗口发现 | 自有 fixture 已通过；未证明库存明确报错 | 未实施 | [C5.2a](window-inventory-acceptance.md)；C5.2b 定位真实应用自然空库存及闭窗/隐藏/Space 边界 |
| AX 观察 → AXPress → 独立读回 | 窄范围 advertised AXPress 已通过；attached sheet 等边界受限 | 未实施 UIA | [C3](native-checkpoint-acceptance.md)；补选定真实应用流程，按支持范围扩展 |
| 截图 → 像素点击 → 独立读回 | SCK 精确窗口、自有前台 fixture 已通过；遮挡等拒绝 | 未实施 WGC/输入 | [C3](native-checkpoint-acceptance.md)、[C5.1](app-script-acceptance.md)；记录真实应用与权限/窗口状态 |
| 单 app MCP、可信 grant 与 Harness | 协议、预授权与实际 Caelis 本地接线已验收 | 未完成平台接线 | [C4](app-protocol-acceptance.md)；Caelis 配套仍按要求仅本地提交，未发布 |
| 单 app 脚本组合观察/动作 | 五个核心方法、有界 VM、JSON state 和回执已验收 | 未完成原生闭环 | [C5.1](app-script-acceptance.md)；以 C5.2b 的真实流程验证使用体验 |
| 取消、超时与未知效果 | Core/IPC 控制与 macOS fixture 迟到效果已验收；未知不重放 | 仅共享逻辑，原生未验收 | [C3](native-checkpoint-acceptance.md)、[C5.1](app-script-acceptance.md)；随新增动作验证真实边界 |
| 文本、键盘、滚动、拖拽、菜单、focus | 未迁入 | 未迁入 | C6 按工作流所需逐组迁入，不以全量复制为本次目标 |
| Browser/profile/DOM | 未迁入 | 未迁入 | 后置，需独立资源约束与真实流程验证 |
| 默认入口与构建依赖 | 新入口显式启用；旧默认入口和无条件 CUA SDK 依赖待删除 | 同属清理范围；未实现能力明确 unavailable | 下一独立切片：只保留新版入口，依赖树与构建产物无旧 CUA 运行链 |
| 安装、签名、TCC 与分发 | 新版包合同未完成 | 新版包合同未完成 | 选定发布范围后建立新版候选构建，验收最终产物；旧发行自动化已退出 |

## 分阶段推进

1. **开发反馈**：本轮轻量 CI 检查格式、版本、Core/IPC、验收负控与 Swift 库存分类。CI 绿色不等于以上产品工作流通过。
2. **单一新版基线**：删除旧入口、依赖及包路径，验证新版 MCP/脚本构建与启动；不增加 legacy feature 或兼容发行物。当前这一项仍待实施。
3. **可用工作流**：每次选定一个应用、平台、窗口状态与任务，完成实现 → 操作 → 独立读回 → 保留失败与证据 → 更新矩阵。Windows 可独立推进，完成前不宣称支持。
4. **按需保护与发行**：对稳定且需保护的行为增加针对性 CI；按实际已验证的平台和能力定义 release 范围，再做安装包、签名、权限和真实 Harness 验收。无须先把整个矩阵填满。

新增条目先写清任务与成功标准，再实施和验收。矩阵不等同于测试数量清单，也不要求为每一格建立永久 CI。详细执行规则见[实施检查点](implementation-checkpoints.md)，发行准入见[releasing](releasing.md)。
