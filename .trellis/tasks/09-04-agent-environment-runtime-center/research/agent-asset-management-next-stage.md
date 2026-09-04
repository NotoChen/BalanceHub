# Research: Agent Skill, MCP, Plugin, And Status UI Management

- Query: Define the next product and architecture phase after the Agent environment/runtime/Hook work for Skills, MCP servers, Plugins/Extensions, and Status UI, without falsely unifying Agent semantics or taking over user configuration.
- Scope: mixed
- Date: 2026-09-04

## Findings

### 1. Executive decision

This work should be a **separate Trellis child task**, not an additional phase silently appended to the current implementation task.

Recommended child:

```text
Title: Agent 资产控制中心
Slug: agent-asset-control-center
Parent: .trellis/tasks/09-04-agent-environment-runtime-center
```

The current task explicitly limits its writable scope to BalanceHub-owned Hooks and lists full Skill, Plugin, MCP, and Statusline management as out of scope (`prd.md:9-14`, `prd.md:42-46`, `prd.md:81-88`). Its Phase 1 inventory is the prerequisite, but it still records two foundational gaps: it discovers only one executable per Agent and currently leaves declared/effective/trust state unknown (`implement.md:3-10`). Mixing asset mutation into this task would weaken both its completion boundary and rollback story.

The next child should own:

- logical asset discovery and effective-state parsing for the current four Agents on native macOS, Linux, and Windows;
- one list-first control surface covering Skills, MCP servers, Plugins/Extensions, and Status UI;
- Rust-owned, per-record action capabilities and disabled reasons;
- revision-safe plan/apply/verify for only those native enable/disable actions whose semantics are documented and testable;
- source, scope, precedence, trust, collision, restart/reload, and diagnostics display;
- adapter registration that allows a fifth Agent without changing generic orchestration.

It should not own:

- Hook lifecycle management, which remains in the current task and is linked from the asset view rather than duplicated;
- Agent installation, upgrade, package-manager execution, marketplace browsing, asset installation/uninstallation, or remote catalog synchronization;
- a BalanceHub-owned cross-Agent Skill/MCP/plugin source of truth, directory relocation, copying, or symlink creation;
- arbitrary configuration editing, auth file editing, trust-store changes, managed/system-scope writes, shell-profile changes, or privilege escalation;
- WSL discovery or mutation, which requires a separate environment/transport task;
- the deferred decision center.

The child can be planned now. Mutation implementation should not start until the minimum dependency gates in section 3 are complete.

### 2. Current implementation is a file inventory, not yet an asset manager

#### Files found

- `.trellis/tasks/09-04-agent-environment-runtime-center/prd.md`: current runtime/Hook scope, non-takeover requirements, and explicit asset-management exclusion.
- `.trellis/tasks/09-04-agent-environment-runtime-center/design.md`: environment identity, read-only inventory, Hook ownership, and list-first UI design.
- `.trellis/tasks/09-04-agent-environment-runtime-center/implement.md`: completed work and unresolved multi-installation/effective-state, Hook atomicity, long-session, and real-platform validation gaps.
- `.trellis/tasks/09-04-product-evolution-roadmap/research/agent-assets-versioning-audit.md`: official four-Agent capability matrix, CC Switch comparison, versioning strategy, WSL boundary, and safe mutation requirements.
- `src-tauri/src/models/agent_environment.rs`: current Rust environment, installation, source, capability, asset, state, trust, and preview contracts.
- `src-tauri/src/services/agent_cli/contracts.rs`: registry-owned `EnvironmentAdapter` and file/directory declaration contract.
- `src-tauri/src/services/agent_cli/environment/inventory.rs`: native inventory orchestration, directory child expansion, stable IDs, metadata revision, and read-only capability generation.
- `src-tauri/src/services/agent_cli/environment/path_access.rs`: opaque-ID re-resolution, bounded preview, secret suppression, and symlink protection.
- `src-tauri/src/services/agent_cli/{claude,codex,gemini,grok}/mod.rs`: current Agent-specific file/directory declarations.
- `src-tauri/src/models/agent_hook.rs`: existing Hook inspection, action capability, ownership, and plan contracts.
- `src-tauri/src/services/agent_runtime/managed_hook/`: existing revision checks, structural identity/fingerprint checks, atomic writes, plan/apply revalidation, and Hook-specific adapters.
- `src/components/settings/agent-environment/AgentEnvironmentConsole.vue`: current list-first Agent/Hook console.
- `src/components/settings/agent-environment/AgentAssetInventory.vue`: current read-only asset list and category/search controls.
- `src/components/settings/agent-environment/AgentInstallationDetail.vue`: current detail tabs and low-frequency asset/configuration entry.
- `src/composables/useAgentEnvironmentCenter.ts`: current workspace, selection, filtering, preview, and stale-response state.

