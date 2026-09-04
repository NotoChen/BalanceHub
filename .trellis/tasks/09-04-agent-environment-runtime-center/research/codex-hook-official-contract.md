# Codex Hook 官方合同核验

- 核验日期：2026-09-04
- 本机版本：`codex-cli 0.153.2`
- 官方文档：<https://developers.openai.com/codex/hooks>

## 已确认事实

- Codex 当前将 `hooks` 标记为 stable，默认启用；用户级 Hook 可来自 `~/.codex/hooks.json` 或 `~/.codex/config.toml`。
- 同一配置层同时存在 `hooks.json` 和 inline `[hooks]` 时会合并并产生提示，因此 BalanceHub 首轮只选择独立 `~/.codex/hooks.json`，不修改 `config.toml`。
- 非 managed Hook 必须由用户通过 Codex `/hooks` 审阅并信任，信任按当前定义 hash 记录；定义变化后必须重新审阅。
- BalanceHub 不读取、不修改、不伪造 Codex trust store。安装成功只能证明节点存在，不能证明已信任或健康。
- 生命周期输入从 stdin 接收 JSON，公共字段使用 snake_case：`session_id`、`transcript_path`、`cwd`、`hook_event_name`、`model`。
- 官方 payload 不提供 BalanceHub event ID，也不提供 `BALANCEHUB_CLI_INSTANCE_ID` 字段。事件 ID 由 bridge 在本地生成；关联 ID 只从 Hook 进程继承的同名环境变量读取。
- 首轮运行态只订阅 `SessionStart`、`UserPromptSubmit`、`Stop`、`Interrupt`、`SessionEnd`。不订阅 tool Hook，避免保存工具数据或制造高频 helper 进程。
- `SessionEnd` 不是终端窗口关闭的精确同义词；正常关闭、归档/删除及无客户端连接后空闲 30 分钟都可能触发。`Stop` 和 `Interrupt` 仅投影为 idle，不投影为 ended。
- `SessionEnd` 默认时间预算很短，因此 bridge 必须只做有界 stdin 解码和原子落盘，所有失败 fail open 且不输出影响 Agent 上下文的内容。

## 首轮受管资源决策

1. `inspect`、`health`、`verify` 和 `repair plan` 始终只读。
2. `install` 在用户确认 plan 后，只向 `~/.codex/hooks.json` 的五个事件数组加入 BalanceHub 自有 matcher group；保留其他顶层字段、事件和 handler。
3. BalanceHub 通过结构化 handler 身份、写入后 fingerprint 和独立 ownership manifest 证明所有权；revision 或 fingerprint 漂移时停止写入。
4. `disable` 只移除仍与 manifest 一致的自有节点并保留 helper/manifest；`enable` 重新加入自有节点。
5. `remove` 只删除仍可证明归属 BalanceHub 的节点和 ownership 记录；没有所有权证明时返回 conflict，不修改原文件。
6. helper 使用当前 BalanceHub 可执行文件的隐藏 ingest 模式，不启动 Tauri 窗口；命令使用绝对路径并在 helper 缺失或落盘失败时成功返回。
7. 健康状态拆分为 installed、enabled、trust、helper、spool、last event、revision/conflict。首次真实 `SessionStart` 到达前不得显示 healthy。

## 不做的事情

- 不传入 `--dangerously-bypass-hook-trust`。
- 不修改 `requirements.toml`、managed/system/project Hook、Shell profile 或权限策略。
- 不读取 transcript 内容作为 Hook 运行证据。
- 不根据 cwd、时间窗口、Agent 名或当前默认配置猜 Provider、账号、Key 或 BalanceHub 实例关联。
