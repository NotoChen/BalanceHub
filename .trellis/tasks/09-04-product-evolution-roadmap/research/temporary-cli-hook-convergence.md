# Research: Temporary CLI And Hook Runtime Convergence

- Query: 现有临时 CLI 运行状态机制在引入 Agent Hook 后应删除、保留还是重构；如何形成可安装、可启停、可验证、可回滚且不破坏用户配置的统一 Agent 运行时能力？
- Scope: internal
- Date: 2026-09-04

## Findings

### Decision

**保留现有临时 CLI 的强证据机制，但把它重构为统一 `AgentRuntimeSession` 的一个证据来源；删除“临时 CLI 实例”作为独立产品域和独立 UI 真源。**

不能直接删除现有机制，因为 BalanceHub 启动的会话拥有官方 Hook 无法稳定提供的信息：Provider、账号、API Key 本地标识、用户选择的 Agent、工作目录、终端类型、启动实例 ID、启动失败和退出码。当前编排在打开终端前注册实例，任何脚本写入或终端启动失败都会显式标记退出（`src-tauri/src/services/temporary_cli.rs:200-249`）。这些是确定事实，不应降级成 Hook 或进程扫描猜测。

也不能原样保留两套列表。当前 `TemporaryCliInstance` 强制要求 Provider 和 Terminal，只有 `starting/running/exited` 三态（`src-tauri/src/models/provider_results.rs:690-720`）；外部 Hook 会话通常没有 Provider、PID、终端或可激活窗口。若硬填空值或默认值，会把未知事实伪装成已知；若另建“外部会话”列表，则同一个 BalanceHub 启动会话会被 status file 与 Hook 重复展示，并让顶栏计数、Provider 卡片计数和会话工作台长期维护两套真源。

正确的迁移边界是：

```text
BalanceHub launch registration/status file ----+
                                                |
Official Agent Hook -> durable event spool -----+-> Runtime reducer
                                                |       |
Session adapters -> title/model enrichment -----+       v
                                                AgentRuntimeSession projection
Optional process probe -> weak liveness --------+       |
                                                        v
                                            one runtime/session UI contract
```

现有 `cli_runtime` 不是被 Hook 替换，而是被拆为：

- `launch_instance_repository`：保存 BalanceHub 启动元数据、status file、PID、退出码和 terminal locator。
- `runtime_event_repository`：消费 Hook helper 的有界离线事件。
- `runtime_reducer`：合并多个证据源并推导统一状态。
- `runtime_projection`：向 IPC/UI 返回统一会话列表与可执行动作。
- `runtime_enrichment`：复用现有 Agent session adapter，按需补标题、模型和最近活动，不决定进程存活。

迁移完成后，`TemporaryCliInstance`、`CliRuntimeSnapshot.instances` 及前端同名状态应被统一合同替代；旧入口、旧计数和旧轮询不可作为兼容分支继续保留。

### Current behavior worth preserving

当前状态文件机制具备四个应保留的工程属性：

1. `instance.json` 与 `status.json` 分离，记录目录权限在 Unix 被限制为 `0700`（`src-tauri/src/services/cli_runtime.rs:107-161`, `src-tauri/src/services/cli_runtime.rs:331-340`）。
2. JSON 写入使用临时文件加 rename；Windows 还包含目标备份和失败恢复（`src-tauri/src/services/cli_runtime.rs:357-423`）。
3. Unix 启动脚本在 CLI 前写入 `running + $$`，退出后写入退出码；工作目录失败也会落终态（`src-tauri/src/services/temporary_cli/shell_runtime/script/unix.rs:36-61`）。
4. Windows 脚本通过 CIM 取得父进程 PID，所有变量使用 `BH_` 命名空间，并在各失败分支落 `exited`（`src-tauri/src/services/temporary_cli/shell_runtime/script/windows.rs:43-60`）。

当前调和规则是启动超过 2 分钟视为退出、有 PID 时按进程是否存在判断、无 PID 的 running 记录最多保留 24 小时、退出记录在活动列表中隐藏并在 2 分钟后清理、活动列表最多 80 条（`src-tauri/src/services/cli_runtime.rs:23-30`, `src-tauri/src/services/cli_runtime.rs:222-295`）。这些数值不能直接沿用到外部 Hook 会话：外部会话没有 PID 时，超时只能变为 `unknown`，不能伪造为 `ended`。

