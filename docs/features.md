# 独立 feature 实现与操作验收

2026-10-04。本批源码 helper 命令为 `dtw`，公开 alpha 尚未更新，caelis-bot 的 M0 更新与统一联调后置。实现沿用 World / Actor / 原生后端这一条执行路径。

| Feature | 完成的实现 | 独立真实场景 | 实机状态 |
| --- | --- | --- | --- |
| F1 no_shared_input | 可信宿主上限；整份混合计划先拒绝；收据保留动作 channel 和原请求 | 前台持续 Unicode 输入时，后台语义填写并提交订单；验证前台/指针稳定、两端文本与提交次数 | macOS arm64 通过 |
| F2 set_expanded | 显式期望状态；AXExpanded / UIA ExpandCollapse；完整写前检查、自动验证、已满足时无 setter | 展开 Shipping details、重复展开、收起；应用独立记录状态和实际明细可见性 true/false | macOS arm64 通过；UIA 映射待 Windows 实机 |
| F3 字段读取计划 | fields + match 必需字段下推；局部缓存合并与每字段采样时间；座席仅采样身份 | 同一窗口的 12 个慢值 getter 做宽/窄对照，然后完成订单填写/提交 | macOS arm64 通过；Windows 属性成本待实机 |
| F4 UIA 续扫 | 保留 DFS 栈及兄弟 frontier；访问预算、10k 总上限、16 游标/90 秒、消费后 dirty | 1,000 行控件之后找 Deep submit，64 节点/次续扫后填写并提交，验证独立应用日志只提交一次 | 已实现、交叉构建；Windows 实机待验收 |
| F5 Windows managed transport | 两条私有匿名管道、受限句柄继承、可信启动元数据；独立控制/数据通道 | host.Start → Grant → 填写/提交 → EndTurn；后续写入被拒绝，原收据可恢复 | 已实现、交叉构建；Windows 实机待验收 |

F1 承诺不主动发送共享键鼠或切换焦点。应用自身的语义动作可能弹窗或激活应用，不能从 AppKit fixture 推导所有应用都无干扰。F4 的 live UIA 树续扫只可发现目标，dirty / incomplete 不能证明不存在。游标容量不足会拒绝新扫描，保留原 frontier；错误和未知 delivery 不会自动重放。

UIA 续扫按公开的 [TreeWalker](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nn-uiautomationclient-iuiautomationtreewalker) 逐子节点/兄弟前进；展开状态区分 [Collapsed / Expanded / PartiallyExpanded / LeafNode](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationcore/ne-uiautomationcore-expandcollapsestate)。LeafNode 不广告展开能力，未知状态不被映射成 false。这些 API 依据和交叉构建不替代 Windows 原生运行。

## 渐进式披露与预算

- `dtw schema` 只给约 164 bytes 的目录；`dtw schema act ACTION` 只给本次动作。当前 set_expanded 为 5,491 bytes，完整 act 为 12,538 bytes。保留 target、前后条件、严格参数与超限约束，Handler 继续做完整校验。
- JavaScript 默认 summary 32 条 / 8 KiB，outline 只取 name/role、深度 4 / 32 条 / 8 KiB。值、URI、状态、能力、几何显式请求。match 必需的隐藏字段用于筛选，不顺便输出。
- 中间 observation 保存在本地 state；list 在同一 observation 上展示分页，next 才续取原生页。脚本最多 32 调用 / 60 秒 / 8 KiB print，outline 页系列仍受累计 24 KiB 输出上限约束。
- 局部读取不擦除未请求事实，也不把其 sampled_at 刷新为现在。新的 protected 状态清除旧敏感值；写前完整 fresh-read 可拒绝实际已经 disabled 的控件。
- coverage、身份、未知/脱敏、channel、delivery、verification、fault 与原收据不因 compact 或预算被隐藏。字节数和原生 getter 次数均不等于模型 tokens。

## 复跑

在已登录、现有 OS 授权的真实桌面运行；脚本不申请权限。每项启动独立应用、唯一标题和新日志，所有验收动作经 `dtw` / `host` SDK，业务结果来自应用自己的回调。清理只终止本次创建的 fixture。

```sh
export GOWORK=off
python3 scripts/accept-features.py F1 F2 F3
./scripts/check.sh
```

Windows 11 amd64 已登录桌面：

```powershell
$env:GOWORK = "off"
python scripts/accept-features.py F4 F5
```

结果写入新的 `artifacts/feature-acceptance-*` 目录，包括 per-feature 测试输出、应用日志、helper 版本/哈希、base commit、源码 SHA256 清单。F1 使用第二份 helper 模拟人的共享键盘输入，后台任务费用单独计量。F3 显式值读取对照单独计量，其余费用包含实际发现与提交路线。计量是 SDK 请求数及 host.Content 文本投影 bytes；没有运行 LLM，未计启动 hello、私有授权控制、MCP structuredContent 重复内容或额外 Runtime 元数据。

## 验证记录

本批最终证据位于 [2026-10-04 独立 feature 记录](evidence/features-20261004/README.md)。包含 F1/F2/F3 的独立实机成功记录和合成测试应用的日志，不保存其他桌面应用内容。另补跑原生键鼠/生命周期、值/保护字段、超时及恢复、16 游标容量回归。旧生命周期验收此前只消费大字段首个页，新增明细后无法发现排在后页的新控件；现按 name/role 和 continuation 发现替换控件，仍验证旧 Ref 被拒绝、替换值保持为空。

`scripts/check.sh` 覆盖 Go race、vet、Windows 交叉构建/vet、协议示例及 17 个 JavaScript 测试。真正的继承子进程控制测试已在 macOS 运行；Windows 对应测试程序可编译，但本轮没有 Windows 交互式机器，不能据此把 F4/F5 标记为验收完成，也不声称远程 CI、双平台正式发布或 Bot 联调通过。
