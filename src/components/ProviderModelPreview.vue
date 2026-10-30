<script setup lang="ts">
import { computed, ref } from "vue";
import { providerModelChips } from "../utils/provider-models";
import { useBoundedChipPreview } from "../composables/useBoundedChipPreview";
import type { ProviderModelListState } from "../stores/provider-types";

/// 四类卡片（中转站、API Key、账号授权、CPA 网关）共用这一块模型区：
/// 同样的标题行、同样的三行芯片、同样点开完整列表。卡型只决定传进来的数据，
/// 不再决定模型区长什么样。
const props = withDefaults(
  defineProps<{
    models: string[] | null | undefined;
    state: ProviderModelListState;
    syncTime?: string;
    interactive?: boolean;
  }>(),
  { syncTime: "", interactive: true },
);
const emit = defineEmits<{ open: [] }>();

/// 模型区固定三行，和页脚一样是卡片的固定层，不随卡型改行数。
const MODEL_ROWS = 3;
/// 测量上限只是防止把上千个模型全部塞进隐藏测量层；三行远到不了。
const MODEL_MEASURE_LIMIT = 72;

const selection = computed(() =>
  providerModelChips(props.models, MODEL_MEASURE_LIMIT),
);
const total = computed(() => selection.value.total);
const models = computed(() => selection.value.names);

const listRef = ref<HTMLElement | null>(null);
const measureRef = ref<HTMLElement | null>(null);
const moreRef = ref<HTMLElement | null>(null);
const { visibleCount } = useBoundedChipPreview({
  listRef,
  measureRef,
  moreRef,
  count: computed(() => models.value.length),
  total,
  rows: MODEL_ROWS,
  revision: selection,
});
const visibleModels = computed(() => models.value.slice(0, visibleCount.value));
const hidden = computed(() => Math.max(0, total.value - visibleModels.value.length));

/// 标题右侧只报总数或失败原因；同步时刻属于排查信息，放进悬停提示，四类卡片
/// 一视同仁，不再有的写「同步 10/29 16:05 UTC+8」有的什么都不写。
const statusText = computed(() =>
  props.state.error
    ? "同步失败"
    : total.value > 0 || props.state.updatedAt
      ? `${total.value} 个`
      : "未同步",
);
const emptyText = computed(() =>
  props.state.error
    ? "模型列表未能加载"
    : props.state.updatedAt
      ? "暂无可用模型"
      : "暂未获取模型列表",
);
const entryTitle = computed(() =>
  [
    props.state.error ? "模型列表同步失败" : `${total.value} 个可用模型`,
    props.syncTime ? `同步于 ${props.syncTime}` : "",
    props.interactive ? "点击查看完整列表" : "",
  ]
    .filter(Boolean)
    .join(" · "),
);
</script>

<template>
  <section class="provider-card-models" aria-label="可用模型">
    <button
      type="button"
      class="provider-card-model-entry"
      :disabled="!props.interactive"
      :title="entryTitle"
      @click.stop="emit('open')"
      @pointerdown.stop
    >
      <span class="provider-card-section-heading">
        <span>可用模型</span>
        <span
          class="provider-card-section-heading-status"
          :title="state.error || ''"
          >{{ statusText }}</span
        >
      </span>

      <span
        v-if="models.length"
        ref="listRef"
        class="provider-card-model-list"
      >
        <span
          v-for="name in visibleModels"
          :key="name"
          class="workspace-card-chip"
          :title="name"
        >
          {{ name }}
        </span>
        <span
          v-if="hidden > 0"
          class="workspace-card-chip workspace-card-chip-more"
          :title="`另有 ${hidden} 个模型`"
        >
          +{{ hidden }}
        </span>
      </span>
      <span v-else class="provider-card-model-empty">{{ emptyText }}</span>

      <!-- 量宽用的隐藏副本：芯片和「+N」都按真实字号排版，再决定这一块能放下
           几个。只读不显示，不参与布局。 -->
      <span
        v-if="models.length"
        ref="measureRef"
        class="provider-card-model-measure"
        aria-hidden="true"
      >
        <span
          v-for="name in models"
          :key="name"
          class="workspace-card-chip"
          data-preview-measure-chip
        >
          {{ name }}
        </span>
        <span
          ref="moreRef"
          class="workspace-card-chip workspace-card-chip-more"
          >+0</span
        >
      </span>
    </button>
  </section>
</template>