Unix 使用 `kill(pid, 0)` 检查存活；Windows 每次通过有 3 秒预算的 `tasklist` 检查（`src-tauri/src/services/cli_runtime.rs:429-459`）。统一运行时后，进程检查只能作用于具备强 PID 证据的 launch instance，不能按 Agent 名或 cwd 扫描后强行绑定 session。

前端启动确认目前以 200 ms 间隔轮询最多 30 秒，并要求 running 稳定 500 ms（`src/utils/temporary-cli-launch.ts:12-17`, `src/utils/temporary-cli-launch.ts:26-78`）；实例列表只在非空时每 4 秒刷新一次（`src/composables/useCliRuntime.ts:128-175`）。统一模型应保留“启动确认”这个 command-scoped 短轮询，但长期运行状态应改为后端事件推送加窗口重获焦/恢复时快照校准，避免随着外部会话数量增加而继续全量轮询和执行 Windows `tasklist`。

终端激活能力必须继续是可选能力。当前只有保存了 locator 的实例可激活（`src-tauri/src/services/cli_runtime.rs:180-199`），而 macOS 当前实际只有 Ghostty 能返回并使用精确 terminal ID（`src-tauri/src/services/temporary_cli/terminal/macos/launch.rs:81-96`, `src-tauri/src/services/temporary_cli/terminal/macos/activation.rs:5-13`）。Linux 和 Windows 启动均返回 untracked（`src-tauri/src/services/temporary_cli/terminal/linux.rs:201-254`, `src-tauri/src/services/temporary_cli/terminal/windows.rs:105-120`）。因此统一 UI 应按 `actions.can_activate` 展示，而不是看到 running 就默认可打开终端。

### Target runtime contract

Rust 应定义唯一运行时合同，前端只负责显示：

```text
AgentRuntimeSession {
  runtime_id,
  runtime_scope,          // native | wsl:<distro-id>
  origin,                 // balancehub_launch | external_hook
  agent_kind,
  agent_session_id?,
  balancehub_instance_id?,
  provider_ref?,          // provider/account/key local ids; never secrets
  workdir?,
  title?,
  model?,
  process_evidence?,      // pid, observed_at; only when exact
  terminal_evidence?,     // kind, locator; only when exact
  state,                  // starting | busy | idle | ended | unknown
  evidence[],             // source, observed_at, confidence
  started_at?,
  last_activity_at?,
  ended_at?,
  exit_code?,
  actions                 // activate, view_detail, resume, dismiss
}
```

`runtime_id` 是 BalanceHub 记录自身 projection 的稳定 ID；Agent 官方 `session_id` 是可选关联键，二者不能混为一谈。聚合主键至少包含 `(runtime_scope, agent_kind, agent_session_id)`，因为 Windows native 与不同 WSL distro 可能产生相同格式的本地 ID。

状态来自证据而不是来源名称：

| Evidence | Allowed transition | Forbidden inference |
| --- | --- | --- |
| launch registration | `starting` | 不能证明 CLI 已运行 |
| launch status running + exact PID | `starting -> idle`，随后 Hook 可转 busy | 不能仅凭 PID 推导正在生成回复 |
| Hook prompt/before-agent | `idle/unknown -> busy` | 不能证明终端可激活 |
| Hook stop/after-agent/interrupt/failure | `busy -> idle` | 不能当作会话结束 |
| launch status exited / exact PID gone | `* -> ended` for BalanceHub launch | 不应用到无精确 PID 的外部会话 |
| Hook SessionEnd | `* -> ended` | 未收到不能反推仍在运行 |
| external evidence timeout | `busy/idle -> unknown` | 不能伪造 `ended` |
| transcript/session DB activity | update title/model/activity metadata | 不能单独创建 active 或 ended 状态 |

Reducer 必须幂等处理 event ID，并按供应商事件时间和接收时间处理乱序。`ended` 为吸收态，除非收到同一 Agent session ID 的官方 resume/start 证据；恢复后是否复用 runtime ID，应由每个 Agent 的真实 session 语义 fixture 决定，不能在公共层猜测。

### Correlating BalanceHub launches with Hooks

启动脚本必须注入 `BALANCEHUB_CLI_INSTANCE_ID=<registered.id>`，Windows payload 同样通过 `setEnv` 注入。Hook helper 从继承环境读取该值并写入 normalized event：

