# Research: Availability Decision Center

- Query: Inspect the current BalanceHub implementation and define a source-backed boundary and MVP for a model/provider/API-key availability decision center.
- Scope: internal
- Date: 2026-09-04

## Findings

### 1. Files found

- `src-tauri/src/models/provider/state.rs`: persisted Provider identity, credentials, quota, model list, automation timestamps, liveness configuration/history, and runtime status.
- `src-tauri/src/models/provider_results.rs`: API-key metadata, refresh progress, model-sync result, Agent default-config snapshots, and temporary CLI instance contracts.
- `src-tauri/src/models/liveness.rs`: the persisted liveness observation shape.
- `src-tauri/src/contracts.rs`: Rust-owned IPC presentation contract and action capability derivation.
- `src-tauri/src/services/provider_service/refresh.rs`: serialized refresh entry points, bounded concurrent refresh, stale-result protection, and observation merge.
- `src-tauri/src/services/provider_service/capabilities.rs`: capability probing and explicit model synchronization.
- `src-tauri/src/services/provider_service/api_keys.rs`: local/remote API-key catalog synchronization and current-key mutation.
- `src-tauri/src/adapters/protocol/contracts.rs`: separation of credential side effects and remotely observed Provider facts.
- `src-tauri/src/adapters/new_api/adapter.rs`, `new_api/quota.rs`: NewAPI account/key refresh and model discovery behavior.
- `src-tauri/src/adapters/sub2_api/adapter/refresh.rs`: Sub2API token refresh, account refresh, and best-effort model discovery.
- `src-tauri/src/adapters/api.rs`: generic OpenAI-compatible model-only refresh.
- `src-tauri/src/services/liveness.rs`: real Agent CLI liveness execution, proxy use, timeout, cleanup, latency, token, and cost collection.
- `src-tauri/src/services/scheduler.rs`: background refresh/liveness cadence and concurrency.
- `src-tauri/src/services/cli_runtime.rs`, `cli_runtime/config.rs`, `agent_cli/config_support/mod.rs`: Agent catalog, current default-config discovery, and exact Provider/Key matching.
- `src/stores/provider-types.ts`: TypeScript mirror of Provider, API-key, liveness, Agent, and runtime IPC data.
- `src/stores/providers.ts`: frontend snapshot/revision handling and explicit refresh/model/key actions.
- `src/stores/cli-runtime.ts`: frontend Agent config and temporary-instance state.
- `src/components/ProviderBoard.vue`: current per-card Agent binding and active-instance derivation.
- `src/components/provider-card/ProviderCardBody.vue`: current quota/model/liveness presentation.
- `src/components/AvailableModelsModal.vue`, `src/components/LivenessDetailsModal.vue`: existing model and liveness detail UI that can be reused or linked.
- `src/components/AppWorkspace.vue`, `src/App.vue`: current single-board workspace composition and top-level orchestration.

### 2. Existing reusable facts

The persisted `Provider` already contains most raw observations needed for a first decision surface:

- Stable Provider identity, protocol, URL, display name, username, user ID, and local remark are present in `ProviderIdentity` (`src-tauri/src/models/provider/state.rs:54-73`).
- Current authentication material and the local/remote API-key catalog are present in `ProviderAuth` (`src-tauri/src/models/provider/state.rs:106-131`).
- Provider quota carries available, used, known/total-known, scope, unlimited, and display metadata (`src-tauri/src/models/provider/state.rs:133-154`).
- Dynamic model names are persisted in `ProviderCapabilities.available_models` (`src-tauri/src/models/provider/state.rs:156-181`).
- Provider-level refresh time is persisted as `automation.last_synced_at` (`src-tauri/src/models/provider/state.rs:183-197`).
- Each liveness record already stores checked time, success, latency, model, Base URL, Agent kind, token usage, cost, and error/result text (`src-tauri/src/models/liveness.rs:5-35`).
- Runtime state distinguishes enabled and `ok`/`warning`/`error`/`syncing` (`src-tauri/src/models/provider/state.rs:301-307`, `src/stores/provider-types.ts:63-65`).

