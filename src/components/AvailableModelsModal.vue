<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IconCloud, IconCopy, IconRefresh, IconSearch } from "@arco-design/web-vue/es/icon";
import type { Provider } from "../stores/providers";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  loading: boolean;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  refresh: [];
  copy: [model: string];
  copyAll: [];
}>();

const keyword = ref("");

watch(
  () => props.visible,
  (visible) => {
    if (!visible) {
      keyword.value = "";
    }
  },
);

const modalTitle = computed(() =>
  props.provider ? `${props.provider.identity.name} · 可用模型` : "可用模型",
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
        content="获取模型列表需要 API Key；当前实现会使用 API Key 调用 OpenAI 兼容的 /models 接口。"
      />

      <div class="available-models-toolbar">
        <a-input v-model="keyword" allow-clear placeholder="搜索模型">
          <template #prefix><icon-search /></template>
        </a-input>
        <a-button :disabled="models.length === 0" @click="emit('copyAll')">
          <template #icon><icon-copy /></template>
          复制全部
        </a-button>
        <a-button type="primary" :loading="loading" :disabled="!canRefresh" @click="emit('refresh')">
          <template #icon><icon-refresh /></template>
          刷新
        </a-button>
      </div>

      <a-spin :loading="loading">
        <div v-if="models.length === 0" class="available-models-empty">
          暂无模型列表
        </div>
        <div v-else class="available-models-body">
          <div class="available-models-summary">
            <span>模型数量</span>
            <strong>{{ filteredModels.length }} / {{ models.length }}</strong>
          </div>
          <div class="available-models-list">
            <button
              v-for="model in filteredModels"
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
        </div>
      </a-spin>
    </div>
  </a-modal>
</template>
