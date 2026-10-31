> 本文仅记录旧 CUA 版本的历史验收，不适用于重写版。当前 Windows 验收入口是[重写版矩阵](../tests/acceptance/windows/README.md)，请使用与当前源码一致的候选包。

# Windows acceptance — 2026-10-11/12

> Historical evidence: this report applies only to its named source revision, binary and entry point. The current product uses [standard MCP and explicit app IDs](app-protocol-v1.md); any grant, manifest, SDK/worker or custom Host setup below is superseded and is not a current installation instruction. No acceptance result here is claimed to have been rerun for the new entry.

> Historical checkpoint record. Build flags, default entry points and release plans below belong to the recorded revision. The current rewrite-only build is described in [building](building.md); this evidence does not validate a newly built binary.

> Historical Windows evidence: the implicit Runtime context and CUA 0.35.4 candidate in this worktree have not been rebuilt or GUI-validated on Windows. See [runtime acceptance](runtime-acceptance.md).

**结论：Windows 全面验收未通过。** 本机工具链已安装，源码构建、插件独立安装、MCP 协议以及常用原生/Chrome 操作有通过证据。主线的两处 DTW 适配问题已在本地修复；固定 CUA 源码中的 DPI 截图问题和文件选择器路径仍阻塞完整验收。该记录不代表生产发布或干净机器验收。

## 基线与交付物

