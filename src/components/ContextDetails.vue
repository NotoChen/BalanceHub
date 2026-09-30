<script setup lang="ts">
defineProps<{ label: string }>();
</script>

<template>
  <details class="context-details">
    <summary>{{ label }}</summary>
    <div class="context-details-body"><slot /></div>
  </details>
</template>

<style scoped>
.context-details { min-width: 0; color: var(--surface-muted); font-size: 12px; line-height: 1.7; }
.context-details > summary { width: fit-content; max-width: 100%; cursor: pointer; }
.context-details > summary:hover { color: var(--surface-ink); }
.context-details > summary:focus-visible { outline: 2px solid var(--surface-accent); outline-offset: 3px; border-radius: 3px; }
.context-details-body { display: grid; min-width: 0; gap: 10px; padding-top: 10px; overflow-wrap: anywhere; }
.context-details-body > :deep(p) { margin: 0; }
</style>
