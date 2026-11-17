# PR #126 本机审查与验收

日期：2026-11-17。审查起点为 `8a0ee955`，生产修复提交为 `038fb60`。
本记录补充[原 macOS 验收](direct-input-policy-acceptance-2026-11-17.md)，不覆盖其历史结果。

## 修复结果

- Windows 内容拖拽在每个新输入前读取实际鼠标位置，与本次手势上一次移动后的实际位置比较。状态在反馈回调及等待之前记录；外部移动阻止 Down 或后续 Move。已按下的按钮仍绕过取消、焦点及几何校验完成配对 Up。结束时不恢复原鼠标位置。
- macOS 的 `AXComboBox` 只有当前实例具有字符串 AXValue 且可写时，才作为文本接收者优先聚焦；选择型 ComboBox 保留 AXPress。文本框和文本区域保留原有聚焦修复。派发选择和文本后置检查使用相同能力判断。
- API 与工具说明改为公开 `effects` 提供 dispatch 和可选的 interrupted、acknowledgements；route、reason 保留在内部诊断账本，公开接口没有 `effects.delivery`。
- Windows scope 验收入口更新为直接输入策略：保留前台与鼠标结果，独立 actor 显式制造切走再切回；旧执行停止，新执行恢复。README 和菜单探针的旧恢复策略说明同步修正。

## 本机结果

Windows x64，主屏工作区 2560×1380，120 DPI / 125%。Rust 1.98.0，MSVC，release profile 与静态 CRT。
冻结开发二进制 SHA-256：`81c39764f9a204dbab686c7502c998853079d2487ccfdf47fa968829711557b7`。

| 验证 | 结果 |
| --- | --- |
| 完整 Rust 库与 CLI | 648 + 5 项通过，含新增外部移动前拒绝 Down、持有按钮时停止并释放用例 |
| 实机执行作用域 | 5 项通过：Unicode 内容及保存字节正确、目标前台与指针保留、OS 层级保留、外部切走再切回仍拒绝后缀、新执行恢复 |
| 实机完整 Windows 功能审计 | 26 项通过，包含语义输入、左右／双击、hover、内容／标题拖拽、双轴滚轮、过期几何、菜单、模态框、多实例顺序与像素、透明度、保存提示、取消、停止应用启动及正常退出 |
| 真实持有拖拽中断 | cancel、EOF、host-death、deadline 四种全部通过：实际观察到 held，恰好一次 paired Up，未执行后缀业务点击，worker 退出；适用场景新执行恢复 |
| 两个编译版本的包升级演练 | 通过，覆盖 Unicode 路径、pins、去重、rollback/pending 保护、真实 MCP/native worker lease、损坏拒绝与自有包清理；模拟 0.5.2 不是发布包 |
| 跨平台源码检查 | [Linux portable CI](https://github.com/caelis-labs/desktop-world/actions/runs/44074086147) 通过；本机执行中的六组 POSIX/macOS 定向测试存在 Bash/python3、POSIX 路径、symlink 权限及信号接口限制，原失败日志保留 |
| 修复提交完整 CI | 同一运行全部通过：macOS 679 库 + 6 CLI + 3 MCP、原生 Swift inventory/input 及 worker 清理；Windows 648 库 + 5 CLI、包升级及验收 fixture 编译；Source checks 汇总通过 |

新增 Swift 纯能力用例覆盖支持 Press、可聚焦、不可编辑的 ComboBox 必须选择 action，以及可编辑 ComboBox／文本框继续优先 focus、不可写焦点保留 Press、原始读取失败传播。
它们随原生 macOS CI 编译执行；本机没有宣称完成 macOS GUI 下拉框复现。

[结构化证据](evidence/pr126-review-2026-11-17/summary.json)保留源提交、二进制与原始报告哈希、scope 回执、功能用例清单、四种中断回执及清理结果。完整本机日志、独立 fixture state/journal、截图和原始报告位于 `artifacts/pr126-review/`，失败尝试保留原样。

上述实机结果针对开发二进制和自有 Windows fixture，不等同于最终签名发行包、任意应用、混合 DPI／多屏、锁屏／提权桌面、Linux GUI 或完整 Agent 模型任务验收。最终发行仍须完成既有双平台构建、签名／公证、完整资产验证及自动发布流程。