PR [#32](https://github.com/caelis-labs/desktop-world/pull/32) 已合并。验收基线为最新抓取的 `main`：`b048ad68d40c4797b761cb14f95808d7d335f5d0`，其 [CI run 38153435126](https://github.com/caelis-labs/desktop-world/actions/runs/38153435126) 已成功。最初测试的 PR head 为 `76032075e51e3c7d58469cc2126816fd372d3f9d`，两者源树相同。CUA 固定于 `ac967b9006aafa10870aadb5a645b038cba9721a`；本地 CUA checkout 保持干净，未修改或更换 pin。

本地分支：`codex/windows-cua-acceptance-20261011`。以下适配修复、验收脚本和文档为本地改动，生成初次记录时尚未推送；后续责任归属及发布门禁见 [release readiness](release-readiness.md)。

| 交付物 | 本地位置 |
| --- | --- |
| 原始 PR 构建包 | `D:\WorkDir\desktop-world\artifacts\windows-cua-20261011\package-baseline` |
| 本地修复后的最终验收包 | `D:\WorkDir\desktop-world\artifacts\windows-cua-20261011\package-acceptance` |
| 原始回执、截图、应用日志和构建日志 | `D:\WorkDir\desktop-world\artifacts\windows-cua-20261011` |

最终包的 adapter SHA-256 为 `f51757d52ad1a1fd333d8c165d2d70dbdefb308e1825b837d0fb2dde957c2518`；worker 为 `c8414baba6f310174776b0538be42530e71eedbeda1ded98335fa3e518d21cb4`。`package-final` 与最终包两个可执行文件的哈希相同；最终包再次通过正式打包脚本生成，使用已经从固定源码编译、并校验哈希的 worker，纳入最终 Skill 文档。所有 12 项 `SHA256SUMS` 均通过。

## 工具链与运行依赖

测试机器为 Windows 11 家庭中文版 x64，版本 `10.0.22631`，显示缩放为 125%。环境记录在 `environment.json`。

| 工具 | 本次状态与用途 |
| --- | --- |
| Rust / Cargo | 新安装 `1.98.0`，目标 `x86_64-pc-windows-msvc`，含 rustfmt；源码构建 |
| VS Build Tools 2022 | 新安装 `17.14.41`，MSVC `14.44.35207`、SDK `10.0.26100.0`；源码构建 |
| Spectre libraries | 补装 `Microsoft.VisualStudio.Component.VC.Runtimes.x86.x64.Spectre`；缺失时固定依赖 `msvc_spectre_libs` 会终止构建 |
| PowerShell 7 | 安装 `7.6.6`；实际构建调用本机已有的 `7.6.5`；构建/打包脚本需要 |
| Python / Pillow | Python `3.11.9` 已存在，新安装 Pillow `12.3.0`；验收与 PNG 几何比对 |
| Chrome | 已存在 `155.0.8059.39`；使用新建隔离 profile 验收 |
| C# 编译器 | 系统 .NET Framework `csc.exe`；编译拥有独立事件日志的 WinForms 测试应用 |
| VC++ x64 runtime | 系统已安装 `14.51.36247.0`；两个二进制依赖 `VCRUNTIME140.dll`，已通过 dumpbin 确认 |

PowerShell 7 **不是插件运行依赖**。MCP 宿主直接启动 `dtw.exe`，它再启动包内 `cua-driver.exe`；运行不需要 Rust、MSVC 工具、Node 或 Python。固定 worker 的 integrity-level 诊断调用系统自带 Windows PowerShell 5.1 `powershell.exe`，与 PowerShell 7 分开。本次运行测试将 PATH 限制为：

```text
C:\Windows\System32;C:\Windows;C:\Windows\System32\WindowsPowerShell\v1.0
```

该环境没有 PowerShell 7、Rust、Node 或 Python 的 PATH 条目。VC++ runtime 仍由系统提供；本地未在空白机器上验证安装器，也未将 redistributable 打入包中。可分发机器需满足 [Microsoft VC++ runtime 要求](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist)。

## 已通过的检查

所有桌面输入与 UI 读取均经 DTW 的唯一 `exec`，没有使用 CDP、DOM 填充、剪贴板或另一套桌面控制器。宿主只负责启动自有临时进程、HTTP 测试页、读取测试应用日志/保存文件、编译 fixture 和清理自己启动的进程。GUI 测试顺序运行，使用确切 PID/window ID，不操作用户现有文档或浏览器 profile。

| 范围 | 验证结果与证据 |
| --- | --- |
| 从固定源码构建 | adapter、worker 完整编译成功；`build-spectre.log`、`build-final.log`。最终封装见 `build-acceptance.log` |
| 本地适配回归 | 3 个 Rust 测试通过，覆盖 Windows 精确前台 HWND、错误/缺失 HWND 不误报、macOS 字段保留；`adapter-tests-final.log` |
| 静态检查 | rustfmt、Python compileall、`git diff --check` 通过 |
| MCP 与 Session | 仅暴露 `exec`；计数为 `1,2,1`，Session 隔离、默认 Session、错误保留、大结果分页通过；`probe-final.log`、`contract-final.log` |
| 权限诊断 | `status:ok`，integrity 为 `Medium` / RID `8192`，非提权，UIA 与 PostMessage available；`probe-final.log` |
| 结果边界 | 100,120-byte 结果以 32,768-byte 分页；外部 Session 的 receipt lookup 被拒绝；原始执行可查询且不重复提交；`contract-final.log` |
| native pass-through 边界 | menu/desktop 参数校验、无效窗口拒绝、`run_script` 禁止调用 `get_desktop_state` 通过；`native-ops-final.log` |
| DPI 感知原生窗口 | discovery、PNG、精确焦点、中文/emoji/多行前台输入、一次语义提交、Ctrl+S 保存并独立磁盘读回、checkbox、截图点击、capture ID 重用拒绝通过。最终包的 `windows-live-0af2dac3/summary.json` 为所选 core 范围 PASS（5 轮设值及菜单，picker 明确 SKIP） |
| 原生稳定性与菜单 | 20 轮 `setValue` 的 UIA 读回及测试应用 Ctrl+S 磁盘读回通过；原生 `File → Record menu` 恰好一次应用回调。见 `windows-live-9896c550/summary.json` 中通过的独立 checks；该次整体因随后 picker 检查失败而为 FAIL |
| Chrome | 原生 UIA、中文/emoji/多行键盘输入、可信 submit 事件与 UIA 标题读回、800×643 PNG 坐标点击、capture ID 重用拒绝、5 轮重复输入/提交、可信滚轮与 scrollY 变化通过；`windows-chrome-bbb25df5/summary.json` 整体 PASS |
| Chrome PNG 几何 | 自有绿色按钮的像素边界与 UIA screenshot_frame 在 3 PNG-pixel 容差内；降采样边界允许颜色混合，使用独立颜色 mask，不使用 DOM 几何 |
| 独立安装 | 含中文和空格路径的通用插件 copy，12 项校验和一致；包外 cwd 可启动私有 worker；重复安装 exit 2 且未覆盖原目录；`package-installation.json` |
| Antigravity 项目安装 | 隔离项目 manifest、单一 MCP server、绝对 `.exe` 路径、独立启动、重复安装拒绝通过；改写 manifest 后移除源校验和清单。机器没有 `agy`，未执行宿主 `agy plugin validate` 或 Gemini Agent 验收 |

WinForms 的 UIA ValuePattern 设值未触发托管 `TextChanged` 回调，因此稳定性判定读取 UIA 值，并调用测试应用自己的 Ctrl+S handler，从保存文件验证它实际读取的 `TextBox.Text`；没有以“未收到回调”作为重试输入理由。Chrome `typeText` 是插入操作，重复测试先经 DTW 发 Ctrl+A，再输入，独立页面事件验证最终值。

## 主线问题与本地修复

1. **Windows permissions 使用了 macOS 参数。** 原始 adapter 将 `{"prompt":false}` 传给所有平台，Windows 严格 schema 拒绝该参数。改为仅 macOS 传 prompt，Windows 传 `{}`；验收 harness 现在断言诊断 status，防止命令 exit 0 掩盖拒绝结果。
2. **Windows focus 成功没有 compact 摘要。** Windows 返回 `landed_on_target` 与十六进制 `now_fg_hwnd` / `target_hwnd`，原 adapter 只识别 macOS 字段。本地转换为统一摘要，并仅在 landed 且两个有效 HWND 相等时标记 verified；活体验证为确切测试窗口 ID。
3. **Skill region recipe 不适配 UIA payload。** Windows 按钮使用 `invoke` 而非 `AXPress`，且省略 `screenshot_frame_valid`。本地 recipe 支持两种 payload，并强调 capture 存在不证明像素对齐。query guidance 排除 display-only 行并检查实际 native role；Windows 的 `textfield` aliases 可同时匹配 `Text` 标签与 `Edit` 输入框。固定动作现在点击已验证的 token。
4. **Python probe 依赖 POSIX pipe select。** 改为后台 JSON-line reader 与 queue，保留超时与 EOF/异常处理，明确 UTF-8。Windows 环境保留必要系统变量并隔离 PATH。

这些是 DTW 本地修复，未修改 upstream CUA，也不能据此宣称未修复的 main 已通过相应检查。

## 阻塞项

### DPI 不感知窗口的截图与坐标不一致

`windows-live-aa992d88` 使用默认 DPI-unaware WinForms，在 125% 缩放下得到 900×588 PNG。绿色按钮的独立 PNG 边界为 `[198,206,155,39]`，而 UIA `screenshot_frame` 为 `[250,257,194,50]`：位置分别偏移 **52、51 像素**，尺寸也错位；右侧与底部有黑色填充。对应图片为 `native-alignment.png`。存在 capture ID 且没有截图 error 仍会发生。

同一 fixture 切换 `SetProcessDPIAware` 后，PNG 为 720×477，绿色按钮实际与声明 frame 都为 `[200,212,155,40]`，四项误差为 0。对比记录 `windows-live-ecd2f4b3`、`windows-live-9896c550`。这限定了已验证的成功场景，不能推广到所有原生窗口。检测到错位时 harness 跳过截图坐标点击，没有继续用不可靠图像定位。

源码检查发现 Windows `capture.rs` 使用 `PrintWindow` 和物理窗口 geometry，主要在几乎全黑时切换 WGC；这次部分黑边没有触发相同拒绝。**这是根因方向的推断**，尚未通过 upstream 修复验证。发布前需在保持 adapter/worker 版本一致的前提下修复 capture/DPI 路径并重新验证 pin；不能靠放宽字段检查或让所有测试窗口 DPI aware 来消除失败。

### Native file-picker 路径没有闭环

`windows-live-9896c550` 的语义按钮点击原始执行 `a699fe93-9fe4-4e7e-a91d-c1654863228f` 在 45 秒后仍为 running；没有重发 opening action，停止后续输入并清理自有 fixture。该证据证明执行未及时结束，不证明请求从未生效。

在全新 fixture 的截图点击路线中，`windows-live-78ce0645` 的应用日志记录 `picker_opened`，但原始点击返回 `foreground_unavailable`：点击后的实际 foreground HWND 与原窗口不同，未被接受为已验证的同进程新窗口。`windows-live-02d64253` 同样有恰好一次 `picker_opened` 回调；随后仅只读的 `listWindows({pid})` 返回主窗口，没有期望的新 picker。没有重复 opening action，也没有猜坐标或发送 Return 给不明焦点。不能宣称文件选择、picker 窗口发现或其后的读取/确认通过。

当前记录保留语义路线超时、像素路线交付后的错误，以及 PID scoped discovery 失败；modal/UIA invoke 等原因尚需进一步诊断，不能仅依据失败回执断定按钮没有被激活。

## 取消与尚未覆盖的范围

`cancel` 会保留 `cancel_requested` 与原始 execution ID；测试中的 1.5 秒 sleep 仍最终 completed，值为 `SLEPT-ONCE`。它证明请求状态记录与 Session 隔离，不证明立即中断原生调用或阻止所有迟到输入。未知/仍运行的写操作应对账原始 receipt，不能自动 replay。

未验收：提权应用/UIPI 跨完整性输入、UAC 与安全桌面、同时多个 Agent 的前台仲裁、多显示器/混合 DPI、干净机器 runtime 安装、cursor overlay、生产签名、WPS/Obsidian Windows 原生业务任务、真实宿主 Agent 消费 Skill。测试中的 Chrome 页面与 WinForms 程序为受控 fixture；通过结果限于本机、这些 provider 与已测操作。

## 复现

在仓库根目录运行，使用已构建的 `package-acceptance`；GUI 测试会取得前台焦点，应顺序执行：

```powershell
python -m pip install Pillow==12.3.0
$pkg = 'D:\WorkDir\desktop-world\artifacts\windows-cua-20261011\package-acceptance'
python tests/acceptance/probe.py "$pkg/bin/dtw.exe"
python tests/acceptance/contract_probe.py "$pkg/bin/dtw.exe"
python tests/acceptance/native_ops_probe.py "$pkg/bin/dtw.exe"
python tests/acceptance/windows_probe.py $pkg --dpi-aware --rounds 20 --picker-mode skip
python tests/acceptance/windows_chrome_probe.py $pkg --rounds 5
# Expected negative cases; exit 1 is retained as evidence:
python tests/acceptance/windows_probe.py $pkg --rounds 2 --picker-mode skip
python tests/acceptance/windows_probe.py $pkg --dpi-aware --rounds 2 --picker-mode semantic
python tests/acceptance/windows_probe.py $pkg --dpi-aware --rounds 2 --picker-mode pixel
```

新 run 写入独立目录；原始 JSON 回执、PNG、fixture application logs 与退出状态应一起保留。此前调试时的 harness 假失败（例如 UIA Text/Edit 行混淆、Unicode surrogate 日志序列化、降采样颜色边界、hover 颜色以及 typeText 插入语义）没有作为插件回归计数；最终脚本纠正这些 oracle，真实平台失败仍单独保留。测试进程已清理，未提交 GitHub 评论、推送分支或发布安装包。

## 后续责任归属核对

2026-10-12 绕过 DTW 直接调用固定 CUA worker，截图错位、语义 Invoke 等待和点击后 foreground_unavailable 均复现。窗口枚举过早属于 harness 时序问题；补限时只读轮询后，新 fixture 完成 picker 选择并独立记录 OK，原打开动作未重发、失败回执未改判成功。用户明确授权跳过这些 CUA 限制后继续 rc.4，详见 [发布就绪约束](release-readiness.md)。原始失败证据仍保留。
