# Research: Agent Environment List Console Design

- Query: Replace the current drill-down-first Agent environment UI with a single-screen, registry-driven list that exposes installation/version facts, Hook status, enablement and common actions directly; keep low-frequency assets/configuration in detail.
- Scope: mixed
- Date: 2026-09-04

## Findings

### Product conclusion

The current interaction is structurally wrong for a Hook control center. It makes every installation card a navigation target, then hides the only managed capability behind the detail view. The default surface should instead be a scan-friendly list of all registered Agents, with one row per Agent/runtime scope and direct Hook controls. Detail remains available, but only for low-frequency installation evidence, assets, configuration preview and diagnostics.

This is also closer to the most relevant Dynamic Island implementation found. AgentBro separates its live island from an Integration settings surface and renders every detected tool in one list with status plus direct configure/open/install/reinstall/uninstall actions. It also exposes detect/install-all/uninstall-all at the list level. Atoll uses a still simpler one-command install/remove path. These projects are not identical and should not be copied mechanically, but neither requires entering each Agent detail merely to discover or activate the primary Hook integration.

The runtime island and Hook administration are different surfaces:

- The island answers "what are my sessions doing now?"
- The environment console answers "which Agents are installed and which integrations are active?"
- The detail view answers "what files/assets/evidence belong to this Agent?"

### Current interaction and structural problems

1. `SettingsAgentEnvironmentCenter` switches the whole body between `AgentInstallationDetail` and `AgentInstallationGrid`; only one Agent can be understood at a time (`src/components/settings/SettingsAgentEnvironmentCenter.vue:153`, `src/components/settings/SettingsAgentEnvironmentCenter.vue:184`).
2. The grid only passes a `select` event (`src/components/settings/agent-environment/AgentInstallationGrid.vue:42`), and every installation is a single large button (`src/components/settings/agent-environment/AgentInstallationItem.vue:14`). It shows identity/version but no Hook state or action.
3. `AgentHookManager` is mounted only in the detail overview (`src/components/settings/agent-environment/AgentInstallationDetail.vue:85`, `src/components/settings/agent-environment/AgentInstallationDetail.vue:94`). Therefore status discovery, installation, enabling, disabling, verification, repair and removal all require drilling into each Agent.
4. Each `AgentHookManager` owns its own plan modal and lifecycle (`src/components/settings/agent-environment/AgentHookManager.vue:27`, `src/components/settings/agent-environment/AgentHookManager.vue:155`, `src/components/settings/agent-environment/AgentHookManager.vue:260`). Rendering this component unchanged in every list row would create duplicated modal state, concurrent mount requests and invalidation coupling instead of a real list controller.
5. `AgentHookManager` invalidates shared state on unmount (`src/components/settings/agent-environment/AgentHookManager.vue:160`). A list/detail transition could therefore discard state still required by the outer console.
6. The Hook store already has the correct per-Agent stale-response boundary: inspections, request IDs, busy states and errors are keyed by `AgentCliKind` (`src/stores/agent-hooks.ts:23`, `src/stores/agent-hooks.ts:31`). This state should be orchestrated once at page/list level rather than separately by each row.
7. The Rust-owned `AgentHookInspection` reports orthogonal facts but does not report action availability/reasons (`src/stores/provider-types.ts:528`). If the list derives permission rules from `installed`, `enabled`, CLI availability or state strings, it would recreate backend capability logic in Vue. The inspection contract should expose backend-computed action capabilities.
8. The permanent GUI PATH deep-scan block appears before the installation list (`src/components/settings/SettingsAgentEnvironmentCenter.vue:92`). It consumes the most valuable first-screen space even though it is an exceptional recovery action.
9. The card header still says `只读盘点` (`src/components/settings/SettingsAgentEnvironmentCenter.vue:82`) even though this surface now performs explicitly confirmed managed Hook writes. The label is misleading.
10. The workspace selector is labeled as the scope for the whole inventory (`src/components/settings/SettingsAgentEnvironmentCenter.vue:84`), while managed Hook operations are native user-scope operations keyed only by Agent kind. Workspace selection must not imply that the Hook target changes.

### Proposed single-screen information architecture

Default screen, in order:

1. Header: `Agent 环境`; trailing summary such as `3 可用 / 4 已注册`. Do not retain `只读盘点`.
2. Compact toolbar: asset workspace scope on the left; `检查版本`, `刷新`, and `深度扫描` icon actions on the right. Deep-scan results open in a closable secondary panel/modal and do not permanently push the Agent list below the fold.
3. Table-like Agent list with one continuous bordered surface and row separators, not a grid of decorative cards.
4. One shared Hook plan modal for whichever row initiated a write.
5. Explicit `详情` action opens the existing detail view/drawer for assets and configuration only.

Desktop row layout within the existing approximately 820 px settings content width:

```text
Agent                         安装 / 版本             会话 Hook                 操作
[icon] Claude Code            v1.2.3  有新版本        [运行正常]  [on switch]   [检查] [more]
       自动发现 · /path
```

Recommended CSS grid tracks:

```css
grid-template-columns:
  minmax(180px, 1.45fr)
  minmax(132px, 0.9fr)
  minmax(170px, 1.05fr)
  auto;
```

The path is secondary, one line, ellipsized with a tooltip. Installed version is the primary version fact; latest/check time appears only when useful. Hook state uses text plus a state glyph, never color alone. Row height should stay stable when diagnostics exist; diagnostics occupy a one-line subrow or an expandable inline message rather than changing unrelated row structure.

At widths below 620 px, remove the column header and make each row a compact two-column layout:

```text
[icon] Agent name             Hook state
installed/latest version      switch
diagnostic or path
[详情] [检查] [修复/安装]
```

Controls wrap onto a final row. They must not horizontally scroll, overlap, or collapse the Agent identity. Touch targets remain at least 32 px high. The whole row is not clickable; navigation is an explicit details icon/button so toggling or checking a Hook cannot accidentally open detail.

### Dynamic registry and grouping rule

The public list must be created only from `inventory.installations` and the Rust-provided capability/inspection results. New shared UI code must not contain `if agent === codex/claude/gemini/grok`, four literal rows, or a fixed label map.

The semantic row identity is `(agentKind, runtimeScope)`, because the Hook is installed per Agent/user runtime scope, not per executable path. A row contains `installations[]`; the currently discovered single installation renders normally, while future multiple installations render `N 个安装` with their individual paths/versions available in detail. This prevents the same Hook switch from being duplicated when multi-install discovery is completed.

The UI may continue to obtain icons from the existing visual registry (`src/agent-cli/visuals.ts:5`). Adding an Agent may require adding its visual asset, but must not require editing the console, row component, Hook state machine or action rendering.

Do not select an "effective" executable in Vue. If multiple installations require an authoritative primary/effective selection, Rust must add that fact to the inventory contract. Until then, show the installations as parallel facts and do not invent precedence from array order.

### Direct actions versus detail actions

Actions shown directly in each outer row:

| Condition/capability | Direct control | Behavior |
| --- | --- | --- |
| Hook not installed and install permitted | `安装 Hook` | Generate plan, show shared confirmation modal, then apply. |
| Hook installed | enable switch | Switch represents only enabled/disabled. Toggling generates an enable/disable plan and does not optimistically change state before confirmation/apply. |
| `conflict`, `helper_missing`, or `spool_blocked` and repair permitted | `修复` | Generate repair plan; never silently apply. |
| Any inspectable Agent | health-check icon | Refresh only this row. |
| Installed and verifiable | `验证事件` in overflow | Run verify and update row status. |
| Owned Hook removable | `删除 Hook` in overflow, danger | Generate removal plan and confirm. |
| Always | `详情` | Open low-frequency detail. |

Actions reserved for detail:

- Installation source/channel/executable path evidence.
- All installation instances when more than one exists.
- Asset inventory, category/filter/search, declared/effective state and trust evidence.
- Configuration file location, bounded preview, copy path, open file/directory.
- Full Hook diagnostic facts: config path, helper/spool availability, trust, ownership fingerprint/resources and last event time.

The outer row may reveal a short diagnostic inline, but it must not duplicate the full fact grid.

`安装` must not be represented as an off switch. A switch whose off->on action sometimes means "enable" and sometimes means "install" has unstable semantics. Not-installed rows use an explicit install command; installed rows use the switch.

### Backend-owned action capability contract

Extend the Rust inspection result with an ordered action list or named action map, for example:

```text
actions: [
  { action: install, available: false, reason: "未找到 Agent CLI" },
  { action: enable, available: false, reason: "Hook 尚未安装" },
  { action: disable, available: false, reason: null },
  { action: remove, available: false, reason: null },
  { action: health, available: true, reason: null },
  { action: verify, available: false, reason: "尚未安装" },
  { action: repair, available: false, reason: null }
]
```

Names are illustrative; the concrete Rust enum is authoritative. The frontend only renders the result, uses `reason` as tooltip/help text, and does not infer permissions from `state` strings. This preserves existing rules such as rejecting Install/Enable when the CLI is absent while still allowing Disable/Remove for owned resources.

If this contract extension is deferred for the first visual iteration, do not pre-disable based on frontend guesses. Let the backend plan call reject safely and show its reason. However, the capability contract is required before calling the control surface complete.

### Loading, errors and concurrency

- Initial inventory load: list skeleton/empty state only when there is no cached inventory.
- Inventory refresh: preserve rows and show a small toolbar progress indicator; do not blank or disable all rows.
- Initial Hook inspection: each row independently shows `读取中`; cap concurrent inspection requests (recommended 3) and inspect each unique `(agentKind, runtimeScope)` exactly once.
- Per-row health/verify/plan/apply: only the matching Agent row becomes busy. Other Agents remain operable.
- Plan generation: keep the list usable and open one shared closable modal after the plan arrives.
- Plan apply: close the modal immediately, show row-level progress, and release in `finally` on success/failure/timeout. Do not lock the whole settings card.
- Stale result: retain the store's request-ID behavior. Switching workspace, closing detail or reloading the inventory must not allow an older row result to overwrite a newer inspection.
- Cached facts: a refresh failure keeps the last successful inspection/version and adds an inline stale/error message. Do not replace a known healthy/disabled state with a fabricated unknown state.
- Expected inspection failures: render at row level with retry. Toasts are reserved for explicit user actions; mounting the page must not emit four simultaneous error toasts.
- Unsupported: status stays visible as `暂不支持`; write controls are absent/disabled with a backend-provided reason, while detail and read-only inventory remain available.
- Permission/conflict: no retry with elevated privilege and no automatic repair. Show `权限不足`/`配置冲突` as a row warning and keep the disk unchanged.

### Component and state responsibilities

#### Files to replace or change

1. `src/components/settings/SettingsAgentEnvironmentCenter.vue`
   - Remains page orchestration only.
   - Renders toolbar, list, one shared plan modal and low-frequency detail.
   - Owns page-level mounting/unmounting of inventory and Hook inspections.
   - Moves deep-scan results behind a secondary action.

2. `src/components/settings/agent-environment/AgentInstallationGrid.vue`
   - Replace grid semantics with `AgentEnvironmentConsole.vue`, or rename this file if minimizing churn.
   - Renders the header and registry-driven row collection.
   - Emits semantic row actions; does not call IPC.

3. `src/components/settings/agent-environment/AgentInstallationItem.vue`
   - Replace with `AgentEnvironmentRow.vue`.
   - Pure rendering of identity, installation/version facts, Hook state, switch and buttons.
   - Receives backend-computed action availability and per-row async state.
   - Contains no per-Agent switch branches and no modal.

4. `src/components/settings/agent-environment/AgentInstallationDetail.vue`
   - Remove `AgentHookManager` from the overview.
   - Keep installation evidence, assets and configuration.
   - Add complete Hook diagnostics only as a read-only section if needed, without restoring primary controls there.

5. `src/components/settings/agent-environment/AgentHookManager.vue`
   - Do not reuse the current mounted-per-Agent component in rows.
   - Split the shared plan UI into `AgentHookPlanModal.vue`.
   - Move request/operation orchestration into a composable. Delete the old component once no caller remains; do not retain a parallel hidden entry.

6. `src/composables/useAgentEnvironmentCenter.ts`
   - Add the registry-to-console projection grouped by `(agentKind, runtimeScope)`.
   - Keep selection/search/preview state for detail.
   - Delegate Hook workflow to `useAgentHookConsole`; do not make the view infer capabilities.

7. `src/composables/useAgentHookConsole.ts` (new)
   - Own unique-Agent inspection queue, bounded concurrency, row operation IDs, selected plan, planning/applying states, confirmation and cleanup.
   - Expose commands by stable key: inspect, health, verify, requestPlan, confirmPlan and cancelPlan.
   - Ensure all row busy state is released in `finally` and stale results do not write back.

