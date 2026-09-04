# Research: Windows/WSL Agent Runtime and Config Management

- Query: Audit current Windows and WSL compatibility plus Agent configuration browsing/editing, then define the correct runtime-environment and configuration contracts.
- Scope: mixed
- Date: 2026-09-04

## Findings

### 1. Files found

- `src-tauri/src/agent_cli_catalog.rs`: compile-time catalog of the four built-in Agent identities.
- `src-tauri/src/models/agent_cli.rs`: Agent descriptor, feature capability, installed path, and installed-version IPC models.
- `src-tauri/src/models/app_settings.rs`: one preferred CLI path per Agent kind and one global terminal preference.
- `src-tauri/src/services/agent_cli.rs`: registry-owned Agent discovery and optional launch/session/liveness/default-config adapters.
- `src-tauri/src/services/agent_cli/discovery.rs`: native executable discovery and bounded `--version` probing.
- `src-tauri/src/services/agent_cli/discovery/paths.rs`: native host PATH, HOME/USERPROFILE, package-manager, and Windows npm path resolution.
- `src-tauri/src/services/agent_cli/{codex,claude,gemini,grok}/config.rs`: current Agent-specific default-config roots, parsing, rewrite, validation, revision, and write behavior.
- `src-tauri/src/services/agent_cli/{codex,claude,gemini,grok}/sessions*`: Agent-specific history discovery rooted in the same native config homes.
- `src-tauri/src/services/agent_cli/config_support/mod.rs`: stable reads, file-set allow-listing, optimistic revisions, and atomic per-file replacement.
- `src-tauri/src/services/cli_paths.rs`: native process environment to user-home resolution.
- `src-tauri/src/services/cli_runtime.rs`: BalanceHub-launched CLI instance registry and status reconciliation.
- `src-tauri/src/services/cli_runtime/config.rs`, `cli_runtime/app.rs`: current Provider-coupled default-config preview and switch orchestration.
- `src-tauri/src/services/temporary_cli.rs`: explicit BalanceHub launch path and registration of temporary CLI instances.
- `src-tauri/src/services/temporary_cli/shell_runtime/`: Unix and Windows launch-script generation, environment restoration, and quoting.
- `src-tauri/src/services/temporary_cli/terminal/{mod,windows,linux}.rs`: platform terminal catalogs and visible terminal launch behavior.
- `src-tauri/src/platform/process.rs`: bounded child output, timeout, process-tree termination, and hidden Windows background commands.
- `src-tauri/src/commands/cli.rs`: current CLI/session/config IPC entry points and blocking-work boundaries.
- `src-tauri/src/models/provider_results.rs`: current config file/preview contract, including absolute paths and complete contents.
- `src/stores/provider-types.ts`, `src/stores/cli-runtime.ts`: frontend mirror and shared runtime state.
- `src/composables/useCliRuntime.ts`: Provider/Key-oriented config preview flow and 4-second polling of BalanceHub-launched instances.
- `src/components/CliConfigPreviewModal.vue`: custom line-diff editor used only when switching a Provider into an Agent's default config.
- `src/components/settings/SettingsCliManager.vue`, `SettingsAgentSection.vue`: current Agent detection/version surface; no config browse/edit entry exists.
- `src-tauri/Cargo.toml`: no filesystem-watching dependency is currently present.
- `.github/workflows/quality.yml`, `.github/workflows/ci.yml`: native Linux/macOS/Windows compile and test coverage, but no WSL runtime coverage.

### 2. Current native Windows support

Native Windows support is real, but it is specifically a Windows-host runtime rather than a Windows-plus-WSL abstraction.

