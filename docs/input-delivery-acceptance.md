# 输入策略与投递回执：检查点 1 验收

本轮基于 main `0935778d2537536e95c7e9012715999b1d6d5ab6`，只完成服务输入策略、投递回执和由新增生命周期验收发现的退出清理修复。保持标准 stdio MCP 与 `exec(app, source)`；光标覆盖层、滚动锚点留给后续检查点。

最终本地签名候选 S 的二进制 SHA-256 为 `aa6b32b7ad1d0c239c9e6bf0dcfde2fe06fa281657d28c6d22eae4a6f3231ace`。其 `source.json` SHA-256 为 `aa65db58a8f2ad3f26e0bcb963edce8c1c7105e25865a816024a26769c8a188c`；64 个源码及构建文件逐项与检查点 1 验收时的源码（提交 `e5525d8`）匹配，GUI 样本使用同一二进制。

## 本轮契约

- `dtw mcp --input-policy auto|background-only` 是服务启动配置，默认 `auto`。`apps` 与 `api` 返回 `service.input_policy`；筛选单个方法的 `api` 也保留该配置。执行工具仍只接收 `app` 与 `source`，不新增逐次投递参数或审批流程。
- `auto` 优先采用语义或后台路线；只有尚无准备及输入副作用的预检拒绝才可改用前台。已准备、已投递或效果不明时不改道重放。
- `background-only` 在需要前台准备或物理回退时返回 `foreground_required / not_dispatched`，投递路线为空、`foreground_borrowed=false`。它约束 DTW 主动请求前台，不能承诺语义动作不会触发应用自己的界面变化，也不替代 Harness 授权。
- 原生回执按实际路径提供可选 `delivery`：路线、原因、是否借用前台及归还结果。运行时中断包装、原始账本和 compact 输出保留这些证据；提供方未给出的信息不补造。
- 显式 `app.focus()` 的动作回执保持 `focus_return=pending`，脚本终结后的 `script.focus_return` 单独报告结果。取消后 worker 已停止而仍可能持有借用时报告 `unconfirmed`；退出成功和独立桌面观察均不能把它改写为 `restored`。

## 同一 S 候选的验证

321 项 Rust 库测试通过。轻量 `scripts/check-source.sh` 退出码为 0，其中 Python 各组为 6、2、4、26、47、20、29、4 项，共 138 项；本轮相关四组为 100 项。源码哈希复核与 `git diff --check` 通过，没有完整重编译上游 CUA。

| 真实标准 MCP 场景 | 可核验结果 |
| --- | --- |
| Chrome / 默认 `auto` | 普通文本、遮挡坐标点击、默认滚动报告后台路线；菜单快捷键和 supplementary Unicode 文本报告前台路线，两次借用均返回 `restored`。独立页面读回文本正确、保存恰好 1 次、滚动从 0 到 107。 |
| Chrome / `background-only` | 显式 focus、需要前台的快捷键及 Unicode 文本共 3 次派发前拒绝。普通文本、遮挡点击和滚动仍走后台；Unicode 替换通过 `setValue` 走语义路线。页面保存恰好 1 次，滚动从 0 到 158.5。 |
| AppKit / 显式与重复 focus | 一次、两次及同脚本 primary→sibling→primary 三组都保留原动作 `pending`，终结报告 `restored`；独立确认原 PID 与原窗口，按键配对无残留。 |
| 显式 focus 后取消后台长文本 | 单独样本及完整生命周期中的取消均通过：只提交一次、保留部分前缀、不执行后缀或重放、按键释放、精确原窗口归还。动作仍为 `pending`，可查询的终态为 `unconfirmed`，没有 shutdown error。 |
| 显式 focus 后 EOF / parent exit | 完整生命周期三项全部通过；2140 字符输入在中断前分别留下 45 字符前缀，单次派发、无重放、释放配对、精确原窗口归还及 worker 回收均核验。EOF 另存已收到的终态响应；parent exit 没有终态账本，不补造完成记录。 |

后台效果来自独立 owned AppKit/Chrome 页面事件与状态读回；`dispatched` 本身仍只是投递证据。桌面采样有有限间隔，不能排除更短的变化，也不用于推断是谁改变前台。

## 新增生命周期路径定位到的清理阻塞

R 的“显式 focus → 后台长文本 → 取消”连续样本在归还激活阶段超过 500 ms，最终由父进程 SIGKILL；原回执保留 `pending`、终态 `unconfirmed` 与 shutdown error。未将其归因于桌面使用者。

未暂停父进程的线程采样同时捕获：主线程停在第二次 `shutdown_worker` 的 `park_timeout`，watchdog 停在 `releaseFocus → NSRunningApplication.activateWithOptions → _yieldToApplication → NSCondition`。主线程停车使 AppKit 归还无法推进。

修复只让第二个 shutdown 调用者中的主线程按 10 ms tick 服务默认 CFRunLoop；非主线程继续等待，唯一清理 owner 仍负责关闭输入、持锁归还及退出。派发结束后在下一次 stdin 读取前检查终止标志。没有修改 AppKit 归还语义、引入第二个清理 owner，或延长 400 ms 清理 / 500 ms 强退预算。S 的取消、EOF 与 parent exit 已重新验证这条路径。

## 保留的失败证据

| 原始样本 | 结论 |
| --- | --- |
| Q / strict | 新增语义用例缺少 `desktop` 聚合字段，探针最终 KeyError。不能把已经执行的局部路径改写成整项通过。 |
| R / focus | sibling 用例精确返回原应用及窗口，但原生回执为 `changed_externally`；旧探针错误地把最终采样等同于连续所有权证明。现分别验证实际返回和原生证据，仍拒绝错误窗口，不更改原生通知判定。 |
| R / focus-cancel 与 control | 实际清理停滞、SIGKILL、未归还样本保留；其修复依据来自线程栈，不能当作探针误判处理。 |
| R / sample 系列 | `notify` 参数、worker 选择和外部采样超时等诊断工具失败分别保留；sample5 提供上述同时停滞的线程栈。所有原始失败状态与哈希均收录，不以 S 通过覆盖。 |

原始记录留在本机 `/private/tmp/dtw-input-observability-*`；仓库只收录[脱敏证据、回执事实与源码哈希](evidence/input-delivery-2026-10-30.json)，不提交本机应用库存、进程列表、窗口截图或无关桌面事件。

本轮没有 Windows/Linux GUI、agy/Claude 外部模型重跑、安装包、公证或 release 验收。Windows/Linux 的策略及回执属于源码实现边界，Linux 桌面仍不作支持承诺；光标覆盖层和滚动锚点尚未进入本检查点。
