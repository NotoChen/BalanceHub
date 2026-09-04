# Research: Agent Runtime Hooks And External Session Discovery

- Query: BalanceHub 能否通过 Agent Hook 发现不经 App 启动的外部 Agent CLI 会话，并与现有临时 CLI、配置管理和 Windows WSL 形成可扩展的统一运行时能力？
- Scope: mixed
- Date: 2026-09-04

## Findings

### Executive conclusion

可以做，但产品名称和承诺必须准确：它应是用户显式启用的 **Agent Runtime Bridge**，不是“监控所有终端进程”。

四个已接入 Agent 都有官方生命周期 Hook，足以在事件发生时发现会话启动、每轮忙碌/空闲变化和正常结束，并取得稳定的 Agent 会话 ID 与工作目录。Hook 不普遍提供 Agent PID，也无法可靠定位终端窗口；CLI 崩溃、被强杀或 Hook 未被信任时，结束事件可能缺失。因此外部会话只能形成“事件证据”，不能伪装成与 BalanceHub 自己启动的临时 CLI 同等精确的进程管理。

推荐使用如下证据优先级：

1. 官方 Hook 事件是外部会话生命周期的首要证据。
2. 现有 Agent 会话解析器补充标题、模型和最近活动时间，但不单独证明进程仍在运行。
3. BalanceHub 自己启动的临时 CLI 继续使用现有状态文件和 PID 作为强证据。
4. OS 进程扫描最多作为“疑似仍有 Agent 进程”的弱证据，不能据此关联 session ID、Provider、账号、API Key 或终端窗口。
5. 长时间没有 Hook、会话文件变化或可关联的强证据时，外部会话进入“状态未知”，不得伪造为正常结束。

### Files found

- `src-tauri/src/services/agent_cli.rs` - Agent 注册表和当前能力声明；尚无 Hook、运行时发现或配置资源管理能力。
- `src-tauri/src/services/temporary_cli.rs` - BalanceHub 在打开终端前主动登记临时 CLI 实例。
- `src-tauri/src/services/cli_runtime.rs` - 当前实例只从 BalanceHub 临时目录加载，并依赖状态文件与 PID 调和状态。
- `src-tauri/src/models/provider_results.rs` - 当前 `TemporaryCliInstance` 强制携带 Provider、Terminal 和 PID 语义，不适合直接冒充外部会话。
- `src-tauri/src/services/temporary_cli/terminal/windows.rs` - Windows 仅注册 Windows Terminal、Command Prompt、PowerShell，没有 WSL terminal/runtime 定义。
- `src-tauri/src/services/temporary_cli/shell_runtime/script/windows.rs` - Windows 启动脚本是 cmd/PowerShell 原生链路，不会进入 WSL。
- `src-tauri/src/services/agent_cli/claude/sessions.rs` - 已能从 Claude transcript 补充会话标题和模型。
- `src-tauri/src/services/agent_cli/codex/sessions/index.rs` - 已能从 Codex 数据库、session index 和 rollout 补充标题、模型和会话路径。
- `src-tauri/src/services/agent_cli/gemini/sessions.rs` - 已能从 Gemini 会话文件提取标题、模型和时间。
- `src-tauri/src/services/agent_cli/grok/sessions.rs` - 已能从 Grok Build summary 提取标题、当前模型和时间。
- `AGENTS.md` - 要求 Rust 作为业务真源、跨平台真实降级、异步状态可收口、平台脚本使用 BalanceHub 命名空间。
- `.trellis/spec/guides/cross-layer-thinking-guide.md` - 要求外部 JSON 事件只在一个边界解码，并由单一 reducer 回放状态。
- `.trellis/spec/guides/code-reuse-thinking-guide.md` - 要求相同事件字段不由多个消费者重复解析，状态迁移集中处理。

### Current implementation boundary

