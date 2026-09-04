# Research: Runtime Session Enrichment Producer Design

- Query: 设计一个增量、事件驱动、可缓存的 session enrichment producer，把现有四个 Agent 的标题、模型和最近活动时间补入统一 `AgentRuntimeSession`，且绝不在两秒 runtime 刷新中全量扫描历史会话。
- Scope: mixed (current repository implementation, local version-pinned storage observations, and already-recorded official Hook contracts)
- Date: 2026-09-04

## Findings

### Executive decision

当前缺口应通过一个独立的 `AgentRuntimeEnrichmentProducer` 补齐，而不是在 `AgentRuntimeRepository::refresh_with_launch_snapshots_at` 中调用现有 `SessionAdapter::list`。

最小可实施边界如下：

```text
Hook helper -> normalized event spool
                    |
                    v
2-second runtime refresh
  - consume bounded Hook batch
  - commit lifecycle/runtime events
  - derive exact single-session lookup requests
  - return immediately; no history/session source reads
                    |
                    v
AgentRuntimeEnrichmentProducer (separate bounded worker)
  - coalesce by runtime/session identity
  - invoke SessionAdapter.lookup_metadata only
  - read one exact session source incrementally
  - cache source identity, cursor and reduced metadata in memory
  - reject timed-out/stale generations
                    |
                    v
repository.append_enrichment(event)
  - persist result in the existing runtime event history
  - replay through the existing reducer
  - emit snapshot only when semantic metadata changed
```

这不是新的历史索引器，也不依赖用户是否启用了会话全文索引。现有全文索引、工作区摘要缓存和新的运行时富化缓存服务于不同访问模式，不能混用：

- 历史搜索是 `(agent, workdir) -> up to 100 summaries + message index`，允许后台遍历该工作区的历史会话。
- 运行时富化是 `(runtime scope, agent, exact session id) -> one metadata snapshot`，只能读取被 Hook 明确指向的单个会话。
- 富化结果是统一 runtime 的持久化事实；解析 cursor 只是进程内性能缓存，不保存提示词、回复或工具内容。

### Files found

- `src-tauri/src/models/agent_runtime.rs` - 唯一 `AgentRuntimeSession` IPC 合同，已有 `title`、`model`、`last_activity_at` 和 `SessionAdapter` evidence source。
- `src-tauri/src/services/agent_runtime/reducer.rs` - 唯一 runtime reducer；已有 `RuntimeEnrichment` 事件，但没有 producer，且所有 Enrichment 当前都被记为 SessionAdapter evidence。
- `src-tauri/src/services/agent_runtime/hook/event.rs` - normalized Hook payload 已保存 `session_id`、`cwd`、`transcript_path`、可选 model/title；转为 runtime event 时 transcript path 被丢弃。
- `src-tauri/src/services/agent_runtime/decoders.rs` - 四 Agent decoder；Gemini CLI 和 Grok Build 明确不从 Hook payload 生成 title/model。
- `src-tauri/src/services/agent_runtime/repository/mod.rs` - 每两秒消费 spool、合并 launcher snapshot、原子提交 event history，再 ack Hook 文件。
- `src-tauri/src/services/agent_runtime/repository/storage.rs` - runtime event 按 evidence time 回放，projection JSON 有 8 MiB 上限。
- `src-tauri/src/services/agent_runtime/service.rs` - 两秒调度、五秒 refresh timeout、单 in-flight token 和 snapshot emit 所在位置。
- `src-tauri/src/services/agent_cli/contracts.rs` - `SessionAdapter` 当前只有工作区 list、正文 search/detail/index，没有单会话元数据 lookup。
- `src-tauri/src/services/cli_sessions/mod.rs` - 工作区 summary cache 为 30 秒 TTL，key 是 `(agent, normalized workdir)`，miss 时调用全量 `adapter.list`。
- `src-tauri/src/services/cli_sessions/index.rs` - 每 Agent 独立 SQLite 全文索引，summary 行可存 title/model/time，但索引可关闭、可滞后且只在历史搜索时构建。
- `src-tauri/src/services/agent_cli/codex/sessions/` - Codex 从只读 state SQLite、`session_index.jsonl` 和 rollout 读取会话。
- `src-tauri/src/services/agent_cli/claude/sessions.rs` - Claude Code list 会遍历项目目录下全部顶层 JSONL 并完整解析；单会话文件通常可由 session ID 直接定位。
- `src-tauri/src/services/agent_cli/gemini/sessions.rs` - Gemini CLI list 会枚举 workspace chat 文件并逐个解析；摘要需要处理 `$set`、`$rewindTo` 和消息更新。
- `src-tauri/src/services/agent_cli/grok/sessions.rs` - Grok Build list 最多递归 10,000 个目录、2,000 个 summary；单个 `summary.json` 已包含所需 title/model/time。

