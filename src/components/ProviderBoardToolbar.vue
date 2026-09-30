<script setup lang="ts">
import { computed, ref } from "vue";
import { GripVertical, X } from "@lucide/vue";
import RadioChoiceGroup from "./RadioChoiceGroup.vue";
import {
  providerFilters,
  type ProviderFilterCounts,
  type ProviderFilter,
} from "../utils/provider-filters";

const props = defineProps<{
  filter: ProviderFilter;
  counts: ProviderFilterCounts;
  visibleCount: number;
  totalCount: number;
  hasSearch: boolean;
}>();

const emit = defineEmits<{
  select: [filter: ProviderFilter];
  reset: [];
}>();

const filtersRef = ref<HTMLElement | null>(null);
const filtered = computed(() => props.filter !== "all" || props.hasSearch);

function reset() {
  emit("reset");
  filtersRef.value
    ?.querySelector<HTMLInputElement>('input[type="radio"]')
    ?.focus();
}
</script>

<template>
  <div ref="filtersRef" class="provider-board-toolbar">
    <RadioChoiceGroup
      :model-value="filter"
      :options="providerFilters"
      label="筛选中转站"
      class="provider-board-filters"
      option-class="provider-board-filter-option"
      @update:model-value="emit('select', $event)"
    >
      <template #default="{ option }">
        <span>{{ option.label }}</span>
        <span class="provider-board-filter-count">{{
          counts[option.value]
        }}</span>
      </template>
    </RadioChoiceGroup>
    <div v-if="filtered" class="provider-board-filter-summary">
      <span
        role="status"
        aria-live="polite"
        aria-atomic="true"
        :aria-label="`显示 ${visibleCount} / ${totalCount} 个中转站`"
        :title="`显示 ${visibleCount} / ${totalCount} 个中转站`"
        >{{ visibleCount }} / {{ totalCount }}</span
      >
      <button
        type="button"
        aria-label="重置筛选"
        title="清除全部筛选条件"
        @click="reset"
      >
        <X :size="13" aria-hidden="true" />
      </button>
    </div>
    <span v-else class="provider-board-sort-hint">
      <GripVertical :size="13" aria-hidden="true" />
      拖拽排序
    </span>
  </div>
</template>
