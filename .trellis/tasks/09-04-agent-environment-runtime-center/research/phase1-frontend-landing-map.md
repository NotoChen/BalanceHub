# Research: Phase 1 Agent Environment Center Frontend Landing

- Query: Map the minimum frontend landing for the Phase 1 native Agent environment and read-only inventory center, including current settings navigation, type/store/composable boundaries, reusable UI, IPC calls, state handling, configuration preview, version display, and Rust-contract dependencies.
- Scope: internal
- Date: 2026-09-04

## Findings

### 1. Recommended product landing

Phase 1 should live inside the existing **Agent 与终端** settings section. It should not create a top-bar icon, a new top-level settings category, or a marketing-style page.

Use a two-level work surface inside the current settings content:

```text
应用设置
  -> Agent 与终端
     -> Agent installation overview
        -> selected Agent installation detail
           Overview | Assets | Configuration
     -> Terminal
     -> Session index
     -> Liveness
     -> Prompt settings
```

The installation overview replaces the current passive four-card detector as the first card of `SettingsAgentSection.vue`. Selecting an installed Agent replaces that card with a focused detail card in the same content column; the existing Terminal, Session index, Liveness, and prompt cards remain below it. The detail card provides a visible back action and preserves the settings section/scroll context.

This is the smallest useful landing because:

- `SettingsDrawer.vue:20`, `48-72`, `115-169` already owns top-level settings navigation and has a stable `agent` section.
- The settings content width is constrained to 820 px by `src/styles/modules/surfaces/settings-editor.css:2-9`, `232-247`, so a separate permanent inner sidebar would consume too much space.
- The current section is already scrollable and uses full-width cards; an overview/detail replacement fits without changing the 1020 px settings modal contract.
- A large nested modal for the entire Agent center would duplicate the settings navigation. Only bounded file content may need its own fixed-height viewer inside the detail surface.

Do not put Assets, MCP, Hooks, Status UI, and Configuration into separate top-level navigation entries. They are properties of a selected installation/environment, not global App destinations.

### 2. Current frontend call chain

#### Settings navigation

- `src/components/SettingsDrawer.vue:20`: `SettingsSectionKey` contains `agent` as one of five top-level sections.
- `src/components/SettingsDrawer.vue:48-72`: the label is already `Agent 与终端` and uses `IconCommand`.
- `src/components/SettingsDrawer.vue:76-83`: reopening settings resets to `appearance`; Phase 1 local detail state will also reset because the modal uses `unmount-on-close` at lines 98-104.
- `src/components/SettingsDrawer.vue:133-188`: all settings pages render inside one vertical `<a-form>`. Read-only inventory actions must not mutate the autosaved `AppSettings` draft.
- `src/components/SettingsDrawer.vue:163-169`: `SettingsAgentSection` is the only correct insertion point.

No `SettingsDrawer` navigation change is needed for Phase 1.

#### Current Agent section

- `src/components/settings/SettingsAgentSection.vue:147-155`: the first settings card is the Agent detector and delegates to `SettingsCliManager`.
- `src/components/settings/SettingsAgentSection.vue:157-320`: Terminal, session index, liveness, and prompt controls already make this component an orchestration surface. New inventory request state should not be added directly to this file.
- `src/components/settings/SettingsAgentSection.vue:20-139`: it currently owns only session-index workflow state and event cleanup; adding environment, asset, version, and preview workflows here would make it materially heavier.

`SettingsAgentSection.vue` should only embed the new center component and continue composing the other settings cards.

#### Current CLI detector