API-key rows already have useful key-scoped facts:

- Stable local identity and local remark are separated from remote token ID/name (`src-tauri/src/models/provider_results.rs:68-84`).
- Status, used/remain quota, unlimited flag, group, model restrictions, allow-list, and timestamps are retained (`src-tauri/src/models/provider_results.rs:84-100`).
- Full-key usability is centrally defined as non-empty and non-redacted, rather than trusting a stale boolean (`src-tauri/src/models/provider_results.rs:231-238`).
- `local_id` is deterministically derived from protocol plus full key, token ID, or mask (`src-tauri/src/models/provider_results.rs:240-264`). This is the correct stable key for decision rows and action routing; plaintext keys must not appear in the decision IPC result.

Rust already owns capability decisions at the IPC boundary. `ProviderView` derives account management, check-in, API-key management, invitation, and model-only refresh flags from protocol/domain rules (`src-tauri/src/contracts.rs:12-74`). The decision center should extend this pattern with Rust-owned action availability instead of recreating it in TypeScript.

Agent default binding is also available as a real observation rather than a configured guess:

- `CliConfigSnapshot` carries Agent kind, whether it is configured, matched Provider ID, matched API-key local ID, config modification time, and read error (`src-tauri/src/models/provider_results.rs:659-668`).
- The runtime snapshot iterates the dynamic Rust Agent registry, not a hard-coded four-Agent switch (`src-tauri/src/services/cli_runtime.rs:76-85`).
- Matching normalizes both endpoint and API key, then returns the exact Provider and local Key identity (`src-tauri/src/services/agent_cli/config_support/mod.rs:214-245`).
- The board already joins these snapshots to a Provider and resolves the selected key label (`src/components/ProviderBoard.vue:86-99`). That join should move into the backend decision projection rather than be copied into a new frontend component.

The existing direct actions can be reused:

- Selecting the current Provider key is a stable `provider_id + local_id` mutation (`src-tauri/src/services/provider_service/api_keys.rs:62-77`).
- Agent default switching already has a revisioned preview/confirm flow (`src-tauri/src/services/cli_runtime/config.rs:24-53`, `src-tauri/src/services/agent_cli/config_support/mod.rs:154-160`).
- Temporary CLI launch already routes through Provider ID, API-key local ID, model, workspace, Agent, and terminal contracts; the decision center should open that existing flow rather than create another launcher.
- Existing model and liveness details can be opened through `useAvailableModels` and `useProviderActions` (`src/composables/useAvailableModels.ts:20-47`, `src/composables/useProviderActions.ts:166-188`).

### 3. Current end-to-end data flow

#### Startup/local facts

```text
data.json
  -> AppState.data (loaded once, then in-memory RwLock)
  -> load_app_data command / AppDataView
  -> Pinia provider store (revision-aware merge)
  -> AppController
  -> AppWorkspace
  -> ProviderBoard / ProviderCard
```

`AppState` loads configuration into memory once and persists mutations atomically; reads do not repeatedly scan the JSON file (`src-tauri/src/state.rs:7-24`). The frontend refuses older full snapshots and merges Provider updates by monotonic revision (`src/stores/providers.ts:68-98`). A new decision snapshot should include the source App revision so the frontend can discard an older response using the same mechanism.

#### Provider refresh

```text
topbar/card refresh
  -> provider store marks rows syncing
  -> refresh_providers / refresh_all_providers_with_progress
  -> ProviderService refresh gate
  -> ProtocolAdapter.refresh_provider
  -> protocol-specific account/quota/model requests
  -> ProviderOperationOutcome credential + observation patches
  -> request-context CAS against latest Provider
  -> atomic persistence
  -> ProviderView returned to Pinia
```