| Area | Current native Windows behavior | Evidence | Assessment |
|---|---|---|---|
| Agent identity | Agent kind is registry-driven; generic orchestration does not branch on the four current Agents. | `src-tauri/src/services/agent_cli.rs:21-35`, `85-135` | Good reuse boundary, but it identifies a product, not an installation/runtime environment. |
| Executable discovery | Checks preferred path, Agent-specific env vars, native home candidates, process PATH, shell discovery, `.cmd` and `.exe`, plus `%APPDATA%/npm` and `%LOCALAPPDATA%/npm`. | `src-tauri/src/services/agent_cli/discovery.rs:24-83`, `86-175`; `discovery/paths.rs:197-219`, `236-258`, `303-317` | Supports native Windows npm/global installs. |
| Version probe | Executes the discovered native path with `--version`, a five-second timeout, bounded output, and a hidden background console. | `src-tauri/src/services/agent_cli/discovery.rs:269-321`; `src-tauri/src/platform/process.rs:25-64` | Installed version is supported; latest-version lookup is a separate concern. |
| User home/config root | Uses `USERPROFILE`, falling back to `HOMEDRIVE` + `HOMEPATH`; each Agent then resolves its native config directory. | `src-tauri/src/services/cli_paths.rs:9-28`; Agent `config.rs` files | Correct for native Windows only. |
| Terminal discovery | Registers Windows Terminal, Command Prompt, and PowerShell; probes `wt`, `cmd`, `pwsh`/`powershell`. | `src-tauri/src/services/temporary_cli/terminal/windows.rs:18-103` | Native terminal support exists. |
| Visible launch | Windows Terminal is explicitly launched as `wt -d <workdir> cmd /K <script>`; other paths use cmd or PowerShell. | `src-tauri/src/services/temporary_cli/terminal/windows.rs:105-155` | This always launches a Windows command environment, even if Windows Terminal has WSL profiles. |
| Shell environment | Captures current process variables, cmd variables, and PowerShell profiles; restores registered Agent aliases/functions into a `-NoProfile` launch. | `src-tauri/src/services/temporary_cli/shell_runtime/environment/windows.rs:9-71`, `87-142`; `script/windows.rs:63-79` | Good native compatibility; none of this observes WSL shell state. |
| Script safety | Windows launch uses JSON payload plus namespaced `BH_`/`BALANCEHUB_` variables rather than interpolating secrets into the batch file. | `src-tauri/src/services/temporary_cli/shell_runtime/script/windows.rs:17-80`, `90-146` | Good native boundary. |
| Process status | BalanceHub-created scripts update a private runtime status file; Windows liveness uses `tasklist`, while timed-out background commands use `taskkill /T /F`. | `src-tauri/src/services/cli_runtime.rs:23-37`, `107-208`, `272-323`, `438-459`; `src-tauri/src/platform/process.rs:163-178` | Tracks only launches that registered through BalanceHub. |
| Sessions/configs | Session and config paths are derived from the native host config root and native workdir. | `src-tauri/src/services/agent_cli/codex/sessions/mod.rs:24-44`, `202-205`; `claude/sessions.rs:26-52`; `gemini/sessions.rs:29-46`; `grok/sessions.rs:32-50` | Native Windows installations can work; WSL installations are invisible. |
| CI | Clippy/tests run on `windows-latest`; no-bundle compilation also runs Windows x64 and ARM64. | `.github/workflows/quality.yml:17-29`, `74-80`; `.github/workflows/ci.yml:34-50`, `79-84` | Proves native compilation and unit tests, not real terminal/profile/WSL behavior. |

### 3. WSL support is currently absent

A repository-wide search found no product-code reference to `wsl.exe`, WSL distribution discovery, `WSL_DISTRO_NAME`, `wslpath`, `/mnt/<drive>`, or `\\wsl$`/`\\wsl.localhost`. The `is-wsl` entry in `Cargo.lock` is transitive and is not used by BalanceHub source.

The missing support is structural, not one missing path candidate:

| Required WSL concern | Current assumption | Result |
|---|---|---|
| Environment identity | `AgentCliKind` uniquely identifies the effective installation; settings store one path per kind (`BTreeMap<AgentCliKind, String>`). | Cannot represent Codex on Windows plus Codex in Ubuntu plus Codex in Debian. |
| Executable path | Candidate must be a native `Path::is_file()` and is then passed to `Command::new(path)`. | A Linux ELF inside WSL cannot be executed as a Windows native CLI. A UNC path alone does not solve execution. |
| Home/config root | Config roots use native `USERPROFILE` and native environment variables. | `~/.codex`, `~/.claude`, `~/.gemini`, and `~/.grok` inside each WSL user are not discovered. |
| PATH/profile | Windows captures cmd/PowerShell environment; Unix logic runs only when BalanceHub itself is built for a non-Windows target. | WSL login-shell PATH, aliases, functions, Node managers, and Agent wrappers are not loaded. |
| Workdir | IPC passes an unqualified host path string and native canonicalization is used. | `C:\repo`, `/mnt/c/repo`, and `/home/user/repo` are treated as if they were the same path namespace. |
| Terminal | Windows Terminal launch explicitly starts `cmd /K`. | Selecting Windows Terminal does not launch a WSL profile or `wsl.exe`. |
| Sessions | Agent session adapters resolve under the native Agent config root and compare against the native workdir representation. | WSL session history and resume IDs are absent even when the same project is visible from Windows. |
| Config write | Native Rust filesystem calls write the resolved native paths. | No WSL ownership, mode, symlink, or atomic-rename semantics are defined. |
| Process tracking | Registered status file contains the visible terminal/script PID; Windows health checks use `tasklist`. | There is no WSL Linux PID identity or process-lifetime bridge. |

An explicitly configured `\\wsl.localhost\<distro>\...\codex` path would still fail as a native executable. Conversely, choosing a Windows `codex.cmd` while the project is also mounted at `/mnt/c` remains a native Windows Agent run, not WSL support.

Microsoft documents `wsl --list --verbose` for installed distribution/state/version discovery and `wsl --distribution <name> --user <name>` for targeting a distribution/user. It also states that commands passed to `wsl.exe` are forwarded without path conversion and therefore require WSL-format paths. Windows applications can browse a distribution through `\\wsl$`, but Microsoft recommends keeping Linux-command-line projects in the WSL filesystem for performance. These facts require an explicit environment/path contract rather than opportunistic UNC probing.

### 4. Correct environment and installation model

The foundational object should be a **runtime environment**, not another terminal kind and not a boolean `is_wsl` on an Agent.

```text
Agent product (Codex / Claude Code / ...)
  x Runtime environment (native host / WSL distribution + user)
    -> Agent installation (executable, installed version, capabilities)
    -> Config roots/files
    -> Session roots
    -> Launch and process identity
```

Recommended Rust-owned contracts:

```text
AgentRuntimeEnvironment
  id                    // stable scalar, e.g. native:windows or wsl:Ubuntu:default
  kind                  // native | wsl
  hostPlatform          // windows | macos | linux
  guestPlatform         // linux for WSL, otherwise null
  displayName
  distributionName?     // exact WSL name; never shell-concatenated
  userName?             // resolved user, default-user-only in MVP
  homePath              // path in that environment's namespace
  pathStyle             // windows | unix
  state                 // available | stopped | unavailable | error
  capabilities          // probe, launch, configRead, configWrite, sessions, watch

AgentInstallation
  id                    // environmentId + agentKind + canonical executable identity
  environmentId
  agentKind
  executablePath        // path in the target environment namespace
  commandName
  installedVersion
  discoverySource
  available
  message

RuntimePath
  environmentId
  value                 // never interpreted outside the named environment
```

`AgentCliDescriptor` remains the Agent product/capability descriptor. Discovery results must change from one result per Agent kind to zero or more `AgentInstallation` rows. Persisted preferred paths and temporary-CLI preferences must identify an installation/environment, not only `AgentCliKind`. Existing native settings can migrate deterministically to `native:<host>`.

The generic execution boundary should accept an `ExecutionTarget`:

```text
NativeExecutionTarget
  -> Command::new(native executable)

WslExecutionTarget { distribution, user }
  -> wsl.exe --distribution <distribution> [--user <user>] --exec <executable> <args...>
```

Arguments must remain separate `Command` arguments. Do not construct a shell command from user-controlled distro, user, path, model, session ID, or provider values. A login shell is permitted only behind a fixed adapter-owned script when an Agent genuinely depends on aliases/functions; values are supplied as positional arguments or a bounded stdin payload.

WSL discovery should be opt-in and bounded because probing a stopped distribution starts its VM and can be visibly slow:

1. Read `wsl.exe --list --quiet`/`--verbose` with a Windows-aware UTF-8/UTF-16 decoder and a timeout.
2. Initially inspect only running distributions without side effects.
3. Show stopped distributions as known but unprobed.
4. Probe a stopped distribution only after an explicit user action that states it will start WSL.
5. Batch `HOME`, `id -un`, `SHELL`, `command -v` for every registered Agent, and version checks into one bounded distribution probe rather than starting one `wsl.exe` process per fact.

For the first WSL release, support the distribution's default user only. Multi-user WSL support changes config/session ownership and should be added as a later explicit environment variant instead of silently assuming `/home/<name>`.

### 5. WSL launch, paths, and process semantics

#### Workspaces and paths

A workspace must become `RuntimePath`, because these are not interchangeable strings:

- Windows native: `C:\work\project`
- The same Windows directory mounted in WSL: `/mnt/c/work/project`
- WSL-native project: `/home/user/project`
- Windows browsing path for that WSL-native project: `\\wsl.localhost\Ubuntu\home\user\project`

Never convert by string replacement. When the user explicitly asks to use a Windows-native workspace in WSL, resolve it through `wsl.exe -d <distro> --exec wslpath -a -- <path>` and preserve both representations. A WSL-native workspace should retain the WSL path as canonical; its UNC representation is only a host browsing transport.

#### Visible terminal launch

The current `wt -d <workdir> cmd /K <script>` path cannot be reused. A WSL target requires a separate terminal launch adapter, for example Windows Terminal invoking `wsl.exe --distribution <distro> --cd <wsl-workdir> --exec <shell-or-launcher>`. Command Prompt and PowerShell may host `wsl.exe`, but they are not the execution environment and should not be advertised as equivalent WSL terminals until explicitly implemented and tested.

An MVP can support WSL launch through Windows Terminal only and return a Rust-owned unsupported reason for other terminal/environment combinations. This is more honest than silently falling back to native cmd.

#### Process and instance identity

BalanceHub-launched instances should retain the existing status-file handshake, but metadata must add `environment_id` and distinguish host terminal PID from guest Agent PID. Windows `tasklist` can only prove the host-side launcher is alive. Guest process liveness/cancellation must run through the same WSL execution target and a guest status record; do not infer the Linux Agent's state from a `wsl.exe` parent alone.

This environment contract also prepares the separate hook-based runtime-observation feature. Hooks can report externally launched Agent sessions, but every report still needs `environment_id`, Agent kind, session ID, workdir in its native namespace, and a stable run ID. Hooks do not remove the need for WSL identity.

### 6. Current config preview/editor contract

The current feature is not a general Agent configuration manager:

```text
Provider card action
  -> choose Agent
  -> choose one Provider API Key
  -> preview_cli_config(providerId, agentKind, keyLocalId)
  -> Agent-specific rewrite of endpoint/key
  -> return originalFiles + rewritten files + revision
  -> custom inline diff editor
  -> switch_cli_config(...files)
```

Evidence:

- `CliRuntimeService::preview_config` requires a Provider ID and looks up that Provider before the Agent adapter is called (`src-tauri/src/services/cli_runtime/app.rs:26-38`).
- The adapter contract is named `DefaultConfigAdapter` and accepts Provider/Key targets, not an environment or config-file identity (`src-tauri/src/services/agent_cli/contracts.rs:336-394`).
- The four implementations currently expose only:
  - Codex: `config.toml`, `auth.json` (`codex/config.rs:14-25`, `72-105`).
  - Claude Code: `settings.json` (`claude/config.rs:13-24`, `68-87`).
  - Gemini CLI: `settings.json`, `.env` (`gemini/config.rs:14-25`, `73-111`).
  - Grok Build: `config.toml`, `auth.json` (`grok/config.rs:18-30`, `76-110`).
- The frontend flow exists only as Provider/Key selection and preview state (`src/composables/useCliRuntime.ts:177-230`).
- `SettingsCliManager` renders detection/version items but provides no Agent detail or config entry (`src/components/settings/SettingsCliManager.vue:21-55`, `59-99`).
- `CliConfigPreviewModal` implements an in-house O(lines x lines) LCS diff with a 250,000-cell cutoff and editable `contenteditable` lines (`src/components/CliConfigPreviewModal.vue:9-25`, `52-65`, `82-141`, `469-503`). It is tightly coupled to a switch preview and should not become the general editor foundation.

