<script setup lang="ts">
import { IconLoading } from "@arco-design/web-vue/es/icon";
import type { TemporaryCliTerminalKind } from "../stores/providers";
import type { SelectOption } from "../utils/liveness-options";
import TerminalBrandIcon from "./TerminalBrandIcon.vue";

withDefaults(
  defineProps<{
    modelValue: TemporaryCliTerminalKind;
    options: SelectOption<TemporaryCliTerminalKind>[];
    disabled?: boolean;
    loading?: boolean;
    emptyLabel?: string;
  }>(),
  {
    disabled: false,
    loading: false,
    emptyLabel: "未检测到可用终端",
  },
);

const emit = defineEmits<{
  "update:modelValue": [value: TemporaryCliTerminalKind];
}>();
</script>

<template>
  <div class="environment-icon-selector" role="radiogroup" aria-label="终端" :aria-busy="loading">
    <button
      v-for="option in options"
      :key="option.value"
      type="button"
      class="environment-icon-option"
      :class="{ active: modelValue === option.value }"
      :disabled="disabled || loading"
      :title="option.label"
      :aria-label="option.label"
      :aria-checked="modelValue === option.value"
      role="radio"
      @click="emit('update:modelValue', option.value)"
    >
      <TerminalBrandIcon :kind="option.value" :name="option.label" :size="20" />
    </button>
    <span v-if="options.length === 0" class="environment-icon-empty">
      <IconLoading v-if="loading" />
      {{ loading ? "正在扫描终端" : emptyLabel }}
    </span>
  </div>
</template>

<style scoped>
.environment-icon-selector {
  display: flex;
  min-width: 0;
  min-height: 34px;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.environment-icon-option {
  display: inline-grid;
  width: 34px;
  height: 34px;
  flex: 0 0 34px;
  place-items: center;
  border: 1px solid var(--color-border-2);
  border-radius: 6px;
  background: var(--color-fill-1);
  color: var(--color-text-2);
  cursor: pointer;
  padding: 0;
  transition: border-color 0.18s ease, background 0.18s ease, transform 0.18s ease;
}

.environment-icon-option:hover:not(:disabled),
.environment-icon-option:focus-visible {
  border-color: rgb(var(--arcoblue-5));
  background: rgba(var(--arcoblue-1), 0.72);
  outline: none;
}

.environment-icon-option:focus-visible {
  box-shadow: 0 0 0 2px rgba(var(--arcoblue-6), 0.18);
}

.environment-icon-option.active {
  border-color: rgb(var(--arcoblue-6));
  background: rgba(var(--arcoblue-1), 0.9);
}

.environment-icon-option:active:not(:disabled) {
  transform: translateY(1px);
}

.environment-icon-option:disabled {
  cursor: not-allowed;
  opacity: 0.48;
}

.environment-icon-empty {
  display: inline-flex;
  min-height: 34px;
  align-items: center;
  gap: 7px;
  color: var(--color-text-3);
  font-size: 12px;
}

.environment-icon-empty > svg {
  animation: environment-icon-spin 0.9s linear infinite;
}

@keyframes environment-icon-spin {
  to { transform: rotate(360deg); }
}
</style>