- 值合法且 launch instance 存在：把 `agent_session_id`、busy/idle、标题和模型补入该 runtime，不创建外部记录。
- 值缺失：按 external Hook session 创建 runtime。
- 值非法、不存在或 runtime scope 不匹配：忽略关联值并记录聚合诊断，不把事件丢弃。
- 同一实例在 SessionStart 到达前仍按 status file 展示；到达后稳定合并，UI 不闪现第二条。

不得使用 cwd、时间窗口、Agent 可执行文件名或当前默认 Provider 猜测关联。一个目录可并发多个会话，默认配置可在会话中途变化，相同 URL 也可对应多个 Key。只有受命名空间保护的实例 ID 是可靠 join key。

### Hook management operations

Hook 管理必须是配置资源管理，不是一个“已安装”布尔值。公共服务为每个 Agent adapter 提供以下操作：

| Operation | Contract |
| --- | --- |
| `inspect` | 只读解析实际配置、BalanceHub ownership manifest、helper 版本、Agent trust/enable 状态和最近观测事件；不因读取失败修改配置。 |
| `plan` | 基于刚读取的磁盘 revision 生成结构化 node diff，说明将新增、替换或移除哪些 owned resource；同时列出用户配置冲突。 |
| `install` | 复制/更新稳定 helper，并只添加 plan 中确认的 BalanceHub-owned Hook 节点；应用前再次校验 revision。 |
| `remove` | 只移除可证明由 BalanceHub 拥有且身份匹配的节点/文件；不恢复整个历史配置文件。 |
| `enable` | 在 Agent 官方语义允许时启用已安装节点；不允许时通过添加/移除 owned registration 表达启用状态，并在 plan 中明确。 |
| `disable` | 保留 helper/manifest，仅停止未来 Hook 参与；不得禁用或删除用户 Hook。 |
| `health` | 分别返回 installed、enabled、trusted、helper reachable、last observed event、spool writable、conflict，不折叠为单一绿色状态。 |
| `repair` | 重新 inspect/plan，修复缺失或漂移的 owned helper/node；用户改动导致的冲突必须等待确认。 |
| `verify` | 通过下一次真实 Agent Hook event 验证；文件存在或安装命令成功只能到 `installed_unverified`。 |

建议的状态集合为：

```text
not_installed
installed_untrusted
installed_unverified
healthy
disabled
conflict
helper_missing
spool_blocked
unsupported
```

`health` 可同时包含多个诊断事实，UI 主状态按阻断级别投影。例如 helper 已安装、Agent 已启用但没有新会话时，应显示“已安装，等待新会话验证”，不是“异常”；最近一次 Hook 超时或失败也不进入全局失败通知红点。

### Ownership, conflict and rollback

每个受管资源写入 ownership manifest：

```text
OwnedHookResource {
  agent_kind,
  runtime_scope,
  config_path,
  structural_identity,   // plugin id / extension name / hook name / owned file name
  content_fingerprint,
  helper_version,
  installed_at,
  last_verified_at?
}
```

操作规则：

1. 使用 JSON/TOML/Agent 官方结构化 API 修改配置，不使用文本 replace。
2. 每次 plan 与 apply 前重新读取文件并计算 revision，避免覆盖用户刚发生的编辑。
3. 删除只匹配 structural identity 且 fingerprint 仍一致的 owned node。
4. fingerprint 漂移进入 `conflict`；除非用户审阅新 plan 并确认，不覆盖、不删除。
5. 原子写入采用同目录临时文件、flush、权限保持和 replace；不能在失败时留下半个配置。
6. 回滚只恢复本次操作修改的节点，并且仅当当前 post-state 仍等于本次写入结果；否则停止并报冲突。
7. 不保存和恢复整份旧配置快照。整文件恢复会删除安装后用户或其他工具新增的 Hook，是不可接受的回滚方式。

各 Agent adapter 只负责配置位置、schema、官方 enable/trust 语义和 raw payload decoder；ownership、revision、原子写入、plan/result、spool 与 reducer 都是共享实现。这样新增 Agent 时只注册能力和 adapter，不修改四分支公共函数。

### App-offline event spool

Hook helper 是随 App 发布的小型 Rust 二进制/子命令，安装时复制到稳定 App-data 路径。一次调用只执行：

