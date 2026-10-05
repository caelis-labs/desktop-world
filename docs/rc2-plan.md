# Issue #18 打磨方案与 rc.2 候选范围

目标：外部 Agent 少量观察后知道怎么操作，编排程序按语言即可接入，并保持原生授权、部分投递与回执契约。来源：[Issue #18](https://github.com/caelis-labs/desktop-world/issues/18)。

| 建议 | 判断与 rc.2 交付 |
| --- | --- |
| cooperative 事务语法糖 | 采纳。Go/TS/Python/Rust Plan 本地聚合，JS/TS transaction 单次提交；native bind_focus 在执行时解析范围内焦点 |
| --write-app 未运行即失败 | 采纳并扩展。统一 pending/active/ambiguous/unresolved/expired/revoked，可信宿主动态追加/撤权；精确实例绑定不跟随重启 |
| audit 已存在导致失败 | 采纳。默认轮转，显式 create/append，排他 writer lock、末行完整性检查、session epoch 和授权元数据；完整内容 debug opt-in |
| Python SDK | 采纳，并按用户要求加入 TS、Rust，与 Go 共同覆盖。原生 supervisor 共用控制管道，保留完整 facts/coverage/fault/receipt，不自动重试 |
| Canvas 混合视觉兜底 | 留作后续独立设计。当前按需窗口截图与坐标映射仍可用；不把视觉匹配当作原生 identity，也不自动降级绕过授权 |

交付包括版本握手、各语言 README、Agent skill、跨平台进程夹具测试、原生新增流程脚本及无本地缓存的候选打包。Go private host-control 与直接 serve 七操作保持兼容；新增 session 协议携带 owner/desktop 分离，模型参数不能选择 owner channel。

按用户最后指示，正式 rc.2 发布前停止，最终已提交 SHA 的 Windows 验收交接 Issue 是后续入口。Windows 交接不以交叉构建或 rc.1 历史验收代替实机结果。完整验收与发布条件见 [Windows 交接](rc2-windows-handoff.md)。