The frontend immediately marks selected rows `syncing` and releases them in `finally` (`src/stores/providers.ts:332-370`). The backend serializes refresh/authentication operations with `refresh_gate` (`src-tauri/src/services/provider_service/refresh.rs:20-45`) and refreshes at most six Providers concurrently (`src-tauri/src/services/provider_service/refresh.rs:181-261`). Network results only merge if the complete Provider credential context still matches, preventing old results from overwriting a newly selected account/key (`src-tauri/src/services/provider_service.rs:25-83`, `src-tauri/src/services/provider_service/refresh.rs:141-179`).

The protocol layer explicitly separates credential rotation from observed quota/model/runtime fields (`src-tauri/src/adapters/protocol/contracts.rs:10-74`, `src-tauri/src/adapters/protocol/contracts.rs:83-188`). This is an important reuse boundary for any future explicit refresh initiated from the decision center.

#### Model discovery

```text
explicit model refresh or Provider refresh
  -> ProviderService / protocol adapter
  -> shared OpenAI-compatible GET /models using current Provider API key
  -> normalize/truncate model names
  -> persist ProviderCapabilities.available_models
```

The shared model request uses the Provider-aware network client and bearer-authenticates with the current `provider.auth.api_key` (`src-tauri/src/adapters/api.rs:189-204`). Generic API Providers only perform model discovery and mark account quota unknown (`src-tauri/src/adapters/api.rs:153-185`). NewAPI account refresh first obtains quota and then fetches models when a current key exists (`src-tauri/src/adapters/new_api/adapter.rs:314-364`). Sub2API refresh authenticates/rotates tokens, refreshes account data, and attempts models with the current key (`src-tauri/src/adapters/sub2_api/adapter/refresh.rs:126-190`, `src-tauri/src/adapters/sub2_api/adapter/refresh.rs:251-260`).

#### Liveness

```text
scheduler due check
  -> spawn_blocking, max 3 concurrent
  -> ProviderService.run_liveness
  -> Agent registry selects liveness adapter
  -> isolated temporary HOME + Provider-aware proxy
  -> real installed Agent CLI process and real model request
  -> bounded output + timeout + process cleanup
  -> LivenessRecord persisted on Provider (max 40)
```

The liveness runner requires a usable API key, resolves Agent/model/URL from settings and Provider overrides, then starts the real CLI (`src-tauri/src/services/liveness.rs:38-88`, `src-tauri/src/services/liveness.rs:90-186`). It applies the common network proxy and records elapsed latency plus actual token/cost output (`src-tauri/src/services/liveness.rs:157-166`, `src-tauri/src/services/liveness.rs:186-255`). Scheduler execution uses `spawn_blocking` and a concurrency cap of three so child-process waiting does not block async runtime threads (`src-tauri/src/services/scheduler.rs:531-583`). Records are bounded to 40 while cumulative usage remains separate (`src-tauri/src/services/provider_service/liveness.rs:24-71`, `src-tauri/src/limits.rs:27-29`).

### 4. Missing or ambiguous data

The current data must not be presented as more precise than it is.