#### What already works

- Rust already defines the right top-level nouns: category, source scope, declared/effective state, trust, mutation class, and diagnostics (`src-tauri/src/models/agent_environment.rs:36-95`, `src-tauri/src/models/agent_environment.rs:138-218`).
- Agent-specific locations are registered through `AgentCliDefinition.environment` rather than frontend switches. The four adapters declare user/workspace roots for Skills, Plugins/Extensions, MCP, Hooks, and Status UI.
- Opaque IDs are re-resolved in Rust by rescanning adapter declarations; the frontend cannot submit an arbitrary path (`src-tauri/src/services/agent_cli/environment/path_access.rs:25-54`).
- Preview is bounded to 128 KiB, rejects symlink paths, and suppresses credential-bearing files before IPC (`src-tauri/src/services/agent_cli/environment/path_access.rs:79-145`, `src-tauri/src/services/agent_cli/environment/path_access.rs:177-198`).
- The current asset UI already has one search field, category selection, source/state/trust display, and direct open/copy controls (`src/components/settings/agent-environment/AgentAssetInventory.vue:26-56`, `src/components/settings/agent-environment/AgentAssetInventory.vue:59-109`).
- The Hook console proves the desired interaction pattern: a continuous list, Rust-provided action availability, row-local busy/error state, and a shared plan confirmation surface (`src/components/settings/agent-environment/AgentEnvironmentConsole.vue:35-47`, `src/components/settings/agent-environment/AgentEnvironmentConsole.vue:59-103`; `src/components/settings/agent-environment/AgentEnvironmentRow.vue:33-70`, `src/components/settings/agent-environment/AgentEnvironmentRow.vue:75-153`).

#### What must change before management is credible

1. The current `AgentAssetRecord` is usually a declared file, directory, or one immediate directory child, not a parsed native asset. A `config.toml` containing ten MCP servers is currently one MCP record; a directory child is accepted without parsing its manifest (`src-tauri/src/services/agent_cli/environment/inventory.rs:276-336`, `src-tauri/src/services/agent_cli/environment/inventory.rs:339-408`).
2. Presence deliberately maps to `unknown`; declared versus effective state, trust, collision, and precedence are not computed yet (`src-tauri/src/services/agent_cli/environment/inventory.rs:347-366`).
3. Every discovered category currently receives `mutation = ReadOnly`, with `requires_restart = false` and `requires_trust = false` regardless of Agent semantics (`src-tauri/src/services/agent_cli/environment/inventory.rs:251-273`).
4. Installation identity is currently `environment + agent kind`, even though the UI and model claim to allow multiple installations. It must include executable/install identity before asset operations can target the correct CLI (`src-tauri/src/services/agent_cli/environment/inventory.rs:98-170`).
5. Assets are selected in the frontend by Agent kind rather than installation ID (`src/composables/useAgentEnvironmentCenter.ts:71-91`). With multiple installations, that can mix sources and action semantics.
6. Inventory `revision` is metadata-derived (`length:modified-time`), which is adequate for display invalidation but not a write precondition. Mutations need a content digest plus existence/type identity (`src-tauri/src/services/agent_cli/environment/inventory.rs:309-336`, `src-tauri/src/services/agent_cli/environment/inventory.rs:483-490`).
7. Current Hook apply correctly locks, re-inspects, compares revision, regenerates the plan, and compares changes before write (`src-tauri/src/services/agent_runtime/managed_hook/codex/apply.rs:16-45`), but config and ownership manifest are two separate atomic file operations (`src-tauri/src/services/agent_runtime/managed_hook/codex/apply.rs:78-89`). That known half-commit edge should not be copied into a generic asset engine.

