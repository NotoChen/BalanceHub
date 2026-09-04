# Research: GitHub Agent Activity UI Detection Patterns

- Query: GitHub 上 Agent + Dynamic Island / notch / HUD 项目如何发现从外部终端启动的 Claude Code、Codex 等 Agent，会使用 Hook、statusline、进程/文件轮询还是包装 Agent 进程？
- Scope: external
- Date: 2026-09-04

## Findings

### Executive conclusion

这类项目能看见“不是由 App 启动的终端会话”，主要不是因为扫描出了终端，而是因为它们把 **用户级 Agent Hook** 安装进 `~/.claude/settings.json`、`~/.codex/hooks.json` 等配置。Hook 属于 Agent CLI 自身的生命周期，无论 CLI 是从 Terminal、iTerm、Ghostty、IDE 内置终端还是脚本启动，只要该 Agent 实例加载了这份用户配置，事件都会由 Agent 主动发送给状态 App。

成熟项目通常使用多种证据，而不是只用一种：

1. Hook 提供 session ID、事件类型、cwd、tool 等高语义状态。
2. Hook 子进程从 `PPID`、`tty`、`TERM_PROGRAM` 和其他环境变量推导 Agent 进程与终端来源。
3. App 通过 `kill(pid, 0)` 或 macOS `proc_pidinfo` / `NSRunningApplication` 校验存活并沿进程树定位宿主 App。
4. statusline 补充 Hook 不提供或不持续提供的模型、上下文、额度和成本等状态。
5. Agent 自己的 session registry / transcript / rollout 文件用于恢复 App 启动前的会话、补标题和聊天内容，或作为 Hook 丢失后的弱证据。

所以前一份研究的判断需要精确表述为：**Hook 足以发现外部启动的 Agent 会话，是第一实时信号；Hook 单独不足以可靠证明终端窗口、进程存活和异常结束。**

### Representative projects

#### 1. Notchi: per-Agent Hook + Unix socket + process ancestry

Repository: `sk-ruban/notchi`, inspected commit `03c2a6b980a70806413ba881f168dae98e22fedb`.

- README 明确给出 `Claude Code / Codex -> Hooks -> Unix Socket -> Event Parser -> State Machine`，并在首次启动时安装 Claude/Codex Hook。
- Claude forwarder 在 `/tmp/notchi.sock` 不存在时立即 `exit 0`，因此 App 未运行时事件不会排队或恢复。它从 Agent stdin 读取 session/cwd/transcript/tool，同时扫描 `ps -axo pid,ppid,command`，从 Hook 的 `PPID` 向上寻找 `claude` / `claude-code` 进程。
- Codex forwarder同样扫描进程树，记录 Codex PID，并通过有无 TTY 粗分 `cli` 和 `desktop`；源码明确承认被长寿命 wrapper 隐藏时可能误认 wrapper/shell，导致会话显示过久。
- 终端跳转不是 Hook 原生能力：macOS App 用 `proc_pidinfo` 向父进程回溯，再用 `NSRunningApplication` 找宿主 bundle ID。Codex Desktop 则使用 `codex://threads/<id>` 深链。

可复用结论：Hook 子进程确实处在最适合采集 PID/TTY/终端环境的位置，但这些字段是进程派生证据，不是 Agent Hook 的稳定公共字段。进程树和 bundle ID 逻辑是 macOS 特有实现，不能作为三端公共契约。

#### 2. MioIsland: Hook + socket + PID liveness + transcript watcher

Repository: `MioMioOS/MioIsland`, inspected commit `a76df9029385f1b0d9fa397e124fbefad6e3970b`.

- Claude 与 Codex Hook 调用 `~/.claude/hooks/codeisland-state.py`，脚本向 `/tmp/codeisland.sock` 发送生命周期事件。
- Python Hook 以父进程作为 Agent PID，通过 `ps` 读取 TTY，并从 `TERM_PROGRAM`、`TMUX`、`ZELLIJ`、`CMUX_*` 等环境变量识别宿主环境。对非审批事件是 fire-and-forget；socket 连接失败时返回 `None`，事件不持久化。
- App 每 30 秒使用 `kill(pid, 0)` 扫描一次已知会话：进程消失而没有 `SessionEnd` 时标记为 ended，解决终端被强杀的僵尸状态；一小时后清理 ended 会话。
- App 继续监听 transcript/agent JSONL 增量，补工具明细、对话和 subagent 状态。Hook 是状态入口，文件不是唯一生命周期真源。
- 精确跳回终端依赖多层 macOS 策略：TTY/PID、AppleScript、tmux/Yabai、cmux surface ID、Kitty/WezTerm CLI、bundle activation；对不支持 tab 定位的终端只能激活整个 App。

