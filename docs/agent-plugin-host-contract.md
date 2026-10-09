# Agent Plugin 宿主集成契约

Desktop World 正式 Plugin 包是一份通用本机发行 payload；Bot 等宿主消费同一固定版本、同一平台归档，不复制或重写 MCP/JS 桥。`plugin.json` 与 `mcp.json` 是 Agent Plugins 1.0 固定位置；不支持标准包格式的宿主可以分别加载同一份 Skill 与 stdio MCP。

## 根目录与数据目录

- 安装根目录保持只读：`runtime/node[.exe]`、`mcp/server.mjs`、`mcp/worker.mjs`、`bin/dtw[.exe]`、`clients/`、`skills/`、许可与校验文件。必须先校验官方发行 SHA256 和包内 `SHA256SUMS`。
- `PLUGIN_DATA` 由宿主创建为插件实例专用、持久、可写目录。MCP 进程在其中创建 `sessions/<uuid>/`，放 owner descriptor、capture PNG 和元数据审计。不要把它当成可分享的模型工作目录，也不要写回安装根。升级包不得删数据目录；卸载可由宿主按其保留策略清理。
- 不支持 Plugin 变量展开的宿主在自身 MCP 配置中使用绝对可执行路径与脚本路径，并显式传 `--data-dir` 的绝对路径。`mcp.json` 的 `command` 是包内 `./runtime/node[.exe]`，不对 `command` 做变量展开。

## 进程与授权

一个 stdio MCP 连接对应一个 supervisor、一个持久 `dtw session` 和一个可被监督的 JS worker。`desktop_status` 从 supervisor 响应，不依赖 worker 的事件循环。worker 退出会报告 `state_lost`，不能自动重建、重放脚本或声称原生 helper 的 Ref 立即失效。helper 重启才会更换 epoch 并使旧 Ref 失效。

默认没有原生写 grant。可信宿主可在启动 MCP 时根据用户批准传 `--write-app NAME`（可重复）、`--input-mode`、`--input-policy`；模型的 `desktop_exec` 参数只有 `execution_id` 与 `code`。同一会话内动态批准或撤销时，可信宿主通过 `desktop_status.owner_file` 调用现有 `dtw auth`，必须保留原 owner 请求 ID 与回执。`--write-app-window` 属于原生 helper 的兼容入口，其实际 grant 是所属 APP 实例，不是单个窗口。OS Accessibility/输入/截屏由用户自行授权。客户端审批完整 JS 脚本；MCP annotation 与 Skill 说明不强制执行审批。`node:vm`、worker 和 APP grants 都不构成任意 JS 安全沙箱。

正常 exec 间 `state` 与 helper epoch 保留，脚本局部变量不保留。一个连接只接受一个并发脚本。相同 `execution_id` + 相同代码返回原状态/结果；不同代码冲突。容量满、连接断开和重启均无跨会话幂等承诺。失败、partial 或 unknown 时查询原 execution 和 native run ID，不能换 ID 重播。

取消、60 秒超时和 stdio 断开先阻断新 native 请求，再调用可信 owner `end_turn`，之后终止 worker；本地审计记录清理结果。MCP 取消通知本身无确认响应，同一连接仍活着时用 `desktop_status` 查询。断开的 stdio 不可再调用 status。`close_incomplete` 不能证明原生输入清理成功。通用 MCP 缺少可信用户 turn 身份，因此原生 grant 作用于此连接及显式撤权；需真实 turn 级权限的宿主必须在自己的生命周期层管理，不应向模型开放 owner 通道。

虚拟 cursor overlay 是独立原生进程。只有已完成投递且具有解析后 desktop point 的指针回执会更新它；取消/断开时退出。它不发送事件、不改变真实鼠标、不抢焦点或接收点击。`visible_region` 的每张图像有独立 `image_to_desktop`，`window_content` 是目标窗口本地坐标，不能以任一图像的像素直接授权点击。

Bot 迁移应等待同 SHA 双平台包、macOS 原生验收与 Windows 原生验收；安装/更新应固定 release 版本与哈希，保留 Bot 自有权限、真实 turn、进程与数据目录生命周期。此仓库不含 Bot 产品配置或专用 bridge。