### 3. Exact dependency boundary

#### Hard prerequisites before any writable asset action

- **Installation identity:** each `AgentInstallation.id` must include native environment, Agent kind, canonical executable identity, and installation source. Asset records must carry `installation_id`.
- **Logical parsers:** each writable category must parse the native source into logical records and compute effective state. Directory existence and config-file existence are not enough.
- **Exact source revision:** writable source revisions must hash exact bytes plus missing/file/type state. Directory assets need a bounded manifest/index revision, not recursive content hashing.
- **Generic plan/apply kernel:** extract the safe primitives from Hook management without reusing Hook-specific models or pretending all assets are owned.
- **Capability contract:** Rust returns available actions, mechanism, disabled reason, restart/reload consequence, trust requirement, and scope restriction per record.

#### Dependencies that may remain independent

- Claude/Gemini large-session suffix parsing does not block asset inventory or native toggles.
- Runtime reducer/enrichment changes do not block asset control once installation identity is stable.
- Real Hook-event validation does not block asset control, because Hook operations continue through the existing Hook controller.
- WSL support does not block native three-platform work, but all IDs must retain `environment_id` so WSL does not force a later schema rewrite.

#### Recommended task tree

One direct child should own the product contract and integration review. Its execution can be split into non-overlapping subtasks only after the child PRD/design are approved:

```text
agent-asset-control-center
  1. logical-asset-inventory-foundation
  2. mcp-control
  3. plugin-extension-control
  4. skill-control
  5. status-ui-control
```

The foundation is a prerequisite for every category. MCP, Plugin/Extension, Skill, and Status UI adapters can then be implemented independently by file ownership, but generic contracts and shared UI must have one owner.

### 4. Capability-driven domain model

The common model should unify identity, evidence, and operations, not native configuration shapes.

```text
AgentAssetTarget
  stableId
  environmentId
  installationId
  agentKind
  category
  nativeKind             // skill, codexPlugin, claudePlugin, geminiExtension, ...
  nativeId

AgentAssetSource
  sourceId
  scope                  // user | workspace | local | system | managed | plugin
  opaqueLocatorId        // path stays backend-resolved
  displayPath?
  precedence
  writable
  trustState
  revision               // exact digest for writable files

AgentAssetState
  presence               // present | absent | invalid | unknown
  declaredState          // enabled | disabled | unspecified | unknown
  effectiveState         // enabled | disabled | shadowed | blocked | invalid | unknown
  shadowedByStableId?
  diagnostics[]

AgentAssetDetails        // tagged Rust enum, not a bag of nullable fields
  Skill { invocationPolicy?, description?, manifestPath? }
  Mcp { transport?, endpointKind?, toolPolicy?, health? }
  Plugin { nativePluginKind, version?, origin?, bundledComponents[] }
  StatusUi { mode: builtIn | command | disabled | unknown, segments[] }

AgentAssetAction
  action                 // enable | disable | inspect | open | reveal | copyPath
  available
  reason?
  mechanism              // structuredEdit | officialCli | ownedResource | readOnly
  confirmationRequired
  reloadEffect           // immediate | newSession | restartAgent | unknown
  trustEffect            // none | requiresExistingTrust | wouldChangeTrust
```

Important modeling rules:

