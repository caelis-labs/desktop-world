# #8 Review Findings 修复与验证

2026-10-04，审查基线 `4342f7a`。修复三项 P2：

1. Windows 扫描使用独立的提前让出预算，为在途 UIA 调用、registry commit 和授权检查留出外层 deadline 的余量；查询期间的 provider 连接/事务 timeout 为 50 ms，返回 `uia_timeout` 与保留 frontier 的 continuation。预算耗尽不追加可选 Seat provider 读取；调用方取消释放本次扫描，不保留不可达游标。查询之后恢复原有 500 ms provider timeout。
2. sync/bind 显式传递内部 `NoContinuation`，两端后端释放未完成的一次性扫描；无需抢占 16 个调用方续扫槽位，仍返回原有 coverage_incomplete / search_incomplete。COM 释放保留在所属工作线程。
3. host 解码实际 hello 的 `input_policy`，默认模式归一化为 shared_input，并拒绝与可信宿主请求不一致的策略。真实继承子进程测试覆盖默认、shared_input、no_shared_input 与错误策略。

查询 timeout 的 API 依据：[IUIAutomation2 ConnectionTimeout](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation2-put_connectiontimeout)、[TransactionTimeout](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomation2-put_transactiontimeout)。UIA provider 的实际延迟与兼容性仍需 Windows 实机验证，受控 provider 回归不替代实机任务。

## 本地验证

`scripts/check.sh` 通过：Go race/vet、Windows amd64 交叉构建/vet、9 个 JSON 示例和 17 个 JavaScript 测试。新增 Windows 回归测试另已交叉编译/vet；其运行结果由修复提交的 Windows CI 验证。

独立 macOS 实机轮次 `20261004T055641Z-5d5c36`，通过当前未提交修复源码构建 `dtw`，base commit 为 `4342f7a`、vcs.modified=true。helper 哈希/环境/版本见 [summary.json](summary.json)，源码指纹见 [source-sha256.json](source-sha256.json)；保存时逐项验证与本轮源码一致。这不是 `4342f7a` 已发布的二进制。

| 验收 | 结果 | 时间 / 固定场景成本 |
| --- | --- | --- |
| [F1](f1-test.txt) | 后台填写/提交，前台持续 Unicode 输入完整，后台无共享输入；原收据可恢复 | 2.96 s；后台 6 次 SDK 数据调用 / 12,414 bytes |
| [F2](f2-test.txt) | 展开、重复展开和收起验证状态及明细可见性；重复展开无 setter | 0.56 s；6 次 / 12,000 bytes |
| [F3](f3-test.txt) | 12 个慢值 getter 在窄字段读取时为 0，然后完成订单提交 | 1.16 s 含对照；任务 6 次 / 15,052 bytes，对照另 1 次 / 7,365 bytes |
| [扫描所有权实机回归](TestNativeScanCapacityPreservesCursor.txt) | 20 次不完整一次性 sync 不占用扫描槽，16 个调用方游标保留，第 17 个拒绝，第一个仍可 1→2 续扫 | 1.29 s |
| [超时和恢复实机回归](TestNativeObserveTimeoutFixture.txt) | 103.9 ms 返回 ax_timeout 部分覆盖，窄读取恢复，节点/输出预算独立，EndTurn 撤销授权 | 0.66 s |

每项使用新测试应用实例和唯一窗口，业务结果来自应用自身日志。保存的 jsonl 仅包含合成测试内容，去除临时 PID。请求与文本投影字节计量口径沿用 [上一轮记录](../features-20261004/README.md)，未运行 LLM，不声称 token 成本。

Windows 回归通过真实 engine 调用受控慢 provider，检查时间预算先于节点预算耗尽仍交付可消费 continuation，续扫找到远端目标；20 次 sync 和 20 次失败 bind 不积累扫描槽且保留调用方游标；满槽一次性查询、取消和 COM Release 线程另有断言。真实交互 Windows F4/F5 与 UIA F2/F3 仍待独立实机验收。

下一步选择功能补全，优先收齐当前批次的 Windows 证据，见 [交付决策](../../next-stage.md)。本轮不发布 pre-release，不更新 caelis-bot M0。
