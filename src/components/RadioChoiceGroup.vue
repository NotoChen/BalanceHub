<script setup lang="ts" generic="Option extends { value: string; label: string; description?: string; disabled?: boolean }">
import { useId } from "vue";

const props = defineProps<{
  modelValue: Option["value"];
  options: readonly Option[];
  label: string;
  disabled?: boolean;
  optionClass?: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: Option["value"]];
}>();
const groupName = useId();

function selectOption(option: Option) {
  if (!props.disabled && !option.disabled) emit("update:modelValue", option.value);
}
</script>

<template>
  <div class="radio-choice-group" role="radiogroup" :aria-label="label" :aria-disabled="disabled || undefined">
    <label
      v-for="option in options"
      :key="option.value"
      class="radio-choice"
      :class="[optionClass, { active: modelValue === option.value, 'is-disabled': disabled || option.disabled }]"
      :title="option.description"
    >
      <input
        class="radio-choice-input"
        type="radio"
        :name="groupName"
        :value="option.value"
        :aria-label="option.label"
        :checked="modelValue === option.value"
        :disabled="disabled || option.disabled"
        @change="selectOption(option)"
      />
      <slot :option="option" :selected="modelValue === option.value">
        <span>{{ option.label }}</span>
      </slot>
    </label>
  </div>
</template>

<style scoped>
.radio-choice-group {
  display: grid;
  min-width: 0;
}

.radio-choice {
  position: relative;
  min-width: 0;
  cursor: pointer;
}

.radio-choice-input {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  margin: 0;
  opacity: 0;
  cursor: inherit;
}

.radio-choice:focus-within {
  outline: 2px solid var(--surface-accent);
  outline-offset: 3px;
}

.radio-choice.is-disabled {
  cursor: not-allowed;
  opacity: 0.55;
}
</style>
