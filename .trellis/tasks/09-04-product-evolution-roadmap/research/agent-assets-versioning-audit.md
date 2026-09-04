# Research: Agent Assets, External Sessions, Versioning, and Safe Configuration Control

- Query: Determine whether BalanceHub should inventory and manage Agent skills, plugins/extensions, MCP, hooks, status UI, configuration files, externally launched sessions, WSL installations, and installed-versus-latest versions; compare the approach with CC Switch and define a safe phased boundary.
- Scope: mixed
- Date: 2026-09-04

## Findings

### 1. Executive conclusion

The proposed direction is feasible and fits the existing Agent registry, but it is not one homogeneous feature. The four Agents expose similar nouns through different storage, precedence, trust, and mutation semantics. BalanceHub should unify the **inventory and presentation contract**, while each Agent adapter remains the authority for discovery, parsing, effective-state calculation, supported mutations, and version sources.

The recommended first deliverable is a read-only **Agent environment center**:

- one Agent detail surface per discovered installation/environment;
- Skills, Plugins/Extensions, MCP, Hooks, status UI, and configuration files as separate asset categories;
- source, scope, path, enabled/effective/trust state, diagnostics, and override/conflict relationships;
- installed version versus latest stable version, with an explicit source and last-success time;
- open file, open containing directory, bounded read-only preview, and manual refresh;
- no installation, deletion, relocation, SSOT migration, symlink creation, automatic update, or bulk rewrite.

Controlled writes should follow only after the inventory model is proven. A generic boolean switch is unsafe: some Agents have a native `enabled` field or official enable/disable command, while others require removal, have scope-specific state elsewhere, or do not define a disable operation at all.

Hooks can extend runtime observation to Agent sessions launched outside BalanceHub. They cannot prove that the enclosing terminal is alive, cannot guarantee a final event after a crash or force-kill, and cannot see sessions when hooks are disabled, unsupported, blocked by policy, or installed in a different runtime environment. Hook telemetry must therefore be modeled as observed Agent activity with leases, not as authoritative terminal process tracking.

Windows native support exists today; WSL support does not. WSL must be represented as a separate runtime environment and installation identity, not as an extra executable path or a terminal option. The detailed prerequisite design is recorded in `research/wsl-agent-config-audit.md`.

### 2. BalanceHub files and current boundaries

- `src-tauri/src/agent_cli_catalog.rs:6-13`: compile-time registration of Claude Code, Codex CLI, Gemini CLI, and Grok Build.
- `src-tauri/src/models/agent_cli.rs:5-17`: current public capabilities cover launch, model selection, session history/search/detail/resume/name, liveness, and default configuration only.
- `src-tauri/src/services/agent_cli.rs:21-35`: `AgentCliDefinition` already owns Agent-specific adapters and is the correct extension point.
- `src-tauri/src/services/agent_cli.rs:37-59`: capabilities are derived from registered adapters rather than duplicated in the frontend.
- `src-tauri/src/services/agent_cli.rs:165-192`: all four registered Agents are probed concurrently.
- `src-tauri/src/services/agent_cli/discovery.rs:269-321`: installed versions currently come only from a bounded `<executable> --version` call.
- `src/components/settings/SettingsCliManager.vue:21-43`: current settings UI shows availability, installed version, path, and failure details; it has no asset inventory, latest-version comparison, or direct configuration entry.
- `src-tauri/src/services/agent_cli/config_support/mod.rs`: current default-configuration support already contains useful stable-read, allow-list, revision, and atomic replacement primitives.
- `src-tauri/src/services/agent_cli/{codex,claude,gemini,grok}/config.rs`: current configuration knowledge is limited to Provider default-switch files, not the complete Agent configuration universe.
- `src-tauri/src/services/cli_runtime.rs`: current runtime registry tracks BalanceHub-launched CLI instances; it has no event source for externally launched Agent sessions.
- `src-tauri/Cargo.toml`: no first-party filesystem watching layer is currently declared.
- `.github/workflows/quality.yml` and `.github/workflows/ci.yml`: macOS, Linux, and native Windows compilation/tests exist; there is no WSL runtime test.

