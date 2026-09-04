# Research: Product Roadmap Dependency Audit

- Query: Audit the existing implementation foundation and gaps for the Agent session workspace, automation rule center, one-click diagnostics center, and provider onboarding wizard. Define their dependencies on the availability decision center, recommended order, and independent acceptance boundaries.
- Scope: internal
- Date: 2026-09-04

## Findings

### Executive conclusion

The four directions have materially different starting points:

| Direction | Existing foundation | Main missing boundary | Dependency on availability decision center |
| --- | --- | --- | --- |
| Agent session workspace | Strong | A standalone workflow and multi-scope query ownership | Soft |
| One-click diagnostics center | Medium | A read-only, typed, cancellable diagnostic orchestration contract | Soft / recommended shared contract |
| Provider onboarding wizard | Strong but fragmented | A side-effect-aware preflight plan and final review transaction | Medium; diagnostics reuse is more important |
| Automation rule center | Medium infrastructure, weak product model | Persisted rules, canonical fact events, cooldown state, and execution history | Hard |

Recommended delivery after the availability decision center contract is frozen:

1. Agent session workspace and diagnostic contract can start in parallel because their core file ownership does not overlap.
2. Finish the diagnostics center before the onboarding wizard so site, proxy, certificate, Agent, and terminal checks share one result vocabulary and renderer.
3. Build the onboarding wizard on the existing provider editor, protocol registry, and duplicate-save semantics; do not create a second provider form.
4. Build the automation rule center last, against the canonical availability fact projection rather than reading card state or re-deriving business rules.

The only hard dependency is automation rules -> canonical availability facts. The other ordering is chosen to maximize reuse and reduce UX duplication, not because the features are technically blocked.

### 1. Agent session workspace

#### User value

- Makes session discovery, content search, detail reading, resumption, active instances, and workspace context a first-class workflow instead of a subordinate step inside temporary CLI launch.
- Lets users find work before deciding which provider, API Key, model, or terminal to use.
- Preserves per-Agent fidelity: unsupported history, detail, naming, or resume actions remain unavailable based on Rust-owned capabilities.

#### Existing components, services, and contracts

- Rust already exposes a dynamic Agent capability contract for session history, search, detail, resume, and naming (`src-tauri/src/models/agent_cli.rs:5-17`). Those values are derived from registered adapters rather than hard-coded UI assumptions (`src-tauri/src/services/agent_cli.rs:37-59`).
- Session summaries already carry the useful cross-Agent intersection: ID, title, preview, models, timestamps, workdir, CLI version, archive state, resume capability, and Agent-owned metadata source (`src-tauri/src/models/cli_sessions.rs:5-24`). Details carry normalized messages plus truncation metadata and an Agent-owned content source (`src-tauri/src/models/cli_sessions.rs:26-54`).
- Search is adapter-driven, bounded to 50 returned results, supports an index and controlled cold-scan fallback, and keeps summary cache entries for 30 seconds (`src-tauri/src/services/cli_sessions/mod.rs:37-49`, `src-tauri/src/services/cli_sessions/mod.rs:57-164`).
- The index already has a serialized work queue, cancellation flags, capacity control, ready/failure cooldowns, and background-task events (`src-tauri/src/services/cli_sessions/index.rs:26-41`, `src-tauri/src/services/cli_sessions/index.rs:50-129`, `src-tauri/src/services/cli_sessions/index.rs:250-319`). Background scans use paced I/O and explicit cancellation checks (`src-tauri/src/services/cli_sessions/io.rs:138-174`).
- Frontend types mirror the session summary/detail/search/index contracts (`src/stores/provider-types.ts:636-697`), and `useCliRuntimeStore` already owns IPC calls for search, detail, index status, and active instances (`src/stores/cli-runtime.ts:83-119`).
- Existing views are reusable: `WorkspaceSessionHistoryPanel.vue` renders search, index state, metadata, selection, and copyable Resume IDs (`src/components/WorkspaceSessionHistoryPanel.vue:69-154`); `CliSessionDetailModal.vue` renders a paged/searchable conversation timeline and groups tool activity (`src/components/CliSessionDetailModal.vue:102-125`, `src/components/CliSessionDetailModal.vue:139-224`).
- Active temporary CLI instances already include provider/account origin, title, Agent, workdir, terminal, timestamps, PID, status, and activation support (`src/stores/provider-types.ts:569-585`), and the existing instance modal renders that data (`src/components/TemporaryCliModal.vue:140-249`).
- Existing async UI code already uses request IDs and `finally` cleanup for stale-result protection (`src/composables/useWorkspaceSessionHistory.ts:48-95`, `src/composables/useWorkspaceSessionHistory.ts:125-161`).