8. `src/stores/agent-hooks.ts`
   - Remains shared inspection/cache owner.
   - Key state by a stable Hook target key capable of including runtime scope, rather than assuming Agent kind is forever sufficient.
   - Add batch/unique inspect support only if it eliminates duplicated request orchestration; keep IPC calls in actions.

9. `src/stores/provider-types.ts` plus Rust model/command/service files that own `AgentHookInspection`
   - Add backend-computed action availability/reason.
   - Keep Rust as capability truth and update TypeScript only as the receiving contract.

10. `src/styles/modules/agent-environment.css`
    - Replace the auto-fit card grid with stable table/list tracks and row separators.
    - Add responsive compact-row rules, ellipsis/tooltips, fixed control dimensions and per-row loading/error states.
    - Preserve existing settings surface variables and 8 px maximum card radius.

11. `tests/agent-environment.test.ts`
    - Expand behavioral/component tests described below.

#### Files expected to remain reusable

- `AgentVersionStatus.vue`: reuse for installed/latest/check facts, adding a row-density variant only if necessary.
- `AgentCliIcon.vue`: reuse registry-backed official icons.
- `AgentAssetInventory.vue`, `AgentConfigBrowser.vue`, `AgentWorkspaceScopeSelect.vue`: remain detail/toolbar children.
- `src/stores/agent-environment.ts`: keep inventory/version/preview cache and request-ID behavior; do not merge Hook state into it.

### Workspace scope clarification

The toolbar label should be `资产范围`, not a generic `盘点范围`, unless Rust later makes Hook targets workspace-specific. Changing the workspace may reload workspace assets/configuration, but the outer row must continue to show the same native user Hook target. The shared plan modal should state the concrete Hook config path and `用户级` scope before apply.

### Tests and acceptance conditions

Automated acceptance:

1. A fixture with more than four registry entries renders all rows without changing console code.
2. Every row shows Agent identity, availability, installed version/latest state and Hook state before opening detail.
3. A not-installed row exposes `安装 Hook`; an installed row exposes an enable switch; abnormal owned rows expose `修复`; removal/verify remain accessible in overflow.
4. Clicking a switch/action never emits the detail event and clicking `详情` never mutates Hook state.
5. One Hook target is inspected once even if the fixture contains two executable installations of the same Agent.
6. Operating Agent A disables only Agent A's controls; Agent B remains usable.
7. Closing the shared plan modal does not apply. Apply begins only after explicit confirmation, closes the modal immediately, and releases busy state after success, failure and timeout.
8. A stale inspection/apply result cannot overwrite a newer result for the same stable Hook target.
9. Inventory/version/inspection refresh failures preserve prior facts and render an inline stale error.
10. Unsupported, CLI-missing, permission-denied, conflict and owned-resource removal states render backend reasons without frontend capability inference.
11. `AgentInstallationDetail.vue` no longer imports or renders the old interactive `AgentHookManager`.
12. No public console/list/row file contains literal branches for the current four Agent kinds.
13. SSR/component tests confirm the entire row is not a button and direct controls have accessible names.
14. At 820 px, 620 px and 390 px widths, text and controls do not overlap, no horizontal scrollbar appears, and each row keeps a stable readable layout.

Manual acceptance:

1. On opening Agent settings, the user can determine all installed Agents, versions and Hook states without clicking into any row.
2. The user can install, enable/disable, check, verify, repair or remove a Hook from the list, with a plan confirmation before every write.
3. A failed operation leaves the rest of the list interactive and presents an actionable reason on the affected row.
4. Details remain reachable for assets/configuration, but returning to the list preserves current Hook facts and scroll position.
5. Changing asset workspace scope does not misleadingly change or duplicate the user-level Hook control.

Quality gate after implementation:

```bash
npm run build
npm test
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings
cd src-tauri && cargo test
npm run doctor:platform
git diff --check
```

## Files Found

