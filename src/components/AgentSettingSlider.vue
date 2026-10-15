<script setup lang="ts">
import { computed, ref, useId, watch } from "vue";
import { IconEdit, IconUndo } from "@arco-design/web-vue/es/icon";
import type { AgentNativeSetting } from "../stores/agent-settings-types";
import {
  currentSettingValue,
  knownSettingValue,
  settingValueLabel,
} from "../utils/agent-setting-display";
import "../styles/modules/agent-setting-slider.css";

const props = defineProps<{
  field: AgentNativeSetting;
  value?: string;
  disabled?: boolean;
  inputId?: string;
  resetLabel: string;
  resetHint: string;
}>();
const emit = defineEmits<{ "update:value": [value: string | undefined] }>();
const customOpen = ref(false);
const customDraft = ref("");
const numericCeiling = ref(1);
const dragging = ref(false);
const generatedId = useId();
const controlId = computed(() => props.inputId ?? generatedId);
const discrete = computed(() => props.field.control === "select");
const currentValue = computed(() => currentSettingValue(props.field));
const effectiveValue = computed(() =>
  props.value === "auto"
    ? knownSettingValue(currentValue.value)
    : (props.value ?? knownSettingValue(currentValue.value)),
);
const effectiveNumber = computed(() => numeric(effectiveValue.value));
const choiceIndex = computed(() =>
  props.field.choices.findIndex(
    (choice) => choice.value === effectiveValue.value,
  ),
);
const choiceLabel = computed(() =>
  props.value === undefined
    ? (props.field.choices[choiceIndex.value]?.label ??
      settingValueLabel(currentValue.value, props.field))
    : (props.field.choices[choiceIndex.value]?.label ?? props.value),
);
const minimum = computed(() => props.field.minimum ?? 0);
const hasPosition = computed(() =>
  discrete.value ? choiceIndex.value >= 0 : effectiveNumber.value !== null,
);
const maximum = computed(() =>
  discrete.value ? props.field.choices.length - 1 : numericCeiling.value,
);
const rangeMinimum = computed(() => (discrete.value ? 0 : minimum.value));
const rangeValue = computed(() =>
  discrete.value
    ? Math.max(0, choiceIndex.value)
    : Math.max(
        minimum.value,
        Math.min(maximum.value, effectiveNumber.value ?? minimum.value),
      ),
);
const progress = computed(() =>
  !hasPosition.value || maximum.value <= rangeMinimum.value
    ? 0
    : ((rangeValue.value - rangeMinimum.value) /
        (maximum.value - rangeMinimum.value)) *
      100,
);
const numericStep = computed(() => (props.field.unit === "tokens" ? 1_000 : 1));
const numberModel = computed({
  get: () => effectiveNumber.value ?? undefined,
  set: (value: number | undefined) => {
    // Displaying or confirming an inherited value must not create an override.
    if (value === (effectiveNumber.value ?? undefined)) return;
    emit("update:value", value === undefined ? undefined : String(value));
  },
});
const valueText = computed(() => {
  const value = discrete.value
    ? choiceLabel.value
    : effectiveNumber.value === null
      ? settingValueLabel(currentValue.value, props.field)
      : `${effectiveNumber.value.toLocaleString()} ${props.field.unit}`;
  return props.value === undefined
    ? settingValueLabel(currentValue.value, props.field)
    : value;
});
const rangeTitle = computed(() =>
  discrete.value
    ? valueText.value
    : `拖动范围：${minimum.value.toLocaleString()}–${maximum.value.toLocaleString()} ${props.field.unit}；可在右侧精确输入`,
);
const valueDetail = computed(() => {
  if (!discrete.value && effectiveNumber.value === null)
    return settingValueLabel(currentValue.value, props.field);
  if (props.field.percentageBase && effectiveNumber.value !== null) {
    const percentage =
      (effectiveNumber.value / props.field.percentageBase) * 100;
    return `占上下文 ${percentage.toLocaleString("zh-CN", { maximumFractionDigits: 2 })}%`;
  }
  return "";
});