#### Gaps and minimum new capability

1. The current controller is structurally coupled to temporary launch: it requires modal visibility, a selected `sessionMode`, selected model, and one directory (`src/composables/useWorkspaceSessionHistory.ts:16-33`). A standalone `useSessionWorkbench` should own Agent/workspace/query/detail selection without mutating launch state.
2. The backend search command uses one process-wide `SESSION_SEARCH_GENERATION`; starting any search invalidates every older search, including a legitimate concurrent search for another Agent/workspace (`src-tauri/src/commands/cli.rs:44-80`). The workspace needs a caller-supplied request ID/cancellation key or cancellation scoped by `(window/workbench, Agent, workspace)`, not a global generation.
3. Search currently requires exactly one `cli_kind + workdir`, has no cursor, and returns at most 50 sessions (`src-tauri/src/commands/cli.rs:48-78`; `src-tauri/src/services/cli_sessions/mod.rs:37-40`). The minimum workbench may deliberately query one Agent/workspace at a time, but the UI must not imply global search. If cross-workspace search is required, add a typed aggregate command with bounded fan-out and per-source errors instead of issuing frontend calls that cancel one another.
4. Resume is currently a side effect of selecting a session inside the launch form and clears the selected model (`src/composables/useWorkspaceSessionHistory.ts:164-177`). Extract a typed `SessionResumeIntent { cliKind, workdir, sessionId, sessionTitle }` that opens the existing launch preview with explicit provider/Key/model choices.
5. There is no standalone navigation entry or retained workbench state. Keep detail modal and message renderer reusable, but move session workflow state out of `TemporaryCliModal` ownership.

#### Relationship to the availability decision center

- Soft dependency: the session list/detail workflow can ship independently using Agent and workspace sources.
- Reuse dependency: when resuming a session, the availability decision center should supply a typed candidate selection (`providerId`, `apiKeyLocalId`, `model`, `cliKind`) rather than the workbench deriving provider suitability.
- The workbench must not consume or copy a UI ranking score. It should receive only an explicit selected candidate/action intent.

#### Independent acceptance boundary

- From a top-level App entry, the user can select every registered Agent dynamically and a known/browsed workspace, search session metadata/content, view detail, copy the session ID, and resume a resumable session.
- Two searches for different Agent/workspace scopes can run concurrently without cancelling or overwriting each other.
- Unsupported capabilities are hidden/disabled from the Rust descriptor, not a frontend Agent-name branch.
- Closing the view or changing scope prevents stale detail/search results from writing back; timeout/failure releases loading state.
- Tool activity remains collapsed by default, detail truncation is explicit, and original Agent session files are never modified.

### 2. Automation rule center

#### User value

- Converts passive observations into controlled reminders: balance thresholds, repeated liveness failures, check-in failures, model additions/removals, and API Key state changes.
- Makes each rule's scope, schedule/trigger, cooldown, last result, notification target, and disable switch inspectable.
- Avoids noisy repeated alerts by evaluating transitions and cooldowns instead of notifying on every scheduler tick.

#### Existing components, services, and contracts