1. **Model freshness has no independent timestamp.** `available_models` has no `models_synced_at`. Explicit `sync_available_models` replaces the list but does not update `last_synced_at` or `probed_at` (`src-tauri/src/services/provider_service/capabilities.rs:125-162`).
2. **`last_synced_at` conflates different facts.** Generic API refresh means models were fetched (`src-tauri/src/adapters/api.rs:170-182`), while account protocols use it mainly for account/quota refresh. Sub2API ignores model-fetch failure and still records a successful sync time (`src-tauri/src/adapters/sub2_api/adapter/refresh.rs:168-205`). Therefore it cannot prove model freshness.
3. **Switching current Key deliberately keeps the old model list visible.** It clears `last_synced_at` but retains `available_models` until an explicit refresh (`src-tauri/src/models/provider/input.rs:257-275`). This is good card UX but means the model list is explicitly stale/unknown for the new key.
4. **Models are observed through the current key only.** A Provider may hold up to 100 keys, but `/models` uses `provider.auth.api_key`; no per-key model observation exists. Alternate keys only have explicit `model_limits` when the upstream key catalog supplies them.
5. **Liveness records have no Key identity.** They live under a Provider and record Agent/model/URL, but not `api_key_local_id` (`src-tauri/src/models/liveness.rs:7-35`). After the current key changes, historical success cannot safely be attributed to the new key.
6. **API-key catalog freshness is unknown.** Key metadata is updated by the explicit remote list path (`src-tauri/src/services/provider_service/api_keys.rs:94-115`) but neither the Provider nor individual keys record when that catalog was fetched.
7. **Key status is not one normalized domain enum.** Sub2API maps variants to `enabled`/`disabled`/`expired`/`exhausted` (`src-tauri/src/adapters/sub2_api/keys.rs:135-143`), while NewAPI retains the server value. The frontend currently duplicates string interpretation (`src/components/provider-editor/ProviderApiKeyVault.vue:146-168`). A decision engine must normalize this once in Rust.
8. **Provider runtime status is composite.** It can reflect quota/auth/model refresh behavior and must not be treated as a direct assertion that one selected model is unavailable. Independent evidence dimensions are required.
9. **Positive quota is not universal.** Generic API/key-only Providers intentionally expose unknown account quota (`src-tauri/src/adapters/api.rs:170-181`). Unknown quota must not rank as zero or failure.
10. **Legacy liveness records remain unattributed.** Adding optional key identity can make future records precise, but existing records must remain `unknown-key` rather than being assigned to whichever key is current now.

### 5. Refresh and freshness semantics

Current automation provides cadence but not a sufficiently granular freshness contract:

- Global refresh defaults to 300 seconds and is enabled by default (`src-tauri/src/models/app_settings.rs:78-89`).
- Provider-specific `refresh_interval` overrides the global interval (`src-tauri/src/models/provider_domain/automation.rs:7-13`).
- The Rust scheduler evaluates due work every 30 seconds and records attempts so a failed refresh does not retry on every tick (`src-tauri/src/services/scheduler.rs:68-100`, `src-tauri/src/services/scheduler.rs:147-233`).
- Automatic liveness is disabled by default (`src-tauri/src/models/app_settings.rs:93-105`) and only runs for enabled Providers with an API key and effective liveness enabled (`src-tauri/src/models/provider_domain/liveness.rs:3-16`).

Recommended contract additions before claiming precise freshness:

- `quota_synced_at_ms: Option<u64>` for the last successful quota/account observation.
- `models_synced_at_ms: Option<u64>` plus `models_api_key_local_id: Option<String>` for the key that produced the model list.
- `api_keys_synced_at_ms: Option<u64>` for the remote key catalog.
- `api_key_local_id: Option<String>` on new liveness records.
- IPC-only normalized timestamps in milliseconds. Existing persisted values mix Unix seconds (`last_synced_at`) and milliseconds (`checked_at`), so UI code should not guess units.

Freshness should be explicit per evidence source:

- `unknown`: no successful observation, missing timestamp, legacy record, or source read error.
- `fresh`: observation age is within the configured interval that owns that source, allowing the 30-second scheduler tick tolerance.
- `stale`: a valid observation exists but its configured next refresh time has passed.
- `refreshing`: an explicit background task currently owns that source.

Freshness is not availability. A stale successful result remains “last observed successful, now stale”; it must not become either green-current or red-failed. Similarly, an error must carry the failed attempt time separately from the last successful observation so failure does not erase useful history.

### 6. Real-request side effects

The decision center must be locally read-only on open and while filtering.