- `Plugin` and `Extension` may share one UI family named “扩展”, but `nativeKind` preserves official terminology: Codex Plugin, Claude Code Plugin, Gemini CLI Extension, Grok Build Plugin.
- An MCP server is a logical record inside a source file or plugin, not the source file itself. The same native ID in user and project scopes produces separate declarations plus one computed effective result.
- A Skill path, a Skill logical identity, and an invocation policy are separate facts. Claude's `disable-model-invocation` is not equivalent to disabling the Skill.
- Status UI is a tagged mode. A built-in segment list, a command process, and a disabled value are not one generic script field.
- Hook records should reference the existing `AgentHookInspection`/actions by target; they should not be reparsed into a competing generic Hook mutation implementation.
- `AgentAssetCapability.mutation` is too coarse as a single category-level value (`src-tauri/src/models/agent_environment.rs:69-86`). Replace or supplement it with `actions[]` on each logical record and an adapter/category capability summary.
- The adapter, not the category, decides support. Generic orchestration calls `inspect -> actions -> plan -> apply -> verify` without matching on Agent kind.

Recommended adapter surface:

```text
AgentAssetAdapter
  discoverSources(environment, installation, workspace?)
  parseSource(sourceSnapshot) -> declarations
  resolveEffective(declarations, trustContext) -> records
  plan(action, stableId, expectedRevision) -> mutationPlan
  apply(mutationPlanToken) -> applyResult
  verify(stableId, desiredState) -> record
```

Each Agent registers zero or more category adapters in `AgentCliDefinition`. An unsupported category or action is represented explicitly; it never falls through to a default Agent implementation.

### 5. Read-only versus controlled actions

The first child release should preserve all existing read-only actions for every discovered asset. Writable actions then roll out per documented native semantic and local installed version.

| Asset | Codex CLI | Claude Code | Gemini CLI | Grok Build |
|---|---|---|---|---|
| Skills | Managed enable/disable through `[[skills.config]]` only after exact-path resolution; no move/delete | Read-only; `disable-model-invocation` is not a full disable | Managed enable/disable through official user/workspace Skill state | Managed enable/disable through `[skills].disabled`; `ignore` and compatibility-source switches remain advanced/read-only |
| MCP | Managed enable/disable of the exact `[mcp_servers.<id>]`; preserve transport/env/tool policy | Version-gated: inventory approval, rejection, and project-disabled state; only enable/disable when adapter proves the exact source and supported settings contract, otherwise read-only | Managed enable/disable through the official state/command for the exact user/workspace source | Managed enable/disable through `grok mcp enable/disable`, with explicit source/collision handling |
| Plugin/Extension | Managed enable/disable only for an installed native Plugin with a documented native ID/Space state; install/uninstall/update deferred | Managed enable/disable for installed Plugins through official semantics; install/uninstall/update and marketplace changes deferred | Managed `gemini extensions enable/disable <name> --scope ...`; install/uninstall/update deferred | Managed `grok plugin enable/disable`; trust grant, install/uninstall/update deferred |
| Status UI | Read-only first; `tui.status_line = null` cannot be generically reversed without retaining the prior segment value | Read-only first; command statusline is single-owner executable configuration, not a harmless toggle | Read-only first; footer settings are ordinary configuration fields rather than an independent installed asset | Conditional managed mode toggle only after an adapter proves that other statusline fields are preserved and restart/reload behavior is known; otherwise read-only |
| Hook | Existing Hook controller only | Existing Hook controller only | Existing Hook controller only | Existing Hook controller only |

Notes:

- Claude Code documentation now describes project MCP approval/rejection and `disabledMcpServers` behavior in recent releases. This must be version-gated; older installations and user/local/project sources cannot be assumed to share one toggle contract.
- Official CLI commands are preferred when they provide machine-readable output, exact scope targeting, idempotence, and bounded execution. Otherwise a structured parser/editor is safer than automating an interactive TUI.
- “Managed” above means a user-confirmed native state change. It does **not** mean BalanceHub owns the asset.
- Install, uninstall, update, trust grant, marketplace registration, and shared cross-Agent distribution are deliberately deferred because their provenance, network, execution, and rollback risks are materially larger than local enable/disable.

### 6. Ownership, revision, plan, apply, and verify