- A Rust scheduler already runs while the App process is alive, sleeps between completed ticks to prevent self-overlap, and evaluates every 30 seconds (`src-tauri/src/services/scheduler.rs:68-81`, `src-tauri/src/services/scheduler.rs:115-131`).
- It already handles due refresh, check-in, and liveness work with bounded concurrency and emits task state to the UI (`src-tauri/src/services/scheduler.rs:147-235`, `src-tauri/src/services/scheduler.rs:237-317`, `src-tauri/src/services/scheduler.rs:319-367`).
- Refresh due time is centralized in Rust and combines provider override, global interval, last attempt, and last successful sync (`src-tauri/src/models/provider_domain/automation.rs:7-39`).
- Check-in retries already have an in-memory per-provider daily attempt cap and backoff (`src-tauri/src/services/scheduler.rs:78-101`, `src-tauri/src/services/scheduler.rs:379-420`). Refresh and liveness notifications are edge-triggered on healthy -> error transitions (`src-tauri/src/services/scheduler.rs:502-529`, `src-tauri/src/services/scheduler.rs:531-583`).
- Notification delivery is abstracted across configured channels and honors global/provider-specific selection (`src-tauri/src/services/notifications.rs:57-102`, `src-tauri/src/services/notifications.rs:149-161`).
- Provider state already stores the facts needed for several rule types: refresh/check-in timestamps and records (`src-tauri/src/models/provider/state.rs:183-197`), liveness records and cumulative counters (`src-tauri/src/models/provider/state.rs:208-251`), quota and available models in the frontend projection (`src/stores/provider-types.ts:177-202`, `src/stores/provider-types.ts:366-370`).
- Background-task reporting has a common event shape (`src-tauri/src/app_events.rs:3-21`) and a UI observer that retains at most 12 completed tasks for 15 minutes (`src/composables/useBackgroundTaskCenter.ts:79-95`, `src/composables/useBackgroundTaskCenter.ts:250-274`).

#### Gaps and minimum new capability

1. There is no persisted automation rule model. `AppSettings` contains only fixed global switches/timing and liveness configuration (`src-tauri/src/models/app_settings.rs:9-76`), while per-provider automation contains only refresh/check-in timing and observations (`src-tauri/src/models/provider/state.rs:183-206`).
2. Add Rust-owned `AutomationRule`, `AutomationCondition`, `AutomationScope`, `AutomationAction`, and `AutomationRuleState` models. Persist rule definitions plus the minimum restart-safe state: last evaluated fact revision, active/inactive edge, last triggered time, and cooldown-until.
3. Add a canonical availability fact projection before the evaluator. It should expose normalized, timestamped facts, not card display state: balance-known/unlimited/value, latest liveness outcome and consecutive failure count, last check-in result, model set/revision, Key status, and provider revision.
4. Keep evaluation separate from scheduling. The existing scheduler should ask a rule service to evaluate a fact snapshot; it must not gain one condition branch per rule type.
5. Add Rust CRUD/test commands and a bounded execution history. The existing background task center is presentation-only and ephemeral, so it cannot be the rule audit log.
6. Existing scheduler retry state is intentionally process-memory-only (`src-tauri/src/services/scheduler.rs:83-101`). Rule cooldown/deduplication cannot reuse it because restarting the App would re-alert immediately.
7. Task kinds are currently duplicated as Rust strings and a frontend union (`src-tauri/src/services/scheduler.rs:17-37`; `src/composables/useBackgroundTaskCenter.ts:14-27`). A rule-center task contract should be Rust-owned and serialized, avoiding another manually mirrored list.
8. Model-change rules require a previous model-set fingerprint/revision. The current model sync stores the latest list but not a durable change event (`src-tauri/src/services/provider_service/capabilities.rs:125-162`). Key-state rules similarly need a stable per-Key identity and observed-at time; remote Key metadata alone is not a transition log.

#### Relationship to the availability decision center

- Hard dependency: rules must evaluate the same canonical fact snapshot and freshness semantics used by the decision center.
- The dependency is on the fact projection and revisions, not on UI ranking or presentation labels.
- The decision center should ship read-only facts first. Rule actions remain notification-only in the first automation release; automatic Key switching, Agent reconfiguration, or paid requests require separate user approval and are outside this roadmap.

