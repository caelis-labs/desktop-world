# 2026-10-04 独立 feature 实机证据

最终 F1/F2/F3 轮次：`20261004T042852Z-1343ef`，macOS 27.0.1 arm64、Go 1.26.8，已登录交互桌面。独立 `dtw` managed helper 执行操作，AppKit fixture 通过自己的事件/业务回调记录结果。本轮未申请或修改系统权限。

运行来自 `225772c423d9cf285fa973d591bc94bce4b1e3cc` 的未提交源码，helper 的 vcs.modified=true；这不是该 commit 已发布的二进制。helper SHA256、版本、环境在 [summary.json](summary.json)，本轮编译输入指纹在 [source-sha256.json](source-sha256.json)。保存时逐项校验，均与当前源码一致。

| Feature | 原生测试时间 | SDK 数据调用 / 文本投影 bytes | 独立应用结果 |
| --- | --- | --- | --- |
| F1 | 3.21 秒 | 后台 6 次 / 12,724 | 后台订单提交一次；前台持续 Human-🙂-input 完整且提交一次；前台窗口、系统指针稳定；后台无 key_down/pointer |
| F2 | 0.75 秒 | 6 次 / 12,331 | expanded 和 details_visible 均为 true,false；重复展开没有第二次 setter；展开/收起均验证，无物理输入 |
| F3 | 1.23 秒，含对照 | 实际发现/任务 6 次 / 15,344；显式宽字段对照另 1 次 / 7,347 | 慢值 getter 宽读取 12 次、窄读取 0 次；随后填写并提交订单一次 |

这些是固定合成场景的 SDK 调用量和 host.Content 的文本 JSON bytes，没有使用 LLM，不等于 tokens。F1 的另一 helper 模拟人的连续共享输入，其费用不计入后台 Agent 任务。授权/启动 hello、私有控制流、structuredContent 的重复体积和 Runtime 元数据未计入此表。整体限制与使用方式见 [features.md](../../features.md)。

测试输出：[F1](f1-test.txt)、[F2](f2-test.txt)、[F3](f3-test.txt)。应用自身日志：[F1 后台](f1-background.jsonl)、[F1 前台](f1-human.jsonl)、[F2](f2.jsonl)、[F3](f3.jsonl)。日志只包含这些测试应用的合成内容，移除了临时 PID；未保存其他桌面应用文本、完整 inventory 或截图。

完整检查见 [checks.json](checks.json)：Go race / vet、Windows amd64 交叉构建/vet、9 个 JSON 示例、17 个 JavaScript 测试均通过。Windows host/driver 测试程序另已交叉编译，未在 Windows 执行。

最终另行补跑的原生回归全部通过：[键鼠与替换 Ref](regression/TestNativeFixture.txt) 1.14 秒、[标量值/保护字段/未知验证](regression/TestNativeValueFixture.txt) 2.10 秒、[超时与授权恢复](regression/TestNativeObserveTimeoutFixture.txt) 0.74 秒、[16 份游标与续扫](regression/TestNativeScanCapacityPreservesCursor.txt) 0.89 秒。各自使用新实例与独立日志。生命周期发现改用窄字段及续页，仍保留旧 Ref 写入必须被拒绝的断言。

F4/F5 **没有 Windows 实机通过记录**。它们的真实 Win32 操作验收已实现，可在已登录的 Windows 11 amd64 上运行 `python scripts/accept-features.py F4 F5`；交叉构建和 macOS 上的继承子进程管道测试不能替代这两项验收。F2/F3 的 UIA 映射/读取成本也尚未获得 Windows 实机证据。本批未更新公开 release 或 caelis-bot M0，未声称远程 CI 或双平台正式发布通过。
