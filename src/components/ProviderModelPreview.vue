<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { selectProviderModels } from "../utils/provider-models";

const props = defineProps<{
  models: string[] | null | undefined;
}>();

const MODEL_GAP = 5;
const MODEL_MEASURE_LIMIT = 32;

const selection = computed(() => selectProviderModels(props.models, MODEL_MEASURE_LIMIT));
const availableModelCount = computed(() =>
  selection.value.groups.reduce((count, group) => count + group.models.length, 0),
);
const visibleModelCount = ref(MODEL_MEASURE_LIMIT);
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

function fitsWithinTwoRows(widths: number[], availableWidth: number) {
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
    if (row > 2) {
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
    if (fitsWithinTwoRows(widths, list.clientWidth)) {
      visibleModelCount.value = count;
      return;
    }
  }

  visibleModelCount.value = 0;
}

function scheduleModelMeasure() {
  if (measureFrame !== null) {
    window.cancelAnimationFrame(measureFrame);
  }
  measureFrame = window.requestAnimationFrame(() => {
    measureFrame = null;
    measureModelPreview();
  });
}

function observeModelList() {
  resizeObserver?.disconnect();
  resizeObserver = null;
  if (typeof ResizeObserver !== "undefined" && modelListRef.value) {
    resizeObserver = new ResizeObserver(scheduleModelMeasure);
    resizeObserver.observe(modelListRef.value);
  }
}

async function resetModelPreview() {
  visibleModelCount.value = selection.value.models.length;
  await nextTick();
  observeModelList();
  scheduleModelMeasure();
}

watch(selection, () => {
  void resetModelPreview();
});

onMounted(() => {
  void resetModelPreview();
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  if (measureFrame !== null) {
    window.cancelAnimationFrame(measureFrame);
  }
});
</script>

<template>
  <section class="provider-card-models" aria-label="可用模型">
    <div class="provider-card-section-heading">
      <span>可用模型</span>
      <span>{{ availableModelCount > 0 ? `${availableModelCount} 个` : "未同步" }}</span>
    </div>

    <div v-if="selection.models.length" ref="modelListRef" class="provider-card-model-list">
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
    <span v-else class="provider-card-model-empty">暂未获取模型列表</span>

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
