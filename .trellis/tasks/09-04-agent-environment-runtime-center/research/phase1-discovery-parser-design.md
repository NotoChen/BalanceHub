# Research: Phase 1 multi-install discovery, asset parsing, and deep-path adoption

- Query: Design the remaining Phase 1 contracts for bounded multi-installation discovery, structured Agent asset state, and recovery of the explicit GUI PATH deep-scan/adopt workflow without hard-coding the current four Agents or reading/writing real Agent configuration.
- Scope: mixed (repository source plus official public documentation)
- Date: 2026-09-04

## Findings

### Executive decision

Phase 1 should not extend the current path-template inventory in place. It needs three explicit boundaries:

1. `discovery` enumerates validated executable installations and keeps **candidate origin**, **installation owner**, and **effective selection** as separate facts.
2. each registered Agent owns versioned parsers and a resolution policy that turn bounded source snapshots into logical assets; the shared inventory layer only orchestrates and serializes those results.
3. deep scanning remains an explicit, read-only action. It returns candidates; adopting one candidate is a second explicit UI action that updates the existing settings draft only if the original preferred-path snapshot is still current.

This preserves Rust as the source of truth, keeps the existing user-selected path, avoids restoring `SettingsCliManager.vue`, and makes a fifth Agent a registry addition rather than another shared `match AgentCliKind` branch.

### Files found

- `src-tauri/src/services/agent_cli.rs`: registry and generic dispatch; a single catalog generates definitions, but `find` and `probe_all` still collapse discovery to one executable (`145-200`).
- `src-tauri/src/services/agent_cli/discovery.rs`: builds preferred/env/home/global/PATH/shell candidates, returns fixed-priority candidates early, otherwise chooses the highest parsed version (`43-175`).
- `src-tauri/src/services/agent_cli/discovery/paths.rs`: collects process PATH, login-shell PATH, common manager locations, NVM/FNM versions, and Windows npm wrappers (`22-55`, `87-159`, `212-317`).
- `src-tauri/src/services/agent_cli/contracts.rs`: `EnvironmentAdapter` currently contains only a path declaration callback and one npm package name (`7-47`).
- `src-tauri/src/services/agent_cli/environment/inventory.rs`: runs `agent_cli::find(..., false)` once per Agent and hashes only environment plus Agent kind for the installation ID (`85-164`). It expands directory children but deliberately leaves declared/effective/trust as unknown (`276-409`).
- `src-tauri/src/services/agent_cli/environment/versioning.rs`: checks the single npm package declared by the Agent adapter and caches solely by package string (`51-109`, `111-177`, `215-263`).
- `src-tauri/src/models/agent_environment.rs`: current serialized contract has one installation per Agent and one overloaded asset-state enum (`60-67`, `99-168`, `179-218`).
- `src-tauri/src/services/agent_cli/{codex,claude,gemini,grok}/mod.rs`: each Agent registers labels, executable candidates, one npm package, and path templates; these are the correct ownership locations for installation/schema metadata.
- `src/utils/cli-environment.ts`: existing safe adoption helper compares the current path with the scan-start snapshot before mutating the settings draft (`134-170`).
- `src/composables/useSettingsController.ts`: the former manual deep probe captures the draft, calls `probeCliTools(true)`, and always releases busy state (`147-165`).
- `src/components/settings/SettingsAgentEnvironmentCenter.vue` and `src/components/settings/agent-environment/AgentInstallationGrid.vue`: the new environment center only refreshes ordinary inventory and has no deep scan/adopt action.
- `src/stores/agent-environment.ts`: version facts already merge by installation ID, so multi-installation identity can remain the join key (`41-65`); request IDs already prevent late store writes (`101-160`).
- `.trellis/tasks/09-04-agent-environment-runtime-center/research/phase1-backend-review-gaps.md`: records the multi-install and structured-parser gaps and their dependency order (`10-87`).
- `.trellis/tasks/09-04-agent-environment-runtime-center/research/phase1-frontend-review-gaps.md`: records the deleted deep-scan entry point and the required safe-adoption semantics (`3-30`).

### 1. Multi-installation discovery contract

#### 1.1 Do not overload “discovery source”

The current `AgentDiscoverySource::{Configured, Automatic}` conflates three different questions:

- **How was this entry point found?** Preferred setting, BalanceHub/Agent environment variable, process PATH, login-shell PATH, known user path, known system path, or package-manager inventory.
- **Who owns the installed bytes?** Native installer, npm, pnpm, yarn, bun, Homebrew, MacPorts, WinGet, apt/dnf/apk, Volta, NVM/FNM, asdf/mise, standalone binary, or unknown.
- **Which entry point will BalanceHub use?** User-preferred, automatically selected fallback, alternate, or unavailable preferred path.

These must be represented independently. A configured path may point to a Homebrew symlink, and a login-shell candidate may resolve to an npm installation; neither fact should erase the other.

Recommended Rust model (names may be adjusted, semantics should not):

```rust
struct AgentInstallation {
    id: AgentInstallationId,
    environment_id: AgentEnvironmentId,
    agent_kind: AgentCliKind,
    owner: AgentInstallationOwner,
    package: Option<AgentPackageIdentity>,
    channel: AgentInstallationChannelFact,
    entry_points: Vec<AgentExecutableEntryPoint>,
    installed_version: Option<String>,
    installed_version_source: AgentVersionEvidence,
    selection: AgentInstallationSelection,
    latest: AgentLatestVersionFact,
    diagnostics: Vec<AgentDiagnostic>,
}

struct AgentExecutableEntryPoint {
    path: String,                 // the stable/user-facing launcher path
    resolved_path: Option<String>,
    origins: Vec<AgentCandidateOrigin>,
    validation: AgentExecutableValidation,
    preferred_path_match: bool,
}

enum AgentInstallationSelection {
    Preferred,
    AutomaticDefault,
    Alternate,
    PreferredUnavailable,
}

struct AgentInstallationOwner {
    kind: AgentInstallationOwnerKind,
    confidence: AgentEvidenceConfidence, // confirmed, inferred, unknown
    root: Option<String>,
    evidence: Vec<AgentEvidenceRef>,
}
```

`AgentCandidateOrigin` and `AgentInstallationOwnerKind` may be finite enums because they describe shared platform mechanisms, not Agent kinds. Agent-specific package/formula/receipt identities remain registry data.

#### 1.2 Dynamic registry boundary

Replace `EnvironmentAdapter { discover, package_name }` with a capability bundle owned by the Agent definition:

```rust
struct EnvironmentAdapter {
    installation: InstallationDiscoveryAdapter,
    assets: AssetInventoryAdapter,
}

struct InstallationDiscoveryAdapter {
    specs: &'static [AgentInstallSpec],
    validate_executable: AgentExecutableValidator,
}

enum AgentInstallSpec {
    NodePackage { package: &'static str, binary: &'static str },
    Homebrew { artifact: &'static str, kind: BrewArtifactKind },
    Native { receipt: NativeReceiptSpec },
    WinGet { package_id: &'static str },
    LinuxPackage { package_ids: &'static [LinuxPackageIdentity] },
    Standalone { candidate_provider: AgentCandidateProvider },
}
```

The shared discovery engine knows how npm-family layouts, Homebrew, WinGet, native receipts, PATHs, and version managers work. An Agent definition supplies declarative identities and its executable validator. Adding an Agent supplies new registry data or an Agent-local receipt parser; it does not edit the shared orchestration.

The current one-package `package_name()` contract cannot survive native/Homebrew/WinGet installs. Latest-version lookup should instead use `AgentVersionQuery` descriptors attached to the confirmed owner/channel, for example `npm:@openai/codex:latest` or a Homebrew cask identity. If there is no documented source appropriate to that owner, latest remains unknown rather than silently comparing it to npm.

#### 1.3 Candidate enumeration and deduplication

Discovery should be a two-stage pipeline:

```text
candidate providers -> lexical candidate set -> bounded executable validation
                    -> owner resolvers -> installation grouping -> selection reducer
```

Candidate providers emit all bounded candidates rather than returning the first valid path. Each candidate keeps every origin that found it. Validation then:

1. normalizes the lexical path for the platform;
2. resolves symlinks/reparse targets with a bounded chain and cycle detection;
3. validates file type and Agent-specific `--version` output under a timeout;
4. reads only bounded, known wrapper/manifest/receipt files;
5. assigns owner evidence;
6. groups entry points that are proven to represent the same installation.

Owner proof levels:

- `confirmed`: package manifest name plus matching `bin` target, a documented native receipt, or a package-manager receipt/package ID agrees with the Agent registry.
- `inferred`: path layout is characteristic of a manager but no stable receipt/manifest proves ownership.
- `unknown`: only executable validation succeeded.