### Current code patterns and demonstrated gaps

1. `AgentRuntimeSession` 已为 enrichment 留出字段和 evidence 类型，不需要另建一份 UI 模型（`src-tauri/src/models/agent_runtime.rs:48-55`, `src-tauri/src/models/agent_runtime.rs:124-143`）。
2. `RuntimeEnrichment` 目前只有 title/model/workdir，缺少 session-source activity time、source revision 和来源区分（`src-tauri/src/services/agent_runtime/reducer.rs:33-39`）。
3. reducer 对除 `ExternalTimeout` 外的每个 event 都用 `event.observed_at` 推进 `last_activity_at`；如果 producer 把“解析完成时间”写入 enrichment event，会错误伪造用户活动（`src-tauri/src/services/agent_runtime/reducer.rs:163-181`）。
4. `AgentRuntimeEventKind::Enrichment` 被无条件标成 `SessionAdapter` evidence；Hook payload 自带的 model/title 也走该 kind，因此当前 evidence 来源不准确（`src-tauri/src/services/agent_runtime/reducer.rs:223-249`, `src-tauri/src/services/agent_runtime/hook/event.rs:63-80`）。
5. repository 在同一个锁内读取 projection、消费 Hook、replay、写 JSON、ack spool；这里不能加入可能扫描大文件的 adapter 调用（`src-tauri/src/services/agent_runtime/repository/mod.rs:162-217`）。
6. runtime service 的 refresh 是固定两秒一次并有五秒外层 timeout；`spawn_blocking` timeout 后工作线程不会被真正取消，所以把历史解析塞入 refresh 会形成脱离控制的阻塞线程（`src-tauri/src/services/agent_runtime/service.rs:17-19`, `src-tauri/src/services/agent_runtime/service.rs:202-248`）。
7. 当前 `SessionAdapter::list` 输入只有 workdir，输出整组摘要；它不是单会话 API（`src-tauri/src/services/agent_cli/contracts.rs:231-247`, `src-tauri/src/services/agent_cli/contracts.rs:294-376`）。
8. 历史摘要缓存 miss 后持有全局 scan gate 并执行 `adapter.list`；这会把 runtime 活跃度与用户是否刚搜索过历史错误耦合（`src-tauri/src/services/cli_sessions/mod.rs:167-235`）。
9. Codex 已能用 `WHERE id = ?` 查询一条 state DB row，但当前 helper 同时要求 cwd；可复用 row mapping，不能先调用工作区 list（`src-tauri/src/services/agent_cli/codex/sessions/index.rs:132-178`）。
10. Claude list 会枚举项目目录全部 JSONL 并对每份从头解析（`src-tauri/src/services/agent_cli/claude/sessions.rs:26-70`, `src-tauri/src/services/agent_cli/claude/sessions.rs:465-531`）。
11. Gemini list 会先收集 workspace chat 文件，再逐个完整解析（`src-tauri/src/services/agent_cli/gemini/sessions.rs:212-265`, `src-tauri/src/services/agent_cli/gemini/sessions.rs:315-409`）。
12. Grok list 的递归上限本身已证明它不适合 runtime hot path；所需字段却集中在单个最多 256 KiB 的 summary（`src-tauri/src/services/agent_cli/grok/sessions.rs:27-30`, `src-tauri/src/services/agent_cli/grok/sessions.rs:699-740`, `src-tauri/src/services/agent_cli/grok/sessions.rs:743-849`）。
13. 现有 session SQLite 是 per-Agent 文件并保存 summary，但它是可选的搜索派生物；其写入依赖历史列表和正文 parser，不能成为 runtime 正确性的前置条件（`src-tauri/src/services/cli_sessions/index.rs:805-877`, `src-tauri/src/services/cli_sessions/index.rs:935-975`）。

### Required contract changes

#### 1. Add a single-session metadata capability to `SessionAdapter`

在 `agent_cli/contracts.rs` 增加独立函数指针，不改变现有 list/search/detail/index 的语义：

