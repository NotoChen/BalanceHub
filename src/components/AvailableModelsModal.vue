<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IconCloud, IconCopy, IconRefresh, IconSearch } from "@arco-design/web-vue/es/icon";
import type { Provider } from "../stores/providers";
import { providerDisplayLabel } from "../utils/provider-display";

const MODELS_PER_PAGE = 100;

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  loading: boolean;
  error: string;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  refresh: [];
  copy: [model: string];
  copyAll: [models: string[]];
}>();

const keyword = ref("");
const page = ref(1);
const modelList = ref<HTMLElement | null>(null);

watch(
  [() => props.visible, () => props.provider?.identity.id],
  () => { keyword.value = ""; page.value = 1; },
);

const modalTitle = computed(() =>
  props.provider ? `${providerDisplayLabel(props.provider)} · 可用模型` : "可用模型",
);

const models = computed(() =>
  Array.from(
    new Set(
      (props.provider?.capabilities.availableModels ?? [])
        .map((model) => model.trim())
        .filter(Boolean),
    ),
  ).sort((left, right) => left.localeCompare(right)),
);

const filteredModels = computed(() => {
  const filter = keyword.value.trim().toLowerCase();
  if (!filter) return models.value;
  return models.value.filter((model) => model.toLowerCase().includes(filter));
});

const displayedModels = computed(() => filteredModels.value.slice((page.value - 1) * MODELS_PER_PAGE, page.value * MODELS_PER_PAGE));
watch(keyword, () => { page.value = 1; });
watch(() => filteredModels.value.length, (count) => { page.value = Math.min(page.value, Math.max(1, Math.ceil(count / MODELS_PER_PAGE))); });
watch([page, keyword], () => { if (modelList.value) modelList.value.scrollTop = 0; }, { flush: "post" });

const canRefresh = computed(() => Boolean(props.provider?.auth.apiKey.trim()));
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal available-models-modal"
    :footer="false"
    :width="720"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title available-models-title">
        <span class="surface-modal-title-icon"><icon-cloud /></span>
        <span class="surface-modal-title-copy">
          <strong>{{ modalTitle }}</strong>
        </span>
      </div>
    </template>
    <div class="available-models-panel">
      <a-alert
        v-if="provider && !provider.auth.apiKey.trim()"
        type="warning"
      >
        请先在中转站的认证凭据中填写 API Key，再获取可用模型。
      </a-alert>

      <div class="available-models-toolbar">
        <a-input v-model="keyword" allow-clear placeholder="搜索模型名称" aria-label="搜索模型名称">
          <template #prefix><icon-search /></template>
        </a-input>
        <a-button :disabled="filteredModels.length === 0" @click="emit('copyAll', filteredModels)">
          <template #icon><icon-copy /></template>
          {{ keyword.trim() ? '复制筛选结果' : '复制全部' }}
        </a-button>
        <a-button type="primary" :loading="loading" :disabled="!canRefresh" @click="emit('refresh')">
          <template #icon><icon-refresh /></template>
          刷新
        </a-button>
      </div>
      <a-alert v-if="error" type="error" show-icon>
        <div class="panel-load-error"><span>{{ error }}</span><a-button size="small" :disabled="!canRefresh" @click="emit('refresh')">重试</a-button></div>
      </a-alert>

      <a-spin :loading="loading">
        <div v-if="models.length === 0" class="available-models-empty">
          {{ loading ? '正在获取模型列表…' : error ? '模型列表未能加载' : '暂无可用模型' }}
        </div>
        <div v-else class="available-models-body">
          <div class="available-models-summary">
            <span>{{ keyword.trim() ? `匹配 ${filteredModels.length.toLocaleString()} 个 · 共 ${models.length.toLocaleString()} 个模型` : `共 ${models.length.toLocaleString()} 个模型` }}</span>
            <span>点击模型名称即可复制</span>
          </div>
          <div v-if="filteredModels.length === 0" class="available-models-empty">
            <span>没有匹配的模型</span>
            <a-button type="text" @click="keyword = ''">清除搜索</a-button>
          </div>
          <div v-else ref="modelList" class="available-models-list">
            <button
              v-for="model in displayedModels"
              :key="model"
              type="button"
              class="available-model-item"
              :title="model"
              @click="emit('copy', model)"
            >
              <span>{{ model }}</span>
              <icon-copy />
            </button>
          </div>
          <div v-if="filteredModels.length > MODELS_PER_PAGE" class="available-models-pagination">
            <span>每页 {{ MODELS_PER_PAGE }} 个</span>
            <a-pagination v-model:current="page" :total="filteredModels.length" :page-size="MODELS_PER_PAGE" size="small" simple />
          </div>
        </div>
      </a-spin>
    </div>
  </a-modal>
</template>