A path substring alone must never produce `confirmed` ownership. Windows `.cmd`/PowerShell wrappers may be read only when bounded and matched by a strict parser; wrapper content is never executed as part of ownership detection.

#### 1.4 Stable ID

Installation identity must exclude installed version, discovery order, current PATH order, and preferred-selection state. Recommended v1 material:

```text
environment_id
agent_kind.key()
confirmed_or_inferred_owner_kind
normalized_owner_root_or_resolved_executable
package_identity_or_empty
owner_channel_identity_or_empty
```

Encode the length-prefixed fields with SHA-256 and a versioned prefix such as `installation:v1:<first-16-bytes-hex>`, using the repository's existing length-prefix hashing pattern (`environment/inventory.rs:417-430`).

- For npm under separate NVM/FNM Node installations, the package root includes the Node-version installation root, so both real installations remain distinct.
- For a native Claude launcher that points into a version directory, use the documented native installation root/receipt as owner root, not the current versioned target. An upgrade then keeps the ID.
- For Homebrew stable/latest casks, the cask identity participates in the ID; the changing Cellar version does not.
- When ownership is unknown, fall back to the canonical executable path.
- Windows hash normalization removes the `\\?\` prefix, normalizes separators and drive letter, and case-folds for identity; the original path remains available for display and launch.

Aliases/symlinks that resolve to one proven owner become `entry_points` of one installation. The user-entered path is retained verbatim (after surrounding quote/whitespace cleanup) as an entry point even when another launcher is recommended.

#### 1.5 Preferred path semantics

`AppSettings.agent_cli_paths` remains the only persisted preference. Discovery does not overwrite it.

- A valid configured path marks its installation `Preferred`.
- A missing/invalid configured path is represented as a diagnostic plus `PreferredUnavailable`; automatic installations are still returned and one may be recommended, but none is silently persisted.
- Automatic selection is deterministic and separate from enumeration: first a stable manager launcher, then explicit environment paths, then process PATH, then known locations, then deep-shell candidates; version is only a tie-breaker within equivalent source quality.
- Launch code continues to use the effective preferred entry point, not an arbitrary canonical target. This preserves manager-controlled launchers and future upgrades.

#### 1.6 “All installations” product boundary

BalanceHub can promise “all validated installations found in bounded supported sources,” not a full-disk proof. Full-disk crawling is too expensive and still cannot prove package ownership. The inventory response should include scan coverage and diagnostics so the UI can say which sources were checked.

### 2. Structured asset parsing and resolution

#### 2.1 Physical sources and logical assets are different entities

`declaration_paths` currently turns each immediate directory child into an asset, while shared JSON/TOML files are repeated once per category. Replace this with:

```text
AssetSourceSpec -> bounded SourceSnapshot -> Agent-owned Parser
                -> ParsedAssetNode -> Agent-owned ResolutionPolicy
                -> AgentAssetRecord IPC projection
```

`AgentAssetSource` describes a physical file/directory and its revision. `AgentAssetRecord` describes a logical Config/Skill/Plugin/Extension/MCP/Hook/Status UI item extracted from that source. A skill directory becomes a logical skill only after its required manifest is parsed; a settings file may yield many logical assets without exposing secret values.

Because two installed versions may interpret the same file differently, each logical record should carry `installation_id`. The physical `source_id` can still be shared. This also fixes the frontend's current Agent-kind-only filtering: installation detail must filter by installation ID, not show the same Agent's assets indiscriminately.

#### 2.2 Orthogonal state contract

The current `AgentAssetState` cannot accurately represent the required facts. Use orthogonal fields:

```rust
struct AgentAssetRecord {
    stable_id: AgentAssetId,
    installation_id: AgentInstallationId,
    source_id: AgentAssetSourceId,
    category: AgentAssetCategory,
    logical_identity: String,
    label: String,
    presence: AgentAssetPresence,
    declared: AgentDeclaredState,
    effective: AgentEffectiveState,
    trust: AgentTrustState,
    shadowed_by: Vec<AgentAssetId>,
    shadows: Vec<AgentAssetId>,
    conflicts_with: Vec<AgentAssetId>,
    evidence: Vec<AgentEvidenceRef>,
    diagnostics: Vec<AgentDiagnostic>,
}
```

Recommended state meanings:

- `presence`: present, missing, unreadable, blocked symlink, or unknown.
- `declared`: declared-enabled, declared-disabled, declared-without-toggle, invalid, unsupported-schema, or unknown.
- `effective`: effective-in-static-config, inactive-disabled, shadowed, blocked-untrusted, conflict, invalid, or unknown.
- `trust`: trusted, untrusted, review-required, not-applicable, or unknown.

“Effective” must be documented in the UI as **static configuration resolution**, not process health. MCP startup success, Hook execution health, or Status UI rendering requires runtime evidence and belongs to later health/runtime projections.

Relations are explicit IDs rather than only prose. A collision is not automatically a conflict: additive/merge schemas may make both declarations effective; replacement schemas may shadow one; only an officially defined incompatibility or ambiguous duplicate becomes `conflict`.

#### 2.3 Adapter-owned parser and policy

Extend the Agent environment adapter with static registrations:

```rust
struct AssetInventoryAdapter {
    source_specs: &'static [AgentAssetSourceSpec],
    parsers: &'static [AgentAssetParserRegistration],
    resolve: AgentAssetResolutionPolicy,
}

