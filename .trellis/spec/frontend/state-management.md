# State Management

> How state is managed in this project.

---

## Overview

<!--
Document your project's state management conventions here.

Questions to answer:
- What state management solution do you use?
- How is local vs global state decided?
- How do you handle server state?
- What are the patterns for derived state?
-->

Pinia stores hold shared frontend state. Composables own workflow-local state;
components keep purely visual state local. Rust `AppState` and its services are
the source of truth for persisted providers, schedules, capabilities, and
background work.

---

## State Categories

<!-- Local state, global state, server state, URL state -->

Separate persisted domain data, derived display state, and transient UI state.
Backend updates are merged through the existing store/controller path instead
of mutating a card copy independently in each component.

---

## When to Use Global State

<!-- Criteria for promoting state to global -->

Promote state when multiple views need the same value, when it must survive a
view change, or when it represents an IPC/backend revision. Keep modal open
flags, input drafts, and hover state local unless another view truly consumes
them.

---

## Server State

<!-- How server data is cached and synchronized -->

Use the Rust command/service and shared network layer for remote data. Keep
request IDs or revisions when an operation can overlap, and reject stale
responses before they overwrite newer provider state.

---

## Common Mistakes

<!-- State management mistakes your team has made -->

Do not make a second store for the same provider data, mutate a deep reactive
copy as an identity token, or let a component infer backend capabilities from
field presence alone.