Current default-switch files are:

| Agent | Files currently known by BalanceHub | Important limitation |
|---|---|---|
| Codex CLI | `~/.codex/config.toml`, `~/.codex/auth.json` | Project `.codex` scopes, hooks, skills, and plugins are not inventoried. |
| Claude Code | `~/.claude/settings.json` | Project/local settings, `~/.claude.json`, `.mcp.json`, skills, plugins, and hooks are not inventoried. |
| Gemini CLI | `~/.gemini/settings.json`, `~/.gemini/.env` | Project/system settings, skills, extensions, hook state, and MCP enablement state are not inventoried. |
| Grok Build | `~/.grok/config.toml`, `~/.grok/auth.json` | Project config, skills, plugins, hooks, and statusline sources are not inventoried. |

### 3. Official capability matrix

The matrix is based on official documentation and upstream source available on 2026-09-04. “Native toggle” means the Agent defines an enable/disable semantic that can be preserved without moving or deleting the underlying asset. It does not imply identical file formats across Agents.

| Capability | Codex CLI | Claude Code | Gemini CLI | Grok Build |
|---|---|---|---|---|
| User configuration | `~/.codex/config.toml`; authentication is separate | `~/.claude/settings.json`; `~/.claude.json` is Agent-maintained state | `~/.gemini/settings.json` plus platform system settings | `~/.grok/config.toml`; authentication is separate |
| Project configuration | `.codex/config.toml`, loaded only for trusted projects | `.claude/settings.json` and `.claude/settings.local.json` | `.gemini/settings.json` | `.grok/config.toml`, with a restricted set of project-allowed keys |
| Skills discovery | Repository `.agents/skills`, user `~/.agents/skills`, administrator `/etc/codex/skills` | User/project `.claude/skills`; nested discovery and hot reload | User/project `.gemini/skills` and `.agents/skills` | User/project `.grok/skills`, `.agents/skills`, plus compatible imported locations |
| Skills disable | Native config can declare a path disabled | No general native enabled flag. `disable-model-invocation` changes auto-invocation only and is not a full disable | Native user/workspace enable/disable state | Native `[skills] disabled` state |
| Plugins/extensions | Native plugins, install/uninstall and per-Space enablement | Native plugins, scoped install/uninstall/update/enable/disable | Native Extensions, user/workspace install/uninstall/update/enable/disable | Native plugins with enable/disable, paths, and trust |
| MCP storage/control | `[mcp_servers.<id>]`; native `enabled=false`, tool allow/deny, and timeouts | Project `.mcp.json` and user/local state; official CLI manages add/list/get/remove by scope, but a universal per-source enabled flag cannot be assumed | `mcpServers`; official add/remove/enable/disable; enablement state is stored separately | `[mcp_servers.<name>] enabled`; official list/add/remove/enable/disable/doctor |
| Hooks | User and project hook files/config; individual inspection/trust/disable plus global feature disable | Settings-based lifecycle hooks with command, HTTP, MCP, prompt, and Agent hook handlers | Settings-based hooks; global and individual enable/disable; currently synchronous | User/project JSON hooks, config/plugin/compatible hooks; TUI reload and individual enable/disable; failures default open |
| Status UI | `tui.status_line` is a list of built-in segments; `null` disables it | `statusLine` may run a script/command receiving JSON on stdin | Configurable built-in footer items and hide flags | `[ui.status_line]` supports built-in, command, or disabled modes; project config cannot inject a command statusline |
| Runtime hot reload | Scope/trust dependent; do not assume every source reloads live | Settings and skills are documented as reloadable, but active-session behavior remains capability-specific | Asset-specific commands/config reload behavior | Hook reload exists; other categories remain capability-specific |
| Built-in auto-update controls | Distribution/channel dependent; comparison must be installation-aware | Native and npm distributions differ | `general.enableAutoUpdate` and `general.enableAutoUpdateNotification` | `cli.auto_update` and `grok update` |

