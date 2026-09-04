# Hook Guidelines

> How hooks are used in this project.

---

## Overview

<!--
Document your project's hook conventions here.

Questions to answer:
- What custom hooks do you have?
- How do you handle data fetching?
- What are the naming conventions?
- How do you share stateful logic?
-->

This project uses Vue composables, not React hooks. A composable owns a
coherent stateful workflow and returns refs/computed state plus named actions.

---

## Custom Hook Patterns

<!-- How to create and structure custom hooks -->

Use names such as `useAppController`, `useWorkspaceLaunchFlow`, and
`useBackgroundTaskCenter`. Keep one source of truth for a workflow; compose
existing composables rather than duplicating IPC calls in another component.

---

## Data Fetching

<!-- How data fetching is handled (React Query, SWR, etc.) -->

Tauri calls and external processes must expose an explicit busy/task state and
finish in `finally` (or an equivalent backend state transition). Add timeout,
cancellation, or backend task status for operations with unpredictable
duration. Ignore late results with a request ID, revision, stable ID, or
explicit cancellation marker.

---

## Naming Conventions

<!-- Hook naming rules (use*, etc.) -->

IPC payloads are defined by Rust. TypeScript types describe the received
shape and view state only; they must not reimplement backend capability or
authorization rules.

---

## Common Mistakes

<!-- Hook-related mistakes your team has made -->

Do not place one-off `invoke` calls in multiple components, create a second
proxy/environment implementation, or leave a pending Promise controlling a
modal's closability after the operation has completed.

## Scenario: Unified Agent Runtime And Managed Hooks

### 1. Scope / Trigger

Use this contract when adding Agent environment inventory, external terminal
observation, official Hook ingestion, or runtime activity UI. The Rust runtime
projection is the only business source for activity; launch confirmation is a
short-lived compatibility path only.

### 2. Signatures

- Tauri commands: `get_agent_runtime_snapshot`, `get_agent_runtime_status`,
  `activate_agent_runtime`.
- Hook commands: `inspect_agent_hook`, `plan_agent_hook`, `apply_agent_hook`,
  `health_agent_hook`, `repair_agent_hook`, `verify_agent_hook`.
- Frontend event: `agent-runtime-updated`, carrying `AgentRuntimeSnapshot`.

### 3. Contracts

- `AgentRuntimeSnapshot` contains `schemaVersion`, monotonic `revision`,
  `updatedAt`, and `sessions`.
- A session has independent `runtimeId`, optional Agent `agentSessionId`,
  optional `balancehubInstanceId`, origin, state, evidence, and action flags.
- BalanceHub launches associate Hook events only through the generated
  `BALANCEHUB_CLI_INSTANCE_ID` environment value. Cwd, timestamps, Agent name,
  or default configuration are not correlation keys.
- Hook helpers accept bounded stdin, persist only allow-listed metadata, and
  exit successfully on all failures so Agent execution remains fail-open.
- The spool is atomic per event, bounded to 5,000 files / 20 MiB / 7 days, and
  is acknowledged only after the projection has been committed.
- Agent lifecycle payloads that do not officially expose title or model leave
  those fields absent. A bounded session-adapter enrichment producer may fill
  them for the affected Agent/session, but it must not scan all history on the
  two-second runtime refresh or decide process liveness.
- Runtime enrichment runs outside the two-second refresh with global
  concurrency 2 and per-Agent concurrency 1. It must coalesce by stable target,
  cancel stale generations, enforce an outer timeout, retain bounded in-memory
  cursors/backoff, retry repository commit failures, and publish snapshots only
  through one monotonic revision claim shared with refresh.
- Exact metadata lookup validates session IDs and canonical path containment.
  Agent layouts may use only the exact workspace/session root or a bounded
  workspace-local locator; recursive home-wide history scans are forbidden.
  Append-only sources must consume their cursor and parse only the new complete
  suffix after the initial bounded baseline.

### 4. Validation & Error Matrix

- Empty or malformed Hook input -> discard/quarantine with no Agent failure.
- Unknown schema or native event -> structured unsupported diagnostic.
- Duplicate event ID -> idempotent projection and discardable spool record.
- Projection write failure -> retain incoming Hook file for a later retry.
- External runtime without an exact terminal locator -> display status only;
  `activate_agent_runtime` must reject it.
- Install/enable when Rust cannot resolve that Agent installation -> an
  unsupported plan and a second availability check before apply. Existing
  owned Hooks remain inspectable and removable/disableable for cleanup.
- A managed matcher group or standalone Hook whose complete ownership
  fingerprint has drifted -> conflict and byte-stable failure; never remove
  only the still-recognizable handler from a user-modified group.

### 5. Good/Base/Bad Cases

- Good: a Codex Hook event with a valid session ID and a launch instance ID
  updates the existing launch session exactly once.
- Base: an external event with only Agent/session metadata creates an
  `external_hook` session with missing fields left unknown.
- Bad: matching sessions by workdir or a time window, or treating a present
  Hook config as healthy before a real event arrives.

### 6. Tests Required

- Reducer tests assert duplicate/out-of-order events, cross-scope isolation, end/resume
  behavior, bounded evidence, and launch snapshots not regressing Hook busy.
- Spool tests assert atomic writes, size/age limits, quarantine, symlink
  rejection, and commit-before-ack behavior.
- Launcher tests assert active-only UI reads still hide exits while the runtime
  source retains recent exit code evidence.
- Frontend tests assert revision/request guards, event-plus-focus calibration,
  no long-running list polling, and activation gating.
- Managed Hook tests assert structural identity/fingerprint/revision checks,
  unknown-field preservation, byte-stable conflict failures, and fail-open
  helper behavior.
- Enrichment tests assert no adapter call on refresh, global/per-Agent limits,
  coalescing, fair priority, cancellation/outer timeout, stale-result rejection,
  Pending cursor continuation, bounded registries, commit retry, restart
  recovery, and per-Agent traversal/symlink/exact-ID fixtures.

### 7. Wrong vs Correct

#### Wrong

```ts
const active = cliRuntime.instances.filter((item) => item.status !== "exited");
```

Using this as the unified source loses launch exit evidence and leaves sessions
stuck in `idle`.

#### Correct

```rust
let launch_snapshots = cli_runtime::runtime_instances();
repository.refresh_with_launch_snapshots(&launch_snapshots)?;
```

The runtime-only source includes recent exited records for reducer evidence,
while the legacy active list remains active-only for short launch UI behavior.