#### Independent acceptance boundary

- CRUD, enable/disable, provider/Key scope, condition, notification channel, and cooldown persist across restart.
- Each supported rule can be evaluated against deterministic fixture facts; unknown/stale facts produce `notEvaluated`, not a false alert.
- A false -> true transition triggers once, remains quiet during cooldown, and can trigger again only after the documented re-arm rule.
- Rule evaluation does not overlap scheduler work, block provider refresh, or derive capability from frontend fields.
- The UI shows last evaluation, last trigger, next eligible time, delivery result, and a bounded history; disabling/deleting a rule immediately stops future evaluation.

### 3. One-click diagnostics center

#### User value

- Replaces vague failures with a single, inspectable run across proxy, TLS/system certificates, Agent CLI, terminals, config files, provider protocol, and updater reachability.
- Produces a previewable, default-redacted report suitable for self-service troubleshooting or Issue attachment.
- Keeps checks read-only and lets the user see per-check progress, duration, failure cause, and remediation.

#### Existing components, services, and contracts

- Agent discovery already checks configured paths, BalanceHub-prefixed environment overrides, common installation paths, `PATH`, and optional login-shell discovery (`src-tauri/src/services/agent_cli/discovery.rs:22-82`). Candidate version checks have a five-second timeout (`src-tauri/src/services/agent_cli/discovery.rs:184-201`). All registered Agents are probed concurrently (`src-tauri/src/services/agent_cli.rs:165-193`).
- Terminal probing is registry-based and platform-specific; it exposes availability/name/version/message and bounds individual command probes to three seconds (`src-tauri/src/services/temporary_cli/terminal/mod.rs:40-80`, `src-tauri/src/services/temporary_cli/terminal/mod.rs:108-128`, `src-tauri/src/services/temporary_cli/terminal/mod.rs:176-215`).
- Network proxy resolution is centralized for global/provider modes and caches system discovery for ten seconds (`src-tauri/src/network/proxy.rs:202-239`). HTTP clients have bounded timeouts, support explicit/system/no-proxy modes, and use an LRU cache (`src-tauri/src/network/client.rs:28-88`, `src-tauri/src/network/client.rs:90-116`).
- Runtime networking is built with native root certificates plus SOCKS/system proxy support, while updater networking uses native TLS (`src-tauri/Cargo.toml:23-25`, `src-tauri/Cargo.toml:44`).
- Provider commands already expose side-effect-free-ish site/protocol/connection probes for draft inputs (`src-tauri/src/commands/provider.rs:65-95`), and protocol definitions own connection and capability behavior (`src-tauri/src/adapters/protocol/definition.rs:91-110`, `src-tauri/src/adapters/protocol/contracts.rs:210-229`).
- Update checking already has a typed backend service and frontend timeout/state lifecycle, but it owns pending-update state and UI interaction (`src-tauri/src/commands/app.rs:150-170`; `src/composables/useAppUpdater.ts:76-120`).
- The existing `npm run doctor` is a developer repository check, not an App diagnostic product surface (`package.json:13-20`). No runtime diagnostics service/command/view exists.

#### Gaps and minimum new capability

1. Add a dedicated read-only Rust `diagnostics` service and IPC contract. Minimum result fields: stable check ID, category, status (`running/pass/warn/fail/skipped/cancelled`), summary, redacted evidence, duration, optional remediation, and platform applicability.
2. Add an orchestration command with a progress `Channel` or typed Tauri events, per-check timeout, overall cancellation token/request ID, and a concurrency ceiling. Do not make the frontend invoke unrelated probe commands and infer a combined verdict.
3. Separate reusable probe primitives from mutating workflows. In particular, updater diagnostics must not populate/replace pending update state, provider diagnostics must not refresh/persist credentials or observations, and configuration validation must not rewrite files.
4. Add explicit proxy diagnostics: resolved mode, redacted endpoint, bypass decision, DNS/connect/TLS/HTTP phase, and whether native roots were used. Do not expose proxy credentials or raw environment values.
5. Add read-only Agent config diagnostics through the existing Agent registry/default-config adapter, plus terminal capability and activation-support diagnostics. Avoid hard-coded checks for four current Agents.
6. Build report redaction centrally in Rust and preview the exact export text. API Keys, tokens, cookies, passwords, proxy credentials, usernames/user IDs, and local provider names must be removed or masked before IPC/export.
7. Add platform applicability results rather than pretending parity: macOS application automation, Linux terminal/tray availability, and Windows executable/PowerShell behavior need distinct checks and `skipped` reasons.