```rust
type SessionMetadataLookup = for<'a> fn(
    SessionMetadataLookupRequest<'a>,
) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError>;

pub(crate) struct SessionMetadataLookupRequest<'a> {
    pub cli_kind: AgentCliKind,
    pub session_id: &'a str,
    pub workdir: Option<&'a Path>,
    pub transcript_path_hint: Option<&'a Path>,
    pub previous: Option<&'a SessionMetadataCursor>,
    pub budget: SessionMetadataLookupBudget,
}

pub(crate) struct SessionMetadataLookupBudget {
    pub max_bytes: usize,
    pub deadline: std::time::Instant,
    pub cancelled: Arc<AtomicBool>,
}

pub(crate) struct SessionMetadataSnapshot {
    pub title: Option<String>,
    pub model: Option<String>,
    pub workdir: Option<String>,
    pub last_activity_at: Option<i64>,
    pub source_revision: String,
}

pub(crate) struct SessionMetadataCursor {
    pub source_identity: String,
    pub source_len: u64,
    pub next_offset: u64,
    pub parser_version: u32,
    // Process-memory only. The owning Agent adapter serializes its bounded
    // reduced parser state; the common producer never interprets it.
    pub opaque_state: Vec<u8>,
}

pub(crate) enum SessionMetadataLookupResult {
    Ready {
        snapshot: SessionMetadataSnapshot,
        cursor: Option<SessionMetadataCursor>,
    },
    Pending {
        partial: Option<SessionMetadataSnapshot>,
        cursor: SessionMetadataCursor,
    },
    NotReady,
}
```

`SessionAdapter::new` 增加 `metadata: Option<SessionMetadataLookup>`，并提供 `supports_metadata()` / `lookup_metadata()`。新 Agent 只需注册自己的函数，不应在 producer 添加 Agent switch。

约束：

- `lookup_metadata` 必须验证 exact session ID；workdir/mtime 只能帮助定位和排序，不能作为会话关联依据。
- `transcript_path_hint` 来自第三方 Hook payload，必须视为不可信路径。Adapter 只能在自己的 Agent home/已知 session root 内接受它，并拒绝 symlink、目录和越界路径。
- cursor 只驻留内存，`opaque_state` 默认上限 256 KiB；不得包含完整提示词、回复、工具输入输出或环境变量。只允许 title candidate、last model、timestamp、有限 message-id/rewrite bookkeeping。
- parser 每次必须检查 byte budget、deadline 和 cancel flag；外层 timeout 只是最后保险，不能依赖不可取消的 `spawn_blocking` timeout 管理 I/O。
- `NotReady` 表示 Agent 尚未 flush 文件或无法精确定位，不是 runtime 错误，不改变现有标题/模型/状态。

#### 2. Make enrichment source and activity semantics explicit

扩展 `RuntimeEnrichment`：

```rust
pub enum RuntimeEnrichmentSource {
    Hook,
    SessionAdapter,
}

pub struct RuntimeEnrichment {
    pub source: RuntimeEnrichmentSource,
    pub title: Option<String>,
    pub model: Option<String>,
    pub workdir: Option<String>,
    pub source_last_activity_at: Option<i64>,
    pub source_revision: Option<String>,
}
```

必须同时修正 reducer：

- Hook payload 直接带来的 metadata 使用 `source = Hook`，evidence 记为 `AgentRuntimeEvidenceSource::Hook`。
- producer 结果使用 `source = SessionAdapter`。
- Enrichment event 的完成时间不能自动推进 `last_activity_at`。
- `source_last_activity_at` 只按 `max` 更新显示用 `last_activity_at`；不能把 producer 完成时间当作 Agent 活动时间。
- 增加 `state_observed_at: Option<i64>`（或同义内部字段）作为 lifecycle/launch/timeout 的独立时钟。`append_external_timeout_events` 必须用它，不得因历史文件 metadata timestamp 而推迟或恢复运行状态。
- Enrichment 无论 evidence time 是否早于最近 Hook，都可以补空字段；对于非空字段，只接受当前 request generation 对应的成功结果。旧 worker 结果在 repository commit 前被拒绝。
- `source_revision` 仅用于幂等和诊断，不通过 IPC 暴露原始路径。

不建议首轮引入复杂的 field-level conflict UI。最小确定性规则是：

1. Hook 自带 metadata 立即写入，保证低延迟。
2. Session adapter 在对应 Hook event 后延迟少量时间读取 Agent 自己落盘的最终 metadata，成功后可覆盖 Hook 值。
3. 同一 adapter source revision 的重复结果不写新 event。
4. 失败、超时、`NotReady` 均保留上次成功值。

#### 3. Introduce an internal enrichment request, not a public IPC model