可复用结论：这是“Hook 负责语义、PID 负责收口、文件负责丰富内容”的典型混合方案。它解决了 App 运行期间的强杀，却仍无法恢复 App 未运行时丢掉的 Hook 启动事件。

#### 3. ClaudeNotch: Hook + statusline + Claude live-session registry

Repository: `rawsun007/claude-notch`, inspected commit `51e3efc7b13cd7062939893a2a8aa3f118fe384a`.

- 主状态仍通过官方 Hook 发送；单一 dispatcher 根据 `hook_event_name` 分发 PreToolUse、PermissionRequest、PostToolUse、Stop、SessionEnd 等事件到 localhost server。
- 它会包装 Claude `statusLine.command`：先将 model、session name、context percentage、5-hour/7-day limits、cost 等 JSON 转发到 App，然后执行并输出用户原有 statusline 命令。statusline 在这里是“富化通道”，不是主生命周期检测通道。
- 它另外每 20 秒读取 `~/.claude/sessions/<pid>.json`，取得 PID、session ID、cwd、CLI version、name 和 busy/idle。再用 `kill(pid, 0)` 和最长 24 小时时间窗排除崩溃残留。
- 该 registry 用来接纳“App 启动前已经存在”或“Hook 没安装到该项目”的会话；Hook 或 statusline 已有更精确信息时，registry 只补缺口，不覆盖活动、标题等高质量字段。

可复用结论：这是目前审计项目里对缺失事件处理最完整的做法。但 `~/.claude/sessions` 是 Claude 特有本地实现，不是四个 Agent 的共同能力，也不应未经官方契约确认就变成 BalanceHub 的必备主真源。statusline 包装还必须保存、串联和卸载时恢复用户原命令，否则会破坏用户现有 UI。

#### 4. CodexDynamicIsland: rollout 文件轮询，无 Hook

Repository: `maxbogo/CodexDynamicIsland`, inspected commit `222e60952ca9b65c59011997a3f5af27712605ed`.

- 每 3 秒递归扫描 `~/.codex/sessions/YYYY/MM/DD`，按修改时间只保留最近 20 个 rollout JSONL。
- 每个已登记会话再启动一个 2 秒循环，从上次 byte offset 增量读取文件；另有 1 秒循环将 actor store 投影到 UI。
- 它解析 `session_meta`、`event_msg`、`response_item`，从 `turn_started`、tool begin/end、approval request 和 `turn_complete` 推导 processing/approval/idle，同时读取完整聊天内容。
- 因为 Codex App 和 CLI 都写同一数据目录，所以无需 Hook 就能看到两者，也能在监控 App 后启动时发现最近会话。
- 代价是依赖 Codex 内部 rollout schema，缺少可靠 PID/TTY/终端映射；如果进程在最后一个 processing 事件后崩溃，源码中未发现用进程存活或超时将其可靠收口的路径。定时递归扫描和每会话独立刷新任务也比事件驱动 Hook 更重。

可复用结论：文件轮询适合作为恢复/富化证据，不适合作为跨 Agent 的唯一实时检测方案。它也解释了为什么有些 Codex 灵动岛不安装 Hook，仍能“看见”外部 CLI 和 Desktop。

#### 5. CCIsland: HTTP Hook only

Repository: `colna/CCIsland`, inspected commit `af6917f640389c662a180556242893c2f4711b0c`.

- 将 Claude 用户级 Hook 写入 `~/.claude/settings.json`，事件直接 POST 到 `127.0.0.1:51515/hook`。
- 以 session ID 建内存 map，`SessionStart/UserPromptSubmit/PreToolUse/PostToolUse/Stop/SessionEnd` 驱动状态；90 秒无事件会把 thinking/tool 标成 done，五分钟后清理 inactive done。
- App 退出时会卸载 Hook，因此 App 未运行时不会检测；所谓“跳回终端”只是遍历已知终端 App 并激活第一个运行者，没有可靠 session-to-window 关联。

可复用结论：这是实现成本最低、也最接近“用户看到实时岛”的方案，但 stale timeout 把“仍在处理但长时间无事件”和“异常退出”混为一谈，不能直接作为 BalanceHub 的准确运行时模型。

#### 6. PocketClaw: SDK-owned session, not external discovery

Repository: `rick-ray-wldd/pocketclaw`, inspected commit `0905c5c300eb979f06c9cd0706e510f1f754af20`.

- Mac daemon 使用 `@anthropic-ai/claude-agent-sdk` 的 `query()` 创建并持有会话，直接消费 async event stream，并持有 `interrupt()`。
- 因为会话由它创建，它能准确获得生命周期、工具审批、kill 和成本；但它不发现任意外部终端中已经启动的 Claude Code。

可复用结论：wrapper/SDK/PTY interception 对自有启动会话最强，但若强制用户通过 BalanceHub 启动，会改变“外部任意终端也能被发现”的产品前提。它只能继续作为 BalanceHub 临时 CLI 的强证据路径，不能替代外部观察。

