# 实现边界与设计映射

原始 `SPEC.md` 和交付 zip 保留不动。本文件记录代码的现状，不修改设计文档中的发布标准。

| 设计契约 | 实现 |
| --- | --- |
| 根包 World / Actor / Fact / Ref / Receipt | `api.go`、`validation.go` |
| registry、revision、scope、snapshot/delta | `internal/engine/world.go`、`actor.go` |
| bind / wait / 前后条件 / 去重 / 取消 / fence | `internal/engine/execute.go` |
| Unicode 分页、Anchor、Capture、Asset | `internal/engine/content.go` |
| 平台边界与固定原生线程 | `internal/backend/driver.go`、engine worker |
| macOS AX / CGEvent / ScreenCaptureKit | `internal/backend/darwin` |
| Windows UIA2 / SendInput / GDI | `internal/backend/windows` |
| 严格 wire 编码与上下文预算 | `internal/wire`、`protocol` |
| 可注入故障与独立事件记录 | `dwtest`、`tests/contract` |
| 原生 fixture 与真实输入验收 | `tests/native-fixtures`、`tests/acceptance` |

## 本地实现选择

1. 原生所有权由固定工作线程管理。macOS 使用 ARC retain 的 AX 对象，Windows 使用同一 MTA apartment 的 COM 引用；World 关闭时等待实际调用退出后释放。超时不能催生无限原生工作线程。
2. 查询按声明 scope / depth / node budget 遍历。原生 registry 与 Go registry 均有上限；达到上限报不完整或资源耗尽，不复用公共 Ref。
3. 当前采用拉取式物化视图。每个 cursor 保存已交付投影，Changes 做有界校对并生成 upsert/remove；超过 revision 距离或 TTL 则 reset。尚无原生 observer 或后台全桌面扫描。
4. 输出分页来自固定采样批次；分页之间不重新扫描桌面。每页可独立同步，不把未交付的对象放入该页 cursor 的基线。
5. 属性刷新时间不推进 material revision。字段变化、生命周期、Seat、环境/权限/拓扑变化推进 revision。拓扑改变使旧 cursor 需要重建，旧 Point 不可执行。
6. 只允许内建键名与有限谓词。`primary` 在 macOS 映射 meta，在 Windows 映射 control；不是“所有应用都有相同快捷键”的保证。
7. 原生 target 参数只来自引擎校验结果。Agent 不能传原生 handle、函数名或任意属性名，也不能选择 Actor。
8. 文本版本是单调分配的序号；用于内部比较的摘要不会输出为 text version，避免公开低熵文本的内容哈希。保护字段不导出 value。
9. 资产留在本地，绑定 Actor 与当前权限 generation，提供 PNG 字节和桌面坐标变换。没有自动上传或 OCR。
10. Go API 默认零值表示采用预算默认值；错误由结构化 Fault 给出。对尚未使用的 control-step 参数也应保持严格校验，不能退化为任意脚本。
11. InputPolicy 是 Actor 的可信上限：no_shared_input 预先拒绝整份包含焦点/共享键鼠的计划，保留原收据。set_expanded / set_checked / set_selected / scroll_into_view 只使用原生语义 setter/pattern/action，始终验证期望状态，已达到状态时不重复发送。三态 checked 不映射为 false，未知状态不盲目 toggle，选择状态不主动清空其他项，provider 的单/多选规则仍适用。
12. observe 的输出字段与 match 必需字段共同生成原生读取计划；部分节点只更新采样字段的 Fact 时间。未请求字段保留原缓存，保护状态变化会清除缓存敏感值。写前完整刷新不依赖观察缓存。
13. Windows UIA continuation 保留有界 DFS 栈和未消费兄弟 COM 引用，最多 16 份、90 秒、每次遵守节点预算、总计 10,000 节点；续扫标记 dirty/incomplete，不以实时树的缺失证明不存在。新扫描不驱逐旧游标。原生引用比较成本仍需 Windows 实机测量。
14. Windows managed helper 继承两个受限制的私有匿名 pipe 端点，控制通道与数据通道分开；Unix 保持 FD 3/4。Windows 11 实机已通过授权、原生填写/提交、回合撤销与原回执恢复；UIA 续扫也完成 1,000 行之后的目标发现和提交。逐 feature 的证据和命令见 [features.md](features.md) 与 [Windows 验收报告](windows-validation.md)。

15. 同一桌面的 `InputModeCooperative` 由宿主开启，已知键鼠步骤在短 Plan 内借用前台并清理恢复；读取与语义动作保留后台通道。公开动作不增加模型参数，no_shared_input、授权与原收据去重继续适用。原生实现和逐项实机证据见 [cooperative-input.md](cooperative-input.md)。

## 尚未完成的正式发布条件

2026-10-05 已完成 Windows 11 x64 的 Chrome 表单、记事本保存、计算器、Win32 键鼠及 managed host 实机验证，并补齐 cooperative 输入。验证针对当前源码与这台电脑；下面的扩展矩阵仍是正式发布需要明确处理的条件。

- Windows 更多 UIA provider、语义状态动作专项与输入竞争场景；Windows arm64 尚不支持。
- macOS amd64 实机运行（当前有交叉构建）；最低 macOS 14 实机运行。
- AXObserver / UIA event invalidation；更完整的人类输入监测及权限矩阵。
- 多显示器混合缩放、旋转、负原点、锁屏 / RDP / 用户切换实测。
- 独立签名宿主与 Wails 集成 smoke，以及原生 / Electron / 浏览器 / Canvas 对照矩阵。
- provider hang 的 helper 隔离评估。嵌入式 native 调用若永久不退出，Close 返回 close_incomplete；不承诺硬取消。
- 大型真实 provider 的性能测量和输入竞争压力测试。没有发布 token 节约率或通用成功率。

## 原生依据

Windows COM apartment / worker 生命周期按 [Microsoft Threading Issues](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading) 实现；SendInput 的返回计数与 UIPI 限制按 [Microsoft SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput) 区分 delivery。COM vtable 与 IID 核对了微软的 `UIAutomationClient.h`，不依赖猜测接口布局。

默认共享输入、AX 查询和 ScreenCaptureKit 窗口捕获沿用公开框架；capture-window Ref 与 AX Ref 分离。可选 cooperative 模式额外动态探测私有 AX→CGWindow ID、SkyLight 前台与 key-focus SPI，绑定实际窗口后短暂借用前台。缺少符号时拒绝操作，单机验收不能推导所有 macOS 版本兼容。来源与许可见 [第三方通知](../THIRD_PARTY_NOTICES.md)。