- `src/components/settings/SettingsCliManager.vue:21-24`: reads `cliEnvironmentProbe` from `useCliRuntimeStore` and derives registered/available Agents.
- `src/components/settings/SettingsCliManager.vue:26-43`: maps only loading/available/error and local installed version/path tooltip.
- `src/components/settings/SettingsCliManager.vue:46-55`: manually probes and preserves concurrent `AppSettings` path edits.
- `src/components/settings/SettingsCliManager.vue:59-99`: renders a passive detection grid with one refresh button.
- `src/components/settings/SettingsDetectionItem.vue:2-8`, `11-25`: the item accepts only `checking|ok|error` and renders a non-interactive `<article>`.
- `src/components/settings/SettingsDetectionGrid.vue:19-41`: the responsive four-item grid can be reused for the overview layout, but its item component cannot represent installation/version states or keyboard navigation.

The current detector should be replaced by a focused `SettingsAgentEnvironmentCenter.vue`; do not keep `SettingsCliManager.vue` as a compatibility wrapper after cutover. The new overview can reuse `SettingsDetectionGrid` only if a new interactive installation item is introduced. It should not mutate `SettingsDetectionItem` into a domain-heavy component used for unrelated detection surfaces.

#### Store and API

- `src/stores/cli-runtime.ts:34-42`: `useCliRuntimeStore` currently mixes runtime instances, CLI detection, and terminal detection.
- `src/stores/cli-runtime.ts:47-65`: CLI/terminal probe actions correctly release loading in `finally`, but there is one global CLI loading boolean.
- `src/api/app.ts:185-190`: current CLI and terminal probes are thin typed Tauri wrappers.
- `src/stores/provider-types.ts:323-359`: current `AgentCliDescriptor`, capabilities, and probe types describe one result per Agent kind.
- `src/utils/cli-environment.ts:27-85`: current selectors are useful for legacy launch/liveness flows but are keyed by Agent kind, not installation ID.
- `src/composables/useSettingsController.ts:147-173`: settings startup/manual probe behavior is intentionally quiet, preserves draft changes, and already separates shallow startup discovery from deep manual scan.

Do not add environment inventory, version network status, asset lists, and config previews to `useCliRuntimeStore`. The later runtime cutover is expected to remove legacy temporary-CLI public state, while the environment inventory is a separate durable domain.

### 3. Required frontend type boundary

Repository rules currently place IPC-facing frontend types in `src/stores/provider-types.ts`; Phase 1 should add Rust-shaped receive types there and continue re-exporting them through `src/stores/providers.ts:50-51`. UI-only state remains local to the composable/components.

Minimum Rust-owned receive shapes:

```text
AgentEnvironmentSnapshot
  revision
  observedAt
  environments[]
  installations[]

AgentEnvironmentDescriptor
  id
  kind
  hostPlatform
  guestPlatform?
  displayName
  capabilities

AgentInstallation
  id
  environmentId
  agentKind
  label
  executablePath
  available
  installedVersion?
  discoverySource
  channel
  versionStatus

AgentVersionStatus
  state                 // current | updateAvailable | ahead | unknown
  installedVersion?
  latestVersion?
  channel
  sourceLabel?
  checkedAt?
  lastSuccessAt?
  error?

AgentInstallationInventory
  installationId
  workspacePath?
  revision
  summary
  assets[]
  configFiles[]
  diagnostics[]

AgentAssetRecord
  id
  category              // skill | plugin | mcp | hook | statusUi
  nativeId
  label
  source
  declaredState
  effectiveState
  trustState?
  override?
  pathDisplay?
  diagnostics[]

AgentConfigFile
  fileId                // opaque ID, never a caller-provided path
  label
  scope
  category
  format
  pathDisplay
  exists
  writable
  sizeBytes
  modifiedAt?
  revision
  sensitivity
  readCapability
  openCapability
  watchCapability

AgentConfigPreview
  fileId
  revision
  format
  content?
  truncated
  totalBytes
  diagnostics[]
```

Do not reuse or extend `CliToolProbeResult` to pretend it is an installation. It has no environment ID, installation ID, source, channel, latest-version fact, or asset capability, and its list currently represents registered Agent kinds even when unavailable. Do not reuse `CliConfigFile`/`CliConfigPreview` from `provider-types.ts:544-569`; those are Provider-switch editable diff contracts and carry different semantics.