### Capability comparison

| Pattern | External terminal while App running | App starts after Agent | Crash / force-close | Exact terminal tab | Cross-Agent / cross-platform |
| --- | --- | --- | --- | --- | --- |
| User-level Hook -> local socket/HTTP | Yes | No, unless queued | No, unless PID/timeout reconciliation | Hook can enrich TTY, but focusing remains terminal-specific | Hook schema and IPC need per-Agent/per-OS adapters |
| Hook + PID/process tree | Yes | Usually no | Better: `kill(pid, 0)` can close zombies | Good on supported macOS terminals, fallback elsewhere | PID liveness portable in concept; ancestry/window APIs are not |
| Statusline wrapper | Yes, for Agents that support it | No | No | No | Agent-specific; risks replacing user statusline |
| Session registry polling | Yes | Yes | Good when registry has PID and cleanup semantics | Usually no | Highly Agent-specific |
| Transcript/rollout polling | Yes | Yes for persisted recent sessions | Weak without PID/end event | No | Internal file formats differ and may change |
| App-owned SDK/wrapper/PTY | Only sessions launched through wrapper | Host owns them already | Strong | Strong because parent owns process | High maintenance and changes user workflow |

### Reliability boundaries

- **External terminal detection**: user-level Hook works regardless of which terminal launched the Agent, provided that Agent loads the user config and accepts/trusts the Hook. Project config can disable/override behavior depending on Agent.
- **App not running**: most inspected island apps deliberately run at login and send to an ephemeral socket/localhost server. When the App is down, the Hook exits successfully and the event is lost. Only file/registry-based projects can recover later. This is why a persistent spool/helper remains materially more robust for BalanceHub if offline recovery is a requirement.
- **Crash/force-close**: `SessionEnd` is not guaranteed after `SIGKILL`, terminal crash, machine sleep/power loss, or Hook failure. PID liveness fixes many cases, but PID reuse, wrappers and remotely hosted sessions require age and identity guards.
- **PID association**: the Hook payload normally does not promise PID. Projects infer it from the Hook process parent/ancestry. Shell wrappers, plugin hosts, IDEs, SDK sessions and Agent implementation changes can produce a wrong parent; retain evidence strength and never treat an inferred PID as universal fact.
- **Window association**: session ID does not identify an OS window. Exact focus needs captured TTY, terminal-specific APIs, URL schemes or app-owned process handles. Most macOS projects use AppleScript/AppKit/Darwin APIs and degrade to activating the whole terminal. Windows/Linux require independent implementations.
- **macOS-only behavior**: notch windows (`NSPanel`/AppKit), `NSRunningApplication`, `proc_pidinfo`, AppleScript, Yabai and macOS TTY/bundle-id strategies do not prove Windows/Linux support. Hook event normalization is portable; window placement and terminal activation are not.
- **Security/privacy**: several projects forward prompt text, tool input or transcript content because their UI displays approvals/chat. BalanceHub's external runtime discovery does not need those fields. Its bridge should allow-list metadata and drop prompt/tool content before persistence.

### Recommended BalanceHub interpretation

The GitHub implementations support a refined hybrid architecture:

```text
Agent user-level lifecycle Hook
  -> per-Agent decoder
  -> session/cwd/model/event + optional inferred PID/TTY/runtime hints
  -> bounded local bridge/spool
  -> Rust normalized runtime reducer
       + Agent session files/registry for recovery and title/model enrichment
       + process liveness for inferred-PID reconciliation
       + BalanceHub-owned process state for App-launched CLI
  -> UI projection with source/evidence/status confidence
```

Specific decisions:

1. Keep Hook as the primary realtime discovery mechanism. It already solves “external terminal was not launched by BalanceHub”.
2. Do not promise terminal monitoring. The feature observes Agent sessions; exact terminal activation is a separate optional capability.
3. Add `runtime_origin`, `runtime_scope`, `evidence`, optional `pid/tty/terminal_hint`, and `status_confidence` instead of forcing external sessions into the current `TemporaryCliInstance` contract.
4. Use each Agent's known session store only through an Agent adapter. Claude registry, Codex rollout and other file layouts are different and may have different stability guarantees.
5. For BalanceHub's three desktop platforms, use a bundled Rust helper plus atomic spool if events must survive App shutdown. A local socket-only MVP is simpler but knowingly loses this guarantee.
6. Treat process and window association as progressive enhancement: macOS can use PID ancestry/TTY and terminal-specific activation; Windows/Linux/WSL must declare separate capabilities and may only show session metadata initially.
7. Do not take over statusline by default. Offer it only as an optional Agent-specific enrichment where the original command can be preserved, chained, conflict-detected and restored.

## Files found

