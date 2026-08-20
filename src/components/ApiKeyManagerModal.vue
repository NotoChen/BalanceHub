<script setup lang="ts">
import { computed } from "vue";
import {
  IconCopy,
  IconDelete,
  IconEdit,
  IconLock,
  IconPlus,
  IconRefresh,
} from "@arco-design/web-vue/es/icon";
import type { Provider, ProviderApiKeyOption } from "../stores/providers";
import { formatQuotaValue, maskApiKey } from "../utils/provider-display";

const props = defineProps<{
  visible: boolean;
  createVisible: boolean;
  createName: string;
  addVisible: boolean;
  addName: string;
  addValue: string;
  renameVisible: boolean;
  renameName: string;
  provider: Provider | null;
  loading: boolean;
  keys: ProviderApiKeyOption[];
  remoteManaged: boolean;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  "update:createVisible": [visible: boolean];
  "update:createName": [name: string];
  "update:addVisible": [visible: boolean];
  "update:addName": [name: string];
  "update:addValue": [value: string];
  "update:renameVisible": [visible: boolean];
  "update:renameName": [name: string];
  refresh: [];
  "show-create": [];
  "show-add": [];
  "show-rename": [option: ProviderApiKeyOption];
  create: [];
  "add-local": [];
  rename: [];
  "set-primary": [option: ProviderApiKeyOption];
  copy: [option: ProviderApiKeyOption];
  delete: [option: ProviderApiKeyOption];
}>();

const managerTitle = computed(() =>
  props.provider ? `${props.provider.identity.name} · 密钥库` : "密钥库",
);
const createNameModel = computed({
  get: () => props.createName,
  set: (value: string) => emit("update:createName", value),
});
const addNameModel = computed({
  get: () => props.addName,
  set: (value: string) => emit("update:addName", value),
});
const addValueModel = computed({
  get: () => props.addValue,
  set: (value: string) => emit("update:addValue", value),
});
const renameNameModel = computed({
  get: () => props.renameName,
  set: (value: string) => emit("update:renameName", value),
});

function displayName(option: ProviderApiKeyOption) {
  return option.localName || option.name || "API Key";
}

function displayMaskedKey(option: ProviderApiKeyOption) {
  return option.maskedKey?.trim() || maskApiKey(option.key) || "完整 Key 不可读取";
}

function isPrimary(option: ProviderApiKeyOption) {
  const provider = props.provider;
  if (!provider) return false;
  const localId = option.localId.trim();
  if (localId) {
    const primary = props.keys.find((candidate) =>
      candidate.key.trim() === provider.auth.apiKey.trim(),
    );
    if (primary?.localId.trim()) return primary.localId.trim() === localId;
  }
  const tokenId = provider.auth.apiKeyTokenId.trim();
  if (tokenId && option.tokenId.trim()) return tokenId === option.tokenId.trim();
  return Boolean(provider.auth.apiKey.trim()) && provider.auth.apiKey.trim() === option.key.trim();
}

function statusLabel(status: string) {
  const value = String(status || "").trim().toLowerCase();
  if (value === "1" || value === "enabled") return "启用";
  if (value === "2" || value === "disabled") return "停用";
  if (value === "3" || value === "expired") return "过期";
  if (value === "4" || value === "exhausted") return "耗尽";
  return value || "已保存";
}

function statusTone(status: string) {
  const value = String(status || "").trim().toLowerCase();
  if (value === "1" || value === "enabled") return "enabled";
  if (value === "2" || value === "disabled") return "disabled";
  if (value === "3" || value === "expired") return "expired";
  if (value === "4" || value === "exhausted") return "exhausted";
  return "unknown";
}

function quotaText(option: ProviderApiKeyOption) {
  if (!option.tokenId) {
    return "本地 Key";
  }
  if (!option.usedQuotaRaw && !option.remainQuotaRaw && !option.unlimitedQuota) return "站点 Key";
  if (option.unlimitedQuota) return "无限额度";
  return `剩余 ${formatQuotaValue(option.remainQuota || 0, {
    quotaDisplayType: option.quotaDisplayType || "currency",
    currencySymbol: option.currencySymbol || "$",
  })}`;
}