#### Relationship to the availability decision center

- Soft dependency: the diagnostics center can run independently.
- Recommended integration: an unavailable/stale decision candidate should deep-link to a diagnostic run pre-scoped to its provider/Key/Agent, using IDs only. Diagnostics returns evidence; it does not modify the decision ranking.
- The shared primitive should be a typed `FactStatus/CheckStatus` vocabulary and timestamps, not shared UI components only.

#### Independent acceptance boundary

- One run reports incremental progress and a final per-check summary; closing/cancelling the view stops or invalidates further UI writes and never locks the main panel.
- Hashes/contents of App settings, provider data, Agent configuration, and updater pending state remain unchanged before/after a diagnostic run.
- Export preview and exported report contain no configured secrets or unmasked credential-bearing URLs.
- Each check is bounded by timeout and has an explicit macOS/Linux/Windows applicability result.
- A failed check does not abort unrelated checks; cancellation is distinguishable from failure.

### 4. Provider onboarding wizard

#### User value

- Turns provider creation into one understandable flow: endpoint -> protocol -> authentication -> capability/connectivity checks -> duplicate resolution -> final review.
- Makes automatic work visible and reversible while preserving an explicit choice before remote or local side effects.
- Explains whether the result will create a card, merge a Key into an existing card, or overwrite an existing provider before saving.

#### Existing components, services, and contracts

- The current provider editor is already divided into basics, credentials, and advanced steps, and centralizes save/probe/test/credential workflows in one controller (`src/composables/useProviderEditor.ts:24-79`, `src/composables/provider-editor-shared.ts:14-35`).
- Protocol detection is dynamic across registered primary protocols with an API-Key fallback and explicit ambiguity handling (`src-tauri/src/adapters/detector.rs:15-68`, `src-tauri/src/adapters/detector.rs:72-124`).
- Protocol definitions own auth field schemas, operation methods, credential assistant behavior, and capability adapters (`src-tauri/src/adapters/protocol/definition.rs:58-110`, `src-tauri/src/adapters/protocol/definition.rs:139-170`).
- Draft-level commands already support credential completion, connection tests, site probes, and protocol detection (`src-tauri/src/commands/provider.rs:65-95`). The editor guards async results with an editor session and input fingerprint (`src/composables/useProviderConnectionTest.ts:15-61`; `src/composables/useProviderCredentialCompletion.ts:306-366`).
- Duplicate semantics are Rust-owned. Save can return `sameAccount`, `sameApiKey`, or `sameUrlDifferentApiKey`, then explicitly merge a Key, create a separate card, or overwrite (`src/stores/provider-types.ts:274-295`; `src-tauri/src/services/provider_service/persistence.rs:17-99`).
- Frontend resolution already maps the user's duplicate choice into typed save options (`src/composables/provider-editor-shared.ts:37-64`), and the same-URL/different-Key interaction exposes both merge and separate-card choices (`src/composables/provider-credential-dialogs.ts:6-57`).
- The existing first-run onboarding modal only routes users to import, add-provider, or settings; it is not the provider onboarding wizard (`src/composables/useOnboardingController.ts:18-74`; `src/components/AppOnboardingModal.vue:42-86`).

#### Gaps and minimum new capability