Reusable backend mechanics do exist:

- bounded stable reads (`config_support/mod.rs:40-72`, `85-93`);
- exact expected-file-set validation (`113-143`);
- optimistic revision rejection (`145-160`);
- atomic same-directory temp write and platform replacement with Windows rollback (`284-356`);
- Agent-specific JSON/TOML rewrites that preserve unrelated source text where implemented.

Important gaps:

1. `CliConfigFile` exposes an absolute path and full content directly over IPC (`src-tauri/src/models/provider_results.rs:670-688`). A general manager should use opaque Rust-issued file IDs and never accept arbitrary frontend paths.
2. Current config revisions use `DefaultHasher`, include Provider target values, and represent the whole preview rather than independent file revisions (`config_support/mod.rs:145-160`; Agent config preview implementations). Direct editing needs SHA-256 per file plus a manifest revision.
3. Revision checking occurs before replacement but cannot eliminate an external editor race between the final read and rename. The UI must describe this as optimistic conflict protection, not locking.
4. Multi-file switches perform ordered atomic writes and attempt rollback, but they are not a single filesystem transaction. This should become a shared transaction plan with backups and explicit partial-rollback reporting.
5. Format validation is inconsistent. Codex validates TOML and JSON; Claude validates JSON; Gemini validates `settings.json` but writes edited `.env` text without validating all assignments; Grok requires its endpoint/auth shape. A general editor needs both syntax validation and an optional Agent-specific semantic validator.
6. There is no backend filesystem watcher. All `watch(...)` matches in the frontend are Vue reactivity; `Cargo.toml` has no `notify` dependency.

### 7. Config-management architecture

Add a `ConfigManagementAdapter` alongside, not inside, `DefaultConfigAdapter`. Default Provider switching is one specialized operation that can reuse the config repository; browsing/editing should not require a Provider.

```text
AgentCliDefinition
  descriptor
  temporaryLaunch?
  sessions?
  liveness?
  defaultConfig?
  configManagement?  -> manifest + syntax/semantic validation + secret policy

AgentConfigService
  RuntimeEnvironmentRepository
  AgentConfigRepository
  ConfigWatchService
```

Recommended query contracts:

```text
AgentConfigManifest
  environmentId
  installationId
  agentKind
  rootLabel
  revision
  files[]

AgentConfigFileDescriptor
  id                    // opaque, stable within environment + Agent + scope
  label
  scope                 // user | project | system
  category              // settings | auth | mcp | hooks | statusline | other
  format                // json | jsonc | toml | env | yaml | markdown | text
  exists
  writable
  sizeBytes
  modifiedAt
  revision              // SHA-256 of exact bytes plus existence state
  sensitivity           // none | mixed | secretFile
  readCapability
  editCapability
  watchCapability

AgentConfigDocument
  fileId
  revision
  displayContent         // credential-free projection only
  secretSpans[]          // opaque protected regions, no plaintext value
  diagnostics[]
```

`list_agent_configs(environment_id, installation_id, workspace?)` returns the allow-listed manifest. `read_agent_config(file_id)` resolves the path again in Rust and returns a bounded document. `validate_agent_config(file_id, revision, edited_content)` returns syntax and semantic diagnostics without writing. `save_agent_config(...)` re-resolves the allow-listed file, re-reads it, rejects a revision mismatch, validates, writes atomically, and returns the new revision/document.

Config paths must be declared by each Agent adapter because scopes and file names differ. The frontend renders the returned categories/formats and must not hard-code the current four Agents or infer hook/MCP support from filenames. Directory resources such as skills/plugins require a separate inventory contract; they should not be recursively loaded into the text-editor manifest.

For native files, reuse and strengthen the existing stable read/atomic replacement implementation. For WSL files, use an environment-specific file transport. Two viable implementations are:

- execute a fixed bounded POSIX helper through `wsl.exe` so read/stat/validate/temp-write/rename happen inside the Linux filesystem; or
- access through `\\wsl.localhost\<distro>\...` for reads and use a WSL-side fixed helper for permission-preserving writes.