`NormalizedHookEvent::into_runtime_events` 仍只生成可回放 event；repository 在消费新 spool record 时另外派生进程内请求：

```rust
pub(crate) struct RuntimeEnrichmentRequest {
    pub trigger_event_id: String,
    pub target_runtime_id: String,
    pub runtime_scope: AgentRuntimeScope,
    pub agent_kind: AgentCliKind,
    pub agent_session_id: String,
    pub workdir: Option<PathBuf>,
    pub transcript_path_hint: Option<PathBuf>,
    pub trigger_observed_at: i64,
    pub priority: RuntimeEnrichmentPriority,
}
```

`target_runtime_id` 必须调用 reducer 暴露的同一个纯函数解析，不能在 repository/producer 再拼一次 `external:{scope}:{agent}:{session}`。有合法 `BALANCEHUB_CLI_INSTANCE_ID` 时，target 仍是同一个 `balancehub:<instance>`，不能创建外部重复项。

repository 内部 refresh 返回：

```rust
struct AgentRuntimeRefreshOutcome {
    snapshot: AgentRuntimeSnapshot,
    enrichment_requests: Vec<RuntimeEnrichmentRequest>,
}
```

公开 IPC 仍返回 snapshot。Service 在 blocking refresh 完成、repository 锁已释放后才 enqueue requests。这样两秒 refresh 的硬性保证可测试：它只处理 bounded spool + launcher + JSON projection，不调用任何 session source reader。

### Producer ownership and data flow

新增建议目录：

```text
src-tauri/src/services/agent_runtime/enrichment/
  mod.rs          contracts exported inside agent_runtime
  producer.rs     coalescing, generation, concurrency, timeout, retry
  cache.rs        bounded in-memory cursor/result cache
```

`AgentRuntimeService` 持有一个 producer handle。Producer 自己拥有待处理 registry，不由 Vue 或 IPC 驱动。

#### Enqueue and coalescing

缓存/任务身份分两层：

```text
lookup key = (runtime_scope.key(), agent_kind, agent_session_id)
target key = runtime_id
```

- lookup key 决定同一 Agent session 的 source/cursor cache；不同 Agent 相同 session ID 不共享。
- target key 决定 enrichment event 写入哪个 runtime。一次 session resume 出现在不同 BalanceHub launch runtime 时可复用 source cache，但分别提交到各 target。
- registry 对同一 target 只保留最新 request，并递增 `generation`。100 个连续 Hook event 不生成 100 个并发解析。
- `Idle/Ended` 优先于 `Start/Busy`；同优先级取最新 trigger。
- registry 建议最多 256 个 target。淘汰顺序为 ended/unknown 最旧项，再淘汰 idle；不得因队列满而阻塞 Hook helper 或两秒 refresh。
- 使用 `Notify + Mutex<HashMap<...>>` 或等价 coalescing actor，不使用会静默丢 request 的无恢复有界 channel。

#### Scheduling

- Hook Start: 约 200 ms debounce，允许 Agent 先创建 metadata source。
- Hook Busy: 更新 generation；已有待处理任务时不另起 worker。
- Hook Idle/Ended: 约 250 ms debounce 后高优先读取，允许最后一次文件 flush。
- App start: 从当前 runtime snapshot 只遍历 runtime sessions，不扫描 Agent home。若 `agent_session_id` 存在，并且最后 SessionAdapter evidence 早于最后 Hook evidence或 title/model 缺失，则生成 recovery request。
- App resume/foreground: 与 App start 相同，只检查 runtime projection evidence，不扫描历史目录。

上述 recovery 使“projection 已提交并 ack spool，但 App 在 enrichment 前退出”可恢复，不需要把 pending job 再持久化成第二套任务数据库。

#### Concurrency, cancellation and timeout

- 全局最多 2 个 lookup worker；同一 Agent 最多 1 个，避免多个大 transcript 竞争磁盘。
- lookup 必须在 `spawn_blocking` 内运行，不占 Tokio async worker。
- 每 slice 建议 `max_bytes = 2 MiB`、cooperative deadline 150 ms；返回 `Pending` 后让出线程，再排下一 slice。
- 单 generation 墙钟上限建议 3 秒；达到上限后写内部 failure/backoff，不提交 enrichment event。
- `spawn_blocking` 外层 timeout 后设置 cancel flag。worker 即使稍后返回，generation token 与 cancel flag 都会阻止 commit。
- adapter panic 通过 join error 隔离为当前 Agent lookup failure；不能终止 producer actor。
- 失败 backoff：1 s、5 s、30 s，最大 2 min。新 Idle/Ended Hook 可提前唤醒但仍合并；长期无新 Hook 不持续扫描。
- 任一 Agent 的超时/解析失败不能占住另一 Agent 的 semaphore permit；permit 在 worker future 的 finally/Drop 中释放。

