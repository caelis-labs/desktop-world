# 2026-10-05 Windows 实机证据

对应 [Windows 适配验收报告](../../windows-validation.md)。Windows 11 x64、单屏 2560×1440、Go 1.26.8，Chrome / Notepad / Calculator / Win32 真实应用。全部操作走 dtw / 原生 SDK；业务日志由测试应用独立产生。

| 证据 | 内容 |
| --- | --- |
| [日常任务摘要及回执](tasks-summary.json) | Chrome 表单/可信输入、同请求去重、12 轮恢复、记事本保存、计算器、Win32 测试；helper 版本与 SHA256 |
| [浏览器 DOM 事件](browser-events.jsonl) | 独立事件记录；提交恰好一次，Unicode/emoji 与 isTrusted |
| [原生测试输出](native-test.txt) / [Win32 消息](native-events.jsonl) | 真实键鼠、取消清理、替换控件后的旧 Ref 拒绝 |
| [F4/F5 摘要](features-summary.json) | 相同构建的 UIA 续扫与私有 managed transport |
| [F4 输出](f4-test.txt) / [F4 日志](f4.jsonl) | 16 页/1013 节点，业务提交一次 |
| [F5 输出](f5-test.txt) / [F5 日志](f5.jsonl) | 授权/提交/撤销和原回执恢复 |
| [JavaScript 第一次调用](js-first.json) / [第二次调用](js-second.json) | named pipe 会话、同 Epoch/state/Ref、局部完整观察与实际 document URI |
| [源码清单](source-sha256.json) | 日常任务构建时的源码 SHA256，含未提交 Windows 适配源文件 |
| [最终检查](checks.txt) | Windows Go race/vet/build、示例及 19 个 Node 测试 |

基础提交 `887b863809f2ba532675668e37e91bde4dac758d`，helper 标记 dev / vcs.modified=true；这是该提交之上的未提交源码构建。SHA256 在摘要中准确保留，不表示已有发布包。之后仅修改说明与证据文档，原生实现与测试代码不变。

保存的日志只含合成任务内容；未保存用户应用的整桌面 inventory。摘要中的图像/文件路径改为仓库根目录相对路径，完整 wire 与 PNG 在本机对应的忽略目录。图像已逐张查看：Chrome submitted:1，记事本两行中文/emoji，Calculator 384。截图包含浏览器周边 UI，因此不纳入公开证据目录。

通过记录不覆盖失败轮次：此前发现的焦点恢复、DPI 坐标、Notepad 末尾段落、Win32 选择/标签关联问题已修复并使用新文件/新日志复验；原失败回执和不确定效果留在本机 artifacts。单机固定场景的数据调用与文本 bytes 不等于 LLM tokens 或通用成功率。