Frontend code should render Rust-owned enum values through small exhaustive label/style maps. It must not infer effective state from path existence or derive update availability with its own semver comparison.

### 4. Store and composable split

#### New Pinia store: `src/stores/agent-environment.ts`

The store owns shared backend facts and command actions, not navigation or presentation:

```text
state
  snapshot
  snapshotState          idle | loading | refreshing | ready | error
  snapshotError
  inventoryByKey         installationId + workspacePath
  inventoryStateByKey
  inventoryErrorByKey

actions
  loadSnapshot(options)
  loadInventory(installationId, workspacePath?, forceRefresh?)
  refreshLatestVersions(installationIds?)
  readConfigPreview(fileId)
  openConfigFile(fileId)
  openConfigDirectory(fileId)
  clearTransientState()
```

The store should preserve the last successful snapshot/inventory while a refresh runs or fails. Rust owns the six-hour version cache and in-flight request coalescing; Pinia must not implement a second TTL/cache policy.

The initial call may bridge legacy discovery internally on the Rust side, but the frontend must consume only the new environment contract. Existing `useCliRuntimeStore.cliEnvironmentProbe` remains temporarily for launch/liveness/default-config consumers until their planned migration; the new store must not mirror or mutate it.

#### New composable: `src/composables/useAgentEnvironmentCenter.ts`

The composable owns workflow-local UI state:

```text
selectedInstallationId
selectedWorkspacePath
activeDetailTab          overview | assets | configuration
assetCategory
assetQuery
selectedConfigFileId
configPreview
configPreviewState/error
copiedPathId

openInstallation(id)
closeInstallation()
selectWorkspace(path?)
selectConfig(fileId)
refreshSelected()
openFile(fileId)
openDirectory(fileId)
copyPath(displayPath)
```

Use independent monotonically increasing request IDs for snapshot, selected inventory, and config preview. Selecting another installation/file or unmounting invalidates the previous request. A late result must not reopen or overwrite the selected detail. Follow the scalar request-ID patterns in `src/composables/useWorkspaceLaunchFlow.ts:172-194`, `useWorkspaceApiKeySelection.ts:23-79`, and `useWorkspaceSessionHistory.ts:52-150`.

The composable may use `src/composables/useClipboard.ts:1-29` for path copying. Opening file/directory must call the new Rust allow-listed API; it must not pass `pathDisplay` to a generic opener.

### 5. Component split

Recommended components and ownership:

| File | Responsibility | Inputs/events | Reuse |
|---|---|---|---|
| `src/components/settings/SettingsAgentEnvironmentCenter.vue` | Feature container; switches overview/detail and binds the composable/store. | `settings` only if legacy manual path adoption remains; otherwise no AppSettings prop. | Existing settings card/header styles and `AgentCliIcon`. |
| `src/components/settings/agent-environment/AgentInstallationGrid.vue` | Overview grid, initial/refresh/empty/error presentation, one semantic button per installation. | snapshot, status; emits select/refresh. | Grid dimensions from `SettingsDetectionGrid`; do not reuse passive item semantics. |
| `src/components/settings/agent-environment/AgentInstallationItem.vue` | Agent icon, environment/source, installed/latest status, diagnostics, accessible selection. | one `AgentInstallation`; emits select. | `AgentCliIcon`, stable card dimensions, tooltips for paths/errors. |
| `src/components/settings/agent-environment/AgentInstallationDetail.vue` | Back action, identity/version summary, tab selector, workspace scope control, refresh. | selected installation/inventory/state; semantic emits only. | Arco segmented/tabs, no nested card layout. |
| `src/components/settings/agent-environment/AgentAssetInventory.vue` | Search/category filter and dense asset rows with scope/effective/trust/override/diagnostic facts. | Rust records/capabilities. | Existing search/input/tag/icon primitives; no mutation switches in Phase 1. |
| `src/components/settings/agent-environment/AgentConfigBrowser.vue` | Config manifest list, file metadata/actions, and fixed-height bounded read-only preview. | config files, preview, independent preview status; emits select/open/copy. | `useClipboard`; visual idea of file tabs from `CliConfigPreviewModal`, not its editable diff implementation. |
| `src/components/settings/agent-environment/AgentVersionStatus.vue` | Consistent installed/latest/channel/source/time/error rendering in overview and detail. | one Rust `AgentVersionStatus`. | Pure presentational component; no semver logic. |