#### Commit and stale-result protection

Producer 完成一次 lookup 后按以下顺序提交：

1. 在 producer registry 校验 `(target, generation)` 仍是最新。
2. 调用 repository `append_enrichment_if_current(request, snapshot)`；repository 重新读取当前 projection。
3. 校验 target runtime 仍有相同 `runtime_scope + agent_kind + agent_session_id`。
4. 比较当前 scalar metadata 与 result；全部不变且 `source_last_activity_at` 未推进时直接返回 unchanged。
5. 生成稳定 event ID：hash `(target runtime id, source revision, reduced metadata digest)`。
6. append unique event、trim、原子写 projection；只在实际变化时 revision + 1。
7. Producer 通过 service 的统一 revision publisher 发送 snapshot。

Runtime refresh 与 producer 可能并发完成。当前 `last_emitted_revision` 的 load-then-store 应收敛为单个 compare-exchange/fetch-max 发布函数：只有 `new_revision > published_revision` 的调用获得发布权，旧 revision 永远不能覆盖新 revision。

### Cache design and invalidation

#### In-memory cache entry

```text
RuntimeMetadataCacheEntry {
  lookup_key,
  source_identity,      // hashed canonical locator + file identity + parser version
  source_len,
  source_modified_at,
  cursor,
  last_snapshot,
  last_success_at,
  last_requested_at,
  failure_count,
  next_retry_at,
}
```

约束：

- 默认 256 entries 或 16 MiB，两者先到即淘汰；这些常量集中在 enrichment cache。
- source path 不进入 event ID 明文、IPC 或普通日志；identity 使用长度分隔的 hash。
- 同一文件 identity 且长度增长：从上一完整换行 cursor 继续解析 suffix。
- 文件长度缩小、file identity 改变、parser version 改变或 locator 改变：丢弃 cursor，从头按 slice 重建。
- mtime 改变但 size/identity 相同：对小 JSON summary 重读并计算内容 hash；对 JSONL 重新检查尾部边界，不能仅凭粗粒度 mtime 宣称 unchanged。
- cache miss 只解析目标 session；禁止回退到 `SessionAdapter::list`。
- parser state 只保留 metadata 所需的有限信息；不保存整条 user/assistant/tool content。

#### Persisted cache

首轮不增加第二个 SQLite/JSON metadata cache。最后一次成功 enrichment 已作为 runtime event 存在 `projection.json`，这就是跨重启缓存和回放真源；进程内 cursor 只是增量性能优化。

这比复用现有全文索引更可靠：

- 全文索引可被用户关闭或清理。
- 它只在历史搜索触发 build，不能保证 active runtime 已入库。
- 它的 freshness key 是工作区历史集合和消息 source fingerprint，不是 Hook event generation。
- 为了读取一行 summary 而启动 index build 会反向触发全历史解析，违背本任务最重要的性能边界。

允许后续把“已存在且 source revision 可证明匹配”的 per-Agent index summary 作为只读 warm hint，但它不能跳过 adapter 的 source-revision 校验，也不能成为 MVP 依赖。

### Per-Agent exact lookup strategy

#### Codex CLI

优先级：

1. 枚举 Codex home 顶层 `state_*.sqlite`，按新到旧最多检查一个集中常量上限（建议 8）；每库执行只读 `threads WHERE id = ? LIMIT 1`。
2. 有 workdir hint 时验证 row cwd 与 hint/canonical hint 一致；不以 cwd 猜 session。
3. 复用现有 `row_to_summary` 字段优先级和 timestamp normalizer。
4. `session_index.jsonl` 的显式 `thread_name` 高于 state DB fallback title。该文件按 `(path identity, len, mtime)` 缓存 ID->title map，只在 revision 变化时事件驱动重建，不在每个两秒 tick 读取。
5. state row 尚未出现时，如 Hook 给出合法 transcript path，可从该单个 rollout 的 metadata/head-tail 补有限字段；不得递归 `sessions/` 或 `archived_sessions/` 2,000 个目录找文件。

Source revision 至少包含命中的 state DB identity、row 的 updated/recency 字段和选定 metadata digest；显式 title overlay revision单独纳入 digest。

#### Claude Code

优先级：