1. 从 stdin 读取有硬上限的 payload。
2. 根据命令参数选择一个 Agent decoder。
3. 丢弃 prompt、assistant content、tool input/output、环境变量和凭据，只保留 allow-list 元数据。
4. 生成独立 event ID，写入同目录临时文件后原子 rename 到 `spool/incoming/`。
5. 无论 BalanceHub 是否运行、事件是否可写，均在严格时间预算内 fail open，不阻止 Agent。

不使用 localhost HTTP 或必须在线的 Unix socket 作为唯一通道，因为 App 关闭时会丢事件；也不让多个 Hook 进程 append 同一 JSONL，因为并发写入和 Windows 文件共享语义容易损坏整个队列。

Spool 需要文件数、总字节、单 payload、单事件和保留时长上限。第一版建议默认 5000 个事件、20 MiB、7 天，但这些应集中为可测试配置，不散落常量。满额时 helper 丢弃最晚事件或拒绝本次写入并成功退出，App 侧显示 `spool_blocked/degraded` 聚合诊断；不得删除用户 Agent 会话文件来腾空间。

App 启动、恢复前台及后台增量任务分批消费；损坏文件、未知 schema 和重复 event ID 单独隔离，不能阻塞后续文件。只有 projection 持久化成功后才能删除 incoming event。已处理 event ID 使用有界幂等窗口，不能无界增长。

### Product and UI convergence

运行实例入口应进入 Agent 会话工作台，而不是继续扩展 Provider 卡片专属“临时 CLI”弹窗。统一列表按 Agent 分组，并明确来源：

- BalanceHub 启动：可显示中转站、账号/Key 备注、终端、退出码，并按能力提供“定位终端”。
- 外部发现：显示 Agent、标题、模型、目录、最后活动和证据状态；没有可靠 Provider/终端时就不展示该字段和动作。
- `busy` 使用不定进度或活动状态，不制造百分比。
- `unknown` 展示“最后观测于 …，当前状态无法确认”，不标红为失败。
- 同一 Agent 的 Hook 安装、启停、信任、验证和冲突处理进入 Agent 管理页，不混入每个会话行。

现有顶栏与 Provider 卡片计数应消费统一 projection：顶栏按 Agent 汇总全部 active/unknown 会话，Provider 卡片只统计 `provider_ref` 明确匹配的 BalanceHub launch；不得把外部会话按当前默认配置归到某张卡片。

### Migration plan and dependencies

1. **Contract and reducer**：新增 `AgentRuntimeSession`、evidence、state transition 和 fixture；暂不接 Hook、不改 UI。覆盖重复、乱序、未知、ended/resume 和不同 runtime scope。
2. **Adapt current launcher**：把 launch instance repository 接到 reducer，并保持现有启动/退出/激活行为。此阶段可用兼容投影临时喂给旧 UI，但不能提交永久双真源。
3. **Correlation ID**：在 Unix/Windows 启动环境注入 `BALANCEHUB_CLI_INSTANCE_ID`，添加脚本快照与平台变量 doctor 测试。
4. **Helper and spool**：实现 payload 上限、Agent decoder、元数据 allow-list、原子单文件队列、批量消费、损坏隔离和配额。
5. **One Agent pilot**：优先选择 Hook schema 和隔离安装形式最稳定的 Agent，完成 inspect/plan/install/remove/enable/disable/health/repair/verify 全闭环，并在 App 开/关场景验收。
6. **Remaining native Agents**：每个 Agent adapter 独立接入和验收；公共 reducer/UI 不增加 Agent switch 分支。
7. **Unified UI cutover**：顶栏、Provider 卡片、会话工作台和激活动作切换到统一 projection；随后删除 `TemporaryCliInstance` 前端/Rust 合同与旧轮询。
8. **Native platform gate**：macOS、Linux、Windows 分别验证正常退出、强杀、App 离线、冲突、helper 缺失、spool 满和卸载。
9. **WSL child task**：原生三端闭环后再实现 distro 枚举、scope、interop probe、逐 distro 安装和路径展示；WSL 不是第一阶段的隐式子功能。

依赖关系：统一 runtime contract 是 Hook、UI 和 WSL 的共同前置；helper/spool 是 App 离线捕获的前置；单 Agent pilot 验证 ownership contract 后才能批量接入其他 Agent；统一 UI 只能在 current launcher 与至少一个 Hook adapter 都能投影后切换；WSL 必须等待 native helper 协议稳定。

### Rollback plan

