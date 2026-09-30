<script setup lang="ts">
withDefaults(defineProps<{
  label: string;
  divided?: boolean;
}>(), { divided: false });
</script>

<template>
  <span v-if="divided" class="workspace-card-action-divider" aria-hidden="true" />
  <div class="workspace-card-action-group" :aria-label="label">
    <slot />
  </div>
</template>
