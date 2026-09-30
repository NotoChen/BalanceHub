<script setup lang="ts">
defineProps<{
  tone?: "copy" | "data" | "site" | "configuration" | "integration" | "launch"
    | "success" | "edit" | "warning" | "danger" | "link" | "refresh"
    | "models" | "key" | "automation";
}>();
</script>

<template>
  <button type="button" class="card-icon-action" :class="tone ? `card-action-${tone}` : undefined">
    <slot />
  </button>
</template>