Consequences for the product model:

1. “Plugin” and “Extension” can share a display category, but their native identifiers, scopes, commands, and trust state remain adapter-owned.
2. Statusline cannot be represented as one script field. Codex and Gemini primarily expose built-in segments/footer settings, while Claude and Grok can execute arbitrary commands.
3. Claude Skills must be read-only in an initial toggle surface. Deleting or moving a directory is an install/uninstall operation, not a harmless switch.
4. MCP effective state must include its source/scope and override relationship. A server present in one file is not necessarily the effective server used by the Agent.
5. Project-scoped assets require a selected workspace and trust context. A user-global scan alone is incomplete.

### 4. Recommended Agent asset contracts

Do not add one large `AgentAssets` structure with fields for every current feature. Use capability-bearing records so a fifth Agent can register only what it supports:

```text
AgentEnvironmentDescriptor
  id                    // native:macos, native:windows, wsl:Ubuntu:default, ...
  kind                  // native | wsl
  hostPlatform
  guestPlatform?
  displayName
  capabilities

AgentInstallation
  id                    // environment + agent kind + executable identity
  environmentId
  agentKind
  executablePath
  installedVersion?
  discoverySource

AgentAssetCapability
  category              // skill | plugin | mcp | hook | statusUi | config
  discovery             // unsupported | user | workspace | system
  mutation              // readOnly | nativeToggle | managedMutation | externalCommand
  requiresRestart
  requiresTrust

AgentAssetSource
  id
  scope                 // user | workspace | local | system | managed | plugin
  environmentId
  workspaceId?
  path?
  precedence
  writable

AgentAssetRecord
  stableId
  category
  nativeId
  label
  sourceId
  declaredState
  effectiveState        // enabled | disabled | shadowed | blocked | invalid | unknown
  trustState?
  diagnostics[]
  revision?

AgentMutationCapability
  action                // enable | disable | install | uninstall | update | edit
  mechanism             // structuredFile | officialCli | unsupported
  confirmation
  validation
```

Each `AgentCliDefinition` should register optional asset scanners/parsers, toggle strategies, configuration manifests, latest-version sources, and hook telemetry adapters. Generic orchestration must not match on Codex/Claude/Gemini/Grok or infer support from a filename.

The Rust result is the source of truth for declared versus effective state. TypeScript renders returned capabilities and actions; it must not decide that a missing `enabled` key means enabled, or that every directory can be disabled.

### 5. Safe read-only MVP

The first release should inventory facts without assuming ownership of user configuration:

- Scan only documented, adapter-declared locations for the selected native environment and optional workspace.
- Show each asset’s Agent, category, name, source scope, real path, declared/effective state, trust requirement, modified time, and diagnostic state.
- Show source precedence and collisions, for example a project MCP entry shadowing a user entry with the same ID.
- Offer category filters and one search field over display name, native ID, path, scope, and diagnostic text.
- Provide “open file”, “open containing directory”, “copy path”, and bounded read-only preview where permitted.
- Refresh explicitly and when the detail view gains focus. Targeted watching may be active only while a real subscriber is present.
- Treat Skills/Plugin directories as inventory records. Do not recursively read arbitrary dependency/cache trees or package contents.
- Do not install, uninstall, update, synchronize, relocate, copy, symlink, or rewrite an asset.
- Do not execute third-party hook/statusline commands merely to preview them.

Recommended information architecture:

```text
Settings -> Agent environment
  -> installation selector (native / WSL distribution in later phase)
  -> Overview
  -> Assets
       Skills | Plugins/Extensions | MCP | Hooks | Status UI
  -> Configuration
  -> Sessions
```

This should be a dedicated Agent detail/workspace surface, not several new top-bar icons and not one modal per asset type.

### 6. Controlled mutation boundary

After the inventory is stable, safe toggles can be added only where official semantics exist:

