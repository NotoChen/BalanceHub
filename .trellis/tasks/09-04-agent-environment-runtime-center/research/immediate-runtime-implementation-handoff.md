# Research: Immediate Runtime Implementation Handoff

- Query: 在暂停可用性决策中心的前提下，确认 Agent Hook 运行时监测与配置管理当前已经落地的范围、唯一关键缺口和立即可执行的实现顺序。
- Scope: internal
- Date: 2026-09-04

## Findings

### Scope decision

本轮只继续 `.trellis/tasks/09-04-agent-environment-runtime-center`，不实现或扩展 availability/decision center。用户当前要求的是把已经讨论并设计完成的 Agent Hook 运行时链路做成可直接验收的实物，而不是继续产品规划。

### Files found

- `.trellis/tasks/09-04-agent-environment-runtime-center/prd.md` - 运行时、Hook bridge、受管 Hook 和“不接管用户配置”的验收合同。
- `.trellis/tasks/09-04-agent-environment-runtime-center/design.md` - launcher、Hook spool、runtime reducer、session adapter 和 UI 的目标数据流。
- `.trellis/tasks/09-04-agent-environment-runtime-center/implement.md` - 当前阶段进度；Phase 2 的 session adapter enrichment producer 尚未完成。
- `.trellis/tasks/09-04-agent-environment-runtime-center/research/runtime-session-enrichment-design.md` - 已完成的增量富化详细设计、四 Agent 精确 lookup 策略和测试标准。
- `src-tauri/src/services/agent_runtime/repository/mod.rs` - 两秒刷新热路径；当前只消费 bounded Hook batch、launcher snapshot 和 external timeout。
- `src-tauri/src/services/agent_runtime/reducer.rs` - 已存在 `AgentRuntimeEventKind::Enrichment`，但 enrichment 的来源时间语义还不足以承载异步 adapter 结果。
- `src-tauri/src/services/agent_cli/contracts.rs` - `SessionAdapter` 目前只有 list/search/detail/index 能力，没有 exact single-session metadata lookup。
- `src-tauri/src/services/agent_cli/{codex,claude,gemini,grok}/` - 四 Agent 已有会话解析真源，后续 exact lookup 必须复用这些 parser，不能再建第二套标题/模型规则。
- `src-tauri/src/services/agent_runtime/service.rs` - runtime 后台刷新和事件发布入口，适合作为 producer 生命周期的编排边界。
- `src/components/AgentRuntimeModal.vue` - UI 已消费统一 `AgentRuntimeSession` 的 title/model/workdir，不需要再新增前端会话模型。
- `src/stores/cli-runtime.ts` - 已具备 snapshot revision/request ID 防过期，不需要前端承担富化调度。

### Code patterns and confirmed state

1. Runtime repository 已把持久化 event history 作为唯一真源，并保证 projection commit 后才 acknowledge spool（`src-tauri/src/services/agent_runtime/repository/mod.rs:150-217`）。富化结果必须继续以 runtime event 提交，不能增加另一份 UI 缓存真源。
2. 当前 refresh 热路径没有调用 session list/parser，这是正确边界（`src-tauri/src/services/agent_runtime/repository/mod.rs:162-217`）。实现后仍必须通过测试保证热路径不读历史会话源。
3. Reducer 已能把 enrichment 的 title/model/workdir 写入统一 projection（`src-tauri/src/services/agent_runtime/reducer.rs:324-328`），但所有非 timeout event 都会推进 `last_activity_at`（`src-tauri/src/services/agent_runtime/reducer.rs:163-181`）。异步解析完成时间不能伪装为用户活动时间，因此必须先修正 enrichment 时间合同。
4. `SessionAdapter` 现有 list/search/detail 函数面向工作台检索，不适合 runtime 单会话富化（`src-tauri/src/services/agent_cli/contracts.rs:231-240`, `src-tauri/src/services/agent_cli/contracts.rs:295-357`）。不能把 `list` 塞进每两秒 refresh。
5. 统一运行时 UI 已按 Rust snapshot 展示 title/model/workdir，前端不需要按 Agent 写四分支（`src/components/AgentRuntimeModal.vue:168-203`）。
6. 当前任务的 Hook 管理、spool、reducer 和 UI 主体已经存在，不能因为富化缺口而重做整套运行时，也不能回退现有用户改动。