- `sk-ruban/notchi/notchi/notchi/Resources/notchi-hook.sh` - Claude Hook forwarder, process ancestry and Unix socket transport.
- `sk-ruban/notchi/notchi/notchi/Resources/notchi-codex-hook.sh` - Codex Hook forwarder, PID and CLI/Desktop inference.
- `sk-ruban/notchi/notchi/notchi/Services/TerminalJumpService.swift` - macOS PID ancestry and terminal activation.
- `MioMioOS/MioIsland/ClaudeIsland/Resources/codeisland-state.py` - Hook normalization, PID/TTY/env collection and Unix socket transport.
- `MioMioOS/MioIsland/ClaudeIsland/Services/State/SessionStore.swift` - Hook reducer, process-liveness zombie scan and transcript enrichment.
- `rawsun007/claude-notch/bin/claudenotch-statusline.sh` - statusline enrichment and chaining of prior user command.
- `rawsun007/claude-notch/Sources/ClaudeNotch/SessionRegistry.swift` - Claude live-session registry parsing and PID validation.
- `rawsun007/claude-notch/Sources/ClaudeNotch/AppState+Sessions.swift` - 20-second registry reconciliation and Hook-over-registry precedence.
- `maxbogo/CodexDynamicIsland/CodexDynamicIsland/Services/Session/CodexSessionMonitor.swift` - periodic Codex rollout discovery.
- `maxbogo/CodexDynamicIsland/CodexDynamicIsland/Services/Session/CodexConversationParser.swift` - incremental internal JSONL state inference.
- `colna/CCIsland/apps/cc-island/src-tauri/src/hook_installer.rs` - Claude HTTP Hook installation.
- `colna/CCIsland/apps/cc-island/src-tauri/src/hook_router.rs` - in-memory session state and stale timeout.
- `rick-ray-wldd/pocketclaw/packages/host/src/claudeAdapter.ts` - Agent SDK-owned session stream.

## Code patterns

- User-level Hook is what makes arbitrary external terminal launch observable; local socket/HTTP is merely the transport.
- PID/TTY/environment capture is most accurate inside the Hook subprocess, then process liveness and terminal ancestry are reconciled by the App.
- Rich projects maintain source precedence: Hook event > statusline enrichment > live registry > transcript inference > timeout heuristic.
- App-owned sessions and externally observed sessions have different guarantees and should remain explicit in the state model.
- File-derived metadata is read incrementally or on a bounded polling interval, not by rescanning every transcript on every UI refresh.

## External references

- Notchi README and source, commit `03c2a6b980a70806413ba881f168dae98e22fedb`: https://github.com/sk-ruban/notchi/tree/03c2a6b980a70806413ba881f168dae98e22fedb
- MioIsland source, commit `a76df9029385f1b0d9fa397e124fbefad6e3970b`: https://github.com/MioMioOS/MioIsland/tree/a76df9029385f1b0d9fa397e124fbefad6e3970b
- ClaudeNotch source, commit `51e3efc7b13cd7062939893a2a8aa3f118fe384a`: https://github.com/rawsun007/claude-notch/tree/51e3efc7b13cd7062939893a2a8aa3f118fe384a
- CodexDynamicIsland source, commit `222e60952ca9b65c59011997a3f5af27712605ed`: https://github.com/maxbogo/CodexDynamicIsland/tree/222e60952ca9b65c59011997a3f5af27712605ed
- CCIsland source, commit `af6917f640389c662a180556242893c2f4711b0c`: https://github.com/colna/CCIsland/tree/af6917f640389c662a180556242893c2f4711b0c
- PocketClaw source, commit `0905c5c300eb979f06c9cd0706e510f1f754af20`: https://github.com/rick-ray-wldd/pocketclaw/tree/0905c5c300eb979f06c9cd0706e510f1f754af20

## Related specs

- `.trellis/spec/guides/cross-layer-thinking-guide.md` - each raw Hook/statusline/file schema must be decoded once at its Agent boundary; UI consumes typed Rust projections.
- `.trellis/spec/guides/agent-routing.md` - cross-platform runtime observation is an architecture task requiring research before implementation.
- `AGENTS.md` - Rust owns capability truth; external process/network operations need bounded lifecycle; native Windows, Linux, macOS and WSL capabilities cannot be inferred from a macOS implementation.

## Caveats / Not Found

- The audit is source-level and did not install or run these third-party apps.
- Repository stars and implementation maturity are not correctness guarantees; projects are cited for representative techniques.
- `~/.claude/sessions/<pid>.json`, Codex rollout JSONL and local SQLite layouts were observed in project code, but this audit did not establish all of them as stable public APIs. They should be adapters with explicit compatibility handling, not global contracts.
- No inspected project provided one portable implementation for precise terminal-window association across macOS, Windows, Linux and WSL.
- No inspected project demonstrated that statusline alone reliably determines session termination or process identity.