| Agent | Safe candidates for a later controlled switch | Keep read-only or require a different action |
|---|---|---|
| Codex CLI | Skills config entries, MCP, Hooks, Plugins when the discovered source supports the documented native toggle | Installation/uninstallation, managed sources, trust-policy changes |
| Claude Code | Plugins; Statusline removal/restoration; individual Hooks only when the exact source and mutation contract are known | Skills; MCP sources without a documented independent enabled state; `~/.claude.json` broad rewrites |
| Gemini CLI | Skills, Extensions, Hooks, MCP through native state/official commands | System/managed settings and untrusted extension installation |
| Grok Build | Skills, Plugins, Hooks, MCP, Statusline through native state/official commands | Project command statusline, managed/trust policy, installation/uninstallation |

Every mutation must:

1. resolve an opaque asset/file ID in Rust against the current environment and allow-list;
2. re-read and compare a content revision/hash to reject stale writes;
3. parse the native structure and edit only the intended node or call the exact official CLI for that installation;
4. display a credential-free diff and require confirmation;
5. preserve unknown fields, formatting where reasonably possible, file permissions, and unrelated scopes;
6. write atomically when file-based;
7. re-read and verify the effective state after the operation;
8. return a typed conflict or unsupported result instead of overwriting the file;
9. expose restart/reload/trust consequences before confirmation.

BalanceHub must not silently migrate assets into a BalanceHub-owned SSOT. If cross-Agent distribution is later desired, it needs an explicit managed-versus-unmanaged model, provenance, conflict policy, backup, rollback, and a per-destination plan preview.

### 7. CC Switch comparison

CC Switch at commit `bc4ed66d3abe587b16338ef19853c7ee76b88ed7` is a useful implementation reference, but its ownership model is more invasive than the recommended BalanceHub MVP:

- `src-tauri/src/mcp/mod.rs:1-40` and `src-tauri/src/commands/mcp.rs:143-200` expose a shared MCP representation with per-App state and import/toggle commands.
- `src-tauri/src/commands/skill.rs:1-15`, `87-114` explicitly define a central Skills SSOT and per-App toggle/import operations.
- `src-tauri/src/services/skill.rs:4-5`, `548-624` place Skills under `~/.cc-switch/skills` or `~/.agents/skills` and map destinations per Agent.
- `src-tauri/src/services/skill.rs:2235-2303` prefers symlinks and falls back to copies. This is effective synchronization, but it takes ownership of filesystem layout and therefore needs migration, backup, and conflict semantics.
- `src-tauri/src/claude_plugin.rs:51-118` and `src-tauri/src/commands/plugin.rs:5-35` show that CC Switch’s plugin handling is substantially Claude-specific rather than proof of one cross-Agent plugin schema.
- `src-tauri/src/commands/misc.rs:99-112` includes installed/latest version and runtime-environment fields.
- `src-tauri/src/commands/misc.rs:152-205` exposes install/update lifecycle operations.
- `src-tauri/src/commands/misc.rs:509-517` maps the current four packages to their official npm names.
- `src-tauri/src/commands/misc.rs:804-834` dispatches latest-version lookup per tool, primarily through npm for these four Agents.
- `src-tauri/src/commands/misc.rs:842-968` distinguishes stable `latest` from Agent-specific prerelease tags and uses prerelease tags only when the local installation is already ahead of stable.
- `src-tauri/src/commands/misc.rs:2419-2504` enumerates installations and maps package identities rather than assuming one executable per Agent.
- `src-tauri/src/commands/misc.rs:2909-3012` selects source-specific update paths; native Grok uses `grok update`, while npm-managed installations use their package manager.

The transferable ideas are capability-aware adapters, effective-state inventory, per-App MCP state, installation-source detection, and source-specific version/update logic. The SSOT plus symlink/copy strategy should not be copied into BalanceHub as the default first release because it can replace the user’s existing directory ownership and creates rollback/conflict obligations.

### 8. Installed-versus-latest version strategy

Live checks on 2026-09-04 produced:

