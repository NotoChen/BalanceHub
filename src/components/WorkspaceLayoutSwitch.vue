<script setup lang="ts">
import { LayoutGrid, List } from "@lucide/vue";
import type { WorkspaceLayout } from "../stores/workspace-layout";

const props = defineProps<{ modelValue: WorkspaceLayout }>();
const emit = defineEmits<{ "update:modelValue": [value: WorkspaceLayout] }>();

const options: Array<{
  value: WorkspaceLayout;
  label: string;
  icon: typeof LayoutGrid;
}> = [
  { value: "cards", label: "卡片布局", icon: LayoutGrid },
  { value: "list", label: "列表布局", icon: List },
];
</script>

<template>
  <div class="workspace-layout-switch" role="group" aria-label="布局方式">
    <button
      v-for="option in options"
      :key="option.value"
      type="button"
      :class="{ active: props.modelValue === option.value }"
      :aria-pressed="props.modelValue === option.value"
      :aria-label="option.label"
      :title="option.label"
      @click="emit('update:modelValue', option.value)"
    >
      <component :is="option.icon" :size="15" :stroke-width="1.9" aria-hidden="true" />
    </button>
  </div>
</template>

<style scoped>
.workspace-layout-switch {
  display: inline-flex;
  min-height: 30px;
  flex: 0 0 auto;
  align-items: center;
  gap: 2px;
  border: 1px solid var(--surface-control-line);
  border-radius: 6px;
  background: var(--surface-panel);
  padding: 2px;
}

.workspace-layout-switch button {
  display: inline-flex;
  width: 26px;
  height: 24px;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: var(--surface-muted);
  cursor: pointer;
  padding: 0;
  transition:
    background 160ms ease,
    color 160ms ease;
}

.workspace-layout-switch button:hover {
  background: var(--surface-panel-soft);
  color: var(--surface-ink-soft);
}

.workspace-layout-switch button.active {
  background: var(--surface-accent-soft);
  color: var(--surface-accent);
}

.workspace-layout-switch button:focus-visible {
  outline: 2px solid var(--surface-accent);
  outline-offset: 1px;
}

@media (prefers-reduced-motion: reduce) {
  .workspace-layout-switch button {
    transition: none;
  }
}
</style>
