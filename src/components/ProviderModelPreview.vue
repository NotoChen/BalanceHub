<script setup lang="ts">
import { computed, ref, toRef } from "vue";
import { selectProviderModels } from "../utils/provider-models";
import { useBoundedChipPreview } from "../composables/useBoundedChipPreview";

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

const DEFAULT_MODEL_MEASURE_LIMIT = 32;
const EXPANDED_MODEL_MEASURE_LIMIT = 72;

const modelMeasureLimit = computed(() =>
  props.rows === 5 ? EXPANDED_MODEL_MEASURE_LIMIT : DEFAULT_MODEL_MEASURE_LIMIT,
);
const selection = computed(() =>
  selectProviderModels(props.models, modelMeasureLimit.value),
);
const availableModelCount = computed(() =>
  selection.value.groups.reduce(
    (count, group) => count + group.models.length,
    0,
  ),
);
const modelListRef = ref<HTMLElement | null>(null);
const modelMeasureRef = ref<HTMLElement | null>(null);
const modelMeasureMoreRef = ref<HTMLElement | null>(null);
const { visibleCount: visibleModelCount } = useBoundedChipPreview({
  listRef: modelListRef,
  measureRef: modelMeasureRef,
  moreRef: modelMeasureMoreRef,
  count: computed(() => selection.value.models.length),
  total: availableModelCount,
  rows: toRef(props, "rows"),
  revision: selection,
});
const visibleModels = computed(() =>
  selection.value.models.slice(0, visibleModelCount.value),
);
const hiddenModelCount = computed(() =>
  Math.max(0, availableModelCount.value - visibleModels.value.length),
);
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
      <span>{{
        availableModelCount > 0 ? `${availableModelCount} 个` : "未同步"
      }}</span>
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
        class="workspace-card-chip provider-card-model"
        :title="`${model.group} / ${model.name}`"
      >
        {{ model.name }}
      </span>
      <span
        v-if="hiddenModelCount > 0"
        class="workspace-card-chip workspace-card-chip-more provider-card-model-more"
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
        class="workspace-card-chip provider-card-model"
        data-preview-measure-chip
      >
        {{ model.name }}
      </span>
      <span
        ref="modelMeasureMoreRef"
        class="workspace-card-chip workspace-card-chip-more provider-card-model-more"
        >+0</span
      >
    </div>
  </section>
</template>
