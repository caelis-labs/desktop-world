# PR #110 Windows 实机验收（2026-11-07）

[PR #110](https://github.com/caelis-labs/desktop-world/pull/110) 的 Windows 修复已在普通用户桌面验证。对 [#109](https://github.com/caelis-labs/desktop-world/issues/109)，同一最终候选包在自然出现的不可见前台状态下分别通过了首次键盘和首次指针输入。用户无需操作不可见窗口。与本轮 macOS 对齐的是已选的实际工作流和证据标准；原始 42 项矩阵并未全跑。

## 环境与产物

- Windows 11 23H2 build 22631，普通用户 Default 输入桌面，单屏 2560×1440、125% 缩放（DPI 120）。
- 实际安装的 DeepSeek Harness 0.2.2-rc.1，随安装包提供的公开 ACP profile，DeepSeek account / deepseek-flash / high。模型通过完整 Cordis 包的 apps / exec 执行业务，使用同包 Skill/API。独立脚本和只读观察器只负责诊断和效果判定。
- C07 原生代码与 Skill 冻结：9314a3554627c7f001fcf117ad164ed6794962c0；后续改动为验收和证据。完整包 ID：0d6eb1a4bafc30ea9f248af9fe338f6e296481d4c02c3d6fe702cf6d35f59261。
- dtw.exe SHA-256：df04b7c575ad47f5c6a1ee76969a4533f4112b29dc0627ae5b8acf3ec7d99da4；ZIP SHA-256：59eca5f0365a4349741affeff0b028c8da2902fe15812eb5e784b9fbdc7b9d04。未签名的 0.3.5 候选；没有发布或启用新的 stable。每轮用实际二进制路径、出生时间和进程句柄核实 DSH 子进程。

## #109：隐藏前台与精确目标

旧 0.3.5 release 在自然出现的同类状态下复现 AttachThreadInput 被拒、SetForegroundWindow 未成功，业务键盘未派发。此处 GameInputServiceWindow 是 Windows 报告的不可见前台 owner；该观察不能确定 GameInput 是根因。修复不关闭服务、不改前台策略，也不放宽实际前台检查。

程序仍优先使用可验证的后台路线。目标确实需要前台且常规准备失败时，只允许在验证过的标准原生标题栏上执行一次成对的固定 CUA PT_PEN 输入；等目标队列处理和光标落点核实后，才派发业务输入。脚本结束时，原窗口、层序和物理光标仅在仍归本次脚本所有时恢复。即使外部可见窗口离开后又切回来，同一脚本也不能重新夺取输入。若原 owner 隐藏且无法验证恢复资格，业务可完成，原 owner 返回仍记为未确认。

C07 两次直接诊断在显示自有 fixture 前后都记录到同一自然隐藏 owner，无人工激活。首次键盘路径用 Ctrl+A、双行 Unicode 和 Ctrl+S 验证了实际保存字节和全局键释放。首次指针路径由只读截图定位，原生记录到实际移动和一次左键释放；随后用独立声明的 Increment 进入编辑区，再验证相同保存字节。两次的 MCP、原生 worker 和 fixture 均正常退出。原始 #109 a01 的键盘消费和收尾仍是 unknown。

Windows 原生 Open/Save/覆盖确认核实同进程 owner 和模态关系，在标准对话框按钮 Invoke 前准备精确模态，避免 inactive BM_CLICK 不生效。菜单在派发前选择对应动作：有子项的菜单先准备 owner 并用 ExpandCollapse，叶项才用 Invoke。普通按钮仍可后台执行。对应规范见 Microsoft 的 [MenuItem control type](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-supportmenuitemcontroltype)。原生 Edit 写入多行时使用 CRLF；Explorer 实例过滤桌面、任务栏和辅助壳窗口。

## 最终 C07：DSH 模型任务

| 任务 | 独立判定 | ACP 工具调用 |
| --- | --- | --- |
| 原生编辑、菜单、文件面板 | 完整值设置、双行 Unicode、Reviewed、Checkpoint 137 和原生菜单提交各一次。原生长文 2 页到 UTF-16 EOF 28029；打开、保存各一次，取消 Open，拒绝覆盖并取消底层面板。实际 [MCP 截图](evidence/windows-pr110-2026-11-07/native-c07-final.png) 核对两行。 | 37 次，4 次失败，0 次回执明确标记 unknown 的派发 |
| 扩展原生交互 | 滑杆从 0 到 200，真实按下/释放各一次；左区滚动 360，右区 0；同一进程 peer 打开、增加、关闭各一次。模型复看的一张图按 SHA-256 证明来自先前 MCP capture。 | 21 次，0 失败；6 个 MCP 图片 part、1 个附件复看 |
| Chrome、表单、canvas、图像 | 自有隔离 profile。中文笔记保存、Checkpoint 137 各一次；表单先有实际无效事件，再唯一有效保存，重开后取消。包含四次连续 document 导航，canvas 只 pin/send 各一次，图形挑战一次正确。长文 3 页到 UTF-16 EOF 11516。 | 77 次，14 次失败，23 个图片 part，0 次回执明确标记 unknown 的派发 |
| 原生光标 | 真实 MCP helper 的归属、透明非激活样式、实际箭头、短暂 pulse 与恢复像素；9 个环上点有颜色变化后复原。物理像素证明与模型 PNG 送达分开。 | 5 次，0 失败 |
| 持键中断 | DSH public ACP session/cancel 时左键实际按住；配对释放、旧 Increment 不执行。同一 MCP 新观察后执行新的完整值设置与 Save。原始 abort 对象仍保留，不从宿主报错推断原生派发或收尾。 | 9 次，唯一失败是预期 execution_interrupted |
| 真实 Explorer / Notepad | 只改名自有目录文件；保存 53 字节、两行 CRLF、无尾换行。第二轮同会话真正重开，读到 UTF-16 EOF 29、截图后关闭；原窗口和文档保留。 | 44 次，6 次失败，5 个图片 part，0 次回执明确标记 unknown 的派发 |

ACP 记录工具调用、状态、失败分类和接纳的图片；独立窗口、事件和磁盘证据判定消费。失败工具的图片未进入模型上下文。任务通过可含可诊断错误，完成状态也不能自动证明派发、消费或物理光标没有移动。这里的 DSH 中断是公开 ACP 取消，不是点击 Desktop UI 的 Stop。

## 直接补充验证和保留的失败

C07 直接脚本另验证原生左/右/双击、hover、拖动、横纵滚动、标题栏移动和 stale capture、前后台原生菜单、peer/同可执行文件窗口、精确 A/B 层序与透明度、打开/覆盖/退出，以及 4 个脚本级归还和干扰场景。后台投递与严格拒绝、专用非输入桌面拒绝、workbench 选定 9 项通过。四种真实持键边界（cancel、EOF、deadline、宿主死亡）均证明释放和 worker 回收，宿主死亡的 MCP 退出码 1 是注入场景的预期结果。Chrome 与 Edge 的 14 项直接诊断覆盖空字段、ASCII/Unicode/多行/终端 emoji、字面 label/FFFC、横纵滚动、原生 chooser 和磁盘字节；Chrome 初次空字段收到 unsupported_capability、业务未派发，单独记为 13/14，带诊断 trace 的新轮次为 14/14。没有据此判定首次失败根因。

修复阶段保留了实际失败：C05 标题栏笔事件晚到，光标落点改变后该步状态不确定，未重放；C06 菜单 Invoke 卡住、worker 异常、MCP 退出 2，最终修复前保留该记录。早期 DSH cancel 有 4 次样本数上限的前置拒绝，随后才有一次实际取消。跳过的隐藏前台尝试没有计作通过。

真实应用早期一轮误选旧记事本标签，立即停止；该标签已撤销并核对到原字符哈希、保存标记和光标位置。原失败和消费不确定保留在私有证据。另一轮漏做真正的重开，被较严格判定记作不完整。现代 Notepad 含尾部换行的文档全选后逐字替换，观察到实际保存的 CRLF；消费确认拒绝且该 MCP 退出 2。验收用原生 setter 完整清空后再输入两行，没有裁剪真实换行，也没有据此宣称任意全选替换都已通过。

采样仅判定时钟、实际间隙和记录的调用/中断边界；没有 50ms 门槛。只有显式声明的前台/光标约束才能评为保持。缺失边界、无法归因的 PID 和间隙内的瞬态事件不能作为通过证据。

## 复现、收尾与范围

入口在 tests/acceptance/windows 的 dsh_native_probe.py、dsh_browser_probe.py、dsh_cursor_probe.py、dsh_real_apps_probe.py；使用实际安装的 dsh.cmd --profile acp --patch <本轮自有补丁>，用 observe_run.py 包裹。每次新建输出，GUI 串行执行。真实应用只使用监督器新建并核实的空白单标签记事本窗口；不退出含其他用户文档的整个记事本。完整退出和重新启动用自有 fixture 单独验证。

[脱敏证据摘要](evidence/windows-pr110-2026-11-07/summary.json) 包含产物哈希、用例结果、清理、调用和采样统计。原始 ACP、会话归档、用户文档恢复核对、可能包含其他窗口的 composite PNG 均留在本地 ignored artifacts/pr110。Windows 本地 623 Rust library、4 CLI、9 observer、2 DSH metrics、5 CI plan、2 Windows fixture controls 通过；本地 POSIX source suite 有 Windows shell/path 限制。验收代码提交 [815a203 的五项 CI](https://github.com/caelis-labs/desktop-world/actions/runs/42334266878) 全过，CI 本身不跑桌面 GUI。

自有记事本窗口已正常关闭，原两个窗口保留。DSH Desktop 已从应用菜单正常退出并重启，原 Cordis patch 字节一致、原 managed current/pending/rollback 恢复，并核对新桌面进程树中的 MCP 确实执行原 0.3.3 包。其他绑定与七个包的清单均未变化，未删除任何包。

混合 DPI/多屏、其他 Windows build、提权/受保护应用、所有 tooltip/shadow 组合和未选择的矩阵场景不在通过范围。