1. The duplicate decision happens only after attempting save (`src/composables/useProviderEditor.ts:136-169`). Add a read-only Rust `plan_provider_save` contract that returns normalized endpoint/protocol, required auth state, duplicate outcome/options, and intended persistence action before final confirmation.
2. Distinguish read-only checks from side effects in the plan. Credential completion may authenticate, refresh tokens, or create/fetch Key material; remote Key creation is explicitly a separate command (`src-tauri/src/commands/provider.rs:179-206`). The wizard must label and confirm those steps rather than calling them as invisible validation.
3. Add a typed, ordered wizard state machine with resumable local draft and stale-request IDs. Reuse the existing editor sections and composables; do not retain both a legacy add flow and a separate wizard implementation.
4. Add a final review showing exact destination action (new card / merge Key / overwrite), protocol, auth mode, detected site identity, selected Key remark, model/capability check state, proxy mode, and automation defaults. Sensitive values remain masked in accordance with repository rules.
5. Capability probing currently targets a persisted provider ID (`src-tauri/src/commands/provider.rs:259-278`). Either add a draft-input capability preview or clearly place the persisted capability probe after save; do not temporarily persist a provider only to probe and roll it back.
6. A combined wizard should preserve the existing protocol-switch warning that clears protocol-specific credentials (`src/composables/useProviderCredentialCompletion.ts:326-366`) and reuse the exact Rust duplicate rules. It must not recalculate URL/user/Key identity in TypeScript.

#### Relationship to the availability decision center

- Medium integration dependency, not a hard delivery dependency. A successfully added provider should surface the same canonical facts/freshness used by the decision center after its first refresh/probe.
- Stronger recommended dependency on diagnostics: the wizard should reuse diagnostic check result contracts/rendering for endpoint, proxy, TLS, Agent, and terminal checks instead of inventing another step-result format.
- The wizard must not rank providers or silently select an Agent default. Its final action is only provider persistence plus explicitly approved remote credential actions.

#### Independent acceptance boundary

- A new endpoint proceeds through protocol detection, explicit ambiguity resolution, auth requirements, read-only checks, duplicate preview, and final confirmation without saving before confirmation.
- Each duplicate outcome offers only Rust-authorized actions, and the final review names the exact card/Key effect.
- Cancelling at any stage leaves persisted providers and remote Keys unchanged except for a remote side effect the user explicitly confirmed in its own step.
- Switching protocol clears only the documented incompatible credential fields after confirmation; stale async results cannot mutate a reopened editor.
- NewAPI, Sub2API, and generic API fixtures cover new, merge, separate, overwrite, invalid credentials, unreachable endpoint, and ambiguous protocol paths.

### Dependency map and task split recommendation

```text
Availability decision center
  -> canonical availability fact projection + freshness/revision contract
      -> Automation rule center (hard dependency)
      -> Session resume candidate selection (optional integration)
      -> Scoped diagnostics deep-link (optional integration)
      -> First-refresh result after onboarding (optional integration)

Diagnostics result contract
  -> One-click diagnostics center
  -> Provider onboarding check/result presentation (recommended dependency)

Agent registry + session adapters
  -> Agent session workspace (independent)

Protocol registry + provider save planner
  -> Provider onboarding wizard
```

Suggested child-task boundaries:

1. `agent-session-workbench`: standalone state/navigation, scoped cancellation, current search/detail reuse, resume intent. No provider ranking changes.
2. `runtime-diagnostics-contract`: Rust check model/orchestrator, safe probe extraction, progress/cancel/redaction, report preview/export. No provider mutation.
3. `provider-onboarding-wizard`: save planner and replacement of the add-provider workflow using existing editor sections. No OAuth/WebView or shield automation.
4. `automation-fact-events`: canonical fact projection, observation revisions, transition semantics, and deterministic tests. This can be a child of or prerequisite inside the automation track.
5. `automation-rule-center`: persistence, CRUD, evaluator, scheduler integration, notification-only action, bounded audit history, UI.

Do not combine all four directions into one implementation task. Their data migrations, IPC contracts, UI entry points, and rollback boundaries are independently testable, while only the fact projection is a genuine cross-feature prerequisite.