当前 `AgentCliDefinition` 只声明临时启动、会话、测活和默认配置适配器；能力模型也只投影这些能力（`src-tauri/src/services/agent_cli.rs:21-35`, `src-tauri/src/services/agent_cli.rs:37-59`）。这说明 Hook 发现不应被硬塞进某个现有 session parser，而应成为 Agent 定义上的独立可选 adapter/capability。

BalanceHub 启动临时 CLI 时，在打开终端之前调用 `register_instance`，同时记录 Provider、账号标签、Agent、工作目录和终端（`src-tauri/src/services/temporary_cli.rs:200-208`）。运行时目录仅为系统临时目录下的 `balancehub-cli-runtime-v1/instances`（`src-tauri/src/services/cli_runtime.rs:23-30`, `src-tauri/src/services/cli_runtime.rs:325-329`）；加载逻辑只遍历这个目录（`src-tauri/src/services/cli_runtime.rs:222-249`）。所以外部终端天然不会进入当前列表。

当前运行状态由启动脚本写入的 `status.json` 和 PID 决定：启动超过 2 分钟、运行 PID 消失，或无 PID 超过 24 小时会转为 exited（`src-tauri/src/services/cli_runtime.rs:272-295`）。Unix 用 `kill(pid, 0)`，Windows 用 `tasklist` 检查 PID（`src-tauri/src/services/cli_runtime.rs:429-459`）。这套模型对 App 自己启动的进程有效，但外部 Hook 没有可靠 PID，不能直接复用其“Running -> Exited”判断。

现有 `TemporaryCliInstance` 还要求 `provider_id`、`provider_name`、`terminal_kind`，并提供 `can_activate`（`src-tauri/src/models/provider_results.rs:698-719`）。外部会话无法从官方 Hook 可靠得到这些字段。正确做法是新增通用运行会话模型，明确 `origin = balancehub | external_hook` 和证据强度，而不是填充虚构 Provider 或终端。

### Official hook capability matrix

本机核验版本：Claude Code `2.1.260`、Codex CLI `0.153.2`、Gemini CLI `0.58.0`、Grok Build `1.0.5`。版本只表示本机当前安装状态，不应写死为产品支持上限。

| Agent | 建议事件 | Hook 直接字段 | 可直接推导状态 | 关键限制 |
| --- | --- | --- | --- | --- |
| Claude Code | `SessionStart`, `UserPromptSubmit`, `Stop`, `StopFailure`, `PostModelSwitch`, `CwdChanged`, `SessionEnd` | `session_id`, `transcript_path`, `cwd`; `SessionStart` 可选 `model`, `session_title`; ModelSwitch 提供前后模型 | 启动、忙碌、空闲、失败后空闲、模型/目录变化、正常结束 | `SessionStart.model` 可缺失；无 PID；强杀无法触发结束；SessionEnd 总预算短 |
| Codex | `SessionStart`, `UserPromptSubmit`, `Stop`, `Interrupt`, `SessionEnd` | `session_id`, `transcript_path`, `cwd`, `model`; turn 事件另有 `turn_id` | 启动、忙碌、空闲、中断、结束证据 | 无标题/PID；transcript 格式不是稳定 Hook API；切走会话不会立即 SessionEnd，空闲且无客户端连接 30 分钟后才可能结束 |
| Gemini CLI | `SessionStart`, `BeforeAgent`, `AfterAgent`, `SessionEnd` | 公共字段 `session_id`, `transcript_path`, `cwd`, `timestamp` | 启动、忙碌、空闲、正常结束意图 | Hook 不直接给模型/标题/PID；`SessionEnd` 是 best effort，CLI 不等待执行完成 |
| Grok Build | `SessionStart`, `UserPromptSubmit`, `Stop`, `StopFailure`, `SessionEnd` | camelCase `hookEventName`, `sessionId`, `cwd`, `workspaceRoot` | 启动、忙碌、空闲、失败后空闲、结束证据 | 无模型/标题/transcript/PID；当前官方 Hook 文档未承诺结束必达；字段命名与另外三个 Agent 不同 |