Do not create generic `manager`, `helpers`, or `utils` modules. Domain-specific label maps may live next to `useAgentEnvironmentCenter.ts` or inside the pure component using them.

`SettingsAgentSection.vue` changes only its first block: import `SettingsAgentEnvironmentCenter`, remove the old outer Agent card/`SettingsCliManager`, and leave existing Terminal/session/liveness/prompt sections intact. Once replaced, delete `SettingsCliManager.vue`; keeping both would leave two Agent overview truths.

### 6. Layout and interaction details

#### Overview

- Keep four current Agent icons from `src/agent-cli/visuals.ts:5-26` through `AgentCliIcon.vue:1-33`; labels and capability text still come from Rust.
- Each installation item is a real `<button type="button">` or an accessible button surface, not a click handler on `<article>`.
- Stable minimum height prevents version/loading text from changing grid geometry.
- Primary line: official Agent label. Secondary line: native environment plus installation source, for example `本机 · npm`.
- Version line: installed version, then concise comparison state. Full path/source/check time belongs in tooltip/detail, not the small card.
- If the same Agent has multiple installations in a future environment, render separate cards keyed by `installation.id`; never key by `agentKind`.

#### Detail

- Header contains back icon, official Agent icon/name, environment/source, and one refresh icon button.
- Use a compact segmented control or tabs for `概览 / 资产 / 配置`; do not add explanatory subtitles.
- Project/workspace scope is optional. Default shows global/user/system sources. A compact workspace selector enables project/local sources and triggers a separately keyed inventory request.
- Asset rows are dense, full-width list rows, not cards inside the detail card. Each row shows category/name, scope/source, effective state, and only meaningful diagnostics.
- Phase 1 must not render toggle controls for assets. If Rust returns future mutation capabilities, ignore them until the controlled-mutation phase is authorized.

#### Configuration

- Left/file header area lists allow-listed files by scope/category; the selected file preview uses a stable `min-height`/`max-height` with its own scrolling.
- Actions are familiar icons with tooltips: open file, reveal directory, copy path. Disabled actions use Rust capability reasons.
- Show actual display path, format, scope, modified time, size, and truncation state.
- A nonexistent declared file is not an error; show `尚未创建` and the metadata/actions that remain supported.
- A secret-only file with no preview content shows `此文件仅提供位置与元数据` rather than a blank editor.
- Do not reuse `CliConfigPreviewModal.vue:418-516`: that surface is an editable Provider switch diff with save semantics and full file contents. Only its file-tab/code-view visual lessons can inform the read-only browser.

### 7. Loading, error, empty, and stale states

Use explicit state machines rather than one global boolean.

| Surface | Initial loading | Refreshing with data | Error with no data | Error with previous data | Empty/unsupported |
|---|---|---|---|---|---|
| Environment overview | Fixed-size skeleton rows/cards; no `0 available` flash | Keep cards visible; animate only the refresh glyph and mark facts refreshing | Inline state with retry inside the Agent card | Keep data and show non-blocking `刷新失败` with last observation time | `未检测到 Agent 安装`; distinguish native environment discovery success from command failure |
| Installation inventory | Detail header stays closable/back-enabled; skeleton only in content | Preserve selected tab/list and mark refresh in header | Inline retry; back remains enabled | Preserve facts and show stale/error banner | Supported category with zero records: `未发现`; unsupported: `此安装不支持`; never conflate the two |
| Version | Installed version remains visible | Latest value remains visible with small checking state | `最新版本未知` | Keep last successful latest/time and show this check failed | Not applicable/source unknown is a typed state, not `0.0.0` |
| Config preview | Fixed-height code skeleton | Keep old preview until matching new result arrives | File-scoped retry; manifest stays usable | Keep old content labeled stale only if Rust permits | Missing file, metadata-only secret file, and unsupported preview each have distinct messages |