| Agent | Installed on this macOS host | Latest stable source | Latest stable | Non-stable observed | Upgrade implication |
|---|---:|---|---:|---|---|
| Codex CLI | `0.153.2` | npm `@openai/codex` dist-tag `latest` | `0.153.2` | `alpha=0.154.0-alpha.3` | Stable users must not be told to upgrade to alpha. Detect standalone/package-manager installs before offering an update action. |
| Claude Code | `2.1.260` | npm `@anthropic-ai/claude-code` dist-tag `latest` | `2.1.260` | `stable=2.1.236`, `next=2.1.260` | Native installer and npm installations require different update mechanisms. |
| Gemini CLI | `0.58.0` | npm `@google/gemini-cli` dist-tag `latest` | `0.58.0` | `preview=0.59.0-preview.0`, `nightly=0.60.0-nightly.20260904.g87a9c71d5` | Respect the current channel; never compare a stable installation against nightly by default. |
| Grok Build | `1.0.5` | npm `@xai-official/grok` dist-tag `latest` | `1.0.13` | `alpha=1.0.18` | The official/native install can self-update with `grok update`; npm installs must update through their owning package manager. The GitHub repository currently has no Releases. |

Recommended source policy:

- Preserve the raw local version string and parse a normalized comparison version separately.
- Detect installation source and channel. An Agent may have multiple installations; compare each installation independently.
- For npm-managed installations, use the package’s npm registry metadata and `dist-tags.latest` as the stable source.
- For a verified standalone/native distribution, prefer that distribution’s official updater metadata or release feed. GitHub Releases is a fallback only where the upstream actually publishes Releases.
- Do not scrape marketing pages or infer a version from repository commits.
- Default to the stable channel. Only compare prerelease/nightly/alpha when the installation or user preference explicitly selects that channel.
- Latest lookup failures produce `unknown` or `last successful result`; they must never be rendered as “already latest.”
- Phase one offers comparison and the upstream release/home link only. It does not execute updates.

Suggested cache and scheduling:

- Cache by `(agent, installation source, package/repository, channel)` for six hours.
- Run at most once after App startup when the Agent management surface becomes relevant; do not put it on a short global poll.
- Manual refresh bypasses the freshness cache but shares in-flight requests so repeated clicks do not fan out.
- Apply an overall bounded task, per-source timeout, cancellation, and a concurrency limit of two remote sources.
- Persist only the last successful version/source/check time plus the latest failure summary. A failure must not erase the last successful fact.
- Add exponential suppression after repeated failures, but do not aggressively retry in the foreground.
- Use `src-tauri/src/network/` so proxy and certificate semantics match the rest of BalanceHub.

For the current four Agents, npm is the lowest-request primary source: one registry response contains all dist-tags and avoids GitHub’s anonymous API limit. GitHub unauthenticated REST requests are rate-limited and this research encountered that limit; use GitHub only when required by the installation source and show a typed limited/unavailable state.

### 9. External CLI observation through hooks

#### What hooks can solve

An opt-in BalanceHub hook can discover Agent sessions that were started from another terminal, IDE, script, or launcher. Normalize only lifecycle metadata that official Agent hooks actually expose:

```text
ExternalAgentEvent
  schemaVersion
  eventId
  observedAt
  agentKind
  environmentId
  nativeEvent
  normalizedEvent       // sessionStarted | activity | sessionStopped | sessionEnded
  sessionId?
  cwd?                  // separately permissioned because it reveals local paths
  transcriptPath?       // optional metadata only; never ingest implicitly
  hookInstallationId
```

Recommended transport is a small `balancehub-agent-hook` helper, separate from GUI startup:

1. The Agent invokes the helper with a documented hook payload on stdin.
2. The helper validates a bounded payload, extracts only allow-listed metadata, and returns quickly.
3. If BalanceHub is running, the helper sends a local event to a versioned local-only receiver.
4. If it is not running, the helper atomically writes one bounded event file into an App-owned inbox; the App ingests and expires it later.
5. Hook execution never waits on the network, launches the GUI, indexes a transcript, or fails the Agent operation.

Do not collect prompt text, model responses, tool input/output, environment variables, credentials, or arbitrary hook payload fields. Transcript ingestion remains the existing explicit session-history feature and must follow its own index/privacy rules.