Reuse the Hook lifecycle shape, but separate **user-authorized mutation** from **BalanceHub ownership**.

#### Ownership rules

- Existing user/workspace assets remain `external`. Toggling one does not create ownership or give BalanceHub permission to remove it later.
- An ownership manifest is created only when BalanceHub creates a namespaced resource. This child does not create assets in its first release, so most operations need no ownership manifest.
- Removal is never inferred from a previous toggle. Later uninstall/delete support must use the official CLI's exact installed identity or a matching BalanceHub structural identity and fingerprint.
- Managed/system sources and trust stores are read-only. A user action does not relax that policy.

#### Revision rules

- File-backed plans include canonical opaque source ID, exact SHA-256 content revision, existence/type state, and intended structural node identity.
- Official-CLI plans include canonical installation ID, executable path identity, installed version, native asset ID/scope, and a digest of the adapter's current machine-readable inspection.
- If any precondition changes between plan and apply, return a typed stale-plan conflict and make no changes.
- Symlink target changes, file replacement, scope collision, trust-policy changes, and executable replacement invalidate the plan.

#### Plan/apply protocol

```text
inspect logical records
  -> request plan(stableId, action)
  -> backend re-resolves target and source
  -> return display plan + opaque plan token/digest
  -> user confirms; modal closes immediately
  -> backend locks only the target source/installation
  -> re-inspect and regenerate canonical plan
  -> compare digest and all revisions
  -> structured atomic edit OR exact official CLI invocation
  -> bounded timeout/cancellation
  -> re-inspect and verify effective state
  -> publish row/task result
```

The frontend must not post back editable file paths, commands, diffs, or change arrays as authority. A stateless option is to return a canonical plan digest and require apply to regenerate the same digest. This strengthens the current Hook approach, where apply already validates target/revision and regenerates changes (`src-tauri/src/services/agent_runtime/managed_hook/codex/apply.rs:17-40`).

For file edits:

- parse the native format and modify only the intended node;
- preserve unknown fields and file permissions;
- write a same-directory temporary file, sync, and atomically replace;
- never restore an old whole-file snapshot over newer user changes;
- re-read and verify the effective result.

For official CLI actions:

- command and arguments are created only by the backend adapter;
- target the exact installation/environment and explicit scope;
- use noninteractive and machine-readable modes where available;
- inherit the shared shell environment and `src-tauri/src/network/` proxy semantics when the command can use the network;
- apply a timeout, cancellation boundary, output-size limit, and credential scrubbing;
- no automatic retry for a state-changing command unless the official command is proven idempotent and the first attempt is known not to have applied.

The existing Hook UI's row-local generation/busy state and shared confirmation modal are reusable interaction patterns (`src/composables/useAgentHookConsole.ts:16-45`, `src/composables/useAgentHookConsole.ts:48-109`, `src/composables/useAgentHookConsole.ts:117-151`). Asset operations should use stable scalar target IDs, close the confirmation surface immediately, and continue through row/backend task state so one slow CLI never locks the page.

### 7. Recommended list-first UX

Do not put management behind “open Agent details, then open Assets, then choose a category.” Keep details available, but make normal controls visible in one asset console.

Recommended information architecture:

```text
应用设置 -> Agent
  segmented view: 环境 | 资产

资产
  workspace selector (only when project scope is relevant)
  one search field
  category tabs: Skills | MCP | 扩展 | Status UI
  source/state filters in a compact menu

  grouped list by Agent
    Agent icon + official name + installation/runtime scope
      asset name + native type badge
      source scope + effective state + trust/collision diagnostic
      direct switch when actions[] exposes enable/disable
      open / reveal / copy / details menu
```

Interaction rules:

- The row switch is rendered only when Rust returns a real enable/disable action. Unsupported assets do not get a disabled fake switch; they show “只读” or the exact reason.
- Plugin and Extension rows use the Agent's official noun in the badge, while the category tab can remain “扩展”.
- The search field covers Agent name, logical asset name, native ID, source path, scope, and diagnostics. Search results remain asset rows, not raw config fragments.
- Group headers show useful nonzero summaries such as `3 已启用 · 1 冲突`; do not display decorative zero counts.
- Row click does not mutate or navigate. Direct controls have tooltips/accessible labels; details use a dedicated button or drawer.
- The details drawer shows provenance, all source declarations, precedence/shadow chain, trust, native fields, diagnostics, and planned reload consequence. It is not required for a normal toggle.
- One shared plan modal shows target Agent, official asset type/name, scope, structural diff or exact official command, restart/reload effect, and conflict warnings.
- Applying one row disables only that row/source. Refresh and external file changes use request IDs/revisions and cannot overwrite newer state.
- Hook appears as an inventory category only for discoverability; its action opens or delegates to the current Hook controller rather than showing a second switch with separate state.

This extends the successful list-first Hook correction instead of reintroducing card drill-down. `AgentInstallationDetail` remains appropriate for installation evidence and deep diagnostics (`src/components/settings/agent-environment/AgentInstallationDetail.vue:108-181`).

### 8. Staged implementation order

#### Stage A: foundation and truthful inventory

1. Fix multi-installation identity and bind assets to `installation_id`.
2. Replace file/directory child records with adapter-parsed logical records while retaining source-file records for configuration browsing.
3. Implement exact source revisions, precedence, collision, declared/effective state, and trust evidence.
4. Add tagged asset details and per-record Rust action capabilities; all mutation actions remain unavailable initially.
5. Convert the existing asset UI to the list-first cross-Agent console and keep open/reveal/copy/details read-only.

Acceptance:

- ten MCP entries in one file produce ten logical records, not one file record;
- duplicate native IDs across scopes show the correct effective/shadow relationship;
- every logical record is tied to one environment, installation, source, and scope;
- unknown or unsupported semantics stay explicit and do not produce a switch;
- a fifth fixture Agent registers through adapters without generic Agent-name branches;
- malformed files, symlinks, over-limit directories, duplicate IDs, and unknown fields are isolated and diagnosed.

#### Stage B: shared mutation kernel

1. Generalize Hook safety primitives into resource-neutral revision/plan/apply utilities without changing Hook behavior.
2. Add canonical plan digest/token, target-scoped locks, structured changes, typed conflicts, atomic write, bounded official-command runner, and post-apply verification.
3. Add frontend row state and background-task integration using the existing Hook console pattern.

Acceptance:

- cancel, stale revision, source replacement, permission failure, timeout, and adapter error leave unrelated files byte-identical;
- one slow operation does not disable another Agent or the page;
- plan contents cannot be changed by the frontend to target another file, Agent, scope, or command;
- success is reported only after re-inspection confirms effective state;
- no secret appears in IPC, plan diff, command preview, logs, diagnostics, or tests.

#### Stage C: MCP controlled pilot

Implement Codex, Gemini, and Grok native enable/disable first. Add Claude only behind versioned source/approval semantics proven by fixtures.

Acceptance:

- user and workspace sources with identical IDs target the intended scope;
- project override, disabled, rejected, pending approval, plugin-provided, and managed-policy states are not conflated;
- enabling/disabling does not launch the MCP server merely to render state;
- health probing is explicit and bounded, not part of passive inventory refresh;
- no MCP add/remove or credential editing is exposed.

#### Stage D: Plugin/Extension control

Use official native IDs and enable/disable semantics for installed Codex Plugins, Claude Code Plugins, Gemini CLI Extensions, and Grok Build Plugins. Keep install/update/uninstall/trust actions unavailable.

Acceptance:

- native name, version, source, scope, bundled components, enabled state, and trust are shown;
- Gemini workspace/user disable targets the exact `--scope`;
- Grok enabled and trusted remain separate facts;
- enabling a plugin cannot silently grant trust or activate blocked executable components;
- Agent restart/new-session/reload consequences are shown before confirmation and verified afterward.

#### Stage E: Skill control

