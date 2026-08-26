# Directory Structure

> How frontend code is organized in this project.

---

## Overview

<!--
Document your project's frontend directory structure here.

Questions to answer:
- Where do components live?
- How are features/modules organized?
- Where are shared utilities?
- How are assets organized?
-->

The frontend is a Vue 3 application. `App.vue` and
`useAppController.ts` compose the app; business rules and IPC commands remain
in Rust. Do not move backend capability checks or protocol rules into Vue.

---

## Directory Layout

```
src/
├── App.vue                         # application shell and top-level events
├── main.ts                         # Vue/Pinia/bootstrap entry
├── components/                     # focused reusable UI blocks and modals
├── composables/                    # stateful orchestration (`use*.ts`)
├── stores/                         # Pinia state and IPC-facing types
├── styles/                         # global and feature CSS modules
└── assets/                         # bundled static assets

src-tauri/
├── src/commands/                   # Tauri command entry points
├── src/services/                   # business orchestration
├── src/adapters/                   # provider protocol implementations
├── src/models/                     # serialized Rust domain models
└── src/network/                    # shared proxy/client transport
```

---

## Module Organization

<!-- How should new features be organized? -->

Keep a component focused on rendering and user events. Put multi-step state
orchestration in a composable, and put rules shared by multiple commands in a
Rust service or adapter. Add a new component to the nearest existing feature
area instead of creating a parallel utility or store.

---

## Naming Conventions

<!-- File and folder naming rules -->

Use PascalCase for Vue components, camelCase for composables and stores, and
descriptive domain names. Keep protocol-specific Rust code under its adapter
directory; do not create generic files containing unrelated provider logic.

---

## Examples

<!-- Link to well-organized modules as examples -->

Examples: `src/components/ProviderCard.vue`,
`src/components/TemporaryCliModal.vue`,
`src/composables/useAppController.ts`, and
`src/stores/provider-types.ts`.
