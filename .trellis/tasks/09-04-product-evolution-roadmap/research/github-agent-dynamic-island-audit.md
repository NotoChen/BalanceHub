# Research: GitHub Agent Dynamic Island Implementations

- Query: GitHub 上已有的 Agent Dynamic Island、notch、HUD、menu bar 和 session monitor 项目，实际如何发现外部 Agent 会话、补充 PID/TTY、判断运行状态，并处理 App 未运行时的事件？哪些设计适合 BalanceHub？
- Scope: mixed
- Date: 2026-09-04

## Findings

### Executive conclusion

已审计的七个代表项目没有一个同时解决以下四项：官方 Hook 事件、App 未运行时不丢事件、可靠的 session-to-PID/TTY 关联、原生 macOS/Linux/Windows 跨平台。

其中只有 OpenIsland 接近完整的多 Agent 运行时观察器，但它的 Hook 直接通过 Unix socket 发给正在运行的 App；App 或 socket 不可用时事件被 fail-open 丢弃。`claude-menubar` 的 `SessionStart` Hook 会直接写状态文件，能留下少量持久映射，但它只记录启动，不记录完整生命周期。其他项目主要依靠 `ps`/`lsof`、会话数据库、transcript 文件更新时间或当前 Claude statusline 输入进行推断。

因此，BalanceHub 不应照搬任一项目，而应组合其已验证部分：

1. 官方 Agent Hook 是外部会话事件的主证据。
2. Hook helper 将元数据事件原子写入有界 spool，解决这些项目普遍存在的“App 不运行即丢事件”。
3. OS 进程扫描只补充当前进程存在、TTY、cwd 和终端信息，不把 cwd 相同误当作 session-to-PID 精确关联。
4. transcript/session database 只补标题、模型、时间和历史，不单独证明进程仍在运行。
5. UI 明确区分 `busy`、`idle`、`ended`、`unknown` 及证据来源；没有真实百分比时只显示不定进度。

### Repositories and inspected revisions