The second approach gives convenient read performance while preserving Linux write semantics. Do not write WSL config files by treating an ELF path or Linux home as an ordinary Windows path, and do not interpolate file paths into shell text.

### 8. Secret-display policy and an existing violation

Current repository rules say API keys, tokens, cookies, and passwords must be hidden/redacted in configuration previews, and the roadmap additionally says IPC must not return plaintext credentials. The existing default-config preview predates or violates that boundary:

- Codex reads `auth.json`, rewrites it with the selected full API key, and returns both original and next contents (`codex/config.rs:72-105`, `183-221`).
- Gemini does the same for `.env` (`gemini/config.rs:73-111`, `206-215`).
- The shared IPC model serializes those file contents without a redaction layer (`models/provider_results.rs:670-688`).

A general configuration center must not expand this behavior. Under the current rules:

- non-sensitive files may be browsed and edited in full;
- mixed files may expose a backend-generated redacted projection with protected secret spans; unchanged secrets are merged from the latest on-disk document only when its revision still matches;
- pure credential files are metadata-only in the in-app browser, with a system-editor/open-location action or a structured write-only credential replacement action;
- logs, task events, diffs, validation messages, and conflict payloads must also remain credential-free.

If the product requirement is instead to display and edit every credential literally inside the WebView, that requires an explicit change to `AGENTS.md` and the parent privacy acceptance criteria before implementation. It cannot be silently inferred from “directly browse/edit config files.”

The existing default-config switch should migrate onto the same protected document/write transaction so it stops returning full API keys to TypeScript while retaining its current preview-and-confirm behavior.

### 9. Editing UX and conflict behavior

The direct entry belongs in an Agent management workspace, not behind a Provider card:

```text
Agent settings/item
  -> Agent detail
     -> choose runtime environment / installation
     -> tabs: overview | configuration | sessions | ecosystem (future)
     -> config file list + one editor surface
```

The configuration tab should show scope, environment, actual path, format, modified time, validation state, and dirty/conflict state. It should not add one modal per file.

Use a maintained editor component such as CodeMirror 6 for syntax-aware text editing, search, line numbers, undo, keyboard behavior, and accessible scrolling. The current hand-built `contenteditable` LCS diff may remain temporarily for the specialized switch preview, but it should ultimately reuse the same editor/diff foundation rather than become a second general editor.

Save flow:

1. User edits an in-memory draft; no file is written on each keystroke.
2. Background validation is debounced and request-ID scoped.
3. Save sends opaque file ID, expected revision, and edited credential-free projection.
4. Rust re-resolves, re-reads, compares revision, restores protected secret spans, validates syntax and Agent semantics, and atomically replaces the file.
5. On conflict, never overwrite. Return current revision plus a credential-free diff basis; offer “reload and discard draft” or “compare changes.” A force overwrite must be a separate explicit action, if supported at all.
6. On success, replace the draft/base revision with the returned document.

Opening another file, closing the view, changing runtime environment, or exiting with a dirty draft requires a discard confirmation. External change events must never silently replace a dirty buffer.

### 10. File monitoring

Near-real-time monitoring is feasible, but “watch every Agent directory recursively forever” is the wrong shape.

- Native macOS/Linux/Windows: use `notify::RecommendedWatcher` only while the Agent management surface or another real subscriber is active. Watch the allow-listed files or their immediate parent directories, debounce rename/write bursts, then re-read and emit a new Rust revision.
- WSL via UNC/network transport: native events cannot be assumed. The `notify` crate documents that network-mounted filesystems may not emit events and recommends `PollWatcher`; it also notes that editors may truncate or replace a file on save. Use bounded polling with content comparison for currently visible/managed WSL files.
- Do not recursively watch session histories, skill trees, plugin caches, or package-manager directories through the config watcher. Those need inventory-specific refresh rules.
- Coalesce events by `(environment_id, installation_id, file_id)` and suppress a write event only after matching the exact revision returned by BalanceHub's own save.
- If a clean file changes, refresh it. If a dirty file changes, set `conflicted=true` and retain the draft.