function numeric(value: string | null | undefined) {
  if (value === undefined || value === null || !value.trim()) return null;
  const result = Number(value);
  return Number.isFinite(result) ? result : null;
}
function ceiling(value: number) {
  const magnitude = 10 ** Math.floor(Math.log10(Math.max(1, value)));
  return Math.ceil(value / magnitude) * magnitude;
}
watch(
  [
    () => props.field.id,
    () => props.field.minimum,
    () => props.field.maximum,
    () => props.field.sliderMaximum,
    () => props.field.unit,
    () => props.field.currentValue,
  ],
  (current, previous) => {
    const sameScale =
      previous &&
      [0, 1, 2, 4].every((index) => current[index] === previous[index]);
    // A persistent draft is parsed after edits. Its updated currentValue must
    // not expand the track or reset pointer state halfway through a drag.
    if (
      previous &&
      current.slice(0, -1).every((value, index) => value === previous[index]) &&
      (props.value !== undefined || dragging.value)
    )
      return;
    // This is a convenient drag range, never a model capacity or an override.
    // The exact input retains the native bounds, including unbounded values.
    numericCeiling.value =
      props.field.maximum ??
      Math.max(
        minimum.value + 1,
        props.field.sliderMaximum ??
          ceiling(
            effectiveNumber.value === null
              ? props.field.unit === "tokens"
                ? 1_000_000
                : 100
              : effectiveNumber.value * 2,
          ),
        props.field.sliderMaximum !== null && sameScale
          ? numericCeiling.value
          : (effectiveNumber.value ?? 0),
      );
    if (!sameScale) {
      customOpen.value = false;
      dragging.value = false;
    }
  },
  { immediate: true },
);
watch(
  () => props.value,
  (value) => {
    const number = numeric(value);
    if (
      props.field.maximum === null &&
      props.field.sliderMaximum === null &&
      number !== null &&
      number > numericCeiling.value
    )
      numericCeiling.value = ceiling(number);
  },
);
watch(customOpen, (open) => {
  if (open) customDraft.value = effectiveValue.value ?? "";
});

function chooseNumber(value: number) {
  if (props.disabled) return;
  emit(
    "update:value",
    String(Math.max(minimum.value, Math.min(maximum.value, value))),
  );
}
function slide(event: Event) {
  if (props.disabled) return;
  const value = Number((event.target as HTMLInputElement).value);
  if (discrete.value) {
    const choice = props.field.choices[value];
    if (choice) emit("update:value", choice.value);
  } else {
    chooseNumber(
      value === minimum.value || value === maximum.value
        ? value
        : Math.round(value / numericStep.value) * numericStep.value,
    );
  }
}
function finishSlide(event: PointerEvent) {
  slide(event);
  dragging.value = false;
}
function stepNumber(event: KeyboardEvent) {
  if (discrete.value || props.disabled) return;
  const directions: Record<string, number> = {
    ArrowRight: 1,
    ArrowUp: 1,
    ArrowLeft: -1,
    ArrowDown: -1,
    PageUp: 10,
    PageDown: -10,
  };
  if (event.key === "Home" || event.key === "End") {
    event.preventDefault();
    chooseNumber(event.key === "Home" ? minimum.value : maximum.value);
  } else if (event.key in directions) {
    event.preventDefault();
    const direction = directions[event.key]!;
    const current =
      (effectiveNumber.value ?? minimum.value) / numericStep.value;
    chooseNumber(
      (direction > 0
        ? Math.floor(current) + direction
        : Math.ceil(current) + direction) * numericStep.value,
    );
  }
}
function formatNumber(value: string) {
  if (!value.trim()) return "";
  const number = numeric(value);
  return number === null ? value : number.toLocaleString("en-US");
}
function parseNumber(value: string) {
  return value.replace(/,/g, "");
}
function applyCustom() {
  if (props.disabled || !customDraft.value.trim()) return;
  emit("update:value", customDraft.value.trim());
  customOpen.value = false;
}
</script>

