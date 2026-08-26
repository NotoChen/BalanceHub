# Type Safety

> Type safety patterns in this project.

---

## Overview

<!--
Document your project's type safety conventions here.

Questions to answer:
- What type system do you use?
- How are types organized?
- What validation library do you use?
- How do you handle type inference?
-->

TypeScript is strict at the frontend boundary. Shared provider shapes live in
`src/stores/provider-types.ts`; UI-only types stay next to their composable or
component.

---

## Type Organization

<!-- Where types are defined, shared types vs local types -->

Rust serde models and Tauri command payloads are authoritative. When a model
changes, update the Rust model, storage defaults/migration, IPC consumers, and
frontend receiving type together.

---

## Validation

<!-- Runtime validation patterns (Zod, Yup, io-ts, etc.) -->

Use explicit unions for finite UI states and `unknown` plus a narrow guard for
untrusted IPC or external data. Do not use `any` or broad casts to hide a
contract mismatch.

---

## Common Patterns

<!-- Type utilities, generics, type guards -->

Prefer discriminated unions, named domain types, and stable string IDs. Keep
protocol-specific capability decisions in Rust and return a typed result to
the frontend.

---

## Forbidden Patterns

<!-- any, type assertions, etc. -->

Avoid `any`, `as` casts that bypass a real guard, duplicated Rust enums in Vue,
and optional fields that silently change the meaning of an existing payload.