Claude 官方说明 Hook 在 terminal、IDE、Desktop 和 Web 中使用同一事件系统；`SessionStart` / `SessionEnd` 每会话触发，`CwdChanged` 跟随实际目录，`PostModelSwitch` 可追踪模型变化。公共字段明确包含 session ID、transcript path 和 cwd。只有 `SessionStart` 可选带 model，且可能省略；`SessionStart` 可选带已有 `session_title`。这使 Claude 的运行元数据最完整，但仍没有 PID。

Codex 官方 Hook 文档明确公共字段包含 session ID、transcript path、cwd 和 model，同时明确 transcript 格式不是稳定 Hook 接口。其 `SessionEnd` 语义不是简单的“终端窗口关闭”：正常退出、归档/删除打开的会话，或会话无客户端连接并空闲 30 分钟都可触发；切换会话或 unsubscribe 不会立即触发。因此 UI 必须把 `Stop` 理解为“本轮空闲”，而不是“会话结束”。

Gemini 官方文档及本机 `0.58.0` 安装包都包含 `SessionStart`、`SessionEnd`、`BeforeAgent` 和 `AfterAgent`。公共字段没有 model；`SessionEnd` 明确是 best effort，CLI 不会等待 Hook 完成。BalanceHub 可以利用现有 parser 从消息补 model，但不能把该派生字段标成 Hook 强证据（`src-tauri/src/services/agent_cli/gemini/sessions.rs:360-409`）。

Grok 官方文档当前列出 `SessionStart`、`SessionEnd`、`UserPromptSubmit`、`Stop`、`StopFailure` 等事件，并使用 camelCase 输入。现有 Grok parser 可从 `generated_title/session_summary/last_turn_summary` 和 `current_model_id` 补充标题、模型（`src-tauri/src/services/agent_cli/grok/sessions.rs:800-848`）。官方当前 Hook 页面没有列出 `StopCancelled`，第一版不应依赖未文档化事件。

### Why statusline is not the primary bridge

Claude 和 Grok 的自定义 status line 都能得到更丰富的持续会话数据。Claude statusline 包含 `session_id`、`session_name`、`transcript_path`、model 和 cwd；Grok 的 command status line也包含 session ID/name、transcript、model 和 cwd。

但 statusline 是用户可见 UI 的唯一/主要配置位。BalanceHub 接管它会覆盖或包裹用户已有展示逻辑，运行频率也明显高于生命周期事件。它适合作为“已存在配置的可视化归纳对象”，不适合作为默认运行时探针。Hook 是低侵入主通道；statusline 最多是用户主动选择的高级增强，第一版不做。

### Recommended architecture

```text
Agent official lifecycle hook
        |
        v
balancehub-agent-bridge sidecar/helper
  - bounded stdin read
  - per-Agent decoder
  - allow-listed metadata only
  - one atomic event file per invocation
        |
        v
BalanceHub app-data spool/incoming
        |
        v
Rust runtime event normalizer + idempotent reducer
        |                         |
        |                         +-> existing session adapters enrich title/model/time
        v
Agent runtime projection
  - BalanceHub-launched (strong PID/status evidence)
  - externally discovered (hook evidence, read-only)
        |
        v
IPC snapshot/event -> Agent session/runtime UI
```

#### Bridge helper

使用一个随 App 打包、启用时复制到用户 App data 稳定路径的小型 Rust helper，而不是 HTTP localhost 服务或依赖 `jq`、Node、Python 的脚本：