### Immediate implementation order

1. 在 `SessionAdapter` 增加内部 `lookup_metadata` capability，以及 bounded budget/result/cursor 合同；不新增公开 IPC。
2. 从四 Agent 现有 parser 抽取共用 metadata 真源，实现按 `(agent kind, exact session ID, bounded workdir candidates)` 的精确 lookup；禁止 fallback 到全量 `list`。
3. 扩展 enrichment event 的 source/source activity/revision 语义，并先修 reducer：解析完成时间不推进用户活动时间，旧 generation 不覆盖新结果。
4. 让 repository refresh 在提交 Hook/launcher projection 后只派生轻量 lookup request；refresh 返回前不得读取 Agent 历史文件。
5. 在 `agent_runtime/enrichment/` 实现独立 bounded producer：`spawn_blocking`、全局最多 2 个 worker、单 Agent 最多 1 个、单次 3 秒、generation 去重和退避。
6. producer 成功结果通过 repository 的 compare-and-append 接口写入现有 event history；只在 title/model/workdir/source activity 有语义变化时落盘。
7. 接入 runtime service 生命周期和事件发布；UI 继续只消费统一 snapshot。
8. 补齐四 Agent fixture、并发/超时/旧结果/重启恢复/隐私/热路径零扫描测试，然后执行任务 `implement.md` 中的完整前后端检查。

### Acceptance focus for the next visible build

- 从 BalanceHub 启动的会话继续保留 Provider、账号、Key、PID、终端定位和退出码。
- 从外部终端启动且官方 Hook 可用的会话会进入同一运行时列表。
- 有精确 session ID 时，列表随后补齐正确的会话标题、模型和最近源活动时间；富化过程不阻塞 UI 和两秒 refresh。
- 同目录并发会话、不同 Agent 相同 session ID、resume 和乱序结果不会串会话。
- Hook 配置仍是用户显式 plan/apply，健康检查只读，不自动抢写或覆盖用户配置。
- 无 Hook、unsupported schema、历史源暂不可读或 lookup 超时时，已有 launcher/runtime 功能继续工作。

### Related specs

- `.trellis/spec/guides/cross-layer-thinking-guide.md` - Rust 合同、IPC、store 和 UI 的单真源数据流。
- `.trellis/spec/guides/code-reuse-thinking-guide.md` - 复用四 Agent 现有 parser，避免重复标题/模型提取规则。
- `.trellis/spec/frontend/hook-guidelines.md` - 异步结果使用 request ID/revision，释放 busy 状态，避免对象身份判断。
- `.trellis/spec/frontend/state-management.md` - Pinia 只承载前端状态，业务规则留在 Rust。

## Caveats / Not Found

- Gemini 会话文件名不能稳定映射完整 session ID；必须在有界候选内读取 header 并核验 `sessionId`，超出预算返回 not-ready，不能扩大为 home-wide scan。
- Grok Build 的目录编码和事件语义需要固定版本 fixture；阻塞 `Stop` 不能无依据解释为整个 session ended。
- Claude/Gemini JSONL 增量 cursor 必须带格式版本并允许失效后有界重建。
- 当前工作区已有大量相关未提交改动；实现 Agent必须在现状上继续并保留用户改动，不能通过 checkout/reset 回退。
- 三端真实 Hook 行为最终仍需要 macOS 实物验收和 Linux/Windows CI/fixture 共同确认；本交接不把 fixture 通过等同于真实 Agent 集成已验收。