- Provider refresh performs remote account/quota/model requests and can rotate/persist Sub2API access and refresh tokens (`src-tauri/src/adapters/sub2_api/adapter/refresh.rs:126-177`). It is not a pure read even when initiated by a button.
- Model refresh performs authenticated `/models` requests. It normally does not make a paid completion, but it is still a network action and can trigger rate limits, gateway protection, or authentication changes.
- API-key list refresh authenticates and persists returned credential changes and key metadata (`src-tauri/src/services/provider_service/api_keys.rs:94-115`).
- Liveness starts a real Agent CLI request and records actual tokens/cost (`src-tauri/src/services/liveness.rs:124-186`, `src-tauri/src/services/liveness.rs:233-255`). It can consume quota and must never run silently as part of sorting/filtering.
- Selecting a current key mutates BalanceHub local configuration. Setting an Agent default writes external Agent configuration files and already requires revisioned preview/confirmation.
- Launching a temporary CLI opens an external terminal/session and is an explicit user action.

Recommended action labels in the snapshot should distinguish `localMutation`, `externalConfigWrite`, `networkMetadata`, `paidProbe`, and `externalProcess`. The UI can then use the existing confirmation level appropriate to each action without owning protocol rules.

### 7. Recommended aggregation boundary

Add one backend-owned, read-only `AvailabilityService`, separate from protocol adapters and persisted models:

```text
AppState Provider snapshot + App revision
  + Agent registry/config snapshots
  -> AvailabilityService projection/evaluator
  -> compact AvailabilitySnapshot IPC contract
  -> frontend rendering and transient selection only
```

The service should not call `ProtocolAdapter`, run CLI probes, or mutate storage. It should consume only already observed facts. Protocol adapters remain responsible for acquiring facts; the availability service is responsible for normalizing evidence and evaluating a selected model.

Suggested IPC shape:

```text
AvailabilitySnapshot
  generatedAtMs
  sourceRevision
  modelCatalog[]
  selectedModel?
  candidates[]

AvailabilityCandidate (one Provider + one Key identity)
  candidateId = providerId + apiKeyLocalId
  provider { id, label, protocol, enabled }
  key { localId, label, current, usable, normalizedStatus }
  modelEvidence { state, source, observedAtMs }
  quotaEvidence { scope, known, unlimited, available, observedAtMs, freshness }
  livenessEvidence { state, checkedAtMs, latencyMs, agentKind, exactKeyMatch }
  agentBindings[] { kind, modifiedAtMs }
  actionCapabilities { launch, selectCurrentKey, setAgentDefaults[] }
  availabilityTier
  reasons[]
```

Do not return plaintext credentials, prompts, raw responses, or whole `Provider` objects. Return stable IDs for actions, and let the existing command validate the latest state again.

Use a query-specific evaluator rather than returning the Cartesian product of all models and keys. Repository limits allow 200 Providers, 100 keys per Provider, and 2,000 models per Provider (`src-tauri/src/limits.rs:15-18`). Precomputing model x key rows could reach tens of millions of combinations. A model catalog plus an evaluation command for one selected model keeps payload and CPU bounded.

The backend should build hash maps/sets once per evaluation:

- Agent binding map keyed by `(provider_id, api_key_local_id)`.
- Provider model set keyed by Provider ID and its producing current Key ID.
- Latest liveness keyed by `(provider_id, api_key_local_id?, normalized_model)`.
- Candidate rows in persisted Provider/Key order for deterministic tie-breaking.

No opaque weighted “score” should be exposed in phase one. Use auditable tiers and ordered reasons:

1. Recent successful liveness for the exact Provider + Key + model.
2. Explicit model support for that Key and a usable/enabled key.
3. Current-key Provider model observation with no contradictory key restriction.
4. Unknown model support but otherwise usable metadata.
5. Explicitly disabled, expired, exhausted, missing-full-key, or explicit model exclusion.

Within a tier, prefer fresh over stale evidence, known usable/unlimited quota over unknown, recent successful observation over older, then lower latency. Preserve Provider and key order as the final stable tie-breaker. Unknown must never be silently converted into unavailable.

