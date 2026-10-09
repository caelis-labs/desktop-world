# Desktop World

**为 AI Agent 提供原生、可验证的跨平台桌面操作。**

Desktop World 将应用、窗口和 UI 控件转换为可观察、可授权的对象。Agent 可以查找控件、读取文本、填写表单、操作键鼠和获取截图；每次执行返回包含投递结果、验证结果和错误的回执。

提供 `dtw` 命令行、持久 JavaScript 会话、Go / TypeScript / Python / Rust SDK 和动态宿主管理接口。桌面操作使用 macOS Accessibility / CGEvent / ScreenCaptureKit，以及 Windows UI Automation / SendInput / Win32；运行核心无需 LLM、API Key、浏览器扩展或云服务。

当前版本为 **v0.1.0-rc.3**，供集成测试使用。提供 Windows x64 与 macOS arm64 安装包、对应源码及 SHA256 校验文件；下载和验收记录见 [Releases](https://github.com/caelis-labs/desktop-world/releases) 与 [Chrome 勾选框修复 Issue](https://github.com/caelis-labs/desktop-world/issues/22)。版本的实际覆盖以该提交的验收记录为准。

## Agent Plugin 安装路径（v0.1.0 发布候选）

首个正式版候选每个平台仅提供 **Full（默认推荐）** 与 **Lite** 两个同版本官方归档。两者共享 `plugin.json`、`mcp.json`、同一份 Skill、预构建官方 MCP SDK、JavaScript runner 和同提交原生 `dtw`；Full 固定随包 Node 24 LTS，终端用户无需系统 Node/npm/Go/Python。Lite 只省 Node，须由用户或宿主显式设置兼容 Node 24.x 的绝对 `DTW_NODE_PATH`；启动时检查，缺失或不兼容会明确报错，不静默下载或修改系统环境。默认只有 `desktop_exec` 与 `desktop_status` 两个 MCP 工具。`desktop_exec` 执行本地 JavaScript，须保留客户端对完整脚本的审批；`node:vm` 和 worker 不是安全沙箱。包未正式发布前，不要把 rc.3 的 `dtw serve/session` 登记为 MCP。

将以下提示词复制给本机 Agent 安装正式发布后的固定版本包：

> 请为当前本机 Agent 安装 [caelis-labs/desktop-world](https://github.com/caelis-labs/desktop-world) 的官方 Desktop World Agent Plugin。先读取 README、对应正式 release 说明和包内 SKILL.md，确认本机 OS/架构与当前 Agent 的 Plugin、MCP、Skills 安装方式。默认选择对应平台固定版本 **Full** 归档，对照官方 `SHA256SUMS` 校验后解压到稳定目录；只有我明确选择 Lite 且提供兼容 Node 24.x 的绝对路径时才用 **Lite**。若正式官方包不存在，请报告缺口并停止，不自造 MCP 桥。优先用 Agent 的本地标准 Plugin 安装入口；若客户端只支持 stdio MCP 与 Skills，使用同一包的绝对路径登记 MCP 服务并安装同一份 Skill，合并现有配置。Full 不要求系统 Node、npm、Go、Python 或 Docker。先运行包内 `dtw version`、`dtw doctor`、`desktop_status` 和一次只读观察，报告版本、Skill 与 MCP 是否分别加载、APP 授权和 OS 权限缺口。保留脚本审批，只在我明确批准的 APP 范围内操作，不自行给 OS 授权或放宽自动批准。若当前 Agent 在云端、SSH 或 WSL 中，先确认服务实际运行于目标本机交互桌面。最后给出一句可直接开始桌面任务的示例指令。

标准包自动安装时，客户端应按 Agent Plugins 1.0 提供 `PLUGIN_ROOT` 和持久可写 `PLUGIN_DATA`。仓库保留 [标准 Plugin 目录](packaging/plugin/plugin.json) 的 manifest、Skill、MCP 源码定位和安装指引；仓库/市场安装仍须定位、校验匹配版本的 Release 运行归档，不要求终端用户自行编译。只支持分开安装的客户端可按 [包内 README](packaging/plugin/README.md) 登记绝对路径；只登记 MCP 不等于 Skill 已加载。动态授权通过可信 owner descriptor 的 `dtw auth` 完成，模型工具参数没有授权入口。Plugin 默认保留通用共享输入功能：用户授权 APP 指针动作后，原生点击会移动真实箭头；需要完全禁止共享指针输入的宿主可显式设置 `no_shared_input`。包内 cursor overlay 在成功投递的 Agent 指针位置重绘青蓝、淡紫和浅粉的小纸飞机标记（无外围圆圈），闲置五秒后隐藏；它不是独立输入设备，不以截图像素或虚拟坐标代替真实输入。完整生命周期和 Bot 宿主迁移依赖见 [Plugin 集成契约](docs/agent-plugin-host-contract.md)。

## 功能

| 能力 | 用途 |
| --- | --- |
| 原生对象发现 | 按应用、窗口、名称和角色定位控件；按需读取值、状态、能力和几何信息 |
| 语义操作 | 调用、设值、展开/收起、勾选、选择和滚动到目标；依控件公开的能力执行 |
| 键鼠操作 | Unicode 文本、组合键、点击、移动、拖拽、滚轮；`primary` 自动映射 Command / Control |
| 短前台事务 | `cooperative` 模式在一份短计划内使用前台，结束时恢复原窗口、焦点和未被用户改变的指针 |
| 截图 | 获取可见桌面区域或独立窗口 PNG，保留坐标变换和图像预算 |
| 有界观察 | 字段投影、范围、节点/输出预算、分页及 AX / UIA 续扫，明确报告不完整覆盖 |
| 执行回执 | 分别记录 delivery 与 verification；查询、取消及相同请求的去重与恢复 |
| 宿主管理 | 按应用授权、每回合撤销权限、独立控制管道、后台输入策略和元数据审计 |

控件是否支持某项操作，以观察返回的 `capabilities` 为准。不支持的语义动作返回明确错误。网页 Canvas 的像素不会被识别成原生按钮。

## 平台与运行要求

| 平台 | 构建要求 | 已有实机覆盖 |
| --- | --- | --- |
| Windows 11 x64 | Go 1.23+，原生后端无需 CGO | Chrome、Windows 记事本、计算器、Win32、Electron |
| macOS 14+，arm64 / amd64 | Go 1.23+、CGO、Xcode Command Line Tools | arm64 上的 AppKit、Chrome、WebKit、Electron、TextEdit；最低系统版本与 amd64 实机仍待扩展 |

真实操作需要已登录的交互式桌面。macOS 需授予宿主 Accessibility、输入及屏幕捕获权限；`dtw doctor` 只检查状态，不弹出权限申请。Windows 受 UIPI、前台切换规则和应用 provider 限制，建议以普通用户权限运行，并操作相同或更低权限的应用。

JavaScript 入口另需 Node.js 20+，无 npm 依赖。其他平台可运行协议及内存示例，原生桌面入口返回 `platform_unsupported`。Windows arm64 暂不支持。

## 安装与检查

从 [Releases](https://github.com/caelis-labs/desktop-world/releases) 下载对应平台的归档和 `SHA256SUMS`，校验后解压到可信目录。直接运行包内 `bin/dtw.exe`（Windows）或 `bin/dtw`（macOS）的 `version`、`doctor` 和 `schema`；使用预编译 helper 无需安装 Go。Python / Rust 客户端无需 Node，TypeScript 与 JavaScript 客户端需要 Node.js 20+。包内 `clients`、`docs` 与 `source` 对应 manifest 中的同一提交。

从源码构建 Windows 版本：

```powershell
$env:GOWORK = 'off'
go build -o bin\dtw.exe ./cmd/dtw
.\bin\dtw.exe version
.\bin\dtw.exe doctor
.\bin\dtw.exe schema
```

macOS：

```sh
export GOWORK=off
go build -o bin/dtw ./cmd/dtw
./bin/dtw version
./bin/dtw doctor
```

`version` 显示构建版本、平台和协议；源码构建默认标记为 `dev`。历史预发布及校验文件见 [Releases](https://github.com/caelis-labs/desktop-world/releases)。

## 外部 Agent 接入

按编排语言选择 [TypeScript](clients/typescript/README.md)、[Python](clients/python/README.md)、[Rust](clients/rust/README.md) 或 [Go host](docs/bot-integration.md)。完整接入契约、动态授权和停止/恢复流程见 [Agent 接入指南](docs/agent-integration.md)。Python/Rust 直接启动原生 helper，无需 Node。

`--write-app` 可声明未启动 APP；可信宿主在运行期间追加/撤销授权。使用 `dtw session --session harness/owner.json`，通过 `dtw auth list|add|revoke` 管理同一会话。声明只有在完整发现证明唯一时才绑定一个 APP 实例，APP 重启需重新授权。

## 快速开始：持久 JavaScript 会话

需要持久脚本工具的 Agent 可使用 JavaScript 入口：一个原生 helper 跨调用保留对象 Ref、观察和回执，脚本中间结果留在本地，只输出需要的信息。

创建 `host.json`，将 helper 路径及窗口标题替换为本机实际值。宿主通过精确窗口标题授权其所属应用：

```json
{
  "helper": "D:/WorkDir/desktop-world/bin/dtw.exe",
  "args": [
    "serve",
    "--input-mode", "cooperative",
    "--write-app-window", "当前精确窗口标题",
    "--assets-dir", "artifacts/captures"
  ]
}
```

在终端一启动并保持会话：

```powershell
node clients/javascript/desktop.mjs serve --host host.json
```

在终端二观察桌面：

```powershell
@'
state.inventory = await dw.observe();
print(dw.list(state.inventory, ['ref', 'kind', 'name', 'app']));
'@ | node clients/javascript/desktop.mjs exec
```

从观察结果选定窗口 Ref，在该窗口内定位控件：

```javascript
// 将 Ref 替换为本会话观察得到的窗口 Ref，控件名称来自实际 UI。
state.windowRef = '替换为已观察窗口的 Ref';
state.fieldObservation = await dw.find(state.windowRef, {
  role: 'text_field', name_equals: '内容'
});
state.field = dw.one(state.fieldObservation, {role: 'text_field'});
await dw.set(state.field.ref, '你好，Desktop World 🌍');
print(await dw.read(state.field.ref));
```

将上述 JavaScript 通过同样的 PowerShell here-string 传入 `exec`；macOS 可使用 heredoc。只读会话可在 `host.json` 中仅配置 `args: ["serve"]`。每轮使用新的会话日志目录，具体配置、分页和脚本预算见 [JavaScript 使用指南](docs/scripting.md)。

结束会话：

```powershell
node clients/javascript/desktop.mjs stop
```

## 输入模式与完成条件

宿主在启动时选择模式和权限上限：

| 配置 | 行为 |
| --- | --- |
| `--input-mode shared` | 默认模式；显式聚焦后操作当前焦点对象，窗口保持当前前台状态 |
| `--input-mode cooperative` | macOS / Windows 的短前台事务；计划结束后归还前台，回执含占用时间和恢复状态 |
| `--input-policy no_shared_input` | 保留可用的语义操作；包含聚焦或共享键鼠的整份计划在产生效果前被拒绝 |

一份计划最多 16 步、10 秒。协作模式采用一秒输入预算，单次文本最多 256 个 UTF-16 单元，拖拽最多 500 ms；原生调用和清理可能超过输入预算。长任务拆为多份已知的短计划，推理和网络等待在事务外完成。Windows 拒绝前台切换时返回 `needs_user_focus`，需要先将目标窗口置于前台。

`focus`、`set_value` 和状态设置动作自动验证。其他动作默认只确认输入投递；需要等待界面结果时，**同时设置 `completion: "verify"` 和 `after`**：

```javascript
await dw.act([{
  op: 'keyboard.type_text',
  target: {ref: state.field.ref},
  type_text: {text: 'Windows 表单验证 🌍'},
  completion: 'verify',
  after: [{
    target: {ref: state.field.ref},
    property: 'value',
    equals_string: 'Windows 表单验证 🌍'
  }]
}]);
```

保存文档、提交表单等任务还应检查文件、应用回调或业务结果。回执中的 `completed` 表示声明的完成条件已满足。详见 [输入事务](docs/cooperative-input.md) 和 [语义动作](docs/semantic-actions.md)。

## Go SDK 与宿主接入

模块路径为 `github.com/caelis-labs/desktop-world`。直接嵌入原生 World：

```go
world, err := local.Open(ctx, local.Options{
    InputMode: desktopworld.InputModeCooperative,
})
if err != nil { return err }
defer world.Close(ctx)

actor, err := world.NewActor(ctx, desktopworld.ActorConfig{
    ID: "inspector",
    ReadScopes: []desktopworld.Scope{{Desktop: true}},
    Operations: []string{"observe", "read"},
})
if err != nil { return err }
observation, err := actor.Observe(ctx, desktopworld.ObserveRequest{
    Scope: desktopworld.Scope{Desktop: true},
    Projection: desktopworld.ProjectionSummary,
    Fields: []string{"name", "role"},
})
```

写入需要单独授予目标范围及动作。空权限表示没有权限。宿主应用建议使用 Go `host` SDK 启动独立 `dtw`：按回合授权、独立控制通道、取消和原回执恢复，同时隔离原生 provider 的阻塞。接入步骤见 [helper 协议](docs/helper.md)、[宿主集成](docs/bot-integration.md) 与 [示例](examples)。

`dtw schema act <action>` 按需获取单个动作的参数；`dtw serve` 使用逐行 JSON 标准输入/输出，无网络监听端口。协议严格验证字段、类型与预算。

## 验证与故障处理

Windows 自动检查：

```powershell
.\scripts\check.ps1
```

Windows 真实任务验收需要打开 Chrome，并传入其当前精确窗口标题；脚本会创建自己的本地表单、记事本文档及测试应用：

```powershell
python scripts/accept-windows.py --browser-window '当前 Chrome 窗口标题 - Google Chrome'
python scripts/accept-features.py F4 F5
python scripts/accept-windows-stability.py --electron 'C:\path\to\electron.exe' --rounds 32
python scripts/accept-rc2.py --helper 'C:\path\to\package\bin\dtw.exe'
python scripts/accept-rc2-package.py --package 'C:\path\to\package'
```

检查结果保存在新的 `artifacts/windows-acceptance-*`、`windows-stability-*` 和 `feature-acceptance-*` 目录，包括回执、应用事件、磁盘文件、PNG 和源码/二进制哈希。Electron 验收需要独立的 Electron runtime，本轮使用 44.5.1。计算器及记事本标签因语言而异，参数和覆盖范围见 [Windows 验收报告](docs/windows-validation.md)。macOS 使用 [原生 fixture](docs/validation.md) 和 `scripts/check.sh`。普通 `go test ./...` 默认跳过真实桌面操作。

从干净提交生成包含 helper、源码、文档、许可和 SHA256SUMS 的本地预发布包：

```powershell
# Windows amd64，PowerShell 7
.\scripts\package-prerelease.ps1 -Version v0.1.0-rc.3
```

macOS arm64 使用 `./scripts/package-prerelease.sh v0.1.0-rc.3`。脚本不创建 tag 或发布 Release；包的 manifest 必须与待验收提交一致。包及源码使用 MPL-2.0，Windows 包未签名，macOS 包使用 ad-hoc 签名且未经公证。

遇到错误时保留原回执：

- `coverage.complete=false`：缩小范围、减少字段或消费 continuation；不能从零匹配推断目标不存在。
- `sync` 返回 `reset_required`：重新取得对应范围的 snapshot；过期或被基线缓存淘汰的游标不再返回差量。
- `ref_gone` / `ref_stale`：重新观察，确认新对象身份后再制定新计划。
- `requires_shared_input`：当前宿主只允许后台语义操作，需要由宿主选择合适的任务或输入策略。
- `partial` / `unknown`：查询原 RunID，核对业务结果；相同请求重试必须保持 ID 和正文不变。
- `seat_health=fenced`：输入清理或恢复无法确认，停止写入并由宿主处理；不要换 ID 重放未知动作。

同一进程中的相同 Epoch / Actor / RequestID / 计划只执行一次。新 helper 意味着新 Epoch、旧 Ref 失效；去重记录不跨进程持久化。UI 文本始终作为不可信数据处理。

正常结束回合时先撤权，再核对原回执和应用结果，最后关闭会话。在途请求遇到 stdin/控制断开可能返回 `session_unknown`，关闭后的 helper 无法再查询原 run；应保留原请求及审计，用应用结果确认已发生的效果，不能通过新会话重放。

## 兼容性边界

原生控件、浏览器及 Electron 的能力取决于应用自身的 Accessibility / UIA provider。Windows 独立窗口截图使用 `PrintWindow`，已验证 Chrome、记事本、计算器、Win32 和 Electron；隐藏或最小化窗口明确拒绝。支持原生复选框、多选列表、树节点展开/收起及列表项、树节点的语义滚动，具体能力以观察结果为准。动作结果不确定时不会自动改成键鼠重试。其他 GPU、受保护或无响应窗口仍可能不能正确渲染。截图不包含鼠标光标。

当前未提供 OCR、视觉定位、自动重绑、剪贴板输入后备、独立物理键鼠或系统级输入隔离。变化同步使用刷新与轮询。多屏混合缩放、RDP、锁屏/用户切换、更多 IME、Windows arm64 及全部 macOS 机型尚未形成完整实机矩阵。详细边界见 [实现说明](docs/implementation.md)。

## 许可

Copyright © 2026 Caelis Labs. 本项目采用 [Mozilla Public License 2.0](LICENSE)，许可通知见 [NOTICE](NOTICE)。第三方组件的原许可和声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
