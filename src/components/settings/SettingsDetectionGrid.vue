<script setup lang="ts">
withDefaults(
  defineProps<{
    wide?: boolean;
  }>(),
  {
    wide: false,
  },
);
</script>

<template>
  <div class="settings-detection-grid" :class="{ 'is-wide': wide }">
    <slot />
  </div>
</template>

<style scoped>
.settings-detection-grid {
  display: grid;
  width: 100%;
  max-width: 530px;
  min-width: 0;
  grid-template-columns: repeat(auto-fill, minmax(min(100%, 140px), 1fr));
  align-items: stretch;
  gap: 10px;
}

.settings-detection-grid :deep(.settings-detection-item) {
  width: 100%;
  max-width: 170px;
}

.settings-detection-grid.is-wide {
  max-width: 760px;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 128px), 1fr));
}

.settings-detection-grid.is-wide :deep(.settings-detection-item) {
  max-width: none;
}
</style>