1. 接受 Hook `transcript_path`，但必须 canonicalize 并证明位于 `~/.claude/projects/<encoded-workdir>/`，且解析出的 session ID 精确匹配。
2. 没有 path hint 时直接尝试 `~/.claude/projects/<encoded-workdir>/<session_id>.jsonl`。
3. 直接路径不存在时返回 `NotReady`；runtime 路径禁止调用当前 `find_transcript_path` 的“遍历所有 JSONL 并逐个解析”fallback。
4. 将当前 `TranscriptSummary::observe` 中 metadata-only 逻辑抽成可增量 reducer：只保留 session ID、workdirs、first visible user title candidate、latest `ai-title`、last non-synthetic model、created/updated time。
5. 新 append 只读取上次完整换行后的 suffix；truncate/replace 重建。

标题优先级继续保持现有行为：latest `ai-title` > first real user message。`isSidechain`、`isMeta`、tool result 和 synthetic model 过滤规则必须复用，不能为 runtime 写第二套不同判断。

#### Gemini CLI

优先级：

1. 使用合法 Hook transcript path（若供应商提供）。
2. 否则从 `projects.json` 的 exact workdir mapping 和现有 legacy hash 规则得到有限 chats roots；不扫描整个 `~/.gemini/tmp`。
3. 若文件名不能直接由 session ID 推导，只枚举目标 chats root 的文件 metadata，按最近修改排序，最多检查 32 个 candidate 的头部 metadata；最终必须读取到 exact `sessionId` 才关联。mtime 只排序，不参与身份判断。
4. 一旦命中，缓存 `(session ID -> source identity/path)`；后续只解析这一份文件。
5. metadata reducer复用 `$set`、`$rewindTo`、upsert/replace 语义，但只保存有限 message metadata：ID、role、至多 240 字的 visible title candidate、model、timestamp。工具调用正文完全忽略。
6. 如果 rewrite bookkeeping 超过上限而无法正确处理 rewind，返回 `Pending` 并按 slice 从头重建；不得猜测标题或模型。

标题继续为 `summary > first resumable user message`，模型为最终 conversation 中最后一个 Gemini message model，时间为 `lastUpdated > file timestamp`。

#### Grok Build

优先级：

1. 由 exact cwd 的官方 percent-encoded workspace root 与 exact session ID 直接尝试 `~/.grok/sessions/<encoded-cwd>/<session-id>/summary.json`，同时尝试 canonical cwd 的编码结果。
2. 必须验证 `info.id == session_id` 且 `info.cwd` 与 workdir hint 等价。
3. 如布局版本不匹配，只允许在 exact encoded-workspace root 下按 session directory name 做 bounded lookup；禁止调用当前全局 10,000-directory recursive scan。
4. `summary.json` 已限制为 256 KiB，可以在 revision 改变时整文件解析，无需 JSONL cursor。
5. 复用现有过滤：hidden/subagent/empty summary 不生成 enrichment。

标题继续为 `generated_title > session_summary > last_turn_summary`，模型为 `current_model_id`，最近活动为 `last_active_at > updated_at`。

### Event persistence and projection size

- 只持久化成功且产生语义变化的 enrichment；`Pending`、timeout 和 transient error 不写 runtime event。
- request 和 cursor 不写 projection，避免 event history 变成任务队列或保存 transcript parser state。
- 恢复依赖“最后 Hook evidence 新于最后 SessionAdapter evidence”的纯 projection 检查，因此 spool ack 后崩溃不会永久漏富化。
- 每次 source revision 改变但 metadata 完全相同不写 event，避免每个 Hook turn 成倍膨胀 projection。
- source revision/event ID 使用 hash，不保存 transcript path 明文。
- event history 仍受 8,192 events/8 MiB 上限约束；producer 不应为一次 Hook event固定追加一条 enrichment，而只在 title/model/workdir/last-activity 实际变化时追加。

### Failure isolation

- 无 session ID：不调 adapter；保留 Hook runtime，title/model 可为空。
- 无 workdir：若合法 transcript hint 足以定位则继续，否则 `NotReady`。
- source 未 flush：保留 Hook 原值，按 backoff 等待当前或下一 lifecycle event。
- source schema 变化：当前 Agent lookup 返回 structured unsupported/parse error；其他 Agent、Hook reducer 和 launcher 不受影响。
- source 越界/symlink：拒绝 lookup，不把完整路径写入日志或 UI。
- timeout/panic：仅记录聚合 diagnostic 和下一次 retry time，不改变 runtime state，不产生后台失败通知。
- stale generation：结果丢弃；允许保留安全 cursor 供更新后的同一 source identity 继续读，但不得 commit scalar metadata。
- repository commit 失败：保留进程内 request 并退避；绝不回滚已经提交的 lifecycle projection，也不重新制造 Hook 文件。

