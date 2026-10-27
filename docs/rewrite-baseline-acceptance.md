# D1：单一重写版运行基线验收

日期：2026-10-27。#58 两项 CI 通过后合并为 `c55348610071e2b26e10ce490744065b0cdd08b7`；
本检查点实现提交为 `9aad23cc941470e8419142f98df4350e99068979`。
环境为 macOS 27.0.1 (26A434)、arm64、Rust 1.98.0、Xcode Swift。
本记录通过的是源码运行基线，不是新版 release 或全平台可用性。

## 目标与实现

默认 Cargo 构建现在直接包含 app 协议、QuickJS 引擎和原生后端。
公开入口只有 `dtw app catalog`、`dtw app serve --grant /absolute/host-grant.json`
及帮助/版本命令；裸 `dtw` 显示帮助，旧安装/MCP 命令明确拒绝。
没有选择旧执行器的 feature、环境变量或 fallback。

删除 CUA SDK、上游 worker、旧无限制脚本 facade、旧安装器、插件包、
嵌入 bundle、签名/公证和包构建工具，以及专属旧验收代码。
Cargo.lock 从 521 缩至 66 个包，没有 CUA 包或 git source；
默认依赖为 base64、serde_json、uuid、rquickjs，macOS 另用 libc。
CUA 0.38.0 是保留完整许可和来源映射的代码来源，不是运行依赖。
DTW 自己的受监管原生子进程、SCK lease、Core grant 和引用边界继续保留。

SCK 的 Scene.swift 迁入 macOS 后端，与 Native.swift 一起默认编译；
旧 discover/validate_point 扩展路径删除。像素操作必须携带并通过 typed target
校验，缺失 target 不再跳过检查。已有 Swift 纯逻辑控制增加这一负控。
旧版历史证据保留并标明版本，不继承为重写版的安装/Windows/Browser 支持。

## 本地验证

| 验证 | 结果与边界 |
| --- | --- |
| 空 target、已有 registry 源码缓存的默认离线构建 | `CARGO_TARGET_DIR=/private/tmp/dtw-rewrite-only-target cargo +1.98.0 test --manifest-path cmd/dtw/Cargo.toml --offline`；首次编译 15.00 秒，无 CUA checkout/下载/编译；不是网络空缓存测速 |
| Cargo 测试 | 139 项 library + 4 项 CLI 集成测试通过；覆盖默认帮助、版本、旧参数拒绝、grant 路径前置拒绝 |
| 轻量源码检查 | 版本、rustfmt、40 Core + 12 IPC、8 Python negative controls 通过 |
| macOS 纯 Swift | 72 项断言通过，无 GUI 访问 |
| fixture 原子语义/像素 | AXPress 独立计数 0→1，像素点击 1→2；peer 不变、旧引用拒绝、图片格式与尺寸检查通过，并人工查看目标窗口截图 |
| 脚本原生生命周期 | timeout/cancel 均有独立迟到效果读回，原回执保留，不重放，原生子进程回收 |
| 实际 Caelis 本地 Harness | semantic 脚本及同脚本 AX+pixel 组合均通过；覆盖审批、图片、异常前效果、JSON state、原 ID 去重与关闭拒绝 |
| TextEdit 真实应用 | 13 阶段、78 新上下文、156 次清单读取，以及闭窗后旧引用拒绝；详见[边界取证](window-inventory-real-app-acceptance.md) |

Caelis 使用已保留的本地 `ec6d463ba596e3e182ae54d26087d0b441a302fe`，
运行 `TestDesktopAppNativeScriptSemanticHarnessAcceptance` 和
`TestDesktopAppNativeScriptHarnessAcceptance`。未修改、推送或发布 Caelis。

验收二进制：`/private/tmp/dtw-rewrite-only-target/debug/dtw`，
SHA-256 `15bd93e8b52b6716d4f5398f54a7c137a9197a50bad3f47c4389e4178e8e0ddd`。
它从独立 target 直接运行，无旧包、旁置 CUA worker 或嵌入 bundle。

原始证据目录：

- `/private/tmp/dtw-rewrite-semantic-20261027-r2`
- `/private/tmp/dtw-rewrite-pixel-20261027`
- `/private/tmp/dtw-rewrite-script-lifetime-20261027`
- `/private/tmp/dtw-rewrite-harness-semantic-20261027`
- `/private/tmp/dtw-rewrite-harness-combined-20261027`
- `/private/tmp/dtw-real-app-20261027`

首次受限 sandbox 启动 fixture 被 LaunchServices 拒绝，未产生 state.json；
`dtw-rewrite-semantic-20261027` 在前置检查失败，未启动协议或动作。
在允许桌面访问的环境启动同一自有 fixture 后，使用新目录 r2 完成验收。
首次源码检查的 Python cache 写入也受工作树沙箱限制；使用临时 pycache 后通过。
这些是启动/写入环境限制，不计为产品 GUI 通过。原始失败未覆盖删除。
本次新建 TextEdit 和两个 fixture 进程均已退出，证据和临时文档保留。

## 检查点结论与下一步

**D1 通过：源码只有一条重写版运行路径。** 两项轻量 CI 的范围不扩大；
本地完整编译和 GUI 证据单独记录。没有生成安装包、release、tag 或候选资产，
没有宣称 Windows、Browser、签名或已安装客户端 TCC 通过。

下一步是 C5-macOS.2b.2 的限定库存合同与首次异常取证，
再按[产品矩阵](product-matrix.md)选择一个可用工作流所需的能力组。
新版包合同、依赖许可载荷、签名和安装验收在发布范围确定后独立推进。