#### Required lease state

Hooks report events, not process truth. The UI should distinguish:

- `confirmedRunning`: BalanceHub owns or can positively probe the process;
- `recentActivity`: a hook event was seen inside the lease window;
- `unknown`: the lease expired without a definitive end event;
- `ended`: a supported final event was received or an owned process exited.

`SessionStart`, prompt/tool/stop activity, and `SessionEnd`-like events should be mapped where each Agent exposes them. A terminal force-close, `kill -9`, crash, machine sleep, helper failure, or disabled hook can omit the final event. Expiry must therefore transition to `unknown`, not falsely to `ended`.

#### Installation and coexistence

- Hook integration is opt-in per Agent, environment, and scope.
- Inventory existing hook configuration first and show the exact planned insertion.
- Add one namespaced BalanceHub entry; preserve order and all unrelated handlers.
- Use a stable installation ID so upgrades replace only BalanceHub’s own entry.
- Uninstall removes only the exact BalanceHub-owned entry after a revision check.
- Where multiple hooks are supported, append without replacing the user’s existing hook.
- Where the format is single-owner or managed by policy, report `requiresManualSetup` or `unsupported` rather than overwrite.
- Gemini hooks are currently synchronous, so the helper path must remain local and extremely short.
- Statusline is not an acceptable monitoring substitute: it is high-frequency, may be single-owner, can overwrite customization, and can cause visible CLI latency.

This observes the Agent session, not the terminal window. It cannot answer whether an idle shell remains open after the Agent exits, and it should not be labeled “all terminals.”

### 10. Configuration browsing, editing, and monitoring

A direct configuration entry is useful and should not depend on switching a Provider default configuration. The configuration surface should be rooted at an Agent installation/environment, then show adapter-declared files by category and scope.

Recommended manifest:

```text
AgentConfigFile
  fileId                // opaque; frontend never submits an arbitrary path
  environmentId
  installationId
  label
  scope                 // user | workspace | local | system | managed
  category              // settings | auth | mcp | hooks | statusUi | other
  format                // json | jsonc | toml | env | yaml | markdown | text
  pathDisplay
  exists
  writable
  sizeBytes
  modifiedAt
  revision              // hash of exact content plus existence state
  sensitivity           // none | mixed | secretFile
  readCapability
  editCapability
  watchCapability
```

Read-only MVP behavior:

- Resolve real paths in Rust from an Agent allow-list; never accept an arbitrary frontend path.
- Bound file size and preview output. Do not follow symlinks outside the declared root without an explicit adapter policy.
- Full preview is allowed for non-sensitive configuration.
- Mixed files return a redacted projection; pure credential files are metadata-only under the current repository privacy rule.
- Open in the system editor and open containing directory remain explicit user actions.
- Show source scope and precedence so users know which file is effective.

Later in-App editing behavior:

- Use one maintained editor surface with syntax mode, search, line numbers, undo, and keyboard/scroll behavior; do not grow the specialized default-switch diff into a second general editor.
- Keep edits in memory until Save. Debounced validation uses request IDs and never writes per keystroke.
- On Save, Rust re-resolves the file ID, compares the revision, parses and validates, writes atomically, then re-reads the result.
- External changes refresh a clean editor. If the editor is dirty, preserve the draft and mark a conflict; never overwrite either side silently.
- Sensitive values must remain redacted across IPC, diff, diagnostics, logs, background tasks, and tests unless the repository privacy rule is explicitly changed.

Targeted monitoring is feasible:

- Native macOS/Linux/Windows: watch only allow-listed files or immediate parent directories while an active UI/background subscriber exists; debounce replace/write bursts and compare content revisions.
- WSL files exposed through UNC/network paths: do not rely on native watcher events. Use bounded polling for the visible manifest/document, because network filesystem events are not guaranteed.
- Do not recursively watch session histories, Skill/plugin trees, package caches, or every home directory. Each inventory gets its own refresh policy.
- A watcher event only invalidates a snapshot; the adapter must re-read and recompute effective state.