| Project | Revision | Date | License at inspection | Classification |
| --- | --- | --- | --- | --- |
| [Octane0411/open-vibe-island](https://github.com/Octane0411/open-vibe-island) | `334c58073ec0ea8a1b34da0c71f969b1affd0959` | 2026-09-03 | GPL-3.0 | Hook + process + transcript + registry hybrid observer |
| [realfishsam/agent-notch](https://github.com/realfishsam/agent-notch) | `443c0f5b6e66cf2bb8d1dcfb2865906d2f2b2d75` | 2026-07-21 | MIT | macOS process monitor with transcript enrichment |
| [maxbogo/CodexDynamicIsland](https://github.com/maxbogo/CodexDynamicIsland) | `222e60952ca9b65c59011997a3f5af27712605ed` | 2026-05-04 | MIT | Codex transcript activity approximation |
| [zxygeitio/claude-dynamic-island](https://github.com/zxygeitio/claude-dynamic-island) | `13c54fbefad43fc43529b6d1f46a3bb107219d88` | 2026-04-30 | no detected license | Windows Claude HTTP Hook monitor/control UI |
| [jarrodwatts/claude-hud](https://github.com/jarrodwatts/claude-hud) | `939eb66485832dead1b0a28a954f76f7aa2bdb06` | 2026-08-28 | MIT | current-session statusline renderer, not external observer |
| [qianxiaofeng/claude-menubar](https://github.com/qianxiaofeng/claude-menubar) | `c46fcfd5a5a47532e92ae5940fd6dba44077ff35` | 2026-03-03 | no detected license | Claude start Hook + macOS process/TTY poller |
| [MongLong0214/claude-codex-session-monitor](https://github.com/MongLong0214/claude-codex-session-monitor) | `4dab12d8c202f30f4b15fb70024953adc257066b` | 2026-07-12 | no detected license | macOS DB/transcript/process polling dashboard |

GitHub star counts observed on 2026-09-04 were respectively 1977, 304, 7, 6, 27815, 0 and 0. Stars are discovery context only, not implementation-quality evidence.

### Comparison matrix

| Project | Primary event source | PID / TTY linkage | App not running | What is inferred by timeout | Main limitation |
| --- | --- | --- | --- | --- | --- |
| OpenIsland | Official Hooks over Unix socket; extensions for Pi family | `ps`/`lsof`, environment metadata, terminal-specific lookup; mapping quality differs by Agent | Hook send fails open and the event is lost; persisted registries/transcripts can later restore partial state | Two missed process polls end most hook sessions; 45 s heartbeat expiry for Pi family | macOS-specific process/terminal layer; content-heavy control scope is broader than BalanceHub needs |
| agent-notch | `ps` + one batched `lsof`, transcript tails | Strong current PID/TTY evidence; Codex rollout FD often maps; Claude falls back to cwd/TTY | No live capture; current processes and files are rediscovered after restart | Busy if recent activity is under 30 s; removal after two missed 3 s polls | TTY-only, macOS-only, mapping can be ambiguous |
| CodexDynamicIsland | Codex rollout JSONL and session index | None | Rollout files survive and latest 20 are rescanned | Phase follows last parsed rollout events; no independent process-exit proof | Can display historical/idle state as if it were runtime state |
| Claude Dynamic Island | Claude HTTP Hooks to localhost Tauri/Axum server | None | HTTP delivery fails while App/server is absent | Approval/selection timeouts only | No durable event queue; no lifecycle/process reconciliation; localhost transport expands attack/config surface |
| Claude HUD | Claude statusline stdin + current transcript | None | Not applicable: Claude invokes it only for the current session | Cache freshness only, not runtime liveness | Cannot aggregate or activate external sessions; occupies statusline integration |
| claude-menubar | `SessionStart` Hook state file plus 2 s process poll | `pgrep`/`ps` for PID/TTY, `lsof` for cwd; Hook walks ancestors to Claude PID | Start mapping can still be written by Hook; no later lifecycle capture while menu app is off | Transcript mtime drives active/pending/idle; pending expires at 120 s; process presence is the liveness gate | Only Claude Hook start event; recursive Codex scan and per-process commands are costly |
| Agent Session Monitor | Codex SQLite + rollout tails; Claude transcripts; `ps`/`lsof`; 1.5 s poll-and-diff SSE | Codex PIDs grouped by cwd, not session; Claude has no PID mapping | No live capture; local source data remains for rescan | Codex recent activity <= 5 min becomes running; Claude is entirely file-recency classified | Cwd-level signals can affect unrelated sessions; “running” can be a recency estimate |

### OpenIsland: strongest hybrid, but Hook events require a live App

OpenIsland implements the most complete architecture in this sample:

```text
Official Agent Hook -> OpenIslandHooks CLI -> Unix domain socket
-> BridgeServer -> AppModel/SessionState -> UI
```

The Hook CLI reads stdin, decodes an Agent-specific payload and sends it directly to a Unix socket (`Sources/OpenIslandHooks/OpenIslandHooksCLI.swift:41-62`, `Sources/OpenIslandCore/BridgeCommandClient.swift:11-32`). If the bridge is unavailable it logs and returns, intentionally allowing the Agent to continue (`Sources/OpenIslandHooks/OpenIslandHooksCLI.swift:62-64`, `Sources/OpenIslandHooks/OpenIslandHooksCLI.swift:80-82`, `docs/hooks.md:19-21`). There is no durable queue in this path, so an App outage loses the event itself.

Its process discovery is explicitly supplementary. A snapshot can contain Agent, session ID, cwd, TTY, terminal app, transcript, tmux target and socket (`Sources/OpenIslandApp/ActiveAgentProcessDiscovery.swift:12-39`). Most candidates without a TTY are excluded, with special cases for IDE-style processes (`Sources/OpenIslandApp/ActiveAgentProcessDiscovery.swift:69-75`). Codex and Claude use different mapping logic and fallback claim keys (`Sources/OpenIslandApp/ActiveAgentProcessDiscovery.swift:78-102`). This proves that a single generic “find Agent process” rule is not enough.

The reducer keeps Hook lifecycle and process fallback separate. A hook-managed session ends after two consecutive missing-process polls when no `SessionEnd` arrives (`Sources/OpenIslandCore/SessionState.swift:422-454`); Pi/Oh My Pi are excluded because they lack reliable process mapping and instead expire from a per-session heartbeat (`Sources/OpenIslandCore/SessionState.swift:434-437`, `Sources/OpenIslandCore/SessionState.swift:481-511`). OpenIsland also distinguishes turn completion from session teardown in its event model (`Sources/OpenIslandCore/AgentEvent.swift:112-115`).

OpenIsland restores recent registries and discovers recent Codex/Claude files at startup (`Sources/OpenIslandApp/SessionDiscoveryCoordinator.swift:93-130`, `Sources/OpenIslandApp/SessionDiscoveryCoordinator.swift:156-205`). This repairs the visible snapshot but cannot reconstruct Hook transitions that were never delivered.

Transferable ideas:

- Agent-specific decoders behind one normalized event contract.
- Separate lifecycle evidence, process liveness and transcript enrichment.
- Two-poll debounce before declaring a process gone.
- Persisted runtime projection and startup reconciliation.
- Low-noise default Hook sets; tool-level events remain opt-in (`docs/hooks.md:51-59`).

Do not copy:

- Direct socket-only delivery without a durable spool.
- One-hour or 24-hour blocking approval Hooks (`docs/hooks.md:342-352`).
- Prompt, assistant response, tool input/output and permission-control capture for BalanceHub's initial metadata-only observer.
- AppleScript/terminal-specific behavior as a cross-platform foundation (`docs/hooks.md:413-426`).

### agent-notch: process liveness is real, “busy” is still a recency guess

`agent-notch` does not use Hooks. It treats OS process discovery as authoritative liveness and explicitly requires a terminal-attached process (`main.swift:20-36`, `main.swift:52-68`). One batched `lsof` call covers all candidate PIDs (`main.swift:89-93`). Codex usually exposes an open rollout file, while Claude often closes its transcript and therefore falls back to cwd/TTY (`main.swift:39-43`, `main.swift:67-83`).

The project sharply separates two facts: `isLive` comes from current process discovery, while `isBusy` requires that liveness plus transcript activity within 30 seconds (`main.swift:20-25`). It reads bounded transcript windows: 128 KiB from the tail, a 64 KiB head fallback for model metadata, and 256 KiB for Codex metadata (`main.swift:305-365`).

Its latest commit is direct evidence of polling fragility: under load, very short subprocess timeouts made every `lsof` call fail and falsely moved Agents to done. The fix batches `lsof`, raises command budgets to two seconds and waits for two missing three-second polls before removal (`main.swift:47-49`, `main.swift:870-872`, `main.swift:1004-1007`, `main.swift:1086-1117`).

Transferable ideas:

- Batch process inspection and run it off the UI thread.
- Bound process commands and transcript reads.
- Debounce missing-process transitions.
- Never equate “recent transcript” with “process alive”.

This remains macOS/TTY-specific. Detached agents, IDE sessions and several wrappers are invisible, and Claude's cwd/TTY fallback cannot produce a universally reliable session ID.

### CodexDynamicIsland: useful transcript UI, not a process monitor

`CodexDynamicIsland` recursively scans `~/.codex/sessions` every three seconds, sorts by modification date and only registers the newest 20 rollout files (`CodexDynamicIsland/Services/Session/CodexSessionMonitor.swift:36-39`, `CodexDynamicIsland/Services/Session/CodexSessionMonitor.swift:55-97`). For each registered file it parses again every two seconds, while the UI store is copied every second (`CodexDynamicIsland/Services/State/CodexSessionStore.swift:37-44`, `CodexDynamicIsland/Services/Session/CodexSessionMonitor.swift:42-51`).

The parser derives `processing`, `idle` and `waitingForApproval` from rollout event records (`CodexDynamicIsland/Services/Session/CodexConversationParser.swift:92-147`), and the display title falls back from thread name to first user message, cwd and ID (`CodexDynamicIsland/Models/SessionState.swift:51-61`). It has no process discovery, PID, TTY or independent end-of-process evidence in the inspected monitor/store.

Because rollout files persist, the App can rediscover them after restart. That is good history behavior, but it does not recover runtime liveness: a file whose last phase is idle remains an idle session even if its process has exited. BalanceHub should reuse the distinction already present in its own design: transcript parsing enriches runtime records but never creates strong “still running” evidence.

### Claude Dynamic Island: real Hooks, volatile localhost transport

This Windows project installs Claude HTTP Hook entries for pre-tool, post-tool, failure, notification and stop (`src-tauri/src/config/mod.rs:18-30`, `src-tauri/src/config/mod.rs:56-118`). A Tauri-owned Axum server binds to `127.0.0.1` (`src-tauri/src/server/mod.rs:10-29`), caps request bodies at 1 MiB, emits events into the frontend and can wait for approval or selection (`src-tauri/src/server/router.rs:18-40`, `src-tauri/src/server/router.rs:117-175`).

This is true Hook-driven interaction, but it does not persist events before frontend delivery. If the App/server is not listening, the HTTP Hook has no receiver. The generated configuration has no request headers and uses a predictable localhost URL (`src-tauri/src/config/mod.rs:56-70`, `src-tauri/src/config/mod.rs:95-115`), so it is not an appropriate transport to copy for BalanceHub without authentication, port ownership and failure-mode design.

It also has no `SessionStart` or `SessionEnd` route in the inspected revision, and no PID/TTY association. It can show current tool activity while running, but cannot be the source of a durable external-session registry.

### Claude HUD: strong current-session/config renderer, wrong runtime primitive

Claude HUD uses Claude Code's native statusline API: Claude writes the current session's JSON to stdin, the command reads the transcript and prints terminal UI to stdout (`README.md:115-125`, `src/index.ts:81-127`). It can count Claude configuration, MCP and Hook data for the current cwd and render model, usage, tools and subagents (`src/index.ts:134-220`).

Its cache is carefully bounded: per-session transcript-path hashes isolate records, writes are throttled to three seconds, cache age is seven days, entry count is capped at 100, and sweeping is probabilistic (`src/context-cache.ts:13-28`, `src/context-cache.ts:56-65`, `src/context-cache.ts:169-204`). These are good patterns for lightweight configuration summaries.

However, it is not an external observer. It runs inside each Claude session's statusline refresh, has no shared runtime registry, PID/TTY linkage, terminal activation or multi-Agent aggregation. Using statusline as BalanceHub's primary monitor would also compete with the user's existing visible statusline configuration. It is better treated as evidence that skills/MCP/Hooks/statusline inventory can be parsed and cached, not as the runtime bridge.

### claude-menubar: persistent start mapping plus live macOS polling

`claude-menubar` installs only a Claude `SessionStart` Hook (`src/settings.rs:21-63`). The Hook reads `session_id` and `transcript_path`, walks its process ancestry to find Claude and TTY, then writes a state file keyed by TTY (`src/hook.rs:14-45`). Unlike socket-only implementations, this start mapping can be written even if the menu bar UI is not active.

The menu app still derives current liveness by polling. It uses `pgrep` to find Claude/Codex, `ps` to find each TTY and `lsof` for cwd (`src/process.rs:78-143`), while the Swift UI invokes a one-shot poll every two seconds (`swift/ClaudeBar.swift:54-65`, `swift/ClaudeBar.swift:121-143`). The Hook state file is preferred for transcript mapping; if missing/stale it chooses the newest transcript not claimed by another active TTY (`src/transcript.rs:327-397`).

Its status is partly heuristic: a recent modification under 10 seconds is active, an unpaired tool is pending after a three-second grace period, and non-plan pending/user states fall back to idle after 120 seconds (`src/transcript.rs:128-178`). Process presence determines which terminal sessions are included, but active/pending/idle remains a transcript timeout inference.

Useful lesson: write-through state from a short-lived Hook can survive UI downtime. Limitation: recording only `SessionStart` cannot reconstruct busy/idle/end transitions, and the per-process `ps`/`lsof`/recursive Codex scan architecture does not scale or port cleanly.

### Agent Session Monitor: honest limitations and bounded polling, but no exact mapping

This Next.js dashboard states its model clearly: Codex comes from local SQLite plus rollout files, Claude from JSONL, and OS processes are polled because neither source pushes updates (`README.md:3-7`, `README.md:17-25`). Its SSE endpoint is actually a 1.5-second poll-and-diff loop with no server-side event history; reconnect starts with a full snapshot (`src/app/api/dashboard/events/route.ts:9-27`, `src/app/api/dashboard/events/route.ts:139-206`).

The Codex side uses one `ps` query, then bounded-concurrency `lsof` cwd lookups (`src/data-access/local-adapter.ts:515-576`). It cannot map a Codex thread to a PID, so all Codex processes sharing a cwd are attached to that session (`src/data-access/local-adapter.ts:1134-1187`). Its own signal action warns that stopping/pausing/resuming may affect multiple sessions in that directory (`src/data-access/local-adapter.ts:1360-1439`). Claude explicitly has no process association and exposes an empty PID array (`src/data-access/claude-code-adapter.ts:537-555`, `src/data-access/claude-code-adapter.ts:720-743`).

Status still includes timeout inference. Codex activity within five minutes becomes running even before process presence is considered; cwd process evidence can separately produce observed/waiting, and old data becomes stale/unknown (`src/data-access/local-adapter.ts:789-842`). Claude is entirely classified from file recency because no unambiguous process mapping was found (`src/data-access/claude-code-adapter.ts:537-555`). The README correctly exposes progress as indeterminate and disables approve/reject because external sessions provide no control channel (`README.md:20-25`, `README.md:110-119`).

The strongest transferable engineering patterns are bounded concurrency, capped active session sets, full-file size ceilings, one-second snapshot cache, in-flight deduplication, revision only when observable content changes, and reconnect resync (`src/data-access/claude-code-adapter.ts:578-679`, `src/data-access/claude-code-adapter.ts:772-804`, `src/data-access/local-adapter.ts:1271-1343`).

### BalanceHub design consequences

#### 1. Runtime bridge must be durable and metadata-only

The surveyed Hook apps optimize for live UI and generally lose events when the receiver is down. BalanceHub's proposed helper should instead perform one bounded operation:

```text
Agent Hook stdin
  -> Agent-specific decoder
  -> allow-listed metadata event
  -> temp file + atomic rename into App-data spool
  -> exit success whether or not BalanceHub is running
```

The App consumes the spool asynchronously and idempotently. Persist only Agent kind, event kind, session ID, cwd, model/title when officially supplied, event time, runtime scope and optional BalanceHub instance ID. Explicitly drop prompts, assistant content, tool arguments/results, environment dumps and credentials.

This combines `claude-menubar`'s UI-independent file write with OpenIsland's normalized multi-Agent reducer, while avoiding the socket/HTTP availability hole.

#### 2. PID/TTY is a separate evidence channel

The projects repeatedly demonstrate that no universal mapping exists:

- Codex often exposes a rollout path through open file descriptors, but not in every client/runtime.
- Claude commonly closes transcript descriptors and falls back to cwd/TTY.
- cwd grouping is not session identity and can target multiple processes.
- TTY filtering misses IDE, detached and headless sessions.
- some Agents require heartbeat rather than process association.

BalanceHub-launched sessions should keep their exact PID/status-file model and inject `BALANCEHUB_CLI_INSTANCE_ID` so their Hook event can be joined deterministically. Externally discovered sessions must expose PID/TTY/jump target only when the Agent-specific adapter reports a verified mapping. Otherwise the UI shows metadata and “状态未知” without terminal activation or process control.

#### 3. State must expose evidence, not one synthetic status

Use a reducer over normalized events and retain at least:

- lifecycle evidence: last Hook kind/time and explicit SessionEnd;
- activity evidence: last turn start/idle/failure time;
- process evidence: last confirmed PID/TTY/cwd and observation time;
- transcript evidence: title/model/update time;
- derived state plus `confidence`/`reason`.

Only explicit `SessionEnd` or verified disappearance of an exactly linked process can yield strong `ended`. A timeout without exact PID becomes `unknown`, not `completed`. “Busy” can follow a current turn-start Hook; transcript recency alone remains a lower-confidence estimate.

#### 4. UI should use an activity hub, not claim fake progress

The best UI pattern across the projects is progressive disclosure:

- collapsed surface: only active count and highest-attention state;
- expanded surface: per-Agent groups and per-session status/title/cwd/time;
- detail: evidence, model, terminal link availability and history;
- completed sessions leave the live surface after a bounded acknowledgement/retention rule.

Do not show a determinate percent because Agent CLIs do not provide total work. Use an indeterminate activity treatment for `busy`, an explicit approval/attention state only when supported, and `unknown` when evidence expires. In BalanceHub this should reuse the existing topbar/background-task visual language and Agent session workbench, rather than adding another permanent header.

#### 5. Config inventory and runtime monitoring are related but separate domains

Claude HUD proves that skills/MCP/Hooks/statusline/config counts can be derived efficiently, but runtime Hooks and configuration ownership have different mutation risks. The product should keep:

- `AgentConfigResourceAdapter`: discover/list/open/parse/validate/diff/apply owned configuration resources;
- `AgentRuntimeAdapter`: inspect/install/remove/verify BalanceHub-owned Hook integration and decode runtime events;
- shared Agent descriptor capabilities: declare which resources and runtime events each Agent supports.

This prevents a future Agent from requiring edits across one central switch. A statusline is an inventory/config resource and optional enhancement, not the default event transport.

#### 6. Performance constraints are product requirements

Polling implementations repeatedly needed caps and repairs. Recommended constraints:

- Hook-driven updates first; process polling only while the activity hub is visible or unresolved sessions exist.
- One batched process snapshot per interval; avoid `lsof` once per PID where possible.
- platform command timeout, cancellation and two-poll debounce.
- bounded transcript tail/head reads; no repeated recursive full-home scan.
- per-Agent scan concurrency and session caps.
- snapshot revision and in-flight dedup so multiple UI consumers do not trigger duplicate scans.
- expensive work off the Tauri/UI thread, with last-known-good state on timeout.

### Files found

BalanceHub:

- `src-tauri/src/services/agent_cli.rs` - current Agent registry and capabilities; natural owner for optional config/runtime adapters.
- `src-tauri/src/services/temporary_cli.rs` - registers BalanceHub-launched instances before opening a terminal.
- `src-tauri/src/services/cli_runtime.rs` - current temp-directory instance loading and PID/status reconciliation.
- `src-tauri/src/models/provider_results.rs` - current temporary CLI result shape is Provider/PID/terminal-specific.
- `.trellis/tasks/09-04-product-evolution-roadmap/research/agent-runtime-hooks-audit.md` - official Hook capability and proposed durable bridge audit.
- `.trellis/tasks/09-04-product-evolution-roadmap/research/wsl-agent-config-audit.md` - WSL runtime/config boundary and distro-scoped requirements.
- `.trellis/spec/guides/cross-layer-thinking-guide.md` - decode external events once and replay through one reducer.
- `.trellis/spec/guides/code-reuse-thinking-guide.md` - avoid repeated payload parsing and scattered status transitions.

External source files:

- `open-vibe-island/Sources/OpenIslandHooks/OpenIslandHooksCLI.swift` - Hook stdin decoder and socket client.
- `open-vibe-island/Sources/OpenIslandCore/SessionState.swift` - Hook/process/heartbeat state reducer.
- `open-vibe-island/Sources/OpenIslandApp/ActiveAgentProcessDiscovery.swift` - multi-Agent process, TTY, transcript and terminal discovery.
- `open-vibe-island/Sources/OpenIslandApp/SessionDiscoveryCoordinator.swift` - persisted registry and startup transcript reconciliation.
- `open-vibe-island/docs/hooks.md` - transport, events, timeout and terminal discovery behavior.
- `agent-notch/main.swift` - process-authoritative liveness, batched `lsof`, bounded transcript reads and two-poll debounce.
- `CodexDynamicIsland/Services/Session/CodexSessionMonitor.swift` - periodic rollout discovery.
- `CodexDynamicIsland/Services/State/CodexSessionStore.swift` - per-session transcript refresh loop.
- `CodexDynamicIsland/Services/Session/CodexConversationParser.swift` - rollout-derived phases.
- `claude-dynamic-island/src-tauri/src/config/mod.rs` - localhost HTTP Hook installation and merge.
- `claude-dynamic-island/src-tauri/src/server/router.rs` - Hook handlers, payload cap and interactive wait.
- `claude-dynamic-island/src-tauri/src/server/mod.rs` - localhost listener.
- `claude-hud/src/index.ts` - current statusline stdin/transcript/config aggregation.
- `claude-hud/src/context-cache.ts` - bounded per-session cache.
- `claude-menubar/src/hook.rs` - persistent SessionStart mapping.
- `claude-menubar/src/process.rs` - macOS PID/TTY/cwd discovery.
- `claude-menubar/src/transcript.rs` - transcript mapping and timeout-based state inference.
- `claude-codex-session-monitor/src/data-access/local-adapter.ts` - Codex DB/process/transcript projection and bounded cache.
- `claude-codex-session-monitor/src/data-access/claude-code-adapter.ts` - Claude bounded transcript scan and file-recency status.
- `claude-codex-session-monitor/src/app/api/dashboard/events/route.ts` - poll-and-diff SSE transport.

### External references

- Repository revisions and metadata were inspected through GitHub repository/commit APIs and raw source URLs on 2026-09-04.
- Official Agent Hook semantics, local installed versions and WSL behavior are covered in `agent-runtime-hooks-audit.md` and `wsl-agent-config-audit.md`; this file intentionally evaluates third-party implementations rather than re-stating all vendor documentation.

### Related specs

- `AGENTS.md` - Rust truth source, dynamic Agent/Terminal discovery, cross-platform honesty, async cleanup and privacy constraints.
- `.trellis/spec/guides/cross-layer-thinking-guide.md` - one decoder boundary and one replay reducer for external JSON events.
- `.trellis/spec/guides/code-reuse-thinking-guide.md` - shared typed projections and exhaustive state transitions.
- `.trellis/tasks/09-04-product-evolution-roadmap/prd.md` - Agent session workbench, dynamic Agent boundary and three-platform acceptance criteria.
- `.trellis/tasks/09-04-product-evolution-roadmap/design.md` - existing session adapter/index boundary and typed background-state direction.

## Caveats / Not Found

- Third-party source is a point-in-time audit of the revisions listed above. Their behavior may change after 2026-09-04.
- `claude-dynamic-island`, `claude-menubar` and `claude-codex-session-monitor` had no license detected by GitHub at inspection. Their source must be treated as study material only unless licensing is clarified.
- OpenIsland is GPL-3.0 while BalanceHub uses its own non-commercial same-source license. Copying implementation code could introduce license obligations or incompatibility; only independently reimplement the architectural ideas after legal review.
- No inspected project provides a proven Windows WSL session-to-PID/TTY bridge. WSL remains a separate distro-scoped feature, not evidence of current three-platform parity.
- No inspected project proves reliable terminal activation for every Agent/client. AppleScript, TTY and cwd fallbacks are platform/client heuristics.
- “App not running” was evaluated from source transport and persistence behavior, not by installing and fault-injecting every third-party binary.
- This research does not authorize Hook installation, process scanning, config writes or product-code changes.