function keyIdentity(option: ProviderApiKeyOption) {
  const parts = [];
  if (option.tokenId) parts.push(`站点 ID ${option.tokenId}`);
  if (option.userId) parts.push(`用户 ${option.userId}`);
  return parts.join(" · ");
}
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal api-key-manager-modal"
    :footer="false"
    :width="980"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title api-key-manager-title">
        <span class="surface-modal-title-icon"><icon-lock /></span>
        <span class="surface-modal-title-copy"><strong>{{ managerTitle }}</strong></span>
        <span class="surface-modal-title-meta">{{ keys.length }} 个密钥</span>
      </div>
    </template>

    <div class="api-key-manager">
      <div class="api-key-manager-toolbar">
        <span class="api-key-manager-hint">本地密钥和站点密钥统一维护，主 Key 用于默认请求与临时 CLI。</span>
        <a-button :loading="loading" @click="emit('refresh')">
          <template #icon><icon-refresh /></template>刷新
        </a-button>
        <a-button :loading="loading" @click="emit('show-add')">
          <template #icon><icon-plus /></template>加入本地 Key
        </a-button>
        <a-button v-if="remoteManaged" type="primary" :loading="loading" @click="emit('show-create')">
          <template #icon><icon-plus /></template>创建站点 Key
        </a-button>
      </div>

      <a-spin :loading="loading">
        <div v-if="keys.length === 0" class="api-key-empty">暂无 API Key。可以先加入本地 Key，或从站点创建。</div>
        <div v-else class="api-key-vault-list">
          <article v-for="option in keys" :key="option.localId || option.tokenId || option.key" class="api-key-vault-item">
            <div class="api-key-vault-main">
              <div class="api-key-vault-title-row">
                <strong>{{ displayName(option) }}</strong>
                <span v-if="isPrimary(option)" class="api-key-primary-badge">主 Key</span>
                <span class="api-key-source-badge">{{ option.tokenId ? "站点" : "本地" }}</span>
                <span class="api-key-status" :class="`api-key-status-${statusTone(option.status)}`">{{ statusLabel(option.status) }}</span>
              </div>
              <code>{{ displayMaskedKey(option) }}</code>
              <small v-if="keyIdentity(option)">{{ keyIdentity(option) }}</small>
            </div>
            <div class="api-key-vault-summary">
              <span>{{ quotaText(option) }}</span>
              <span v-if="option.group">分组 {{ option.group }}</span>
              <span v-if="option.modelLimitsEnabled">模型 {{ option.modelLimits.length }}</span>
              <span v-if="option.allowIps.length">IP {{ option.allowIps.length }}</span>
            </div>
            <div class="api-key-actions">
              <a-tooltip content="设为主 Key">
                <a-button size="small" type="text" :disabled="!option.keyAvailable || !option.localId || isPrimary(option)" aria-label="设为主 Key" @click="emit('set-primary', option)">
                  <template #icon><icon-lock /></template>
                </a-button>
              </a-tooltip>
              <a-tooltip content="复制 API Key">
                <a-button size="small" type="text" :disabled="!option.keyAvailable" aria-label="复制 API Key" @click="emit('copy', option)">
                  <template #icon><icon-copy /></template>
                </a-button>
              </a-tooltip>
              <a-tooltip content="重命名">
                <a-button size="small" type="text" :disabled="!option.localId" aria-label="重命名" @click="emit('show-rename', option)">
                  <template #icon><icon-edit /></template>
                </a-button>
              </a-tooltip>
              <a-tooltip :content="option.tokenId ? '删除站点 Key' : '移除本地 Key'">
                <a-button size="small" type="text" status="danger" :disabled="(!option.localId && !option.tokenId) || (Boolean(option.tokenId) && !remoteManaged)" aria-label="删除或移除 API Key" @click="emit('delete', option)">
                  <template #icon><icon-delete /></template>
                </a-button>
              </a-tooltip>
            </div>
          </article>
        </div>
      </a-spin>
    </div>
  </a-modal>

  <a-modal :visible="createVisible" modal-class="surface-modal api-key-create-modal" title="创建站点 API Key" :footer="false" :width="420" unmount-on-close @update:visible="emit('update:createVisible', $event)">
    <div class="api-key-create-form">
      <label class="api-key-create-label">密钥名称</label>
      <a-input v-model="createNameModel" placeholder="例如：Claude Code、备用密钥" allow-clear @press-enter="emit('create')" />
      <div class="api-key-create-actions"><a-button type="primary" :loading="loading" :disabled="!createNameModel.trim()" @click="emit('create')">创建</a-button></div>
    </div>
  </a-modal>

  <a-modal :visible="addVisible" modal-class="surface-modal api-key-create-modal" title="加入本地 API Key" :footer="false" :width="460" unmount-on-close @update:visible="emit('update:addVisible', $event)">
    <div class="api-key-create-form">
      <label class="api-key-create-label">完整 API Key</label>
      <a-input-password v-model="addValueModel" allow-clear placeholder="粘贴完整 API Key" />
      <label class="api-key-create-label">名称</label>
      <a-input v-model="addNameModel" allow-clear placeholder="例如：备用 Key" @press-enter="emit('add-local')" />
      <div class="api-key-create-actions"><a-button type="primary" :loading="loading" :disabled="!addValueModel.trim()" @click="emit('add-local')">加入密钥库</a-button></div>
    </div>
  </a-modal>

  <a-modal :visible="renameVisible" modal-class="surface-modal api-key-create-modal" title="重命名 API Key" :footer="false" :width="420" unmount-on-close @update:visible="emit('update:renameVisible', $event)">
    <div class="api-key-create-form">
      <label class="api-key-create-label">名称</label>
      <a-input v-model="renameNameModel" allow-clear @press-enter="emit('rename')" />
      <div class="api-key-create-actions"><a-button type="primary" :loading="loading" :disabled="!renameNameModel.trim()" @click="emit('rename')">保存名称</a-button></div>
    </div>
  </a-modal>
</template>
