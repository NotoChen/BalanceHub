# Component Guidelines

> How components are built in this project.

---

## Overview

<!--
Document your project's component conventions here.

Questions to answer:
- What component patterns do you use?
- How are props defined?
- How do you handle composition?
- What accessibility standards apply?
-->

Components use Vue 3 `<script setup lang="ts">` and are responsible for a
bounded visual surface. They receive typed props and emit semantic events;
they should not duplicate Rust capability or permission decisions.

---

## Component Structure

<!-- Standard structure of a component file -->

Keep imports, props/emits, local state, computed values, event handlers, and
template/style concerns in that order. Move long async workflows into a
composable and expose only the state and commands needed by the view.

---

## Props Conventions

<!-- How props should be defined and typed -->

Use `defineProps`/`defineEmits` with imported TypeScript types. Prefer stable
scalar IDs for async selection and stale-result checks; do not compare a
reactive proxy object with its original object by identity.

---

## Styling Patterns

<!-- How styles are applied (CSS modules, styled-components, Tailwind, etc.) -->

Use the existing CSS modules under `src/styles/` and the component class
conventions already used by the surrounding feature. Reuse existing icons,
surface, spacing, and modal styles before adding a new visual primitive.

---

## Accessibility

<!-- A11y requirements and patterns -->

Interactive controls need an accessible label or tooltip, keyboard operation,
and a stable disabled/loading state. Modal content must retain usable padding;
an async operation must never leave the modal or whole panel permanently
locked after success, failure, or timeout.

---

## Common Mistakes

<!-- Component-related mistakes your team has made -->

Do not put provider protocol rules, duplicate API calls, or long-running
process polling directly in a component. Do not leave dead branches or old UI
entry points when a component is replaced.
