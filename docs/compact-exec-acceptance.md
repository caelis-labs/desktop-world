# 默认执行摘要切片验收

日期：2026-10-28。基于 #62 合并后的 `e863a103e86550c899d87a04d6875ef4b7bd8d3d`。运行代码提交 `51d07c3d9c7c5a13cf7581062985bdf7bc684853`；已测二进制 SHA-256 为 `dd164c56272524fa03e0a1abbe976bd977b516a9f2871bde9d2008be3a11bba7`。后续提交只补验收说明、产品矩阵和测试探针的计数诊断，未改变已测运行代码。

这是第一批“通用基础与发现”的首个交付切片：公开说明统一为 `exec({app, source})`，普通执行默认返回小摘要，完整原始记录通过现有 `result` 读取。没有新增工具参数或原子动作。WindowSurface/语义绑定分离、过滤/分页、API 按需发现仍是同批后续工作，不能把本切片视为整批完成。

## 实现与保持的语义

投影只发生在 MCP 出口，按请求的工具名选择。正常完成、在途重复和完成缓存都经过同一出口；`result` 不投影。原始记录依然由原有 Server/Core 保留，不建立第二份执行状态。

`exec` 省略内部 app_ref、冗余 operation 和空 receipt，用 `script.summary` 替代 `script.steps`。保留用户 value、console、最后截图、isError、失败、状态提交、上下文/实例重建及 payload 驱逐标记。摘要累计所有动作的派发、证据、原生 provider 事实和中断；`last_issue` 仅为最后注意项，后续只读错误不会抹掉较早 unknown 的累计数量。脚本捕获动作错误后可以成功返回，摘要仍报告错误/未知效果，完整原账本可查询。合同见 [MCP 协议](app-protocol-v1.md#结果效果与图像)。

这不是所有响应的硬字节上限。用户显式返回的大对象和原有有界 console 不会被投影再次截断；图像保持原样。较短的失败执行保留诊断样本与累计计数后可能比原记录略大。

## 自动检查

| 检查 | 结果 |
| --- | --- |
| Cargo `--all-targets --locked --offline` | 208 项库测试、4 项 CLI、3 项真实 stdio 进程测试通过 |
| `scripts/check-source.sh` | 格式/版本、41 项 Core、20 项 IPC、12 项 Python 检查通过 |
| 新增针对性回归 | 5 项真实 rmcp 内存 I/O 测试、2 项投影保留测试；扩展现有 PNG/console/value 和 discovery 检查 |
| 效果边界 | caught error、无 error 的 unknown、mismatch、较早 unknown 后的新错误、取消与迟到原生 mismatch/error 同时保留 |
| 原记录与传输 | text JSON 与 structuredContent 一致；查询完整账本不新增原生调用；相同 RPC 重送不再次派发；图片和值保持一致 |

保留两项现有轻量 CI，不增加完整 CUA 编译。受控 provider 测试证明结果合同与交错语义，不能代替真实原生效果。

## 输出量

比较同一执行的 exec JSON 与 result JSON，均为紧凑 UTF-8 编码，不含 MCP 外壳或 PNG。没有为比较而重放动作。

| 场景 | 默认摘要 | 原完整记录 | 变化 |
| --- | ---: | ---: | ---: |
| 受控 SDK：25 次只读调用，返回小对象 | 447 B | 3566 B | 减少 87.5% |
| 真实 GUI：发现、观察、press、独立观察，保留较大的脚本返回值 | 2971 B | 4214 B | 减少 29.5% |
| 真实 GUI：发现并截图 | 896 B | 1887 B | 减少 52.5% |
| 真实 GUI：一次像素点击 | 799 B | 993 B | 减少 19.5% |
| 真实 GUI：一次陈旧引用拒绝 | 1221 B | 1145 B | 增加 6.6%，保留完整诊断样本和计数 |

## 自有窗口 GUI

第一轮在语义点击与像素点击后，测试窗口计数已达到 2；取消/恢复阶段最终发现计数为 3，因而失败。原始账本只有两次已派发动作，取消和恢复脚本仅包含只读调用。没有收集额外事件的来源，原因尚未查明，不能归因为用户输入或 DTW，也不能声称该异常已修复。失败记录和正常退出结果一并保留。

随后只为探针补充取消前后计数快照，使用**相同 DTW 二进制、全新 fixture**复核。该轮通过，准备、执行和清理共 **11.181 秒**：

- 语义 press 计数 0→1、前台无遮挡窗口的像素 click 1→2；重送原 RPC 不重复执行。
- 陈旧引用、其他 app 引用、不存在的 app 拒绝；失败结果与原记录一致。
- PNG 640×504；所有普通 exec 的摘要与随后读取的原账本逐项核对。探针为审计添加的 result 查询不属于模型正常流程。
- 取消前后目标计数均为 2、对照 app 均为 0；原记录不变、已提交 state 保留、原生 refs 重建。
- A→B→A 保留状态和窗口；两个 MCP 会话独立 state、不同 app 并发截图，另一会话退出后仍可使用原会话。
- 两个 MCP 会话及其 native workers、两个 fixture 全部正常退出；后续进程检查无本轮残留。

本轮通过证明上述限定样例；首次额外计数事件仍是未归因的验收限制。未重跑真实应用闭窗/Space 矩阵，不扩张平台或后台输入声明。

## 普通 Claude Agent

Claude Code **2.1.312**、模型 **codex/gpt-6.1-terra**，使用本地 stage 的标准插件/Skill 与 stdio MCP。未修改用户配置，无定制 Host。

调用顺序 **Skill → apps → exec → exec**，两个 exec 入参都仅包含 app/source，分别调用 2、4 个原子方法。完成一次 press、独立观察 Count 0→1、640×504 截图；fixture 的 actions_started 与 clicks 均恰好为 1。模型收到摘要格式，无工具错误、重复动作、额外 result 查询或轮询。

CLI 流程 **25.992 秒**，API **25.059 秒**，外层墙钟 **27.782 秒**。调用步数与此前样例一致；单次模型运行不能证明延迟改善。CLI/fixture 正常结束，专属进程无残留。

![普通 Claude MCP 返回的真实窗口截图](evidence/compact-exec-20261028/claude-count-one.png)

[脱敏结构化证据](evidence/compact-exec-20261028/summary.json)保留测量、失败与复核结果，不包含用户运行应用清单。原始记录在 `/private/tmp/dtw-compact-gui-20261028/`、`/private/tmp/dtw-compact-gui-repeat-20261028/`、`/private/tmp/dtw-compact-claude-20261028/`。本轮未重跑 agy、公开发行包、其他平台或新增原子能力；下一步仍按[产品矩阵](product-matrix.md)完成同批基础窗口与发现工作。