### 8. Phase-one minimum delivery

The smallest useful, honest delivery is:

1. Add separate quota/model/key-catalog observation timestamps and future liveness Key identity, with defaults and storage migration/tests as required by repository rules.
2. Add a Rust-only `AvailabilityService` and compact IPC contract. Opening it performs no network or CLI work.
3. Add a model catalog/search selector sourced from cached dynamic models. Selecting a model evaluates and returns session/config candidates, not one row per conversation message or one model x key Cartesian table.
4. Render a dedicated full workspace view entered from the search-context action, rather than another large modal, permanent dashboard header, or permanent peer tab. `AppWorkspace` currently owns a single Provider board and local search (`src/components/AppWorkspace.vue:21-63`), so it is the appropriate view-switch boundary; `App.vue` should remain orchestration only (`src/App.vue:18-86`).
5. Show one compact candidate row per Provider + locally known Key. Alternate keys without key-specific model/liveness evidence remain visible but explicitly marked “未验证”, not ranked as confirmed alternatives.
6. Display separate evidence columns: model match, key state, quota, latest exact liveness, latency, freshness, and Agent default bindings. Do not derive overall state from the existing card color.
7. Reuse existing flows for “启动临时 CLI”, “设为当前 Key”, “设置 Agent 默认配置”, “查看模型”, and “查看测活明细”. Every mutating action resolves the current Provider/Key by stable ID and uses existing backend validation.
8. Keep the existing topbar global refresh as the explicit network-backed source update. In the result area, provide only “重新生成比较”, which recalculates from local observations without network or CLI work. Do not refresh on view open or model input. Do not include “立即验证” (paid liveness) until a separately confirmed flow can disclose Agent/model/prompt and potential quota use.
9. Add Rust projection/ranking tests plus frontend state tests for empty, unknown, stale, mixed-Key, stale-response, and disabled/error cases.

This MVP immediately answers “which currently known configuration is the best-supported choice for this model?” without pretending all keys were tested and without turning BalanceHub into a proxy/router.

### 9. Cross-platform and performance risks

- Pure Provider/Key/model evaluation is platform-neutral Rust and should remain outside `cfg(target_os)` branches.
- Agent default-config snapshots read different native config files and paths on macOS/Linux/Windows. Read errors or an uninstalled Agent mean “binding unknown/unavailable”, not Provider failure. The dynamic Agent registry remains the only Agent enumeration source (`src-tauri/src/services/cli_runtime.rs:76-85`).
- Config-file inspection is blocking filesystem work. If the availability command refreshes Agent bindings, run that scan through `spawn_blocking` and only when the view opens or the user requests refresh; do not repeat it on each keystroke.
- Provider HTTP clients have a 20-second timeout (`src-tauri/src/network/client.rs:62-69`). A six-wide refresh window can still take multiple waves for many Providers; the decision view must remain usable from its previous snapshot while background refresh runs.
- Liveness can wait up to 600 seconds by configuration (`src-tauri/src/limits.rs:27-28`). It must never be part of a synchronous decision-center load.
- A full AppData IPC snapshot can include credentials and up to 200 x 2,000 model strings. The decision contract must be compact, credential-free, and query-specific rather than cloning complete Providers for every candidate.
- Frontend model input should filter the compact catalog locally, debounce only actual backend evaluations, and use a monotonically increasing request ID so a late older result cannot overwrite a newer selected model.
- Timestamp normalization belongs in Rust because existing sources use both seconds and milliseconds. The browser timezone should only format normalized instants for display.
- Candidate identity must use scalar `provider_id + api_key_local_id`, not Vue object identity, matching the repository async-state rule.

### 10. Suggested implementation ownership and verification

Recommended file boundaries for the later child task:

