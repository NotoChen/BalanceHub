<script setup lang="ts">
withDefaults(defineProps<{
  actionsVisible?: boolean;
  label?: string;
}>(), { actionsVisible: true, label: "快捷操作" });
</script>

<template>
  <footer class="workspace-card-footer" @click.stop @pointerdown.stop>
    <div v-if="$slots.meta" class="workspace-card-footer-meta">
      <slot name="meta" />
    </div>
    <div v-if="actionsVisible" class="workspace-card-quick-actions" :aria-label="label">
      <slot />
    </div>
  </footer>
</template>
