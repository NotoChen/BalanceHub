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