- Rust persisted observation additions: `src-tauri/src/models/provider/state.rs`, `models/liveness.rs`, `models/provider/input.rs`, protocol adapters/services that own successful observations, storage migration/default/tests.
- Rust projection: new `src-tauri/src/services/availability.rs` and IPC-only result types under `contracts.rs` or a dedicated result module; command wiring in `commands/provider.rs` and `desktop.rs`.
- Frontend contract/store: `src/stores/provider-types.ts`, `src/api/app.ts`, and a dedicated availability store/composable. Do not add this state to the already broad Provider card composables.
- UI: a dedicated `src/components/availability/` view selected by `AppWorkspace.vue`; existing dialogs/actions remain reused.

Minimum verification matrix:

- Current key with fresh exact model + exact successful liveness.
- Alternate key with explicit model limit but no key-attributed liveness.
- Alternate key with no model limits (unknown, not assumed supported).
- Key disabled/expired/exhausted/unreadable.
- Provider quota known zero, unlimited, and unknown.
- Model list retained after current-key switch but marked stale/unattributed.
- Legacy liveness record without Key identity.
- Agent config matched to exact Key, unmatched config, unreadable config, and dynamically added Agent descriptor.
- Refresh/result revision race and selected-model request race.
- macOS/Linux/Windows compile paths; no platform-specific evaluator branches.
- No network call, child process, config write, or storage mutation when opening/filtering the center.

## Code Patterns

- Keep Rust as the source of action capability and protocol meaning, following `ProviderView::from` (`src-tauri/src/contracts.rs:40-74`).
- Preserve request-context compare-and-set when any explicit refresh result is applied (`src-tauri/src/services/provider_service.rs:25-83`).
- Reuse `ProviderOperationOutcome` for credential and observation changes rather than replacing whole Provider objects (`src-tauri/src/adapters/protocol/contracts.rs:140-188`).
- Enumerate Agents through `agent_cli::definitions()` and descriptors, never a fixed Codex/Claude/Gemini/Grok list (`src-tauri/src/services/cli_runtime.rs:76-85`).
- Cap persisted and returned collections using the existing central limits (`src-tauri/src/limits.rs:15-39`).
- Keep frontend async busy state in `try/finally` and reject stale scalar-request IDs, consistent with current Provider refresh behavior (`src/stores/providers.ts:332-370`).

## External References

No external implementation reference was required for this topic. The relevant semantics are application-specific and were derived from the current repository source. Product comparisons or routing algorithms should not override the observed-data limitations above.

## Related Specs

- `.trellis/spec/guides/cross-layer-thinking-guide.md`: map source -> transform -> store -> IPC -> display, keep payload decoding and business rules at one owner.
- `.trellis/spec/guides/code-reuse-thinking-guide.md`: extend existing Provider actions, Agent registry, refresh outcomes, and dialog flows instead of introducing parallel logic.
- `.trellis/spec/frontend/state-management.md`: use stores for cross-view backend state and local component state for transient filters/selections.
- `.trellis/spec/frontend/type-safety.md`: mirror the Rust IPC contract exactly and avoid local casts.
- `.trellis/spec/frontend/component-guidelines.md`: keep the new workspace view decomposed by responsibility and keep `App.vue` orchestration-only.

## Caveats / Not Found

- There is no current persisted per-Key model observation, per-Key liveness identity, model-specific health summary, or independent quota/model/key-catalog freshness timestamp.
- No exposed manual `run_liveness` Tauri command was found; current liveness execution is driven by the Rust scheduler. Adding an explicit paid verification action is therefore a separate product/command design, not a reuse-only UI change.
- `CliConfigSnapshot` is an observation of local Agent config files, not proof that the Agent executable is installed or that a future launch will succeed. CLI discovery is a separate probe contract.
- Current remote key status values can vary by deployment; unknown values must remain unknown until the Rust domain normalizer has a protocol-backed mapping.
- Exact freshness thresholds beyond configured refresh/liveness intervals are a product policy decision. The research recommends transparent age/due-state rather than inventing a hidden universal TTL.
