# Desktop World rc.2 候选交接

rc.2 目标是外部 Agent 好用、会用、容易集成。实现包含延迟 APP 声明、动态追加/撤销授权、执行时焦点绑定、计划聚合、审计轮转和 Go/TS/Python/Rust 接入。方案对应 [Issue #18](https://github.com/caelis-labs/desktop-world/issues/18)，完整契约见 [Agent 接入](docs/agent-integration.md)。

用户已授权 Windows 实机验收通过后发布 rc.2 GitHub pre-release，并要求 Mac 分发包从最新代码重新编译。DESKTOP-90677U0 的最终验收提交、原回执及发布哈希记录在 [Issue #20](https://github.com/caelis-labs/desktop-world/issues/20)；[验收清单与命令](docs/rc2-windows-handoff.md) 明确原生包及退出条件。rc.1 历史证据不能替代本次复验。npm/PyPI/crates.io 的注册表发布独立于 GitHub 安装包发布。

宿主保留 HostSession；模型仅获得 DesktopClient。Go 使用 host.Start 的私有继承管道；TS/Python/Rust 使用原生 dtw session supervisor，同一内核处理所有输入，不在各语言复制原生后端。动态 Grant/Declare/Revoke/Grants 与 BeginTurn/EndTurn 属于可信宿主。APP 实例退出、撤权或回合结束使授权失效，保留原始回执，不自动重启或重放。

从干净提交打包，核对 manifest 的 vcs.revision、vcs.modified 与 SHA256SUMS。Mac 包 ad-hoc 签名、未经公证；Windows 包未签名。包内客户端由 git archive 提取，排除本地 node_modules、target、缓存和私有验收日志。可信宿主和客户端必须固定同一候选提交。

验证入口：`scripts/check.sh` / `scripts/check.ps1` 运行 Go race/vet、平台构建、JS 及三语言真实进程夹具契约；`scripts/accept-features.py`、`scripts/accept-rc2.py --helper <包内dtw>` 在授权交互桌面产生原生 APP 独立日志。自动检查不代替原生复验，脚本成功不等于独立 LLM Agent 的通用任务成功率。

不耦合实际 caelis-bot 固定版本升级。Canvas/视觉 grounding、Windows arm64、多屏/RDP、更多系统版本及复杂 IME 仍为后续扩展，不作为已交付能力。