### 11. WSL compatibility

BalanceHub currently supports native Windows Agents and terminals, not WSL Agents. Source search found no product implementation for `wsl.exe`, distribution discovery, `wslpath`, WSL home/config roots, Linux executable invocation from Windows, guest PID tracking, or WSL session roots.

WSL support requires the model:

```text
Agent product
  x runtime environment (native host or WSL distribution + default user)
    -> installation
    -> config roots and asset sources
    -> session roots
    -> launch target and process identity
```

An Agent kind can have a native Windows installation and separate Ubuntu/Debian installations simultaneously. Executing an ELF through a UNC path is not WSL execution. A WSL launch must use bounded arguments such as `wsl.exe --distribution <name> --exec <executable> ...`, and Windows/WSL workspace paths must be explicitly converted with `wslpath` rather than string replacement.

Passive discovery must not start stopped distributions. List them first, probe a stopped distribution only after explicit user action, and initially support the distribution’s default user. Real support requires a Windows self-hosted/manual test matrix; GitHub’s `windows-latest` compilation alone cannot verify distribution startup, Windows Terminal profiles, UNC behavior, Linux ownership/mode, guest PIDs, or login-shell state.

The complete runtime, config transport, watcher, path, process, migration, and test design is in `research/wsl-agent-config-audit.md`. Asset inventory and external hook telemetry must key every record by environment/installation so WSL support does not later require another schema rewrite.

### 12. Recommended delivery order and dependencies

1. **Environment/installation identity and read-only inventory contract.** This is foundational for native multi-installation and future WSL support.
2. **Native Agent environment center.** Add asset/config inventory, read-only preview, source/effective-state display, targeted refresh, and installed/latest stable comparison.
3. **Controlled native mutations.** Add only official native toggles and revision-safe file edits, one Agent/category at a time.
4. **External session observation.** Add helper protocol, opt-in hook installation, lease state, bounded spool, and privacy tests. This depends on environment identity and hook inventory.
5. **WSL discovery/read-only assets and sessions.** Distinguish distributions and path namespaces without starting stopped distributions implicitly.
6. **WSL launch/write/hooks.** Add guest-side atomic config transport, terminal launch, guest process identity, and per-distribution hook helper only after real Windows/WSL validation infrastructure exists.
7. **Install/update/synchronization.** Detect installation source and ownership first; add preview, rollback, trust, and signature/provenance checks before executing package managers or creating SSOT links.

These can be separate Trellis child tasks. The first two establish product value without taking ownership of user files. Steps 3-7 each have distinct risk, rollback, and acceptance criteria and should not be hidden inside one “Agent management” implementation.

### 13. Validation requirements

- Parser fixtures for every supported Agent/category/scope, including malformed, unknown-field, duplicate-ID, shadowed, managed, and symlink cases.
- Contract tests proving that adding a fifth Agent needs only registry/adapters and does not require generic orchestration branches.
- Snapshot tests for declared versus effective state and mutation capability.
- No-secret serialization tests over every IPC, diff, error, hook event, spool file, and background-task payload.
- Revision conflict, atomic write, permission preservation, cancellation, timeout, stale-result rejection, and interrupted operation tests.
- Hook helper latency/output bounds, App-not-running spool, concurrent events, missing final event, duplicate event ID, old schema, and disabled hook tests.
- Version tests for stable/prerelease channels, unparsable versions, multiple installations, registry failure, stale cache, GitHub rate limit, and a local version ahead of stable.
- Native macOS/Linux/Windows checks plus real Windows WSL 1/2 smoke tests before claiming WSL support.
- UI verification for long paths/names, missing assets, unsupported actions, conflict state, small screens, keyboard operation, and non-blocking refresh.

## External References