- App 未运行时 Hook 仍可记录事件；不需要后台常驻进程。
- helper 只从 stdin 读取有上限的 JSON，按命令参数指定的 Agent decoder 解析。
- helper 不访问网络、不读取 transcript、不输出 stdout，不得影响 Agent 上下文。
- helper 单次调用设严格时间预算；解析或落盘失败时 fail open，Agent 会话继续。
- 每次 Hook 写一个独立的临时文件，再原子 rename 到 `spool/incoming/`，避免多个终端并发追加同一 JSONL 导致交错或 Windows 文件共享冲突。
- 文件名含 helper 生成的 event ID；Rust 消费者按 event ID 幂等去重。不要把供应商 timestamp 当作唯一游标。
- helper 只做轻量配额判断；App 启动后负责批量消费、清除已处理文件和执行有界保留。
- Hook command 使用绝对路径；卸载/路径失效时包装命令必须静默成功，避免给用户每次 Agent 启动制造错误。

不建议 localhost HTTP：App 不运行时事件会丢失，还引入端口占用、来源认证和防火墙问题。不建议只监听会话文件：文件格式不是所有 Agent 的稳定 API，文件变化也不能区分“仍打开但空闲”和“已经崩溃”。

#### Normalized event contract

建议由 Rust 定义唯一公共事件，TypeScript 只消费投影：

```text
RuntimeEvent {
  schema_version,
  event_id,
  agent_kind,
  event_kind,          // session_started | turn_started | turn_idle |
                       // turn_failed | interrupted | model_changed |
                       // cwd_changed | session_ended
  session_id,
  occurred_at,
  received_at,
  cwd,
  transcript_path?,
  model?,
  session_title?,
  start_source?,
  end_reason?,
  balancehub_instance_id?
}
```

禁止保存 `prompt`、`prompt_response`、assistant content、tool input/output、环境变量和任何凭据。虽然某些 Agent 在 turn Hook 中提供正文，bridge decoder 必须主动丢弃，而不是先完整持久化再脱敏。

各 Agent 原始 payload 只允许在 `agent_cli/<agent>/runtime_hooks` 边界解析一次；公共 reducer 只接受 normalized event。这样符合跨层规范要求，避免 UI、IPC 和多个服务各自维护 snake_case/camelCase 差异。

#### Runtime state model

外部会话至少需要以下状态：

- `active_busy`: 最近证据是 prompt/before-agent 开始。
- `active_idle`: 最近证据是 stop/after-agent/interrupt。
- `ended`: 收到明确 `SessionEnd`。
- `unknown`: 曾经活跃，但超过配置的证据时限且没有明确结束。

状态 reducer 必须按 `(runtime_scope, agent_kind, session_id)` 聚合并处理乱序事件。`runtime_scope` 至少区分 host-native 和 WSL distro，避免不同运行环境的 session ID 意外碰撞。

“状态未知”不是错误，而是 Hook 证据模型的必要结果。外部终端可能仍开着但长时间空闲，也可能已被强杀；没有 PID 或结束事件时无法诚实地区分。UI 可显示“最后活动于 …，当前状态无法确认”，但不能自动标成 exited。

#### Deduplicate App-launched sessions

BalanceHub 自己启动临时 CLI 时，向启动环境注入 `BALANCEHUB_CLI_INSTANCE_ID`。Hook helper 从继承环境读取该值并写入 `balancehub_instance_id`：

- 有合法实例 ID：把 Hook 的 session ID、模型和标题补到现有实例，不创建第二条“外部发现”记录。
- 无实例 ID：创建独立 external runtime projection。
- 外部记录不得根据当前默认配置猜测 Provider/账号/API Key；默认配置可在会话期间变化，且相同 endpoint 可能有多个 Key。

这种关联比命令行、cwd、时间窗口或 PID 猜测稳定，也为未来新增 Agent 保留了统一接入点。

### Hook installation and ownership

必须由用户按 Agent 主动启用，安装前展示结构化 diff；安装完成不等于生效，UI 还应显示“待 Agent 信任/待新会话验证”。

官方配置能力不同：