### Implementation dependency order

1. 在 `contracts.rs` 增加 metadata lookup 合同、budget/result/cursor；四 Agent 注册 capability。此时不接 runtime。
2. 抽取四 Agent 现有 metadata parser 真源并实现 exact lookup；现有 list 与新 lookup 共用 parser/标题优先级。
3. 扩展 `RuntimeEnrichment` 来源、source activity，并拆开 `state_observed_at` 与 display `last_activity_at`；先补 reducer tests。
4. repository refresh 返回进程内 requests，但 refresh 仍只做 spool/launcher/projection；补“list/lookup 均未被调用”的 hot-path test。
5. 实现 producer 的 coalescing、cursor cache、generation、budget、timeout/backoff。
6. 增加 repository conditional append 与 service revision publisher；接入 startup recovery。
7. 逐 Agent fixture 和真实 Hook smoke；最后运行完整三端 CI。

步骤 1-3 是 producer 的前置依赖；步骤 4-6 不应在四 Agent lookup 仍会 fallback 到 list 时合并。Codex/Grok 可先作为小源验证，再接 Claude/Gemini 增量 JSONL parser。

### Tests

#### Contract and hot-path tests

- 构造 `SessionAdapter`，让 `list`/search/detail/index 在被调用时 panic；两秒 refresh + Hook batch 必须成功，证明 hot path 没有 session scan。
- 一个 Hook record只生成一个 exact request；无 session ID 不生成 request。
- BalanceHub launch correlation 的 request target 是 `balancehub:<instance>`，不会生成 external duplicate。
- 同一 Agent/session 100 个 Busy/Idle event 合并为一个 target job和最新 generation。
- 不同 Agent 相同 session ID、相同 Agent不同 runtime scope完全隔离。

#### Reducer tests

- Hook metadata evidence source 是 Hook，adapter metadata source 是 SessionAdapter。
- producer completion time 不推进 `last_activity_at`；仅 source activity timestamp 可以推进。
- adapter activity 不推进 `state_observed_at`，也不能阻止 external runtime 超时进入 unknown。
- enrichment event 即使 evidence time 早于最近 lifecycle，仍可补空 title/model，但旧 generation不能覆盖新结果。
- unchanged semantic metadata不增加 revision/event；rename/model switch/last activity advance各增加一次。

#### Producer concurrency tests

- 全局并发不超过 2，同 Agent不超过 1。
- 一个 adapter cooperative timeout时，另一个 Agent仍能完成。
- outer timeout 后 detached blocking worker返回也不能 commit。
- 新 generation 到达后旧 generation结果被拒绝。
- retry backoff有上限，新 Idle/Ended event可唤醒，长期静默不会周期扫描。
- registry/cache达到上限时按约定淘汰，不阻塞 refresh。
- App restart仅遍历 runtime projection，能恢复“Hook已提交但无 adapter evidence”的 session。

#### Incremental source tests

- JSONL 首次读取按 2 MiB slice返回 Pending，后续从完整换行 cursor继续。
- append只读取 suffix；truncate/replace/parser-version change重建。
- partial final line不推进 cursor，下一次补齐后只处理一次。
- cursor state达到 256 KiB 上限时可控重建，不无限增长。
- transcript path越界、symlink、session ID不匹配全部拒绝。

#### Per-Agent fixtures

- Codex: exact state row、显式 renamed title overlay、多个 state DB、stale transcript hint、row未flush后重试。
- Claude: direct transcript、latest ai-title、model switch、sidechain/meta/tool过滤、large append、truncate。
- Gemini: transcript hint、projects mapping、32-candidate bounded locator、`$set`、rewind、subagent/empty过滤、large append。
- Grok: encoded cwd exact path、canonical cwd、hidden/subagent/empty、summary revision、layout mismatch bounded failure。

#### Privacy and persistence tests

- projection/event/cache diagnostic 不包含 prompt、assistant response、tool payload、environment、credential或明文 transcript path。
- runtime event JSON round-trip保留 source、source activity和revision hash。
- projection commit失败不提交 adapter result；下次 request仍可重试。
- semantic unchanged Hook storm不会线性增长 event history。

### Acceptance criteria