- `src/components/settings/SettingsAgentEnvironmentCenter.vue` - Current page orchestrator and grid/detail switch.
- `src/components/settings/agent-environment/AgentInstallationGrid.vue` - Current card grid with no Hook facts/actions.
- `src/components/settings/agent-environment/AgentInstallationItem.vue` - Entire-card navigation target showing installation/version only.
- `src/components/settings/agent-environment/AgentInstallationDetail.vue` - Detail tabs and the sole mounting point for Hook controls.
- `src/components/settings/agent-environment/AgentHookManager.vue` - Current per-Agent inspection, plan, apply and modal implementation.
- `src/composables/useAgentEnvironmentCenter.ts` - Inventory/detail/deep-scan orchestration and stale-request boundaries.
- `src/stores/agent-environment.ts` - Workspace-keyed inventory/version/preview cache.
- `src/stores/agent-hooks.ts` - Agent-keyed Hook inspection state and stale-response rejection.
- `src/stores/provider-types.ts` - Rust-facing installation, inventory, Hook inspection and plan contracts.
- `src/agent-cli/visuals.ts` - Frontend visual registry and derived `AgentCliKind`.
- `src/styles/modules/agent-environment.css` - Existing grid, detail, Hook and responsive styles.
- `tests/agent-environment.test.ts` - Existing store stale-result and plan-modal safety coverage.

## Code Patterns

- Rust/IPC is capability truth; Vue components render typed results rather than duplicating permission rules (`.trellis/spec/frontend/component-guidelines.md`, `.trellis/spec/frontend/state-management.md`).
- Components emit semantic events and multi-step workflows belong in composables (`.trellis/spec/frontend/component-guidelines.md`).
- Stable IDs/request IDs reject stale async results (`src/stores/agent-environment.ts:31`, `src/stores/agent-hooks.ts:31`).
- Shared backend data remains in Pinia; modal visibility and plan selection remain local workflow state (`.trellis/spec/frontend/state-management.md`).
- Existing settings styles use full-width cards, restrained borders and feature-scoped CSS variables; the Agent console should reuse these rather than add nested cards (`src/styles/modules/agent-environment.css:125`).

## External References

- AgentBro README at commit `06fe149fdfa23f8ef958279dfa1395b46c7a1243`: Integration workflow says run Hook Doctor and install Hooks from one integration surface; Agent management centralizes installation, versions, paths and Hooks. https://github.com/shirenchuang/agentbro/blob/06fe149fdfa23f8ef958279dfa1395b46c7a1243/README.md
- AgentBro `IslandSection.tsx` at the same commit: detected tools are mapped into direct rows with status and configure/open/install/reinstall/uninstall actions; list-level detect/install-all/uninstall-all actions are immediately above them. https://github.com/shirenchuang/agentbro/blob/06fe149fdfa23f8ef958279dfa1395b46c7a1243/src/components/settings/sections/IslandSection.tsx#L2279-L2367
- AgentPulse README at commit `1496e2d48171c256314ce153e9fcc32b54467dcf`: multiple provider Hook events are normalized through one sidecar; setup is centralized rather than exposed only through per-Agent details. https://github.com/yazelin/AgentPulse/blob/1496e2d48171c256314ce153e9fcc32b54467dcf/README.md
- Atoll README at commit `6d2044f462a3fb7233d05e358afbe43cc58026be`: Hook setup/removal is provided as centralized install/remove commands and the runtime island remains focused on sessions. https://github.com/TaylorChen/atoll/blob/6d2044f462a3fb7233d05e358afbe43cc58026be/README.md

## Related Specs

- `.trellis/spec/frontend/component-guidelines.md`
- `.trellis/spec/frontend/state-management.md`
- `.trellis/spec/frontend/type-safety.md`
- `.trellis/spec/frontend/directory-structure.md`
- `.trellis/spec/frontend/quality-guidelines.md`
- `.trellis/spec/guides/code-reuse-thinking-guide.md`
- `.trellis/spec/guides/cross-layer-thinking-guide.md`
- `.trellis/tasks/09-04-agent-environment-runtime-center/prd.md`
- `.trellis/tasks/09-04-agent-environment-runtime-center/design.md`

## Caveats / Not Found

- Dynamic Island projects do not share one standard settings UI. The external evidence supports centralized integration controls, not a claim that every project uses the same list design.
- The current inventory discovers only one installation per Agent. The proposed grouping intentionally avoids duplicating one Hook control later, but full multi-install discovery/effective precedence remains an unfinished backend requirement.
- The current Hook IPC accepts `AgentCliKind` only and the frontend store is keyed the same way. Native is the only implemented runtime scope, but the stable target key should include runtime scope before WSL support is added.
- The current inspection contract lacks backend-computed action availability. A visual list can be built first, but permission-complete direct controls require that contract extension.
- This research intentionally does not cover or implement the postponed decision center.