### Delivery closure matrix

The implementation plan should treat the following as four product deliverables plus one shared prerequisite. “Minimum delivery” is deliberately smaller than the complete direction so every child can be reviewed, tested, and rolled back independently.

| Order | Child task | Required predecessor | Minimum delivery | Explicitly excluded from this delivery | Independent acceptance |
| --- | --- | --- | --- | --- | --- |
| 0 | Availability fact projection | Availability decision-center contract is frozen | Rust-owned normalized provider/Key facts with observed time, freshness, revision, and unknown state; deterministic fixture tests | Rule CRUD/UI, automatic actions, session UI, diagnostics UI | Given fixed provider observations, the same typed fact snapshot and revision are produced after restart; stale/unknown inputs never become an available result |
| 1A | Agent session workspace | None; only the existing Agent registry/session adapters | Top-level entry, Agent/workspace selection, session-result list, detail, copy ID, and resume intent; cancellation scoped per workbench query | Global all-workspace search, new index schema, provider ranking, session-file mutation | Two distinct scoped searches do not cancel or overwrite each other; detail/resume works for every capability-advertising Agent; close/scope change rejects stale results |
| 1B | One-click diagnostics center | None; contract should be reviewed alongside availability facts | Typed Rust diagnostic result, read-only orchestrator, incremental progress, cancellation/timeouts, central redaction, exact report preview/export | Repair actions, credential refresh, provider persistence, updater-state mutation | One run completes independent checks despite partial failures; cancellation releases UI; before/after persisted state is identical; exported evidence contains no secrets |
| 2 | Provider onboarding wizard | Diagnostics result contract complete; provider save planner implemented first inside this child | Replace the current add flow with endpoint, protocol/auth, checks, duplicate plan, and final review; reuse existing editor sections and Rust duplicate semantics | OAuth/WebView, shield bypass, silent remote Key creation, a parallel legacy add form | No provider is persisted before final confirmation; cancel is side-effect-free except separately confirmed remote actions; fixtures cover new/merge/separate/overwrite and failure paths |
| 3 | Automation rule center | Availability fact projection complete and stable | Persisted notification-only rules, scope/condition/cooldown, transition evaluator, scheduler integration, bounded execution history, CRUD/UI | Automatic provider/Key switching, Agent config changes, paid requests, arbitrary scripts | Rules survive restart; unknown/stale facts do not alert; false-to-true triggers once; cooldown and re-arm are deterministic; disabling stops future evaluations immediately |

Parallelism and gates:

1. Complete and freeze the availability fact projection before starting automation rule persistence or evaluation.
2. The session workspace and diagnostics center may proceed in parallel because they do not share primary implementation ownership.
3. Do not start onboarding UI implementation until the diagnostics result vocabulary is stable. The provider save planner may be designed in parallel, but its Rust duplicate outcome remains the only source of truth.
4. Do not start automation UI first. Land and test fact projection, persisted rule model, and transition semantics before scheduler/UI integration.
5. Each child must pass its own frontend/Rust regression checks before the next dependent child starts; parent integration review verifies only the documented cross-links.

Recommended acceptance checkpoints:

1. **Checkpoint A:** availability facts are deterministic and the decision center consumes them without frontend re-derivation.
2. **Checkpoint B:** session workspace and diagnostics each work as standalone top-level flows and pass async cancellation/state-release tests.
3. **Checkpoint C:** onboarding replaces, rather than duplicates, the existing add-provider entry and consumes the shared diagnostic result renderer.
4. **Checkpoint D:** automation evaluates persisted fact revisions and produces notification-only, restart-safe, auditable outcomes.

## Files Found

