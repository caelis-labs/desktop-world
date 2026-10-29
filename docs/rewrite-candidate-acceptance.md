# 重写候选与最终安装验收

日期：2026-10-29。前两批核心实现在 #66 / `7a06c6b`，开发字节的原生与外部 Agent 六轮门槛已通过，见[核心验收](release-core-acceptance.md)。本记录从第三批开始：0.2.0 为候选源版本，尚未发布。

候选构建只在显式触发时运行，不恢复 CUA SDK/worker 或旧打包链路。Windows 最终支持须由用户独立验收；Linux 不制作正式候选或作支持承诺。编译、签名、公证、安装和 GUI 验证分别记录，不能互相替代。

| 层级 | 当前状态 |
| --- | --- |
| 固定来源与候选脚本 | 导出 clean Git commit/tree，固定 Rust 1.98.0/lockfile；目标依赖 notices、平台格式、版本、归档完整性均有检查 |
| macOS 候选 | `fb1045e` 已复用现有 CI 认证完成签名、公证和归档；安装 GUI 暴露输入失焦缺口，保留该包，修复后生成新候选 |
| macOS 最终安装字节 | 首个 CI 包在含中文与空格路径通过完整清单、签名/公证、MCP 发现及多实例 GUI；native all 编辑失败，暂不通过；新候选整体验收待完成 |
| Windows 原生构建 | `a07d1eb` 完成 x64 原生编译、静态 CRT/版本/归档检查、265 个库测试、3 个 CLI 测试及 C# 验收工具编译；下载后重新验证完整归档清单与校验和 |
| Windows 实机 | 用户独立安排，尚未通过；候选自带标准 MCP 双实例 smoke 与手工矩阵 |
| 公开 release/tag | 未创建 |

## Windows 独立验收交付

