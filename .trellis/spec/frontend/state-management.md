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

## Login Accounts and Credential Views

Clearing a login session must retain the account's observed identity and station
bindings. A new platform identity belongs to another account entry; browser
storage is replaceable session state, not the identity owner. Render the Rust
`sessionLabel` and `canLogin` projection instead of treating Cookie presence as
proof that the platform is currently authenticated.

A station's account picker carries its target name, URL, and previous binding.
Account creation must complete that same choice or be cancelled when the picker
closes; late results cannot launch an abandoned station. Full account management
is a separate surface. See `login-account-selection.ts` for this lifecycle.

Credential validation describes the current station request and its scope.
Displaying all stored credentials must not imply that one successful sync
validated every saved PAT, API Key, password, or platform login. Keep the scope
and action label in the Rust credential-details response.