Add native enable/disable only for Codex, Gemini, and Grok. Keep Claude Skills read-only until Claude exposes a true independent disabled-state contract.

Acceptance:

- `disable-model-invocation` is displayed as invocation policy, never as disabled;
- same-name Skills across roots remain separate declarations with accurate invocation/effective state;
- disabling never moves, deletes, renames, copies, or symlinks a Skill directory;
- workspace actions require an explicit selected workspace and never fall back to user scope.

#### Stage F: Status UI

Keep all Status UI read-only through Stages A-E. Add a managed toggle only per Agent when the adapter can prove a reversible native mode without storing or overwriting an entire user configuration.

Acceptance:

- built-in segment/footer configuration and executable command statuslines render as different native modes;
- passive inspection never executes a command statusline;
- a toggle preserves unrelated fields and can restore the exact prior native state or is not offered;
- project policy that forbids command statuslines is represented as blocked/read-only.

### 9. Cross-platform and performance constraints

- Native macOS, Linux, and Windows need parser/path/atomic-write fixtures and CI. Platform compilation is necessary but does not replace manual native behavior checks for official CLI commands.
- Never invoke through an interactive shell solely to run an official Agent command. Resolve the exact executable and pass an argument vector to avoid quoting, reserved-variable, and shell-profile drift.
- Windows atomic replacement and file-lock behavior differs from Unix. Use same-directory temporary files, preserve ACL-compatible behavior where possible, and return a typed busy/conflict result when another process holds the file.
- Paths and IDs must remain Unicode-safe and case-policy-aware. Do not lowercase native IDs unless the Agent contract says they are case-insensitive.
- WSL is explicitly excluded. A Windows host path and a WSL guest path are different environments; never execute an ELF through UNC and call it native support.
- Inventory remains bounded: no recursive scan of dependency caches, marketplaces, plugin repositories, or arbitrary Skill subtrees. Parse declared manifests and immediate roots with per-adapter limits.
- Watch only selected allow-listed source files or immediate directories while a real subscriber exists. Watch events invalidate snapshots; adapters re-read and recompute state. No watcher writes or auto-repair.
- Passive inventory must not start MCP servers, execute plugins, run Hooks/Status UI, start stopped WSL distributions, or make marketplace/network requests.

### 10. No-config-takeover contract

The following are product invariants, not implementation preferences:

1. BalanceHub does not create a central Skill/MCP/plugin SSOT in the first management task.
2. It does not migrate, copy, relocate, or symlink user assets.
3. It does not auto-enable, auto-disable, auto-repair, or reconcile external configuration.
4. External edits always invalidate the current plan; BalanceHub never wins a write race by restoring an older file.
5. Existing assets remain externally owned even after a BalanceHub-triggered native toggle.
6. Managed/system/trust-policy sources remain read-only and no operation retries with elevation.
7. An unavailable safe native operation is omitted or explained; it is never simulated through directory deletion or broad file replacement.
8. Cross-Agent distribution, import, install, update, and synchronization require a later explicit ownership/provenance design and separate user approval.

### 11. External references

The capability conclusions use the official sources already pinned by the earlier audit and spot-checked on 2026-09-04:

- OpenAI Codex configuration, Skills, Plugins, and Hooks: `https://developers.openai.com/codex/config-reference.md`, `https://developers.openai.com/codex/skills.md`, `https://developers.openai.com/codex/plugins.md`, `https://developers.openai.com/codex/hooks.md`.
- Claude Code settings, Skills, Plugins, MCP, Hooks, and Statusline: `https://code.claude.com/docs/en/settings.md`, `https://code.claude.com/docs/en/skills.md`, `https://code.claude.com/docs/en/discover-plugins.md`, `https://code.claude.com/docs/en/plugins-reference.md`, `https://code.claude.com/docs/en/mcp.md`, `https://code.claude.com/docs/en/hooks.md`, `https://code.claude.com/docs/en/statusline.md`.
- Gemini CLI at commit `87a9c71d57a4ec56c00f3ff628970fea8291d812`: `docs/cli/skills.md`, `docs/extensions/reference.md`, `docs/tools/mcp-server.md`, `docs/hooks/`, and `docs/cli/settings.md` in `https://github.com/google-gemini/gemini-cli`.
- Grok Build at commit `72a61251fcffb464bcc687aeb5a998e5a98ec0c9`: `crates/codegen/xai-grok-pager/docs/user-guide/07-mcp-servers.md`, `08-skills.md`, `09-plugins.md`, `10-hooks.md`, and `25-status-line.md` in `https://github.com/xai-org/grok-build`.
- CC Switch comparison at commit `bc4ed66d3abe587b16338ef19853c7ee76b88ed7`: `https://github.com/farion1231/cc-switch`.