Suggested active-view polling fallback is 1-2 seconds for the open WSL document and 5-10 seconds for the visible manifest. No polling is needed after the last subscriber closes. The UI must label this as “实时监听” only where the environment reports native watch capability; WSL polling should be described as “定期检测变更.”

### 11. Phased MVP and dependencies

#### Phase 0: Contract/privacy correction

- Split Agent product identity from runtime installation/environment identity.
- Introduce opaque config file IDs, per-file SHA-256 revisions, and Agent-owned config manifests.
- Move default-config switching onto a credential-free config repository contract.
- Preserve current native behavior and migrate existing preferred paths to the native environment.

Acceptance: no plaintext secret appears in config IPC snapshots, diffs, errors, or tests; current native default-config switches still validate and write atomically.

#### Phase 1: Native Agent configuration center

- Add Agent detail/configuration entry from settings.
- Browse and edit allow-listed native-host configuration files.
- Add syntax/semantic validation, dirty-state handling, optimistic conflict rejection, atomic writes, and targeted native file watching.
- Replace the bespoke general-editing ambition of `CliConfigPreviewModal` with a shared editor foundation; keep the existing Provider-switch workflow intact.

Acceptance: macOS, Linux, and native Windows can edit non-sensitive config and safely preserve protected fields; external editor changes produce reload/conflict states without lost writes.

#### Phase 2: WSL discovery and read-only projection

- Discover installed/running distributions without starting stopped ones.
- Explicitly probe a selected stopped distribution.
- Return separate Agent installations, versions, config manifests, and session roots per distribution/default user.
- Add RuntimePath-aware workspace/session lookup.

Acceptance: native Windows and each WSL distribution appear independently; identical Agent kinds and session IDs do not collide; no write/launch is falsely advertised.

#### Phase 3: WSL config write and launch/resume

- Add WSL-side permission-preserving atomic write transport.
- Add Windows Terminal -> selected WSL distribution launch adapter.
- Restore WSL login-shell environment and pass Provider proxy/Agent environment through a fixed payload contract.
- Track host and guest process identities/status separately.

Acceptance: new and resumed sessions run in the chosen distribution/workdir with the chosen Agent installation; config writes retain Linux ownership/mode; timeout and failure always release UI/background state.

#### Phase 4: Ecosystem and external-run observation

- Build skills/plugins/MCP/hooks/statusline inventories on `environment_id + installation_id`.
- Add hook installation/enablement only for Agents that expose a supported hook contract.
- Let hook events register externally launched Agent runs through a versioned, authenticated/local-only event protocol.

Dependency: this must follow the environment identity work. A hook report without environment identity would recreate the same native/WSL ambiguity.

### 12. Cross-platform validation matrix

| Scenario | Discovery/version | Config read/edit | Session lookup | Launch/resume | Watch/conflict | Required validation |
|---|---|---|---|---|---|---|
| macOS native | Existing + refactored | Phase 1 | Existing | Existing | FSEvents native | Unit tests plus real Tauri smoke test; preserve login-shell aliases/functions. |
| Linux native X11/Wayland | Existing + refactored | Phase 1 | Existing | Existing terminal fallback | inotify native | CI build/test plus real desktop smoke for at least GNOME/KDE fallback. |
| Windows x64/ARM64 native | Existing + refactored | Phase 1 | Existing | Existing Windows Terminal/cmd/PowerShell | ReadDirectoryChangesW native | CI plus real terminal/profile tests; paths with spaces, Unicode, `.cmd`, `.exe`. |
| Windows with no WSL | Report unsupported, no error spam | Native only | Native only | Native only | Native only | `wsl.exe` absent/disabled and command timeout behavior. |
| Windows + stopped WSL distro | List but do not start by default | unavailable until explicit probe | unavailable | unavailable | none | Verify passive scan has no distro-start side effect. |
| Windows + running WSL 1 | Separate environment/installations | Phase 2 read, Phase 3 write | Phase 2 | Phase 3 | bounded polling | Real machine/self-hosted validation; Linux PID and filesystem semantics. |
| Windows + running WSL 2 | Separate environment/installations | Phase 2 read, Phase 3 write | Phase 2 | Phase 3 | bounded polling | Real machine/self-hosted validation; UNC/network watcher fallback. |
| Multiple distributions | No merged Agent rows | Isolated manifests/revisions | Isolated histories | Explicit target | Isolated events | Ubuntu + Debian fixture or real hosts; same Agent installed in both. |
| Windows-mounted WSL workdir | Explicit `wslpath` conversion | Environment-scoped | Workdir match after conversion | Explicit WSL path | n/a | Spaces, Unicode, different drive letters. |
| WSL-native workdir | WSL path canonical | WSL ownership/mode | WSL-native history | Windows Terminal -> WSL | polling | Keep project under `/home`; never canonicalize as a Windows local path. |
| External edit while clean | n/a | Auto reload | n/a | n/a | revision event | Atomic replace, truncate/write, rename-save patterns. |
| External edit while dirty | n/a | Preserve draft, mark conflict | n/a | n/a | no silent overwrite | Same-length/same-mtime edge fixture plus content hash. |
| Invalid config | n/a | Reject before write | n/a | no launch mutation | stable error | JSON/TOML/.env and Agent semantic failures. |
| Sensitive config | n/a | Credential-free projection | n/a | secrets resolved only in Rust | no secret in event | Serialize every IPC/task/error payload and assert known secrets absent. |