All `finally` paths must release the matching busy state. Refresh must not disable the entire settings modal or Agent detail. The back action, settings close, other settings navigation, and unrelated Terminal/session controls remain usable during network/file work.

No generic toast is needed for expected asset/category failures. Show contextual errors in the affected surface. Use a toast only when a direct user command such as open/copy fails and no persistent inline state can explain it.

### 8. Version presentation

Rust must return the comparison state. Frontend presentation:

- `current`: installed and latest stable match; neutral/green `已是最新`.
- `updateAvailable`: show `installed -> latest` and a restrained update badge; no update button in Phase 1.
- `ahead`: show `当前版本高于稳定版`, typically prerelease/channel context rather than an error.
- `unknown`: keep installed version and show `最新版本未知`.
- `checking`: preserve the last successful value and animate only the loading glyph.

Always show channel, source, and last successful check time in detail. If the latest check fails after a successful check, preserve the last success and show the new failure separately. Do not derive `current` from the absence of an error.

Current `agentCliVersionLabel` at `src/utils/cli-environment.ts:39-44` remains useful for legacy display parsing, but the new contract should deliver normalized installed/latest display versions from Rust so frontend regex parsing does not become the version authority.

### 9. Proposed IPC calls

The exact Rust command names must be frozen by the Rust implementation, but the frontend needs these distinct semantics:

| Proposed API wrapper in `src/api/app.ts` | Input | Output | Why separate |
|---|---|---|---|
| `getAgentEnvironmentSnapshot` | `{ deep: boolean }` | `AgentEnvironmentSnapshot` | Local environment/installation discovery without forcing remote latest checks. |
| `getAgentInstallationInventory` | `{ installationId, workspacePath?, forceRefresh }` | `AgentInstallationInventory` | Asset/config scans are selected-installation and optional-workspace scoped. |
| `refreshAgentLatestVersions` | `{ installationIds? }` | updated version facts or snapshot revision | Remote network work has independent loading/cache/failure semantics. |
| `readAgentConfigPreview` | `{ fileId }` | `AgentConfigPreview` | Opaque allow-listed file resolution and bounded content. |
| `openAgentConfigFile` | `{ fileId }` | `void` | Rust re-resolves capability/path; frontend cannot open arbitrary paths. |
| `openAgentConfigDirectory` | `{ fileId }` | `void` | Same allow-list boundary for reveal/open-parent behavior. |

The calls belong in `src/api/app.ts`, then are wrapped by `useAgentEnvironmentStore`; components must not call `invoke` directly. Local scans/file I/O must use Rust `run_blocking`; latest-version checks must reuse the shared Rust network layer, timeout, in-flight coalescing, and six-hour cache from the task design.

No file watcher IPC is required for the first visual slice. If active-view monitoring is included in Phase 1, add a Rust event with installation/file/revision IDs and refresh the matching cached record; do not send file content in events. Listener setup/cleanup belongs in `useAgentEnvironmentCenter`, following the disposal pattern in `SettingsAgentSection.vue:106-139`.

### 10. Existing reuse points