- OpenAI Codex configuration: `https://developers.openai.com/codex/config-basic.md`, `https://developers.openai.com/codex/config-advanced.md`, `https://developers.openai.com/codex/config-reference.md`.
- OpenAI Codex Skills, Plugins, and Hooks: `https://developers.openai.com/codex/skills.md`, `https://developers.openai.com/codex/plugins.md`, `https://developers.openai.com/codex/hooks.md`.
- Claude Code settings, Skills, Plugins, MCP, Hooks, and Statusline: `https://code.claude.com/docs/en/settings.md`, `skills.md`, `plugins.md`, `plugins-reference.md`, `mcp.md`, `hooks.md`, and `statusline.md` under the same documentation root.
- Gemini CLI upstream at commit `87a9c71d57a4ec56c00f3ff628970fea8291d812`: `docs/cli/settings.md`, `docs/cli/skills.md`, `docs/extensions/reference.md`, `docs/hooks/index.md`, `docs/hooks/reference.md`, `docs/tools/mcp-server.md`, `docs/reference/configuration.md`, and `docs/releases.md` in `https://github.com/google-gemini/gemini-cli`.
- Grok Build upstream at commit `72a61251fcffb464bcc687aeb5a998e5a98ec0c9`: `crates/codegen/xai-grok-pager/docs/user-guide/` in `https://github.com/xai-org/grok-build`, plus `https://docs.x.ai/build/overview`.
- CC Switch at commit `bc4ed66d3abe587b16338ef19853c7ee76b88ed7`: `https://github.com/farion1231/cc-switch`.
- Microsoft WSL commands and filesystems: `https://learn.microsoft.com/windows/wsl/basic-commands` and `https://learn.microsoft.com/windows/wsl/filesystems`.
- `notify` filesystem watcher documentation: `https://docs.rs/notify/latest/notify/`.
- npm package metadata checked live for `@openai/codex`, `@anthropic-ai/claude-code`, `@google/gemini-cli`, and `@xai-official/grok` on 2026-09-04.

## Related Specs

- `.trellis/spec/guides/agent-routing.md`: cross-layer architecture and risk analysis belongs to `gpt-5.6-sol/max`; deterministic implementation follows after the design is fixed.
- `.trellis/spec/guides/cross-layer-thinking-guide.md`: define ownership, transport, validation, persistence, and UI consumption before implementation.
- `.trellis/spec/guides/code-reuse-thinking-guide.md`: keep one registry/source of truth and avoid a second per-Agent switch in generic orchestration.
- `.trellis/spec/frontend/component-guidelines.md`: bounded visual surfaces, typed props/events, stable IDs, and async release requirements.
- `.trellis/spec/frontend/state-management.md`: Rust owns persisted capabilities and effective state; frontend state must reject stale responses.
- Repository `AGENTS.md`: dynamic Agent discovery, Rust-owned capability decisions, network reuse, non-blocking async behavior, config privacy, and native three-platform validation.

## Caveats / Not Found

- Official Agent capabilities evolve quickly. The matrix is pinned to the documentation/upstream snapshots and live package metadata checked on 2026-09-04; adapters need fixtures and versioned capability assumptions rather than permanent hard-coded claims.
- No generic hook can guarantee observation of every externally launched session. Unsupported/disabled/managed hooks, old Agent versions, helper failure, hard process termination, containers, SSH hosts, and runtime environments not registered in BalanceHub remain visibility gaps.
- Hooks observe Agent lifecycle, not the lifetime of the outer terminal window. BalanceHub must not merge those meanings in labels or counts.
- “Enabled” is not a cross-Agent primitive. The UI must suppress unsupported switches instead of simulating them through directory deletion, file moves, or broad config rewrites.
- Direct configuration editing is constrained by the current repository rule that credentials cannot cross IPC in plaintext. Literal editing of auth/token files would require an explicit product/privacy-rule change before implementation.
- No WSL runtime was available during this macOS research. Current absence is source-confirmed, while actual WSL behavior requires a real Windows host and WSL distributions.
- CC Switch proves that broad management is possible, but its SSOT and symlink/copy behavior has a materially larger ownership and rollback surface than a read-only BalanceHub inventory.
- GitHub anonymous API requests were rate-limited during research. npm registry metadata was available and is the preferred primary latest-version source for the four current npm-distributed Agents.
