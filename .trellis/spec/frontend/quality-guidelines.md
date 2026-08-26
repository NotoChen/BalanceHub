# Quality Guidelines

> Code quality standards for frontend development.

---

## Overview

<!--
Document your project's quality standards here.

Questions to answer:
- What patterns are forbidden?
- What linting rules do you enforce?
- What are your testing requirements?
- What code review standards apply?
-->

Quality gates are proportional to the change: `npm run build` and `npm test`
for frontend changes; Rust `cargo fmt --check`, Clippy with `-D warnings`, and
`cargo test` for backend changes; `git diff --check` for docs/config changes.

---

## Forbidden Patterns

<!-- Patterns that should never be used and why -->

Do not silence warnings, keep dead compatibility branches, duplicate existing
helpers, or leave unused exports. Do not put secrets, local app data, or build
caches in the repository.

---

## Required Patterns

<!-- Patterns that must always be used -->

Keep core entry points as orchestration only. Reuse the shared network proxy
resolution for business requests, Webhooks, updater, liveness, and temporary
CLI. Add regression coverage for async modal release, timeout/failure cleanup,
and stale-result protection when changing asynchronous UI behavior.

---

## Testing Requirements

<!-- What level of testing is expected -->

Review changed data structures across Rust models, storage migration/defaults,
IPC types, stores, and UI. Check all platform branches and generated shell
templates when platform code changes. Run the relevant quality commands before
finishing the task.

---

## Code Review Checklist

<!-- What reviewers should check -->

(To be filled by the team)