- Contract/reducer 未切 UI 前可以直接回退，不影响现有启动链路。
- Pilot Agent 安装失败时，通过 ownership-aware `remove` 只撤销 BalanceHub-owned node 和 helper；保留 spool 便于诊断，用户可单独清除。
- UI 切换必须放在独立提交；回退时旧 launcher repository 仍保留强证据，不能先删除底层再切 UI。
- Agent adapter 更新失败时保持旧 helper/schema 可读，状态显示 `helper_missing` 或 `installed_unverified`，不得阻断 Agent 启动。
- 任何 fingerprint/revision 冲突都停止自动回滚，由用户选择保留磁盘现状或审阅新的 plan。

### Acceptance criteria

- BalanceHub 启动的会话在 Hook 启用和禁用时均能正确显示启动、运行、退出码和可选终端激活能力。
- Hook 启用后，BalanceHub 启动的会话只显示一条；`BALANCEHUB_CLI_INSTANCE_ID` 缺失或非法不会错误合并其他会话。
- App 完全退出期间启动并使用外部 Agent，重新打开 App 后可消费事件并显示会话；Hook helper 失败不影响 Agent CLI。
- 外部会话收到 turn stop 只进入 idle，未收到 SessionEnd 且证据过期只进入 unknown，不伪造 ended。
- 同 cwd 并发两个会话、不同 Agent 相同 session ID、Windows native 与两个 WSL distro 相同 session ID 均不串联。
- inspect/plan 无写入；install/enable/disable/remove/repair 只修改 BalanceHub-owned node；用户修改 owned node 后进入 conflict。
- 安装完成显示 `installed_unverified`；只有真实新会话事件到达后显示 `healthy`。
- App 关闭、spool 满、损坏事件、未知 schema、重复与乱序事件均有界处理，且不会产生全局失败红点或锁死 UI。
- IPC、日志和 spool 中不包含 prompt、assistant content、tool input/output、API Key、Token、Cookie、密码或完整环境变量。
- Hook adapter 和 runtime UI 不写死当前四个 Agent；新增 Agent 只需注册 capability、config adapter、decoder 和 fixture。
- 完成 `npm run build`、`npm test`、Rust fmt/clippy/test、`npm run doctor:platform`，并由 macOS/Linux/Windows CI 覆盖原生路径。

### Files found

- `src-tauri/src/services/temporary_cli.rs` - 临时 CLI 启动编排、实例预注册和失败收口。
- `src-tauri/src/services/cli_runtime.rs` - 当前实例 repository、状态调和、原子 JSON、PID 检查和激活目标。
- `src-tauri/src/services/temporary_cli/shell_runtime/script/unix.rs` - Unix status-file 写入、PID 和退出码证据。
- `src-tauri/src/services/temporary_cli/shell_runtime/script/windows.rs` - Windows launch payload、PID 获取、状态写入和命名空间变量。
- `src-tauri/src/services/temporary_cli/terminal/macos/launch.rs` - macOS 终端启动与 Ghostty locator 获取。
- `src-tauri/src/services/temporary_cli/terminal/macos/activation.rs` - Ghostty 精确窗口激活。
- `src-tauri/src/services/temporary_cli/terminal/linux.rs` - Linux 终端启动均为 untracked。
- `src-tauri/src/services/temporary_cli/terminal/windows.rs` - Windows Terminal/cmd/PowerShell 原生启动且均为 untracked。
- `src-tauri/src/models/provider_results.rs` - 当前 `TemporaryCliInstance` 与 `CliRuntimeSnapshot` IPC 合同。
- `src/utils/temporary-cli-launch.ts` - 启动确认短轮询与超时。
- `src/stores/cli-runtime.ts` - 前端运行时 store 和实例列表写回。
- `src/composables/useCliRuntime.ts` - 4 秒实例轮询、筛选、配置与激活状态。
- `.trellis/tasks/09-04-product-evolution-roadmap/research/agent-runtime-hooks-audit.md` - 四个 Agent 官方 Hook 能力、WSL 和 bridge 方案。
- `.trellis/tasks/09-04-product-evolution-roadmap/research/github-agent-dynamic-island-audit.md` - 七个开源会话观察器的 Hook、进程、离线捕获与 UI 证据审计。

### Code patterns

