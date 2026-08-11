<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { selectProviderModels } from "../utils/provider-models";

const props = withDefaults(
  defineProps<{
    models: string[] | null | undefined;
    rows?: 2 | 5;
    syncTime?: string;
  }>(),
  {
    rows: 2,
    syncTime: "",
  },
);

const MODEL_GAP = 5;
const DEFAULT_MODEL_MEASURE_LIMIT = 32;
const EXPANDED_MODEL_MEASURE_LIMIT = 72;

const modelMeasureLimit = computed(() =>
  props.rows === 5 ? EXPANDED_MODEL_MEASURE_LIMIT : DEFAULT_MODEL_MEASURE_LIMIT,
);
const selection = computed(() => selectProviderModels(props.models, modelMeasureLimit.value));
const availableModelCount = computed(() =>
  selection.value.groups.reduce((count, group) => count + group.models.length, 0),
);
const visibleModelCount = ref(modelMeasureLimit.value);
const visibleModels = computed(() =>
  selection.value.models.slice(0, visibleModelCount.value),
);
const hiddenModelCount = computed(() =>
  Math.max(0, availableModelCount.value - visibleModels.value.length),
);

const modelListRef = ref<HTMLElement | null>(null);
const modelMeasureRef = ref<HTMLElement | null>(null);
const modelMeasureMoreRef = ref<HTMLElement | null>(null);
let resizeObserver: ResizeObserver | null = null;
let measureFrame: number | null = null;
let disposed = false;

function fitsWithinRows(widths: number[], availableWidth: number) {
  let row = 1;
  let usedWidth = 0;

  for (const rawWidth of widths) {
    const width = Math.min(rawWidth, availableWidth);
    if (usedWidth === 0) {
      usedWidth = width;
      continue;
    }
    if (usedWidth + MODEL_GAP + width <= availableWidth + 0.5) {
      usedWidth += MODEL_GAP + width;
      continue;
    }
    row += 1;
    if (row > props.rows) {
      return false;
    }
    usedWidth = width;
  }

  return true;
}

function measureModelPreview() {
  const list = modelListRef.value;
  const measure = modelMeasureRef.value;
  const more = modelMeasureMoreRef.value;
  const candidates = selection.value.models;
  if (!list || !measure || !more || candidates.length === 0 || list.clientWidth <= 0) {
    visibleModelCount.value = candidates.length;
    return;
  }

  const chipWidths = Array.from(
    measure.querySelectorAll<HTMLElement>("[data-model-measure-chip]"),
  ).map((element) => element.offsetWidth);
  if (chipWidths.length !== candidates.length) {
    return;
  }

  const total = availableModelCount.value;
  for (let count = candidates.length; count >= 0; count -= 1) {
    const widths = chipWidths.slice(0, count);
    const hidden = Math.max(0, total - count);
    if (hidden > 0) {
      more.textContent = `+${hidden}`;
      widths.push(more.offsetWidth);
    }
    if (fitsWithinRows(widths, list.clientWidth)) {
      visibleModelCount.value = count;
      return;
    }
  }

  visibleModelCount.value = 0;
}

function scheduleModelMeasure() {
  if (disposed) return;
  if (measureFrame !== null) {
    window.cancelAnimationFrame(measureFrame);
  }
  measureFrame = window.requestAnimationFrame(() => {
    measureFrame = null;
    if (disposed) return;
    measureModelPreview();
  });
}

function observeModelList() {
  if (disposed) return;
  resizeObserver?.disconnect();
  resizeObserver = null;
  if (typeof ResizeObserver !== "undefined" && modelListRef.value) {
    resizeObserver = new ResizeObserver(scheduleModelMeasure);
    resizeObserver.observe(modelListRef.value);
  }
}

async function resetModelPreview() {
  if (disposed) return;
  visibleModelCount.value = selection.value.models.length;
  await nextTick();
  if (disposed) return;
  observeModelList();
  scheduleModelMeasure();
}

watch([selection, () => props.rows], () => {
  void resetModelPreview();
});

onMounted(() => {
  disposed = false;
  void resetModelPreview();
});

onBeforeUnmount(() => {
  disposed = true;
  resizeObserver?.disconnect();
  resizeObserver = null;
  if (measureFrame !== null) {
    window.cancelAnimationFrame(measureFrame);
    measureFrame = null;
  }
});
</script>

<template>
  <section class="provider-card-models" aria-label="可用模型">
    <div class="provider-card-section-heading">
      <span>可用模型</span>
      <span
        v-if="syncTime"
        class="provider-card-model-sync-time"
        :title="`模型同步于 ${syncTime}`"
      >
        同步 {{ syncTime }}
      </span>
      <span>{{ availableModelCount > 0 ? `${availableModelCount} 个` : "未同步" }}</span>
    </div>

    <div
      v-if="selection.models.length"
      ref="modelListRef"
      class="provider-card-model-list"
      :class="{ 'provider-card-model-list-five-rows': rows === 5 }"
    >
      <span
        v-for="model in visibleModels"
        :key="model.name"
        class="provider-card-model"
        :title="`${model.group} / ${model.name}`"
      >
        {{ model.name }}
      </span>
      <span
        v-if="hiddenModelCount > 0"
        class="provider-card-model-more"
        :title="`另有 ${hiddenModelCount} 个模型`"
      >
        +{{ hiddenModelCount }}
      </span>
    </div>
    <span
      v-else
      class="provider-card-model-empty"
      :class="{ 'provider-card-model-empty-five-rows': rows === 5 }"
    >
      暂未获取模型列表
    </span>

    <div
      v-if="selection.models.length"
      ref="modelMeasureRef"
      class="provider-card-model-measure"
      aria-hidden="true"
    >
      <span
        v-for="model in selection.models"
        :key="model.name"
        class="provider-card-model"
        data-model-measure-chip
      >
        {{ model.name }}
      </span>
      <span ref="modelMeasureMoreRef" class="provider-card-model-more">+0</span>
    </div>
  </section>
</template>
