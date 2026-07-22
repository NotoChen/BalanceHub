<template>
  <div class="settings-detection-grid">
    <slot />
  </div>
</template>

<style scoped>
.settings-detection-grid {
  display: grid;
  width: 100%;
  max-width: 350px;
  min-width: 0;
  grid-template-columns: repeat(auto-fill, minmax(min(100%, 140px), 1fr));
  align-items: stretch;
  gap: 10px;
}

.settings-detection-grid :deep(.settings-detection-item) {
  width: 100%;
  max-width: 170px;
}
</style>