- `src-tauri/src/models/agent_cli.rs` - Rust-owned Agent capability descriptors.
- `src-tauri/src/services/agent_cli.rs` - dynamic Agent registry, discovery, and capability derivation.
- `src-tauri/src/models/cli_sessions.rs` - normalized session summary, message, detail, search, and index contracts.
- `src-tauri/src/services/cli_sessions/mod.rs` - bounded session search orchestration and summary cache.
- `src-tauri/src/services/cli_sessions/index.rs` - per-Agent SQLite index queue, cooldown, cancellation, and background progress.
- `src-tauri/src/commands/cli.rs` - session, runtime, terminal, and temporary CLI IPC commands.
- `src/composables/useWorkspaceSessionHistory.ts` - current launch-modal-coupled session workflow state.
- `src/components/WorkspaceSessionHistoryPanel.vue` - current history list/search UI.
- `src/components/CliSessionDetailModal.vue` - current conversation detail UI.
- `src/components/TemporaryCliModal.vue` - current active-instance overview.
- `src-tauri/src/services/scheduler.rs` - fixed automatic refresh/check-in/liveness scheduler and notifications.
- `src-tauri/src/models/provider_domain/automation.rs` - centralized due-time rules.
- `src-tauri/src/services/notifications.rs` - notification channel dispatch.
- `src-tauri/src/app_events.rs` - background task event contract.
- `src/composables/useBackgroundTaskCenter.ts` - ephemeral frontend task presentation.
- `src-tauri/src/models/app_settings.rs` - persisted settings without generic automation rules.
- `src-tauri/src/services/agent_cli/discovery.rs` - bounded Agent CLI discovery.
- `src-tauri/src/services/temporary_cli/terminal/mod.rs` - platform terminal registry and probes.
- `src-tauri/src/network/proxy.rs` - canonical effective proxy resolution and environment projection.
- `src-tauri/src/network/client.rs` - cached provider/webhook/updater client configuration.
- `src-tauri/src/adapters/detector.rs` - protocol detection and ambiguity resolution.
- `src-tauri/src/adapters/protocol/definition.rs` - provider protocol capability registry.
- `src-tauri/src/services/provider_service/persistence.rs` - save transaction and duplicate resolution.
- `src/composables/useProviderEditor.ts` - current provider creation/edit orchestration.
- `src/composables/useProviderCredentialCompletion.ts` - protocol/site/credential assistant and stale-result handling.
- `src/composables/provider-editor-shared.ts` - provider editor contracts and duplicate decision mapping.
- `src/components/AppOnboardingModal.vue` - lightweight first-run routing, not a provider setup wizard.

## External References

- None. This audit is intentionally based on current repository contracts and implementation, not external product comparisons.

## Related Specs

- `.trellis/spec/frontend/state-management.md` - persisted backend state remains the source of truth; stale responses use request IDs/revisions.
- `.trellis/spec/frontend/component-guidelines.md` - bounded components, semantic events, accessible async states, and no frontend capability duplication.
- `.trellis/spec/guides/cross-layer-thinking-guide.md` - define IPC formats and single payload owners before cross-layer implementation.
- `.trellis/spec/guides/code-reuse-thinking-guide.md` - extend existing services/composables before introducing parallel abstractions.
- `AGENTS.md` - Rust owns capabilities and IPC; network/process operations require timeout/cancellation and UI-state release.

## Caveats / Not Found

- No runtime App diagnostics service, diagnostic IPC model, report exporter, or diagnostic view was found. The repository `doctor` script is development-only.
- No generic persisted automation rule, rule evaluator, cooldown state, or durable rule execution history was found.
- No standalone session-workbench route/view was found; all user-facing history state is currently anchored to the temporary CLI workflow.
- No read-only provider save-plan/dry-run command was found; duplicate conflicts are returned by the mutating save command, although an unchanged conflict path does not persist.
- The current search cancellation mechanism is global to the process, so a future aggregate workbench must not fan out concurrent `search_cli_sessions` calls without first changing cancellation ownership.
- `CliSessionSummary` is intentionally a least-common-denominator contract. Agent-specific fields should remain adapter-owned rather than expanding the shared model with guessed values.
- Provider site and connection probes need a side-effect audit before reuse in diagnostics: some protocol flows can refresh authenticated credentials through `ProviderOperationOutcome`, even if draft-level commands do not persist provider observations.