- `src/components/AgentCliIcon.vue` and `src/agent-cli/visuals.ts`: official icon rendering only; Agent labels/capabilities stay Rust-owned.
- `src/components/settings/SettingsDetectionGrid.vue`: responsive grid dimensions can be reused if semantics remain generic.
- `src/composables/useClipboard.ts`: copy display paths with existing WebView fallback.
- `src/utils/cli-environment.ts:106-173`: snapshot-before-scan pattern protects concurrent settings path edits; retain for the legacy preferred-path side effect until migration.
- `src/composables/useWorkspaceLaunchFlow.ts`, `useWorkspaceApiKeySelection.ts`, `useWorkspaceSessionHistory.ts`: request-ID stale-result guards.
- `src/components/SettingsDrawer.vue` and `src/styles/modules/surfaces/settings-editor.css`: existing settings navigation, width, scroll, focus, and responsive shell.
- `src/styles/modules/surfaces/settings-content.css:2-97`: settings card/header/list-row tokens.
- `src/components/CliConfigPreviewModal.vue:455-505`: file-tab and scroll-region layout is a visual reference only; do not reuse editable diff/save state.
- `tests/agent-cli.test.ts`: current Node test style and Agent registry-order fixtures.
- `tests/ui-async-guard.test.ts`: project-wide checks for stale object-identity comparisons and modal locking.

### 11. Files to change in the implementation slice

Expected frontend ownership:

- Modify `src/stores/provider-types.ts`: add IPC receive contracts after Rust serde shapes are final.
- Modify `src/api/app.ts`: add typed wrappers for environment snapshot, inventory, versions, config preview/open calls.
- Add `src/stores/agent-environment.ts`: cache backend facts and expose actions.
- Add `src/composables/useAgentEnvironmentCenter.ts`: selection, tabs, workspace, preview, request IDs, and cleanup.
- Add `src/components/settings/SettingsAgentEnvironmentCenter.vue`.
- Add `src/components/settings/agent-environment/AgentInstallationGrid.vue`.
- Add `src/components/settings/agent-environment/AgentInstallationItem.vue`.
- Add `src/components/settings/agent-environment/AgentInstallationDetail.vue`.
- Add `src/components/settings/agent-environment/AgentAssetInventory.vue`.
- Add `src/components/settings/agent-environment/AgentConfigBrowser.vue`.
- Add `src/components/settings/agent-environment/AgentVersionStatus.vue`.
- Modify `src/components/settings/SettingsAgentSection.vue`: replace only the first Agent detector block.
- Delete `src/components/settings/SettingsCliManager.vue` after the new overview works.
- Add `src/styles/modules/agent-environment.css` and import it once from `src/styles/app.css`; keep dark/responsive rules in the feature stylesheet unless an existing shared token suffices.
- Modify/add Node tests under `tests/agent-environment*.test.ts`; retain `tests/agent-cli.test.ts` for legacy launch/probe selectors until the later cutover.

Do not modify `SettingsDrawer.vue`, top-bar components, `useAppController.ts`, Provider cards, or runtime-instance UI in this Phase 1 frontend slice.

### 12. Must wait for the Rust contract

The following must not be mocked or inferred in production frontend code:

- environment and installation stable IDs;
- multiple installations of the same Agent;
- installation source and channel;
- normalized installed/latest versions and comparison state;
- latest source, last-success time, stale/error state, and cache semantics;
- supported asset categories and scan scope;
- declared versus effective state, precedence/shadowing, trust, and diagnostics;
- config file opaque IDs, allow-list, symlink boundary, size limit, sensitivity, and supported actions;
- bounded/redacted configuration content;
- open-file/open-directory authorization;
- project/workspace scope resolution;
- filesystem watcher revisions/events.

Before Rust lands, an implementation Agent may safely build pure presentational components against checked-in typed fixtures **only if** the fixture shape is already frozen in Rust design and the components are not wired into production navigation. The production center should not replace `SettingsCliManager` until the environment snapshot and inventory commands exist.

### 13. Direct implementation task list

Dependencies are strict; later items should not start by guessing earlier contracts.