struct AgentAssetParserRegistration {
    category: AgentAssetCategory,
    schema_family: &'static str,
    version_range: AgentVersionRange,
    parse: AgentAssetParser,
}
```

Parser input is a bounded byte snapshot plus installation/environment/workspace context. Parser output is an internal `ParsedAssetNode`; it never returns raw credential-bearing values. Shared code validates size, symlink boundaries, revision, duplicate stable IDs, and serialization, but it never branches on `AgentCliKind`.

Resolution cannot be a single numeric precedence sorter. The Agent-owned policy must declare or implement, per category/schema version:

- replace-by-logical-key;
- merge-by-logical-key/list;
- additive-all-sources;
- alias shadowing within a tier;
- trust gating;
- managed-policy blocking.

Unknown JSON/TOML fields are preserved as ignored facts for forward compatibility; unknown schema or semantics yields `unsupported-schema`/`unknown`, never guessed enabled/effective state. Parse errors affect only that source and do not erase valid lower-scope records.

#### 2.4 What static facts can and cannot prove

| Category | Can prove from documented bounded sources | Must remain unknown without additional official evidence |
|---|---|---|
| Config | file presence, parse validity, declared keys, documented source tier, key-level winner/merge where every controlling tier is observable | session flags/env overrides, server-delivered/managed policy not locally observable, runtime hot-reload completion |
| Skill | valid manifest identity, explicit enabled override, documented precedence/shadowing after all relevant roots are scanned | whether the model will select/use it, trust when the official trust store format is unavailable |
| Plugin/Extension | installed manifest identity, explicit enable/disable record, bundled capability declarations | successful activation, dynamic/command-generated plugin contents unless an official read-only introspection contract is used |
| MCP | logical server ID, transport kind, explicit enabled flag, static winner/merge; secret fields can be recorded only as “present” | authentication success, connection/startup health, policy approval not exposed by a documented store |
| Hook | event/matcher identity, explicit global/individual enablement, static source merge/precedence | execution health before a real event, exact trust when hash storage is undocumented, commands' behavior/output |
| Status UI | explicit configured/null/disabled declaration and static key winner | whether the current terminal/session actually rendered it |

Trust is independent of enablement. Absence of a trust record never means trusted or untrusted. If a format is described only as an internal implementation detail, pinning a source-code fixture may support a specific CLI version, but unknown versions must degrade to `unknown`.

#### 2.5 Agent-specific fact matrix

**Codex CLI**

- Official config reference documents `skills.config[].path/enabled`, `mcp_servers.<id>.enabled`, `features.hooks`, inline `[hooks]`, and `tui.status_line`.
- Official Hook docs document user/project `hooks.json` and inline config, additive loading, project trust gating, exact-hook-hash review, plugin-bundled hooks, and managed-hook policy.
- Therefore Config/MCP/Hook/Status UI declarations can be parsed from documented keys. Static effective Hook state still remains unknown if the exact trust record or managed policy is not available. Plugin installation/activation needs its documented plugin manifest/state, not just `.codex/plugins` directory presence.

**Claude Code**

- Official settings documentation gives user, shared-project, project-local, and managed tiers, and says local overrides project, project overrides user, while many list settings merge. It also documents workspace trust gates and a published JSON schema.
- Parse `settings.json`/`settings.local.json` using the published schema and category-specific keys. Do not apply one generic “smaller precedence number wins” algorithm to hooks/permissions/plugin lists.
- `~/.claude.json` is documented as holding sign-in session, MCP, per-project state, and trust decisions, but it is sensitive and not presented as a stable public schema. Unless a pinned official schema for the installed version is available, records derived from it stay metadata-only/unknown.

**Gemini CLI**

- The official open-source docs document user/workspace/system settings, workspace-over-user precedence, `hooksConfig.enabled`, Hook source order, project-Hook fingerprint trust, extension management, skill precedence, and `~/.gemini/trustedFolders.json`.
- JSON settings, skill manifests, extension manifests, and documented keys can be parsed with fixtures pinned to the installed version. If the trust-file or Hook fingerprint on-disk schema is not versioned/documented, its presence alone does not prove trust.
- An untrusted workspace ignores workspace settings, so a workspace asset cannot be marked effective unless the official trust decision is safely decoded.

**Grok Build**

- The public npm metadata for `@xai-official/grok` currently proves the package name, executable and version, but exposes no repository/homepage or stable Config/Skill/Plugin/MCP/Hook/Status UI schema.
- Existing `.grok/*` templates may remain path-level inventory, but their declared/effective/trust state must stay unknown until an official schema or a version-pinned first-party source contract is recorded. Do not manufacture parity with the other three Agents.

### 3. Deep GUI PATH scan and adopt interaction

#### 3.1 Command semantics

Ordinary inventory and deep discovery need different commands or an explicit mode:

```rust
enum AgentDiscoveryMode { Normal, DeepExplicit }

struct AgentDiscoveryResult {
    scan_id: String,
    mode: AgentDiscoveryMode,
    started_from_settings_revision: String,
    agents: Vec<AgentDiscoveryAgentResult>,
    coverage: Vec<AgentScanCoverage>,
    diagnostics: Vec<AgentDiagnostic>,
}
```

Both commands are read-only. `DeepExplicit` adds shell/package-manager sources but never writes settings. The response returns stable installation IDs, entry points, owner evidence, recommendation reason, and the preferred path observed at scan start.

#### 3.2 Platform behavior

**macOS**

- Normal scan uses the GUI process environment, configured paths, known manager/native locations, and executable validation.
- Deep scan obtains login-shell PATH once per whole scan, then reuses that snapshot for every registered Agent. Preserve the existing `-lc` then `-ic` fallback behavior because interactive zsh config is where many GUI-missing aliases/PATH entries appear.
- Starting a login/interactive shell executes the user's shell startup files. It must occur only after explicit user action, under timeout/output limits, with visible diagnostics; never at App startup or periodic refresh.
- `/opt/homebrew` and `/usr/local` are both scanned. Stable launcher paths are preferred over versioned Cellar/Node targets.

**Linux**

- Normal scan covers process PATH, user bins, supported version-manager roots and `/usr/local/bin`, `/usr/bin`, `/bin`.
- Deep scan obtains login-shell PATH once and may inspect documented package-manager locations/receipts. It does not request root and does not traverse the filesystem.
- Native Linux package sources may be confirmed only by a bounded documented receipt/package identity; otherwise ownership is inferred/unknown.

**native Windows**

- Scan PATH with PATHEXT semantics, `%APPDATA%\\npm`, `%LOCALAPPDATA%`, supported manager/WinGet link locations, and `where.exe` results. Validate `.exe`, `.cmd`, and supported PowerShell launchers separately.
- Windows does not have the same GUI/login-shell split. Do not load arbitrary PowerShell profiles merely to discover aliases/functions: those are not durable executable paths BalanceHub can safely persist and launch. A deep scan may use bounded `where.exe`/`Get-Command -All -CommandType Application` with `-NoProfile` plus documented manager locations.
- Normalize identity case-insensitively but preserve display spelling. WSL remains outside this task.

No platform branch may invoke an Agent Hook, statusline, plugin, MCP server, or arbitrary discovered script.

#### 3.3 UI flow

Add the action inside the existing Agent environment overview/detail; do not restore the deleted settings component or a second probe store.

```text
ordinary refresh
  -> read-only inventory

“深度扫描”
  -> read-only DeepExplicit discovery
  -> show candidates grouped by Agent and installation owner
  -> preselect recommendation only
  -> user clicks “采用此路径”
  -> compare current settings draft with scan-start preferred-path snapshot
  -> if unchanged, update only that Agent's draft path
  -> persist through the existing unified settings save flow
```

The candidate list must show the launcher path, resolved owner/source, installed version, channel confidence, and why it was found. “采用” writes the stable entry point, not a versioned resolved target. If the draft changed during scanning, do nothing and ask the user to rescan/choose again. Cancel, timeout, no result, stale result, and component unmount leave both the draft and persisted settings unchanged.

The Rust result determines candidate validity and recommendation. TypeScript only handles selection, the snapshot conflict check, and the existing settings-save orchestration.

### 4. Performance and safety limits

The exact constants should be centralized and fixture-tested. Recommended upper bounds for Phase 1:

- normal scan: 3 seconds overall target, 6 seconds hard deadline;
- explicit deep scan: 10 seconds target, 15 seconds hard deadline;
- executable candidates: at most 64 lexical paths per Agent, at most 8 validated installations per Agent;
- command concurrency: at most 4 child processes globally, not one unbounded thread per Agent/candidate;
- `--version`: 2 seconds normal, 5 seconds deep, repository system-command output cap;
- login/interactive shell snapshots: at most one successful PATH snapshot per scan, two attempts (`-lc`, then `-ic`), 5 seconds each within the overall deadline;
- wrapper text: 64 KiB; package manifest/receipt/config snapshot: 256 KiB each;
- source files: at most 64 per Agent/installation/category; directory entries: 256 per declared root; traversal depth: 3 for assets, 8 ancestors for ownership proof;
- symlink/reparse chain: 16 hops; cycles are blocked;
- parser total: 500 milliseconds per Agent under fixture load and an 8 MiB aggregate read budget per inventory request;
- latest-version requests: coalesce by the full version-query key, retain the existing six-hour success TTL and bounded failure backoff, and never run during ordinary inventory unless explicitly requested.

When a bound is hit, return partial results plus a typed `truncated` diagnostic. Do not silently describe partial coverage as complete.

### 5. File-level implementation order and dependencies

1. **Serialized models first** — revise `src-tauri/src/models/agent_environment.rs` for installation owner/origin/selection, version evidence, orthogonal asset state, relations, `installation_id`, coverage and diagnostics. Mirror receiving types in `src/stores/provider-types.ts`. Dependency: none.
2. **Registry contracts** — extend `src-tauri/src/services/agent_cli/contracts.rs` and `AgentCliDefinition` in `src-tauri/src/services/agent_cli.rs` with installation specs, asset source/parser registrations and a resolution callback. Remove the one-npm-package assumption only after callers migrate. Dependency: step 1.
3. **Candidate engine** — split candidate enumeration from validation/selection in `src-tauri/src/services/agent_cli/discovery.rs`; keep platform path gathering in `discovery/paths.rs`, add owner resolvers under `discovery/owners/`, and inject filesystem/command runners for fixture tests. Dependency: step 2.
4. **Inventory join** — update `src-tauri/src/services/agent_cli/environment/inventory.rs` to consume all installations, group aliases by stable owner identity, create explicit missing-Agent summaries rather than fake one-per-Agent installations, and bind logical assets to installation IDs. Dependency: step 3.
5. **Version sources** — refactor `environment/versioning.rs` from `package_name` keys to owner/channel-specific `AgentVersionQuery` keys and preserve unknown where no correct source exists. Dependency: stable installation IDs from step 4.
6. **Parsing framework** — add bounded snapshot/parsing/resolution modules under `src-tauri/src/services/agent_cli/environment/` and make path access resolve opaque logical asset/source IDs rather than trusting frontend paths. Dependency: steps 1-2 and installation context from step 4.
7. **Agent parsers one at a time** — add Agent-local `environment.rs` or `environment/` modules under `codex/`, `claude/`, `gemini/`, and `grok/`. Start with Codex; expand only after the shared resolver and unknown-state rules pass. Grok remains path-only until official evidence exists. Dependency: step 6.
8. **IPC commands** — extend `src-tauri/src/commands/cli.rs`, `services/cli_runtime/app.rs`, and command registration with read-only normal/deep discovery contracts. Do not add a write-to-settings discovery command. Dependency: steps 3-7.
9. **Frontend store/composable** — extend `src/api/app.ts`, `src/stores/agent-environment.ts`, and `src/composables/useAgentEnvironmentCenter.ts` with per-scan request IDs, candidate selection and cancellation. Reuse/move the snapshot comparison from `src/utils/cli-environment.ts`; do not duplicate it. Dependency: step 8.
10. **Environment-center UI** — update the existing `SettingsAgentEnvironmentCenter.vue` and focused `agent-environment/` children for multiple installations, coverage, structured states, deep scan and explicit adoption. Filter assets by installation ID. No old component resurrection. Dependency: step 9.
11. **Remove duplicate public probe path only when unused** — migrate remaining `probeCliTools` consumers such as onboarding/workspace picker to the new projection where semantics match; then delete obsolete IPC/store/types. If their semantics differ, keep the narrow old capability until a planned migration, not dead UI state. Dependency: verified steps 8-10.

Managed Hook mutation later must consume these installation/source/logical identities. It must not create a parallel path or parser model.

### 6. Acceptance fixtures

All fixtures use temporary synthetic homes/workspaces and fake command runners. They must not read developer Agent configuration.

#### Discovery fixtures

- same Agent installed once by npm and once by a native/Homebrew source returns two stable IDs;
- PATH symlink, configured launcher and package-manager launcher proven to one owner collapse to one installation with three entry points and all origins;
- two NVM/FNM Node roots containing the same package remain two installations;
- a native launcher target changes version while owner receipt/root remains constant, proving ID stability;
- missing configured path plus valid automatic candidates keeps `PreferredUnavailable`, recommends a candidate, and does not modify settings;
- stable and prerelease artifacts coexist; update channel is unknown unless a receipt/package identity proves it;
- inferred path-only owner is not serialized as confirmed;
- duplicate/case-variant Windows paths dedupe, `.cmd`/`.exe` validation remains distinct, and WSL paths are not admitted;
- symlink/reparse cycle, oversized wrapper, command timeout and output overflow return bounded diagnostics;
- registry fixture adds a fifth fake Agent and discovers it without editing shared orchestration.

#### Parser/resolver fixtures

- each supported Agent/category fixture covers declared enabled, declared disabled, merge/additive, shadowed, explicit conflict, blocked-untrusted, malformed source, unknown schema/version and unknown fields;
- missing trust record yields `unknown`, never trusted/untrusted by absence;
- same logical key in a replace policy creates `shadowed_by`; same key in an additive policy keeps both effective and creates no false conflict;
- managed/server source not observable forces affected effective state to unknown when it could change the result;
- sensitive MCP/Hook/config fixtures assert API keys, tokens, cookies, passwords, headers, command environment values and raw source bytes never cross serde IPC;
- parser failure in one source preserves valid records from other sources;
- selected installation version chooses the matching parser; unsupported versions degrade without falling back to a guessed parser;
- read-only inventory and preview leave every fixture byte and permission mode unchanged;
- fifth fake Agent parser registration reaches the common IPC projection without an Agent-kind switch.

#### Deep scan/adopt fixtures

- normal GUI environment misses an executable; one explicit shell snapshot finds it for deep scan;
- shell snapshot runs once for several Agents, not once per Agent/candidate;
- deep scan returns two candidates and adoption persists only the explicitly chosen stable launcher path;
- settings path changed while scan is pending causes adoption conflict and zero mutation;
- cancellation, timeout, late response and component unmount release busy state and do not mutate draft/persisted settings;
- macOS/Linux/Windows coverage sources are serialized distinctly;
- frontend renders multiple installations for one Agent and asset detail filters by installation ID;
- a fifth fake Agent appears through descriptor/installation data without new frontend conditionals.

### 7. Minimal shippable loop

The smallest useful closed loop is:

1. land multi-install models plus bounded discovery for all registered Agents, with owner/source allowed to be unknown;
2. restore explicit deep scan and safe “adopt this launcher” inside the new environment center;
3. implement Codex Config/MCP/Hook/Status UI structured parsing from official versioned fixtures, with Skills/Plugins remaining explicit unknown where proof is incomplete;
4. prove dynamic behavior with a fifth fake Agent fixture and prove zero writes until explicit adoption;
5. only then expand the same parser contract to Claude Code and Gemini CLI; keep Grok path-level until first-party schema evidence exists.

This loop gives the user visible value (all bounded installations, recoverable GUI PATH, a real structured Agent view) while validating the extensibility contract before multiplying parser work.

### External references

Accessed 2026-09-04:

- OpenAI Codex config reference: https://developers.openai.com/codex/config-reference — documents `skills.config`, `mcp_servers`, Hook feature/config, plugin MCP controls and `tui.status_line`.
- OpenAI Codex Hooks: https://developers.openai.com/codex/hooks — documents Hook sources, additive behavior, trust review/hash semantics, project trust and managed policy.
- OpenAI Codex CLI: https://developers.openai.com/codex/cli — documents standalone, Windows, npm and Homebrew installation channels.
- Anthropic Claude Code settings: https://code.claude.com/docs/en/settings — documents user/project/local/managed sources, precedence/merge behavior, workspace trust and the published settings schema.
- Anthropic Claude Code setup: https://code.claude.com/docs/en/setup — documents native, Homebrew, WinGet, Linux package-manager and npm installations plus native launcher/version layout and release channels.
- Anthropic Claude Code Hooks: https://code.claude.com/docs/en/hooks — source for versioned Hook schema fixtures.
- Gemini CLI settings: https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/settings.md — documents user/workspace settings, precedence, Hook and Status UI settings.
- Gemini CLI Hooks: https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/index.md — documents user/workspace/system Hook precedence, enablement and project-Hook fingerprint trust.
- Gemini CLI trusted folders: https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/trusted-folders.md — documents `trustedFolders.json` location and untrusted-workspace behavior.
- Gemini CLI skills: https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/skills.md — documents built-in/extension/user/workspace precedence and aliases.
- Gemini CLI extensions: https://github.com/google-gemini/gemini-cli/blob/main/docs/extensions/index.md — documents official list/install/manage surface.
- Gemini CLI README: https://github.com/google-gemini/gemini-cli/blob/main/README.md — documents npx/npm/Homebrew/MacPorts and latest/preview/nightly install channels.
- Grok Build npm metadata: https://registry.npmjs.org/%40xai-official%2Fgrok/latest — observed package `@xai-official/grok`, executable `grok`, version `1.0.13`; no repository/homepage/schema evidence was present.

### Related specs and task artifacts

- `.trellis/tasks/09-04-agent-environment-runtime-center/prd.md`: R0 read-only inventory, R4 dynamic extension, three-platform acceptance and no configuration takeover.
- `.trellis/tasks/09-04-agent-environment-runtime-center/design.md`: environment/installation identity, Agent-owned adapters, Rust effective-state resolution, opaque asset access and version caching.
- `.trellis/tasks/09-04-agent-environment-runtime-center/implement.md`: Phase 1 execution and validation ordering.
- `.trellis/spec/frontend/directory-structure.md`: Vue components render, composables orchestrate, Rust owns business/capability rules (`19-54`).
- `.trellis/spec/frontend/component-guidelines.md`: stable scalar IDs and async UI release requirements (`19-41`, `55-72`).
- `.trellis/spec/frontend/state-management.md`: Rust state is authoritative and stale responses must be rejected (`19-22`, `47-63`).
- `.trellis/spec/frontend/type-safety.md`: Rust serde/IPC models are authoritative and finite states should be explicit unions (`19-51`).
- `.trellis/spec/guides/code-reuse-thinking-guide.md`: shared behavior must have one registry/source of truth; new Agent kinds must not require scattered branches.
- `.trellis/spec/guides/cross-layer-thinking-guide.md`: decode external/config data once at its boundary and expose typed projections (`74-122`).

## Caveats / Not Found

- No real Agent configuration, installation database, trust store, shell profile or installed executable was read. All conclusions come from repository source and public first-party documentation.
- Public docs can drift. Every parser must declare the CLI/schema versions its fixtures prove; an unknown version degrades to unsupported/unknown instead of borrowing the nearest parser.
- “All installations” is necessarily bounded to supported candidate sources. Neither PATH nor a package-manager list proves that no executable exists elsewhere on disk.
- Explicit macOS/Linux login/interactive-shell scanning runs user shell startup files. This is the unavoidable cost of recovering paths created there; it must never run automatically, periodically or without timeout.
- Official docs describe trust behavior more often than the persistent trust-store schema. Behavior documentation alone is insufficient to parse private on-disk formats; trust must remain unknown until a stable/version-pinned source contract exists.
- Grok Build's current path templates are not backed by public schema evidence found in this research. Structured parity should not be claimed yet.
- WSL installation discovery, Linux guest homes and Windows/guest path mapping remain explicitly out of scope.
