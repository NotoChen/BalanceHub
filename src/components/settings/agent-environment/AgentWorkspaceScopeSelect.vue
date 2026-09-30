<script setup lang="ts">
defineProps<{
  modelValue: string;
  options: { value: string; label: string }[];
}>();

const emit = defineEmits<{ "update:model-value": [value: string] }>();

function updateValue(value: unknown) {
  if (typeof value === "string") emit("update:model-value", value);
}
</script>

<template>
  <a-select
    class="agent-environment-scope-select"
    size="small"
    :model-value="modelValue"
    aria-label="配置范围"
    :title="modelValue || '全局配置'"
    :options="options"
    @update:model-value="updateValue"
  />
</template>
