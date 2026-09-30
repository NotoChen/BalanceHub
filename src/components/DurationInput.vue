<script setup lang="ts">
import { computed, ref } from "vue";
import { durationUnitOptions, durationUnitSeconds, durationValueToSeconds, formatDuration, type DurationUnit } from "../utils/duration";

const props = withDefaults(defineProps<{
  modelValue: number;
  min?: number;
  max?: number;
  disabled?: boolean;
  label: string;
}>(), { min: 1, disabled: false });
const emit = defineEmits<{ "update:modelValue": [seconds: number] }>();
const unit = ref<DurationUnit>(
  props.modelValue >= 3600 && props.modelValue % 3600 === 0 ? "hour"
    : props.modelValue >= 60 && props.modelValue % 60 === 0 ? "minute" : "second",
);
const factor = computed(() => durationUnitSeconds(unit.value));
const amount = computed(() => props.modelValue / factor.value);

function updateAmount(value: number | undefined) {
  if (value === undefined || !Number.isFinite(value)) return;
  emit("update:modelValue", Math.min(props.max ?? Infinity, Math.max(props.min, durationValueToSeconds(value, unit.value))));
}
</script>

<template>
  <div class="duration-control settings-duration-control" :title="formatDuration(modelValue)">
    <a-input-number
      :model-value="amount"
      :min="min / factor"
      :max="max === undefined ? undefined : max / factor"
      :disabled="disabled"
      :aria-label="label"
      @update:model-value="updateAmount"
    />
    <a-select v-model="unit" :options="durationUnitOptions" :disabled="disabled" :aria-label="`${label}的单位`" />
  </div>
</template>