- Claude Code：user/project/settings 和 plugin hook 会合并；plugin 的 `hooks/hooks.json` 可隔离 BalanceHub 配置。Claude 没有“仅禁用 settings 中某一 Hook”的通用开关，使用独立插件更利于启停和归属。
- Codex：可使用 `~/.codex/hooks.json` 或 plugin-bundled hooks；非 managed Hook 必须按定义 hash 审阅并信任，变化后需重新信任。独立插件或独立 `hooks.json` 比把 `[hooks]` 混入 `config.toml` 更容易管理。
- Gemini CLI：Hook 可来自 project/user/system settings 或 extension；官方支持 `/hooks enable|disable <name>`。优先使用 BalanceHub extension 或带稳定 name 的 user Hook，不能覆盖整个 `hooks` 对象。
- Grok Build：个人 Hook 原生就是 `~/.grok/hooks/*.json` 独立文件，最适合写 `balancehub-runtime.json`；project Hook 需要 trust。

所有 Agent 统一遵守：

1. 记录 BalanceHub 所拥有资源的路径、结构化身份和内容指纹。
2. 安装/升级时重新读取磁盘并生成 diff；发现用户修改则进入 conflict，不自动覆盖。
3. 卸载只删除能够证明属于 BalanceHub 的节点/文件，保留用户其他 Hook。
4. 配置写入使用 JSON/TOML 结构化解析和原子替换，不做字符串拼接。
5. 启用后由下一次真实 Agent 会话上报 `SessionStart` 完成验证；不能只因文件写入成功就显示“已工作”。
6. helper 升级保持事件 schema 向后可读；旧 helper 与新 App 的不兼容状态必须可诊断和修复。

### WSL assessment

当前实现没有 WSL 专门支持。代码搜索没有发现 `wsl.exe`、`\\wsl$`、distro 枚举或 Linux/Windows 路径翻译；Windows terminal registry 只有 Windows Terminal、Command Prompt 和 PowerShell（`src-tauri/src/services/temporary_cli/terminal/windows.rs:18-37`），Windows Terminal 启动也明确打开 `cmd /K`（`src-tauri/src/services/temporary_cli/terminal/windows.rs:124-133`）。Windows 启动脚本本身是 cmd 调 PowerShell（`src-tauri/src/services/temporary_cli/shell_runtime/script/windows.rs:43-50`）。因此“三端兼容”目前只代表原生 macOS/Linux/Windows，不能推导为 Windows App 已兼容 WSL 内的 Agent。

WSL 必须建模为独立 runtime scope，而不是 Windows 的一个目录：

- Windows 原生与每个 WSL distro 分别探测 Agent binary、版本、配置、会话和 Hook 状态。
- 通过 `wsl.exe -l -q` 枚举 distro；所有 distro 内命令都显式指定 distro，不能依赖用户当前默认发行版。
- 首选在 WSL Hook 中通过 WSL interop 调用 Windows 侧 bridge `.exe`，让事件直接进入 Windows App data。Microsoft 官方确认 WSL 可直接运行 Windows 工具；但用户可禁用 interop，所以必须有 capability probe 和明确失败原因。
- 不把 `\\wsl$` 文件监听当作唯一传输。Microsoft 官方建议按所用 OS 将项目放在对应文件系统，跨文件系统访问有性能与语义差异；事件桥接应避免高频遍历整个 Linux home。
- WSL cwd 是 Linux 路径。模型中同时保留原始路径和 runtime scope；只有在明确需要 Windows 打开目录时才做受控转换。
- WSL 里的 Hook 配置属于该 distro 的 Linux home，需要逐 distro 用户确认、安装、信任与验证。

建议第一阶段只交付原生 macOS/Linux/Windows Bridge，并在 Windows UI 明确显示“WSL 尚未启用”；第二阶段再做 WSL distro 管理和 bridge interop。把 WSL 混入第一阶段会同时引入 distro 生命周期、跨边界安装、路径映射、interop 禁用和独立配置发现，显著扩大验收矩阵。

### Performance, privacy and failure behavior

