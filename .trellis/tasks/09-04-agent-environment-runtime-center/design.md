# 技术设计

## Architecture

```text
Agent registry -> environment/installation identity -> asset/config/version inventory

Launch repository/status file --------+
Official Agent Hook -> event spool ----+-> Runtime reducer -> AgentRuntimeSession projection
Session adapter -> title/model --------+
Exact PID/terminal evidence -----------+

Hook adapter -> inspect -> plan -> user confirmation -> apply -> verify
                              |                         |
                              +---- revision check -----+
```

`AgentRuntimeSession` 是 Rust 唯一业务合同。运行态由证据推导，前端不根据 Agent 名、目录或时间自行合并记录。

## Environment And Inventory Contract

- `AgentEnvironmentDescriptor` 标识 native 平台并为未来 WSL scope 留出稳定 ID；`AgentInstallation` 标识可执行文件、安装来源、版本和 channel。
- `AgentAssetRecord` 使用 capability-bearing category/source/effective-state，不建立包含当前四个 Agent 全字段的巨型结构。
- Agent adapter 决定配置/资产路径、scope、优先级、trust 和可用动作；Rust 计算 effective state，TypeScript 只展示。
- 配置预览通过 opaque file ID 重新解析 allow-list 路径，限制大小与 symlink 边界；敏感配置按仓库规则在 Rust IPC 前脱敏或只返回元数据。
- 版本按 `(agent, installation source, package, channel)` 合并 in-flight 并缓存；默认 latest stable、成功缓存六小时，使用 `src-tauri/src/network/`，不执行更新。

## Runtime Contract

- 稳定标识分离 `runtime_id`、可选 Agent `session_id` 和可选 `balancehub_instance_id`。
- `runtime_scope` 区分 native 与未来 `wsl:<distro-id>`；第一阶段只实现 native。
- `origin` 区分 `balancehub_launch` 与 `external_hook`，但状态不能只由 origin 推导。
- 状态为 `starting/busy/idle/ended/unknown`；Hook stop 只表示本轮 idle，只有强退出证据或官方 SessionEnd 才能结束。
- Provider、进程、终端、标题和模型均为可选证据；每项记录来源与观测时间。
- 动作由 `actions` 能力返回，只有精确 locator 才允许激活终端。

## Correlation And Reduction

- BalanceHub 启动环境写入 `BALANCEHUB_CLI_INSTANCE_ID`，helper 验证格式、runtime scope 和 repository 存在性后关联。
- 没有关联 ID 的 Hook 事件建立 external runtime；非法 ID 只产生聚合诊断，不丢事件也不错误合并。
- Reducer 以 event ID 幂等，分别处理供应商事件时间和本机接收时间；乱序事件不能让 ended 会话倒退为旧状态。
- Launch status 的精确 PID/exit code 是强证据；外部 Hook 超时只进入 unknown。
- Session adapters 只富化标题、模型和最近活动，不决定进程存活。

## Hook Bridge And Spool

- Helper 使用有上限的 stdin 和 Agent-owned decoder，只输出允许的运行元数据。
- 每个事件写入独立临时文件并原子 rename 到 App-data spool；不让并发 Hook append 同一 JSONL。
- Spool 默认上限为 5000 个事件、20 MiB、7 天，任一达到即拒绝新事件并成功退出；上限集中定义且可测试，不散落在 adapter。消费采用小批次和有界幂等窗口。
- App 启动、恢复前台和后台增量任务消费 spool；projection 持久化成功后才删除 incoming 文件。
- 任何 helper 错误均成功、快速退出，避免阻止 Agent 主流程。

## Managed Hook Resources

每个 Agent adapter 声明配置位置、结构化节点身份、官方信任与启停语义。公共层维护：

```text
OwnedHookResource {
  agent_kind,
  runtime_scope,
  config_path,
  structural_identity,
  content_fingerprint,
  helper_version,
  installed_at,
  last_verified_at?
}
```

- `inspect` 只读实际配置、ownership manifest、helper 与最近事件。
- `plan` 基于当前 revision 生成节点级变更和冲突说明。
- `install/remove/enable/disable` 在用户确认后重新校验 revision，再以结构化解析和原子替换应用；`repair` 本身只返回新的 plan，不能越过确认直接写入。
- `remove` 和回滚只撤销仍与本次写入结果一致的 owned 节点；不恢复整份旧文件。
- 健康扫描不会触发 apply。缺失、漂移、无权限或冲突仅形成状态与手动修复入口。
- 第一阶段 apply 只接受 user scope 的 BalanceHub-owned Hook 资源；trust store、系统/managed/project 配置、Shell profile 和非 owned 资产永远不在写入 allow-list，也不升级权限重试。
- `verify` 等待安装后的真实事件；文件存在最多证明 `installed_unverified`。

建议主状态：`not_installed`、`installed_untrusted`、`installed_unverified`、`healthy`、`disabled`、`conflict`、`helper_missing`、`spool_blocked`、`unsupported`。详情保留 installed/enabled/trusted/helper/spool/last-event/conflict 等正交事实。

## UI And Migration

- Agent 管理首页使用一份连续的列表式控制台，按稳定 `(agent, runtime scope)` target 展示安装、版本、Hook 状态和首层操作；不使用必须逐卡进入详情的导航结构。
- Hook 安装、启停、健康检查、验证、修复和删除都从列表行发起，并共用一个 plan 确认弹窗。整行不可点击，详情使用独立按钮，避免控制操作误触导航。
- Rust 的 `AgentHookInspection` 同时返回操作可用性和禁用原因；Vue 不从状态字符串复制权限判断。每行拥有独立 busy/error，单行操作不锁定其他 Agent 或整个设置页。
- Agent 详情仅保留安装证据、完整 Hook 诊断、资产与配置浏览。GUI PATH 深度扫描移入次级入口，不长期占据列表前方空间。
- 运行实例入口归入 Agent 会话工作台；统一列表按 Agent 分组并明确 BalanceHub 启动或外部发现来源。
- 第一阶段先新增环境/安装 identity 与只读盘点，再完成合同/reducer、适配 launcher、加入关联 ID、实现 helper/spool、完成 Codex 试点、接入其余 Agent，最后切换 UI。
- 长期状态使用后端事件推送，并在窗口恢复时用快照校准；只保留启动确认的短轮询。

## Compatibility And Rollback

- 优先以独立资源形式安装：Claude Code/Codex CLI 优先插件或独立 Hook 定义，Gemini CLI 使用稳定命名 extension/user Hook，Grok Build 使用独立个人 Hook 文件；最终以实施时官方 schema 和 fixture 为准。
- macOS、Linux、原生 Windows 分别验证；不把 Windows native 结果等同于 WSL。
- Adapter 失败隔离，不影响公共 reducer 和其他 Agent；未知 schema 显示 unsupported/degraded，不猜测解析。
- Hook UI、launcher adaptation 和旧合同删除分成可回退提交；只有统一投影稳定后才删除旧公开域。
- App 内提供明确的“删除 BalanceHub Hook”操作；系统卸载流程不静默改写 Agent 配置。App 被直接移除而 helper 缺失时，Hook 必须 fail open，重新安装后由健康检查提示用户处理。