- 先注册再启动，并在每个失败出口落终态：`src-tauri/src/services/temporary_cli.rs:200-249`。
- 状态文件与元数据分离，读取时进行保守调和：`src-tauri/src/services/cli_runtime.rs:92-104`, `src-tauri/src/services/cli_runtime.rs:261-295`。
- 临时文件加 rename 的原子 JSON 写入：`src-tauri/src/services/cli_runtime.rs:357-423`。
- Unix/Windows 启动脚本均在 CLI 启动前写 running、退出后写 exited：`src-tauri/src/services/temporary_cli/shell_runtime/script/unix.rs:36-61`, `src-tauri/src/services/temporary_cli/shell_runtime/script/windows.rs:43-60`。
- 可激活能力来自精确 locator，而不是运行状态推测：`src-tauri/src/services/cli_runtime.rs:180-199`, `src-tauri/src/services/cli_runtime.rs:297-322`。
- 前端异步启动与激活都有超时/finally，符合不可锁死主界面的约束：`src/composables/useWorkspaceLaunchFlow.ts:174-253`, `src/composables/useCliRuntime.ts:287-299`。

### External references

- Claude Code Hooks Reference, accessed 2026-09-04: https://code.claude.com/docs/en/hooks.md
- Codex Hooks Reference, accessed 2026-09-04: https://developers.openai.com/codex/hooks.md
- Gemini CLI Hooks Overview, accessed 2026-09-04: https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/index.md
- Gemini CLI Hooks Reference, accessed 2026-09-04: https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md
- Grok Build Hooks, accessed 2026-09-04: https://docs.x.ai/build/features/hooks.md
- Microsoft WSL file systems and interoperability, accessed 2026-09-04: https://learn.microsoft.com/en-us/windows/wsl/filesystems
- OpenIsland, inspected revision `334c58073ec0ea8a1b34da0c71f969b1affd0959`: https://github.com/Octane0411/open-vibe-island
- agent-notch, inspected revision `443c0f5b6e66cf2bb8d1dcfb2865906d2f2b2d75`: https://github.com/realfishsam/agent-notch

### Related specs

- `AGENTS.md` - Rust 真源、Agent 动态注册、异步收口、三端降级、平台变量命名和隐私边界。
- `.trellis/workflow.md` - 研究持久化、任务阶段和实现/检查边界。
- `.trellis/spec/guides/cross-layer-thinking-guide.md` - 外部事件只在边界解析一次，由公共 reducer 投影。
- `.trellis/spec/guides/code-reuse-thinking-guide.md` - 复用现有状态文件、Agent session adapter 和终端目录，避免平行实现。
- `.trellis/spec/frontend/state-management.md` - 前端异步状态、防过期和单一真源要求。
- `.trellis/tasks/09-04-product-evolution-roadmap/prd.md` - Agent 会话工作台与共同工程约束。
- `.trellis/tasks/09-04-product-evolution-roadmap/design.md` - Agent registry、会话能力与类型化后台任务的共享边界。

## Caveats / Not Found

- 本研究只定义收敛设计，没有修改产品代码、安装任何 Hook、启动 Agent 会话或执行三端 CI。
- 官方 Hook 不能普遍提供 PID、TTY 或终端窗口 locator；外部会话第一版只能观察，不能承诺定位、关闭或控制终端。
- `SessionEnd` 在不同 Agent 上存在延迟或 best-effort 语义；`unknown` 是必要的真实状态，不是待修复错误。
- 当前临时运行记录位于系统临时目录，不适合作为 App 离线 Hook spool 的稳定位置；新 spool 必须位于隔离的 App data/cache 路径，但不能与用户主配置事务混写。
- Windows 当前仅支持 native Windows Terminal、cmd 和 PowerShell；仓库没有 WSL distro、interop、Linux home Hook 或路径映射实现。WSL 必须作为后续独立子任务验收。
- `BALANCEHUB_CLI_INSTANCE_ID` 当前尚未写入 Unix/Windows 启动环境；在完成关联 ID 前启用 Hook UI 会产生重复记录。
- 当前 4 秒全量轮询在 Windows 可能反复执行 `tasklist`；统一 UI 扩展到外部会话前必须改成事件驱动加快照校准，否则会话规模扩大后存在不必要的进程与 IPC 开销。
- 各 Agent 的 Hook 配置位置、信任和启停语义不同，不能只靠一份通用 JSON 模板完成；公共操作合同相同，但必须逐 Agent fixture 验证。