- 只订阅会话/turn/模型/目录事件，不订阅每次 tool call；这能显著降低 helper 进程数和 spool 写入量。
- 每个 payload、单个事件文件、spool 总字节数、文件数和保留天数均设硬上限。建议第一版默认最多 20 MiB、5000 个待处理事件、7 天；最终数值应作为设计参数确认，而不是散落常量。
- App 启动和后台增量消费采用有界批次，不能在 UI 线程同步解析整个 spool。
- 同一事件重复、乱序、损坏和未知 schema 都必须可跳过并记录聚合诊断，不能卡住后续事件。
- transcript path 可能暴露用户名和目录结构；IPC 展示只传需要的路径，日志默认不打印完整 payload。
- Hook 不应产生桌面通知或后台任务失败红点。配置未信任、helper 缺失和 spool 满应进入 Agent 集成诊断状态，由用户主动查看/修复。
- App 关闭不影响 Agent；helper 失败必须 fail open。BalanceHub 不能成为 Agent CLI 启动的单点故障。

### Suggested implementation dependency order

1. **Runtime contract**：新增 origin/evidence/runtime scope/status 模型和公共 reducer fixture，不接 UI、不改 Agent 配置。
2. **Bridge helper and spool**：实现有界输入、四种 payload decoder、原子单事件落盘、幂等消费、配额与损坏隔离。
3. **App-launched correlation**：注入 `BALANCEHUB_CLI_INSTANCE_ID`，验证同一会话不会双重展示。
4. **Per-Agent installers**：按官方配置机制逐个实现 inspect/plan/apply/remove/verify；每个 Agent 独立验收，不做一个写死四分支的大函数。
5. **Runtime projection/UI**：在 Agent 会话工作台汇总 App 启动和外部发现；外部记录只读且不提供终端激活。
6. **Native cross-platform validation**：macOS/Linux/Windows 各自验证 App 开/关、正常退出、强杀、配置冲突和 helper 缺失。
7. **WSL child task**：最后单独实现 distro scope、interop probe、per-distro installer 和路径语义。

这项能力应成为“Agent 会话工作台”的前置子任务或基础子任务，而不应直接塞进当前 `TemporaryCliInstance`。Hook 配置归纳/开关可复用同一套 managed-resource ownership，但完整 Skill/Plugin/MCP/Statusline 管理应另有设计，避免 Runtime Bridge 第一版同时变成全功能 Agent 配置中心。

### Acceptance evidence required

- App 未运行时，在外部终端启动四个 Agent，随后启动 App 能看到对应 external session。
- App 运行时事件在可接受延迟内出现，且 UI 不轮询扫描整个 home。
- App 启动的 CLI 只显示一个实例，Hook 只补充 session ID/model/title。
- 正常退出显示 ended；强杀或 Hook 丢失最终显示 unknown，不误报正常退出。
- 外部记录不出现虚构 Provider、账号、API Key、PID、terminal locator 或“可激活”。
- 已有用户 Hook 在安装、升级、禁用和卸载后字节级/结构级保留；冲突时不覆盖。
- 未信任 Hook 显示为“待信任/待验证”，不能显示“运行中”。
- payload fixture 断言 prompt、assistant、tool input/output、env 和凭据不会进入 spool、IPC 或日志。
- 并发 100 个事件、重复事件、乱序事件、半写文件、未知 schema 和 spool 超限均不会阻塞 Agent 或 App。
- Windows 原生验收不能代替 WSL 验收；未实现 WSL 时必须显式标注 unsupported。

### Product decisions still required

- 是否确认外部会话采用“显式启用、只采集运行元数据、不采集对话正文、无法确认时显示状态未知”的产品契约。
- App 未运行时是否必须继续记录。本文建议“必须”，否则 Bridge 相比当前 App 内启动记录的增益有限。
- 外部会话是否保持只读。本文建议第一版只读，不提供终端激活、kill、Provider/Key 绑定。
- 待处理事件和已结束外部会话的保留周期、文件数与容量上限。
- WSL 是否作为独立第二阶段，而不是与原生 Windows 首版绑在一起。
- 是否允许 BalanceHub 修改各 Agent 的用户级配置，并由用户在各 Agent 官方界面完成 Hook 信任。