成功构建：[40796800484](https://github.com/caelis-labs/desktop-world/actions/runs/40796800484)，源提交 `a07d1eb48ca391d3fe20a37e6beda4f9a584b847`。下载该 run 的 `dtw-windows-candidate-a07d1eb48ca391d3fe20a37e6beda4f9a584b847` artifact，再解开里面的候选 ZIP；不使用前两次失败 run 的文件。

- ZIP：`desktop-world-0.2.0-x86_64-pc-windows-msvc.zip`，SHA-256 `c1471679ca9e70b29413148eec9eb0decca0b4dba7400be12188b0806ead55b1`。
- `bin/dtw.exe` SHA-256：`940d21d60cf750c04f41fb5e118952261269ef8bcf21eccf6db742907805cef5`。
- 工具链：Rust 1.98.0、MSVC 19.51.36260 x64；该包的 SDK 环境字段为 null，不推测其版本。PE 导入检查通过，不依赖动态 VC CRT。最低 Windows 系统版本仍须实机验证。
- 包内 `acceptance/windows/README.md` 和 `smoke.ps1` 用普通 stdio MCP 操作两个同可执行文件实例，检查分别编辑/保存、Unicode/快捷键、窗口截图和释放状态；另有可选指针组。仅需 Windows PowerShell 5.1/.NET，不需 Rust/CUA/VS。
- 手工矩阵仍包含真实浏览器/Agent、DPI、并发会话、取消/超时、进程退出重启、EOF/父退出、权限与安全桌面，以及截图视觉核验。CI 的 C# 编译不计 GUI smoke 通过。

## 原始失败及修复

- Windows [40764663815](https://github.com/caelis-labs/desktop-world/actions/runs/40764663815)，来源 `6bcd88b`：Git archive 受到 `core.autocrlf` 影响，导出的 lockfile 与提交 blob 不同；在 Rust 编译前停止。`632065c` 显式导出 LF 源码，真实临时 Git 回归同时保护 `.gitattributes -text` 文件的原始字节。
- Windows [40766203496](https://github.com/caelis-labs/desktop-world/actions/runs/40766203496)，来源 `632065c`：已生成 ZIP，但库测试发现 generic Windows 子进程的强制终止分支漏记实际 wait 状态。原始 EOF 主错误不变，后续清理查询却可能错误返回成功。`a07d1eb` 无条件保留实际退出状态，覆盖自然退出与 kill 的交错、exit 1、强杀不确定性及重复查询；native cooperative worker 的既有语义继续保留。
- 本机 macOS `/private/tmp/dtw-candidate-macos-20261029-01`，来源 `6bcd88b`：原 codesign 等待系统私钥；用户确认后返回 `timestamps differ by 10104 seconds`。保留 `FAILURE.json`、`commands.json`、`logs/017.err` 和采样，没有改时间、关闭时间戳或将失败签名标为通过。后续使用 CI 的现有认证重新生成候选。
- macOS [40798307879](https://github.com/caelis-labs/desktop-world/actions/runs/40798307879)，来源 `fb1045e`：259 个库测试、4 个 CLI 测试、3 个 macOS stdio 测试通过；Developer ID/时间戳/Team 校验及公证 Accepted，原提交 ID `78e22d32-0e2d-4234-91f0-98d092485785`，临时钥匙串清理与原搜索列表恢复通过。ZIP SHA-256 `832bcb3eea3aa3c7c98d8b8552ed212f0b59915259fb51b67454412976520599`，二进制 `4d8c05e8bda7a03efab3ac1177e25ee9c082c7842abdd3435c154de90faa0a9a`。安装路径 `/private/tmp/DTW 安装验收 0.2.0/desktop-world-0.2.0-aarch64-apple-darwin`；原归档和安装字节未修改。多实例 `/private/tmp/dtw-installed-multi-instance-20261029-01/evidence.json` 通过；native all `/private/tmp/dtw-installed-native-all-20261029-01/evidence.json` 中文本只到 `DTW `、保存 0 次，回执却无错误。原样本的失焦来源未归因。
- 同一旧安装二进制的独立受控负例 `/private/tmp/dtw-focus-intercept-original-20261029-01/evidence.json`：输入期间由另一个自有 AppKit 测试应用接管焦点，原目标收到 18/1006 字符，另一窗口收到 988 字符并保存 1 次，原回执仍无错误。修复将同步 worker 的 AppKit 前台快照改为系统级 AX 当前应用核验，精确窗口查询前后都检查 PID。修复开发字节 `38c327624264aef8ba72ea3822148e8adebaef0ac1f0a105fde8a6ad339cc640` 的同一控制 `/private/tmp/dtw-focus-intercept-20261029-01/evidence.json` 通过：19 字符后停止，`focus_changed`、`unknown`，保存后缀未执行，另一窗口文本不变，不抢回焦点，两窗口及 native worker 均正常清理。纯焦点控制 11 项及验收回执负例通过；开发字节的通过不替代新签名候选验收。
- macOS [40801105989](https://github.com/caelis-labs/desktop-world/actions/runs/40801105989)，来源 `0289557`：259+4+3 源码测试、签名及公证通过，原公证 ID `3b51f127-57f9-441b-bfa1-a1be039b5bac`。ZIP `5a7b9fecdb5ba023629b98635d70dc79738c63495b462a055f4156698b1c8e64`，二进制 `0551724dc059b68ebcd57b0b3d4f7bfa8d9fb9db7bd04c7bededf238a07d7652`；新安装目录完整清单与公证核验通过，最小 PATH 的标准 MCP 四工具/两参数发现通过。`/private/tmp/dtw-installed-fixed-native-all-20261029-01/evidence.json` 在首次 focus 已激活窗口后读 AXWindows 返回 `CannotComplete (-25204)`；回执明确 `unknown/ax_unavailable`，文本仍为 seed、0 输入事件、0 保存，正常清理。该原始操作不重放，候选仍未通过 GUI 门槛。
- 激活就绪的本地中间字节 `d7446cb007338892fce9bb8ba7fe707a17215d78c7fd6422aeb59565a42cb6d7` 在 `/private/tmp/dtw-focus-readiness-native-all-20261029-01/evidence.json` 暴露初始阶段过严：没有当前 focused app 时尚未尝试聚焦即返回 `not_dispatched/focus_unproven`，0 输入、0 保存。修正将初始聚焦、激活后只读就绪等待、输入中途核验分开；仅前两阶段处理私有类型的焦点缺失，公共错误结构不变。新增阶段控制 13 条、就绪控制 49 条通过。
- 最终本地字节 `ecc41a1c90fa0bd3affd1a9e1a03a822622ecb75ad2def9a99d3ba26c7d9a7ef` 的 `/private/tmp/dtw-focus-phases-native-all-20261029-01/evidence.json` 全组通过，截图已人工查看；`/private/tmp/dtw-focus-phases-intercept-20261029-02/evidence.json` 受控失焦通过，正常清理。前一个 intercept `...-01` 有脚本以外的 Space keycode 49 在输入前加入空格，来源未归因，样本判无效并保留；其原操作在清理时取消，未重放。用户为桌面留出时间后才创建新的独立样本，未将无效样本算为通过。

Windows 第二次失败包 ZIP SHA-256 为 `26ea4131aee920d82608fec7faedb1f04d159726533480fb5c0c0de7595935f8`，二进制 SHA-256 为 `3f70fd10c91816480a427aedd5d9b675c698db6f59765f6bc6d812690c2f40cf`；仅留作原始证据，不计入验收。上述失败均保留原 workflow run，修正后按新源提交触发，不以同一次操作重放掩盖原因。

最终二进制、归档、源提交、工具链、签名、公证与验收日志分别绑定。安装模式直接启动包内二进制并读取包内 Skill，验收前后复核全部清单；不以旧开发字节的通过证明新候选。发布时使用验收过的相同字节。
