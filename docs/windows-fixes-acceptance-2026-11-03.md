# Windows 功能修复与验收（2026-11-03）

本轮处理 [0.3.3 修复前审计](windows-feature-audit-2026-11-03.md) 中的功能缺陷、Windows 能力缺口、当前协议探针与过期文档，并纳入 [#104 DSH 安装兼容](https://github.com/caelis-labs/desktop-world/issues/104)。公开接口保持两个 MCP 工具 `apps` / `exec` 和九个 app 方法。

最终同一 release 二进制完成官方基线 **5/5**、Windows 功能 **26/26**、Chrome **14/14**、Edge **14/14**，均正常收尾；完整 Rust release 测试 **547 + 4** 项通过。二进制 SHA-256 为 `8ee99280c70cb1f5365753e3f7c4dcba31f2ec57722a70e7cfc47df9fdf6460c`。

这是开发分支验收，不是已发布版本或已安装连接的升级声明。源码基线为 `f07fb78e5013b16744a6694e39e5b2f2479a554f`，分支为 `fix/windows-parity-and-install-docs`；精确二进制、源码文件、Skill 和探针身份随最终结果记录在 [证据摘要](evidence/windows-fixes-2026-11-03/summary.json)。源码哈希针对本次 Windows 工作区的实际字节；Git checkout 的换行转换可能改变文件哈希而不改变源码内容。

**修复范围**

| 原审计项 | 当前实现与验证合同 |
| --- | --- |
| W-01 多实例枚举重排 | 用实例成员集合判断变化；枚举顺序变化不清观察，真实成员变化仍重新绑定。跨 exec 选择器与双实例连续操作有回归。 |
| W-02 跨实例窗口次序 | Windows 保留全桌面窗口 rank，并提供原生键盘焦点提示。双实例重叠像素、观察次序、实际前台与键盘接收分别验证。 |
| W-03 菜单拒绝前抢前台 | scene 不再提前 focus；选择器先完成 scope/唯一性准入。已发生的 focus preparation 独立计入 effects，目标动作不重复记账。 |
| W-04 Chromium Unicode | 空 Edit 先证明原始/新建 DocumentRange、单一空 selection、稳定 name 和两次可写空 ValuePattern，再进行输入；非空仍走原精确分区。独立 HTTP 回读覆盖空字段、中文、emoji、字面 label/U+FFFC、末尾插入、选择替换和多行。无剪贴板、setValue 或失败重投。 |
| G-01/G-06 应用发现与身份 | 读取 Start Menu Shell Links、App Paths、AppsFolder 和运行窗口；保留友好名称与真实 AUMID。支持绝对 exe 与已证明的 `win:<AUMID>`，请求路由与内部规范化身份分离。缺失/不可读来源降低 coverage，不伪造完整清单。 |
| G-02 正常生命周期 | 启动仅派发一次，未观察到新实例时保留 unverified。关闭使用 WM_CLOSE，每次派发前复核 HWND/PID/出生戳；未确认 quit 保留同一进程句柄，用其出生戳/image 身份与 signaled 状态跨越退出观察，避免重新打开关闭中的 PID。句柄按完整实例匹配、RAII 释放并有界回收，真实权限/UIA 错误仍上报。保存提示与用户取消保留，不强杀。GUI 验证 exe 停止后启动、多实例正常退出和保存取消。AUMID 路线有实现/单元验证，本轮没有独立打包 app 的 GUI 启动验收。 |
| G-03 move/drag | 单窗口、单显示器内逐点验证几何、命中接收窗口、DPI 与焦点；按钮配对释放。标准标题栏需 HTCAPTION 原生证明后才能搬窗。覆盖 hover、内容 drag、标题栏、取消与清理失败回归。 |
| G-04 菜单/对话框 | 支持当前窗口内的原生菜单和 UIA ExpandCollapse；弹出菜单键盘路由要求原生 menu-owner 证明。系统对话框按实际窗口实例归属处理，测试另用 Win32 owner、进程 image 与出生戳证明浏览器 helper。 |
| G-05/U-05 透明与边框 | exact-HWND 捕获使用 DWM 可见边框，排除不可见 resize border；全局 alpha/color-key 需可读原生证明。未证明的 per-pixel layered alpha 保持明确 `windows_surface_unsupported`，不读取桌面作替代。 |
| U-01/U-04 能力说明 | `apps.provider` 返回有界的 provider、capabilities、scopes 和条件；API、产品矩阵及安装文档统一到当前 Windows 实现及限制。 |
| U-02 错误判断 | 对外保留业务错误码，公共非法参数统一为 `invalid_arguments`；`status:completed` 仅表示执行已结束，不能作为动作成功证据。 |
| U-03 取消 | 保留 `cancelled` 和底层 provider cause；传输停止先返回时仍读取调用取消状态。原 MCP ID 重传读取原记录，未知效果不重放。 |
| T-01 现行验收 | 当前包只交付 三个当前协议探针、transport 和必要 fixture/编译资源；历史 atomic/result 探针仅留源码历史。Unicode 包路径、其他 cwd、包内 import/help 和拒绝旧入口均有测试。 |
| 额外合成缺陷 | popup 向左/上扩展后消失时，保留旧 viewport 的采样原点改用最终 bounds。修复前红测复现；left/top/both 三方向逐像素检查透明空区、四色 base 与前后 `Raster.point` 一致。 |
| 悬停提示抢占语义 | Edge 文件对话框复现中独立确认 `tooltips_class32` / `SysShadow` 位于真实前台 `#32770` 之上。仅把这两类已知装饰窗口从 active 语义候选中排除，保留全部窗口、rank 与合成像素；未知 popup 和菜单仍按原次序选择。 |
| 光标显示目标失效 | 绘制前重新验证目标；普通移动、关闭或模态失效使当前显示 operation 隐藏/退役，晚到 packet 不可复活。原代码在 idle 检查与 paint 间失效时会 Err→exit 1；真正的 monitor/native/render 错误继续上报。两项状态回归覆盖有效 tick 后失效与正常 paint 保留。 |

**DSH 与文档**

[#104](https://github.com/caelis-labs/desktop-world/issues/104) 的失败与 DSH 对 session workspace 内 exe 的路径相关文件限制有关；同一可执行文件位于其他路径时结果不同。当前文档要求使用宿主支持且已授权的工作区外暂存/执行路径，随后使用 `package import` 返回的托管路径配置 MCP。没有更改 ACL、宿主策略或增加绕过权限的步骤。

CLI 的独立缺陷已修复：`dtw package --help`、非法语法、无效 ID/store 形状均在打开 store 前处理；help 返回 JSON，执行帮助不初始化/锁定 store。运行中的托管二进制仍持有原有 runtime lease。

安装总览、DSH/Caelis/agy、插件 README、harness/package 文档、Skill API/PATTERNS、产品矩阵与历史协议页已全面核对。旧迁移计划改为 closed records，过时 API 页收敛为现行引用；有日期的事实与失败证据保留。孤立 POC 页移除已不存在的脚本命令，原说明通过固定历史版本链接访问。Windows 验收指南只给当前三探针的命令。

**验收条件与证据**

实机条件为 Windows 11 `10.0.22631`、120 DPI / 125%、单个普通未锁定交互桌面，主工作区 2560 × 1380。应用为自有 WinForms fixture、Chrome `156.0.8078.74` 与 Edge `156.0.4314.58` 的新建临时 guest profile。GUI 探针串行运行，输入只经 DTW；独立 fixture 状态、UTF-8 文件、物理前台、PNG 像素或 HTTP 事件建立效果证据。

最初 Edge 运行中 12/14 项通过，原生 Open 已出现，但文件列表上方的悬停提示/阴影使图像遇到明确的 per-pixel alpha 限制，探针的首窗归属保护拒绝后续输入并记录强制清理，整轮算失败。独立复现记录了上述真实 HWND 类名后才收敛语义修复。最终探针使用受控的新窗口位置，在首次打开 chooser 前一次性把鼠标移到自有页面的空白下边距，避开浏览器标题区与文件列表的悬停预览。停放点由独有标题对应的窗口 bounds 映射到实际合成图像，不能把图像角点当作自有窗口角点；覆盖/接收窗口证明失败仍前置拒绝。这不是失败后的动作重投，也不改变透明窗口能力边界。

另一轮 Edge 的 14 项业务检查、浏览器实际退出 0 和 MCP 正常收尾均通过，但 quit 后自动 `scene.after` 返回裸 Access Denied，整轮仍记失败。旧日志没有 native operation 证据，不能唯一归因该次失败；源码审阅独立确认了过早释放 quit 进程句柄的身份/退出证明缺口，并补上同句柄证明。后续诊断仅在既有 `DTW_INPUT_TRACE=1` 下输出有界操作名与错误码，不输出请求或 UI 内容。空窗口库存仍不能被当成已退出；shared runtime 的 running 合同未扩展。

较早 WinForms 运行的 26 项业务断言全过，但 MCP 光标收尾返回 2，原探针漏把 finally 的 `passed:false` 反映到退出码。现已修正为收尾/fixture 失败即验收失败。该次 renderer stderr 未保留，不能把其确切退出分支追认成上述 paint 竞态；后续开启诊断的 release 运行收尾正常，所有失败记录仍保留。

最终结果及清理状态以证据摘要中的 `runs` 为准。保留每次失败尝试的本机原始报告；最终通过不覆盖此前失败，也不把只读轮询当成重新派发动作。临时浏览器使用出生戳、可执行文件和独有窗口标题证明归属，正常退出需实际 UI 进程 exit 0；精确 owned handle 的强制清理也算该轮失败。

| 检查 | 最终结果 |
| --- | --- |
| 当前协议真实 GUI | 官方基线 5/5、Windows 功能 26/26、Chrome 14/14、Edge 14/14；浏览器实际 UI 进程与 launcher 均 exit 0，MCP 无强制收尾 |
| 完整 Rust release suite | 547 项库测试、4 项 CLI 测试通过 |
| 托管包与真实 MCP 租约 | 12 项演练通过：Unicode/空格路径、导入去重、两个 host pin、版本切换、失败/回滚保护、MCP/native worker 活跃租约、退出后回收和损坏拒绝；0.3.4 仅为合成测试副本，不是发行 |
| Python Windows 全套 | 133 项中 131 通过、1 跳过、1 本机 Bash 环境失败；当前包布局、transport 与探针 help 单独检查通过 |
| 文档与静态检查 | 相对链接/锚点、当前九方法与 provider 条件核对；rustfmt、版本一致性及 diff whitespace 检查通过 |

**验证限制**

Windows Python 全套中，macOS 签名前置检查测试调用本机默认 `bash` 得到 127，而预期为 2；其余现行打包/布局测试通过。单独使用 Git Bash 已观察到缺少 APPLE_NOTARY_PASSWORD 时 exit 2、零 builder invocation、无凭证输出，但本机 `python3` 命中 Windows Store alias，未生成完整签名 receipt。本轮不修改全局 Bash/Python 配置，也不将该环境限制记作全套通过。

per-pixel layered alpha、跨窗口/跨 app 拖放、跨显示器物理路径、锁屏/安全/提权桌面不在本轮支持承诺。没有重新执行 macOS GUI、其他 DPI 或多屏布局；这些结果不能外推为 Windows 与 macOS 所有环境完全等价。原有安装包与当前 Agent 的 MCP 配置未被替换，未合并或发布。
