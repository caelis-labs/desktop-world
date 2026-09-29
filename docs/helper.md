# Helper 预发布

`desktop-world` 是 SDK 的 stdio 宿主，复用 `local.Open`、Actor、执行器和协议。它不是另一个实现，也不是 MCP 服务；MCP/Agent 工具宿主可在其外层映射调用。当前发布 macOS arm64 开发联调 alpha，采用 ad-hoc 签名，未经 Developer ID 签名或公证。Bot 使用 [managed host 接入](bot-integration.md)；下面的启动授权方式仍供通用 Agent 开发验证使用。

通用 Agent 优先使用随包的 [JavaScript 调用链入口](scripting.md)：持久会话、跨回合 state、观察与动作组合、选择性 print 和自动计量。下面的 NDJSON 是底层协议，不要求每个 Agent 自行编写桥接器。

## 构建与启动

```sh
go build -o bin/desktop-world ./cmd/desktop-world
export PATH="$PWD/bin:$PATH"
desktop-world doctor
desktop-world schema observe
desktop-world schema act

# 由可信宿主选择实际应用名；名称来自桌面 summary。
desktop-world serve --write-app '访达' --write-app '文本编辑' \
  --write-app 'Chrome' --audit artifacts/session-audit.jsonl \
  --assets-dir artifacts/session-captures
```

系统权限不会自动申请；`doctor` 返回当前状态。在开发沙箱中看不到显示器/权限，不代表正常交互式宿主同样不可用。macOS CLI 仍需要 CGO 工具链来构建；用户拿到预编译 helper 后不需要 Go。Windows amd64 构建不需要 CGO。

`serve` 必须保持 stdin 打开。优先使用标准管道。PTY 的 canonical 单行长度可能截断较长 JSON，`stty -echo` 只关闭回显；macOS/Linux 可用 `stty -echo -icanon min 1 time 0` 并在结束后恢复终端配置。不要为每个动作启动新进程。`--write-app` 在启动时解析成当时的应用 Ref，进程重启后不会自动重绑。宿主如果确实需要整个桌面的写权限可显式选择 `--desktop-write`；绝对 Point 还需 `--raw-input`。这两个选项不由请求参数控制。

最初输出一条 hello，包含当前环境和授权摘要。之后每行一条请求，响应可乱序，按 id 关联：

```json
{"id":"inventory-1","op":"observe","args":{"scope":{"desktop":true},"projection":"summary","fields":["name","role"],"budget":{"max_results":64}}}
```

```json
{"id":"inventory-1","protocol":"desktop-world/helper-v0.1","world":"e-...","result":{"objects":[],"coverage":{"complete":true}}}
```

每个响应包含 result 或 error，执行错误也可能同时包含可用的 receipt。省略的 error 表示无错误。无效输入示例：

```json
{"id":"bad-1","protocol":"desktop-world/helper-v0.1","world":"e-...","error":{"code":"invalid_argument","message":"...","retry_class":"never_automatically"}}
```

使用 `schema <operation>` 查看确切结构，不从例子的占位值猜参数。

| 操作 | 参数与用途 |
| --- | --- |
| observe | scope、projection、fields、budget；发现对象与分页 |
| read | target、limit_runes、continuation；读取长文本 |
| sync | cursor、max_output_bytes、wait_ms；有界差量或 reset |
| act | steps、timeout_ms；用外层 id 生成 epoch 内稳定 RequestID |
| capture | kind、target/region、像素预算；导出本地 PNG 路径 |
| get / cancel | run_id；恢复收据或请求终止后续步骤 |

capture 的路径只由宿主启动时选择。observe/sync 的输出预算包含 helper envelope。stdout 仅输出 JSON（`--help` 除外）；启动/命令失败返回非零退出码。正常请求中的业务错误通过逐条 error 表达，不杀死会话。超大输入行会终止会话。不会自动重放失败请求、重建 World 或重新申请权限。

## 边界与指标

- 同一服务最多同时处理两个数据请求与两个收据/取消请求；执行器继续串行仲裁原生写。控制请求不被已占满的数据请求槽阻塞。
- EOF、Ctrl+C 请求取消并给 SDK 有界清理机会。强杀进程不证明输入已清理，也不证明没有副作用。
- 审计只记录 op、id、耗时、请求/响应字节数、返回对象数、访问节点数、coverage、错误码、receipt outcome 和 delivery 分布。默认不保存 UI 文本、键入内容、截图或完整参数。
- 响应 bytes 是接口体积，不能视为 LLM tokens。接入宿主若进一步过滤输出，应分别记录实际呈现给模型的体积。
- 一个 helper 的进程内去重和 Seat 不涵盖其他 helper、其他 SDK 宿主或第三方自动化。初版按一个可信宿主/一个交互式座席运行，不声称系统级互斥。
- 该版本没有持久 exactly-once、自动升级服务、网络监听、OS 沙箱或独立的权限管理员。

随包 Agent 指南见 [SKILL.md](../skills/desktop-world/SKILL.md)。它指导通用操作，不包含验收任务的专用脚本。

## 呈现与作用域消歧

默认 compact 输出将已知 Fact 呈现为 `{"known": value}`，保留 false/0/空字符串；unknown/redacted/unsupported 状态明确保留。逐对象与已知 Fact 的 sample 时间从 Agent 输出省略，coverage 的采样区间、版本、身份、分页与收据不变。`--full-output` 返回原 typed wire 格式。审计同时记录 full_response_bytes 和实际 response_bytes，不能将两者混为模型 tokens。

本机有两个名字都为 Chrome 的应用实例，按名称授权会明确失败。可信宿主可使用 `--write-app-window '一个已观察到的精确窗口标题'` 选中其所属应用；这是应用级授权，后续新对话框仍属同一实例。不会根据相似名称或窗口几何猜测，也不会在实例重启后自动重绑。`--write-app` 可与此选项组合。

当前 summary 默认输出 role/name/app/window；默认全桌面读取 deadline 为 5 秒、局部 2 秒，可显式缩短或增加到最多 10 秒。操作参数可从 schema 按需查询。动作需用焦点 UI 对象 Ref；focus 窗口不等于键盘目标就是窗口本身。