<template>
  <div
    class="cli-setting-slider"
    :class="{
      'is-discrete': discrete,
      'is-overridden': value !== undefined,
      'is-disabled': disabled,
    }"
  >
    <div class="cli-slider-scale">
      <div
        class="cli-slider-track"
        :class="{ 'is-unset': !hasPosition, 'is-dragging': dragging }"
        :style="{ '--cli-slider-progress': progress / 100 }"
      >
        <div class="cli-slider-visual" aria-hidden="true">
          <span class="cli-slider-rail" />
          <span class="cli-slider-fill" />
          <span class="cli-slider-thumb" />
        </div>
        <input
          :id="discrete ? controlId : undefined"
          type="range"
          :min="rangeMinimum"
          :max="maximum"
          :step="1"
          :value="rangeValue"
          :disabled="disabled || maximum <= rangeMinimum"
          :aria-label="field.label"
          :aria-valuetext="[valueText, valueDetail].filter(Boolean).join('，')"
          :title="rangeTitle"
          @input="slide"
          @pointerdown="dragging = true"
          @pointerup="finishSlide"
          @pointercancel="dragging = false"
          @lostpointercapture="dragging = false"
          @blur="dragging = false"
          @keydown="stepNumber"
        />
      </div>
      <div v-if="discrete" class="cli-slider-marks">
        <button
          v-for="(choice, index) in field.choices"
          :key="choice.value"
          type="button"
          :class="{ selected: index === choiceIndex }"
          :style="{ left: `${(index / (field.choices.length - 1)) * 100}%` }"
          :disabled="disabled"
          :aria-label="`${field.label}：${choice.label}`"
          :aria-pressed="index === choiceIndex"
          :title="choice.description || choice.value"
          @click="emit('update:value', choice.value)"
        >
          {{ choice.label }}
        </button>
      </div>
      <div
        v-else
        class="cli-slider-range-labels"
        :title="rangeTitle"
        aria-hidden="true"
      >
        <span>{{ rangeMinimum.toLocaleString() }}</span>
        <span v-if="hasPosition && valueDetail" class="cli-slider-ratio">{{
          valueDetail
        }}</span>
        <span>{{ maximum.toLocaleString() }}</span>
      </div>
      <p v-if="!hasPosition && valueDetail" class="cli-slider-value-detail">
        {{ valueDetail }}
      </p>
    </div>
    <div class="cli-slider-value-row">
      <button
        v-if="value !== undefined"
        type="button"
        class="cli-slider-reset"
        :disabled="disabled"
        :title="resetHint"
        :aria-label="`${field.label}：${resetLabel}`"
        @click="emit('update:value', undefined)"
      >
        <icon-undo />
        <span>{{ resetLabel }}</span>
      </button>
      <template v-if="discrete">
        <strong :title="valueText">{{ choiceLabel }}</strong>
        <a-popover
          v-if="field.allowCustom"
          v-model:popup-visible="customOpen"
          trigger="click"
          position="bottom"
          title="输入原生档位"
          content-class="cli-native-explanation"
        >
          <button
            type="button"
            class="cli-native-icon-button"
            :disabled="disabled"
            aria-label="输入自定义推理档位"
            title="输入原生档位"
          >
            <icon-edit />
          </button>
          <template #content>
            <form class="cli-slider-custom" @submit.prevent="applyCustom">
              <a-input
                v-model="customDraft"
                :disabled="disabled"
                placeholder="模型支持的原生值"
                aria-label="自定义原生档位"
              />
              <a-button
                html-type="submit"
                type="primary"
                :disabled="disabled || !customDraft.trim()"
                >应用</a-button
              >
            </form>
          </template>
        </a-popover>
      </template>
      <div v-else class="cli-slider-exact">
        <a-input-number
          v-model="numberModel"
          :id="controlId"
          :min="field.minimum ?? undefined"
          :max="field.maximum ?? undefined"
          :precision="0"
          :formatter="formatNumber"
          :parser="parseNumber"
          :placeholder="
            field.currentState === 'readError' ? '读取失败' : '未设置'
          "
          :disabled="disabled"
          hide-button
          :aria-label="`${field.label}精确值`"
          :title="valueText"
        />
        <span>{{ field.unit }}</span>
      </div>
    </div>
  </div>
</template>