Observed official semantics relevant to staging:

- Codex documents `[[skills.config]] ... enabled = false`; its Plugins and configuration capabilities are distinct native surfaces.
- Claude Code documents scoped Plugins and recent project MCP approval/disabled states, but a Claude Skill's `disable-model-invocation` controls automatic model invocation rather than full availability.
- Gemini CLI documents `/skills enable|disable`, `gemini extensions enable|disable --scope user|workspace`, and separate MCP/settings behavior.
- Grok Build documents `[skills].disabled`, `grok plugin enable|disable`, `grok mcp enable|disable`, source precedence, plugin trust, and distinct built-in/command/disabled statusline modes.

These contracts evolve quickly. Adapter support must be version-gated and backed by captured fixtures; documentation observed on 2026-09-04 is not a promise for older installed binaries.

### 12. Related specs

- `.trellis/spec/guides/agent-routing.md:5-39`: architecture and cross-layer decisions use the research/review path before deterministic implementation.
- `.trellis/spec/guides/cross-layer-thinking-guide.md:19-50`: define source, transformation, validation, persistence, IPC, and UI contracts before implementation.
- `.trellis/spec/guides/cross-layer-thinking-guide.md:74-101`: decode native payloads once and expose typed projections rather than duplicating parsing in consumers.
- `.trellis/spec/guides/code-reuse-thinking-guide.md:18-39`: inspect and extend existing registries and safe primitives before creating a second implementation.
- `.trellis/spec/guides/code-reuse-thinking-guide.md:86-97`: abstract only the genuinely shared lifecycle; retain Agent-specific semantics in adapters.
- `.trellis/spec/frontend/state-management.md:19-53`: Rust owns persisted capability/effective state and frontend requests reject stale responses.
- `.trellis/spec/frontend/component-guidelines.md:19-41`: bounded typed components and stable scalar IDs.
- `.trellis/spec/frontend/component-guidelines.md:55-62`: direct controls need accessible labels and async work must not lock a modal or page.
- Repository `AGENTS.md`: Rust is the IPC/capability source of truth; no config takeover, duplicate implementation, dead code, arbitrary paths, global async locks, or unverified three-platform claims.

## Caveats / Not Found

- The current source does not yet contain logical parsers for every category or official-command adapters for asset mutation. The action matrix is a design target, not a claim that these actions already work.
- The current native inventory does not enumerate all installations of one Agent, so no write action should be keyed only by Agent kind.
- Claude MCP semantics changed across recent versions; exact minimum-version fixtures and precedence rules must be pinned during the child task before exposing a switch.
- Codex Plugin behavior spans CLI/TUI/desktop documentation and may vary by distribution. Only a locally inspectable native ID and supported state transition should become writable.
- Gemini MCP enable/disable persistence must be verified against the exact installed CLI version and source scope; OAuth `enabled` inside a server definition is not the same as enabling the MCP server itself.
- Status UI should remain read-only unless reversibility is demonstrated. “Can set disabled” is insufficient if BalanceHub cannot restore the exact prior native state without taking ownership.
- No real Linux or Windows native mutation was performed during this research. CI can validate compilation and fixtures, while behavior claims require native smoke tests.
- WSL, remote SSH/container Agents, package installation/update, and cross-Agent synchronization remain separate tasks.