- `AGENT_RUNTIME_REFRESH_INTERVAL` 仍可保持两秒，但 refresh 单元测试确认 `SessionAdapter::list` 和 `lookup_metadata` 调用次数均为 0。
- 新 Hook event 到达后，producer只读取对应 exact Agent session；不得遍历所有历史会话文件。
- Claude/Gemini 长会话后续读取量与 append suffix 大小近似线性，而不是每轮从头 O(total transcript)。
- Gemini/Grok 即使 Hook 不含 title/model，也能在 source flush 后补出当前 title/model；失败保持空或上次成功值。
- title/model/last activity在 App 重启后仍从 runtime event replay存在；无需启用历史全文索引。
- adapter metadata不会把 ended/unknown session改回 active，也不会伪造外部 PID、Provider或terminal action。
- timeout、损坏、未知 schema和单 Agent失败不影响两秒 runtime refresh、其他 Agent或主界面交互。
- 新 Agent 接入只注册 metadata lookup/parser，不修改 producer公共状态机。

### External references

- OpenAI Codex Hooks, accessed 2026-09-04: https://developers.openai.com/codex/hooks
  - Hook common fields include session ID, transcript path, cwd and model; transcript storage is not a stable public Hook API, so parsing remains versioned and failure-isolated.
- Anthropic Claude Code Hooks Reference, accessed 2026-09-04: https://code.claude.com/docs/en/hooks.md
  - Lifecycle payload supplies session ID/transcript path/cwd; optional title/model cannot be assumed present for every event.
- Gemini CLI Hooks Reference, accessed 2026-09-04: https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md
  - Common lifecycle payload does not guarantee title/model; `AfterAgent`/`SessionEnd` are suitable event-driven refresh triggers, not a reason to poll transcript history.
- Grok Build Hooks, accessed 2026-09-04: https://docs.x.ai/build/features/hooks.md
  - Hook payload uses session/cwd identity but does not provide the summary title/model fields consumed from the version-pinned local summary adapter.
- Existing task research `research/codex-hook-official-contract.md` and `.trellis/tasks/09-04-product-evolution-roadmap/research/agent-runtime-hooks-audit.md` contain the version-pinned Hook capability matrix used here.

### Related specs

- `AGENTS.md` code boundary: Rust owns IPC/business truth; cross-layer fields must not be mirrored as frontend rules.
- `AGENTS.md` code quality: reuse existing adapters/parsers, keep one business-rule source, delete superseded paths, and avoid dead-code suppression.
- `AGENTS.md` asynchronous UI/concurrency: external or IPC operations require timeout/cancellation and stale-result guards; detached work must not hold UI busy state.
- `.trellis/spec/guides/cross-layer-thinking-guide.md` - define typed source/transform/store contracts and keep external payload decoding at one boundary.
- `.trellis/spec/guides/code-reuse-thinking-guide.md` - repeated payload/metadata extraction belongs in shared Agent-owned parser/reducer, not runtime/UI copies.
- `.trellis/spec/guides/agent-routing.md` - this cross-layer/concurrency design must be reviewed before mechanical implementation.
- `.trellis/tasks/09-04-agent-environment-runtime-center/prd.md` R1/R2 - session adapters enrich title/model/activity but cannot decide process liveness; Hook bridge remains bounded and privacy-filtered.
- `.trellis/tasks/09-04-agent-environment-runtime-center/design.md` Correlation And Reduction - stable launch ID is the only BalanceHub/external merge key; cwd/time/Agent name must not be guessed.

## Caveats / Not Found

- Grok Build workspace directory percent-encoding is visible in the installed version's local layout but the current repository does not yet expose a shared encoder or versioned official storage schema. The adapter must pin this layout in fixture tests and fail boundedly when it changes.
- Gemini CLI filenames do not universally expose the complete session ID. The bounded candidate-header lookup is exact only after reading matching `sessionId`; it must return `NotReady` when more than the bounded candidate set would be required, rather than expanding to a home-wide scan.
- Claude/Gemini transcript formats are append/rewrite implementation details, not stable Hook contracts. Incremental cursors require parser-version invalidation and must degrade without affecting runtime lifecycle.
- The proposed cursor is deliberately process-memory only. After App restart, already committed enrichment remains available, but a source that changed after the last committed adapter evidence may require one new bounded baseline pass triggered by the persisted Hook evidence.
- Current runtime event-history trimming is a global count, not per-session compaction. This design minimizes enrichment event amplification but does not redesign the broader runtime retention model.
- No live Hook configuration or real Agent session was mutated during this research. Local file-layout checks were read-only and did not inspect or persist conversation content.