The existing GitHub Actions matrix can validate native compilation and deterministic unit tests. It cannot by itself prove WSL terminal, distribution startup, UNC, Linux permission, or guest process behavior. Those checks require a Windows self-hosted runner with WSL 1/2 fixtures or a documented manual release gate. WSL-dependent behavior must not be marked supported solely because `windows-latest` compiles.

## External References

- Microsoft, “Basic commands for WSL”: `https://learn.microsoft.com/windows/wsl/basic-commands` — documents `wsl --list --verbose`, distribution targeting, user targeting, status, and version commands.
- Microsoft, “Working across Windows and Linux file systems”: `https://learn.microsoft.com/windows/wsl/filesystems` — documents `\\wsl$`, `/mnt/<drive>`, cross-environment path behavior, performance guidance, and the fact that `wsl.exe` forwards command arguments without converting paths.
- `notify` crate 8.2.0 documentation: `https://docs.rs/notify/latest/notify/` — documents cross-platform native watchers, network-filesystem event limitations, `PollWatcher`, and editor truncate/replace save behavior.

## Related Specs

- `.trellis/spec/frontend/component-guidelines.md`: bounded components, semantic events, stable scalar IDs, and no backend capability duplication.
- `.trellis/spec/frontend/state-management.md`: Rust service state as truth, explicit persisted/derived/transient state, and stale-response rejection.
- `.trellis/spec/guides/agent-routing.md`: this cross-layer architecture/research task belongs to `gpt-5.6-sol/max`; deterministic implementation follows only after design review.
- `.trellis/spec/guides/cross-layer-thinking-guide.md`: define source, transport, validation owner, and typed IPC contracts before implementation.
- Repository `AGENTS.md`: dynamic Agent/terminal discovery, Rust-owned capability decisions, platform script variable rules, async timeout/cancellation, three-platform verification, and no plaintext credentials in IPC/config previews.

## Caveats / Not Found

- No WSL runtime implementation or WSL-specific tests were found in product source. The conclusion is source-backed; no real Windows/WSL runtime was available in this macOS research session.
- The exact official configuration universe for every current/future Agent was not exhaustively researched here. Current code only knows the default-switch files listed above. Skills, plugins, MCP, hooks, statusline, project-local config, and system config need Agent-specific official-document research before their manifests are frozen.
- WSL distribution names are available from `wsl.exe`, but selecting non-default users and mapping all users' homes requires an explicit product scope and permission model. The proposed MVP intentionally uses the default user.
- Filesystem event behavior through `\\wsl.localhost` is not guaranteed. The recommendation intentionally uses bounded polling as the reliable fallback rather than claiming native real-time events.
- No cross-process API can guarantee zero race with an unrelated editor that ignores locks. Content revisions, a final re-read, atomic replacement, and explicit conflict UX provide optimistic safety; they are not a universal lock.
- The current rule against plaintext credentials conflicts with a literal “edit every byte in the WebView” interpretation. Planning must resolve that explicitly before implementation; this research follows the current repository rule.