1. **Freeze Rust serde and IPC names.** Finalize environment, installation, version, asset, config manifest, preview, and typed state enums. Define command inputs/outputs and error semantics. Frontend work beyond visual fixtures waits here.
2. **Add frontend receive types and API wrappers.** Mirror the Rust contract in `provider-types.ts`; add thin `src/api/app.ts` functions. No semver/effective-state logic in TypeScript.
3. **Implement `useAgentEnvironmentStore`.** Preserve last successful data, separate initial loading from refreshing, separate local inventory from remote version checks, and release every state in `finally`.
4. **Implement `useAgentEnvironmentCenter`.** Add scalar request IDs, selected installation/workspace/tab/category/file, stale-result rejection, unmount invalidation, contextual open/copy actions, and no modal lock.
5. **Build installation overview.** Add accessible installation cards with stable sizing, Agent icons, local version, version state, source/environment, refresh, initial/error/empty states, and keys by installation ID.
6. **Build focused installation detail.** Add back action, identity/version summary, tabs, optional workspace scope, preserved refresh data, and no asset mutation controls.
7. **Build asset inventory.** Add one input search, category filter, dense rows, source/scope/effective/trust/override/diagnostic display, plus distinct unsupported/empty/error states.
8. **Build config browser.** Add allow-listed manifest, metadata, fixed-height read-only preview, truncation/secret/missing states, copy path, open file, and reveal directory. Never submit display paths to open/read commands.
9. **Cut over the first Agent card.** Replace `SettingsCliManager` inside `SettingsAgentSection`, keep all other settings cards unchanged, then delete the old component. Do not change top-level settings navigation.
10. **Add regression tests.** Cover initial/refresh/error-with-stale/empty/unsupported states; request-ID races between installations/files; close/back while pending; independent version and preview loading; no whole-settings disable; opaque-ID-only open/read; dynamic fifth-Agent fixture; long path/name; dark/narrow layout; and zero plaintext secret in rendered/serialized fixtures.
11. **Verify.** Run `npm run build`, `npm test`, relevant Rust tests from the contract implementation, and `git diff --check`; visually inspect the real Tauri settings surface at desktop and narrow widths in light/dark mode.

## Related Specs

- `.trellis/spec/frontend/directory-structure.md`: keep components bounded, workflows in composables, and Rust as capability authority.
- `.trellis/spec/frontend/hook-guidelines.md`: explicit async state, `finally` cleanup, request-ID/revision stale guards, and no duplicated component invokes.
- `.trellis/spec/frontend/type-safety.md`: Rust serde models are authoritative; use discriminated unions and stable scalar IDs.
- `.trellis/spec/frontend/component-guidelines.md`: semantic component events, accessibility, stable modal behavior, and no backend rule duplication.
- `.trellis/spec/frontend/state-management.md`: Pinia holds shared backend state; composables own workflow-local state.
- `.trellis/spec/frontend/quality-guidelines.md`: delete replaced entry points, run frontend/Rust gates, and test async release behavior.
- `.trellis/tasks/09-04-agent-environment-runtime-center/prd.md`: Phase 1 inventory, version, config allow-list, and no generic mutation boundary.
- `.trellis/tasks/09-04-agent-environment-runtime-center/design.md`: environment/installation identity, version cache, configuration privacy, and later unified runtime design.

## Caveats / Not Found

- The Phase 1 Rust models and commands do not exist yet, so command names and exact field names above are recommendations, not confirmed source contracts.
- Current `CliEnvironmentProbeResult` cannot represent multiple installations or WSL and must not be promoted as the final environment snapshot.
- The current settings section has no workspace selector for project-scoped assets. Product implementation must either add the compact optional selector described above or explicitly narrow the first release to global scopes; it must not silently claim a complete project inventory.
- Existing frontend tests are primarily Node source/unit tests rather than mounted Vue component tests. The new async UI needs either focused component-test infrastructure or extracted pure state reducers/selectors that can be tested without brittle source-string assertions.
- `CliConfigPreviewModal` currently has editable Provider-switch semantics and is not a safe general read-only config browser.
- File open/reveal cannot be safely implemented with the current generic frontend-visible path alone. It must wait for Rust opaque-ID resolution.
- The current repository privacy rule requires sensitive configuration to be redacted or metadata-only over IPC; the frontend cannot provide a literal auth/token editor without a separate approved rule change.