### External references

- Claude Code Hooks Reference, accessed 2026-09-04: https://code.claude.com/docs/en/hooks.md
  - Event lifecycle, common input fields, `SessionStart` optional model/title, model switch, config sources, plugin Hook merge and trust behavior.
- Claude Code Status Line Reference, accessed 2026-09-04: https://code.claude.com/docs/en/statusline.md
  - Statusline session/model/path fields and frequent event-driven execution behavior.
- Codex Hooks Reference, accessed 2026-09-04: https://developers.openai.com/codex/hooks.md
  - Hook sources, plugin hooks, hash-based trust, stable common fields, unstable transcript warning and 30-minute idle SessionEnd semantics.
- Gemini CLI Hooks Overview, accessed 2026-09-04: https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/index.md
- Gemini CLI Hooks Reference, accessed 2026-09-04: https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md
  - Common schema, lifecycle/agent events, settings/extension sources, enable/disable commands and best-effort SessionEnd.
- Grok Build Hooks, accessed 2026-09-04: https://docs.x.ai/build/features/hooks.md
  - Per-file personal/project hooks, trust, event list, camelCase payload and fail-open behavior.
- Grok Build Status Line, accessed 2026-09-04: https://docs.x.ai/build/features/status-line.md
  - Session name/model/transcript fields; command statusline is documented as untested on Windows.
- Microsoft WSL file systems and interoperability, accessed 2026-09-04: https://learn.microsoft.com/en-us/windows/wsl/filesystems
  - Windows/Linux filesystem separation, `wsl.exe` interop, direct Windows-tool execution from WSL and cross-filesystem performance guidance.

### Related specs

- `AGENTS.md:21-24` - Rust IPC/capability source of truth and no duplicate compatibility logic.
- `AGENTS.md:28-33` - responsibilities, reuse, no dead code and scoped validation.
- `AGENTS.md:46-52` - asynchronous external-process work must have timeout/cancel/status and release UI busy state.
- `AGENTS.md:54-58` - generated platform scripts and environment variables require BalanceHub namespace and platform doctor.
- `AGENTS.md:78-86` - credentials and real local data must not leak into source, logs, screenshots or commits.
- `.trellis/spec/guides/cross-layer-thinking-guide.md:74-101` - decode external event payload once and expose typed projections.
- `.trellis/spec/guides/cross-layer-thinking-guide.md:105-122` - define boundary contracts and preserve source event IDs.
- `.trellis/spec/guides/code-reuse-thinking-guide.md:61-82` - prohibit repeated untyped payload extraction.
- `.trellis/spec/guides/code-reuse-thinking-guide.md:108-132` - state transitions belong in one exhaustive reducer.

## Caveats / Not Found

- No official Hook examined provides a portable, reliable Agent process PID. A hook helper can observe its own process ancestry, but shell wrappers, Node launchers, terminal hosts and Windows process trees make that a hint, not a stable contract.
- No official Hook provides a portable terminal window identifier. External sessions cannot safely reuse `can_activate` or the current Ghostty locator behavior.
- Provider、账号和 API Key are BalanceHub concepts. External Agent Hook payloads do not contain a trustworthy mapping to them.
- Claude/Codex/Gemini/Grok session/transcript storage formats may change independently of Hook schemas. Existing parsers are useful enrichment adapters but need versioned failure isolation.
- Grok Build current official Hook page does not document `StopCancelled`; do not rely on it without a pinned official version/schema test.
- This research did not mutate real Agent Hook configs or run live Hook smoke sessions, because planning/research scope is read-only outside the task `research/` directory. Live installation and trust behavior must be validated in an implementation child task with isolated test configs before touching the user's actual Agent settings.
- WSL was audited from source and official platform behavior; no Windows/WSL runtime was available in this macOS workspace for live verification.
