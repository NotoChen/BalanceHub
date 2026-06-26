<script setup lang="ts">
import { computed } from "vue";
import {
  IconCopy,
  IconDelete,
  IconPlus,
  IconRefresh,
} from "@arco-design/web-vue/es/icon";
import type { Provider, ProviderApiKeyOption } from "../stores/providers";
import { maskApiKey } from "../utils/provider-display";

const props = defineProps<{
  visible: boolean;
  createVisible: boolean;
  createName: string;
  provider: Provider | null;
  loading: boolean;
  keys: ProviderApiKeyOption[];
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "update:createVisible": [visible: boolean];
  "update:createName": [name: string];
  refresh: [];
  "show-create": [];
  create: [];
  use: [option: ProviderApiKeyOption];
  copy: [option: ProviderApiKeyOption];
  delete: [option: ProviderApiKeyOption];
}>();

const managerTitle = computed(() =>
  props.provider ? `${props.provider.identity.name} · 密钥管理` : "密钥管理",
);

const createNameModel = computed({
  get: () => props.createName,
  set: (value: string) => emit("update:createName", value),
});

function apiKeyStatusLabel(status: string) {
  const value = String(status || "").trim();
  if (!value) return "未知";
  if (value === "1" || value.toLowerCase() === "enabled") return "启用";
  if (value === "2" || value.toLowerCase() === "disabled") return "停用";
  if (value === "3" || value.toLowerCase() === "expired") return "过期";
  if (value === "4" || value.toLowerCase() === "exhausted") return "耗尽";
  return value;
}
</script>

<template>
  <a-modal
    :visible="visible"
    :title="managerTitle"
    :footer="false"
    :width="680"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <div class="api-key-manager">
      <div class="api-key-manager-toolbar">
        <a-button :loading="loading" @click="emit('refresh')">
          <template #icon><icon-refresh /></template>
          刷新列表
        </a-button>
        <a-button type="primary" :loading="loading" @click="emit('show-create')">
          <template #icon><icon-plus /></template>
          创建密钥
        </a-button>
      </div>
      <a-spin :loading="loading">
        <div v-if="keys.length === 0" class="api-key-empty">
          暂无 API 密钥
        </div>
        <div v-else class="api-key-list">
          <div v-for="option in keys" :key="`${option.tokenId}-${option.key}`" class="api-key-row">
            <div class="api-key-info">
              <strong>{{ option.name || "API 密钥" }}</strong>
              <span>{{ apiKeyStatusLabel(option.status) }} · {{ maskApiKey(option.key) }}</span>
            </div>
            <div class="api-key-actions">
              <a-button size="small" @click="emit('use', option)">使用</a-button>
              <a-button size="small" @click="emit('copy', option)">
                <template #icon><icon-copy /></template>
              </a-button>
              <a-button
                size="small"
                status="danger"
                :disabled="!option.tokenId"
                @click="emit('delete', option)"
              >
                <template #icon><icon-delete /></template>
              </a-button>
            </div>
          </div>
        </div>
      </a-spin>
    </div>
  </a-modal>

  <a-modal
    :visible="createVisible"
    title="创建 API 密钥"
    :footer="false"
    :width="420"
    unmount-on-close
    @update:visible="emit('update:createVisible', $event)"
  >
    <div class="api-key-create-form">
      <label class="api-key-create-label" for="api-key-create-name">密钥名称</label>
      <div>
        <a-input
          id="api-key-create-name"
          v-model="createNameModel"
          placeholder="例如：个人电脑、Claude Code、备用密钥"
          allow-clear
          @press-enter="emit('create')"
        />
      </div>
      <div class="api-key-create-actions">
        <a-button @click="emit('update:createVisible', false)">取消</a-button>
        <a-button
          type="primary"
          :loading="loading"
          :disabled="!createNameModel.trim()"
          @click="emit('create')"
        >
          创建
        </a-button>
      </div>
    </div>
  </a-modal>
</template>
