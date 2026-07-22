<script setup lang="ts">
import { computed } from "vue";
import {
  IconCopy,
  IconDelete,
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

function apiKeyStatusTone(status: string) {
  const value = String(status || "").trim().toLowerCase();
  if (value === "1" || value === "enabled") return "enabled";
  if (value === "2" || value === "disabled") return "disabled";
  if (value === "3" || value === "expired") return "expired";
  if (value === "4" || value === "exhausted") return "exhausted";
  return "unknown";
}

function apiKeyQuotaDisplay(option: ProviderApiKeyOption) {
  const quotaDisplay = {
    quotaDisplayType: option.quotaDisplayType || props.provider?.quota.displayType || "currency",
    currencySymbol: option.currencySymbol || props.provider?.quota.currencySymbol || "$",
  };
  if (option.unlimitedQuota) {
    return {
      label: "∞",
      subLabel: `已用 ${formatQuotaValue(option.usedQuota || 0, quotaDisplay)}`,
      percent: 100,
    };
  }

  const remain = Math.max(0, option.remainQuota || 0);
  const used = Math.max(0, option.usedQuota || 0);
  const total = remain + used;
  return {
    label: formatQuotaValue(remain, quotaDisplay),
    subLabel: total > 0
      ? `已用 ${formatQuotaValue(used, quotaDisplay)} / 总量 ${formatQuotaValue(total, quotaDisplay)}`
      : `已用 ${formatQuotaValue(used, quotaDisplay)}`,
    percent: total > 0 ? Math.min(100, Math.max(0, (used / total) * 100)) : 0,
  };
}

function joinedList(values: string[], fallback = "全部") {
  const items = values.map((value) => value.trim()).filter(Boolean);
  return items.length > 0 ? items.join(", ") : fallback;
}

function formatUnixTime(value?: number | null) {
  if (value === -1) return "永不过期";
  if (!value || value < 0) return "-";
  const timestamp = value > 1_000_000_000_000 ? value : value * 1000;
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(new Date(timestamp));
}

function displayMaskedKey(option: ProviderApiKeyOption) {
  return option.maskedKey?.trim() || maskApiKey(option.key) || "完整 Key 不可读取";
}

function isPrimary(option: ProviderApiKeyOption) {
  const provider = props.provider;
  if (!provider) return false;
  if (provider.auth.apiKeyTokenId.trim() && option.tokenId.trim()) {
    return provider.auth.apiKeyTokenId === option.tokenId;
  }
  return Boolean(provider.auth.apiKey.trim()) && provider.auth.apiKey === option.key;
}

function keyIdentity(option: ProviderApiKeyOption) {
  const parts = [];
  if (option.userId) parts.push(`用户 ${option.userId}`);
  if (option.tokenId) parts.push(`ID ${option.tokenId}`);
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
        <span class="surface-modal-title-copy">
          <strong>{{ managerTitle }}</strong>
        </span>
        <span class="surface-modal-title-meta">{{ keys.length }} 个密钥</span>
      </div>
    </template>
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
        <div v-else class="api-key-table-wrap">
          <table class="api-key-table">
            <colgroup>
              <col class="api-key-col-name" />
              <col class="api-key-col-status" />
              <col class="api-key-col-quota" />
              <col class="api-key-col-group" />
              <col class="api-key-col-limits" />
              <col class="api-key-col-time" />
              <col class="api-key-col-actions" />
            </colgroup>
            <thead>
              <tr>
                <th>名称</th>
                <th>状态</th>
                <th>额度</th>
                <th>分组</th>
                <th>限制</th>
                <th>时间</th>
                <th>操作</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="option in keys" :key="`${option.tokenId}-${option.key}`">
                <td class="api-key-name-cell">
                  <strong>{{ option.name || "API 密钥" }}</strong>
                  <span>{{ displayMaskedKey(option) }}</span>
                  <small v-if="keyIdentity(option)">{{ keyIdentity(option) }}</small>
                </td>
                <td>
                  <span class="api-key-status" :class="`api-key-status-${apiKeyStatusTone(option.status)}`">
                    {{ apiKeyStatusLabel(option.status) }}
                  </span>
                  <span v-if="isPrimary(option)" class="api-key-primary-mark">主 Key</span>
                </td>
                <td class="api-key-quota-cell">
                  <strong>{{ apiKeyQuotaDisplay(option).label }}</strong>
                  <span>{{ apiKeyQuotaDisplay(option).subLabel }}</span>
                  <i>
                    <em :style="{ width: `${apiKeyQuotaDisplay(option).percent}%` }" />
                  </i>
                </td>
                <td class="api-key-compact-cell" :title="option.group || '-'">
                  {{ option.group || "-" }}
                </td>
                <td class="api-key-limit-cell">
                  <span :title="joinedList(option.modelLimits)">
                    模型：{{ option.modelLimitsEnabled ? joinedList(option.modelLimits) : "全部" }}
                  </span>
                  <span :title="joinedList(option.allowIps, '不限')">
                    IP：{{ joinedList(option.allowIps, "不限") }}
                  </span>
                  <span>跨组重试：{{ option.crossGroupRetry ? "是" : "否" }}</span>
                </td>
                <td class="api-key-time-cell">
                  <span>创建 {{ formatUnixTime(option.createdTime) }}</span>
                  <span>访问 {{ formatUnixTime(option.accessedTime) }}</span>
                  <span>过期 {{ formatUnixTime(option.expiredTime) }}</span>
                </td>
                <td>
                  <div class="api-key-actions">
                    <a-button size="small" :disabled="!option.keyAvailable" @click="emit('use', option)">
                      {{ isPrimary(option) ? "主 Key" : "设为主 Key" }}
                    </a-button>
                    <a-button size="small" :disabled="!option.keyAvailable" @click="emit('copy', option)">
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
                </td>
              </tr>
            </tbody>
          </table>
          <div class="api-key-card-list">
            <div v-for="option in keys" :key="`card-${option.tokenId}-${option.key}`" class="api-key-card">
              <div class="api-key-card-head">
                <div>
                  <strong>{{ option.name || "API 密钥" }}</strong>
                  <span>{{ displayMaskedKey(option) }}</span>
                </div>
                <span class="api-key-status" :class="`api-key-status-${apiKeyStatusTone(option.status)}`">
                  {{ apiKeyStatusLabel(option.status) }}
                </span>
                <span v-if="isPrimary(option)" class="api-key-primary-mark">主 Key</span>
              </div>
              <div class="api-key-quota-cell">
                <strong>{{ apiKeyQuotaDisplay(option).label }}</strong>
                <span>{{ apiKeyQuotaDisplay(option).subLabel }}</span>
                <i><em :style="{ width: `${apiKeyQuotaDisplay(option).percent}%` }" /></i>
              </div>
              <div class="api-key-card-meta">
                <span>分组 {{ option.group || "-" }}</span>
                <span>模型 {{ option.modelLimitsEnabled ? joinedList(option.modelLimits) : "全部" }}</span>
                <span>IP {{ joinedList(option.allowIps, "不限") }}</span>
                <span>跨组重试 {{ option.crossGroupRetry ? "是" : "否" }}</span>
                <span>访问 {{ formatUnixTime(option.accessedTime) }}</span>
                <span>过期 {{ formatUnixTime(option.expiredTime) }}</span>
              </div>
              <div class="api-key-actions">
                <a-button size="small" :disabled="!option.keyAvailable" @click="emit('use', option)">
                  {{ isPrimary(option) ? "主 Key" : "设为主 Key" }}
                </a-button>
                <a-button size="small" :disabled="!option.keyAvailable" @click="emit('copy', option)">
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
        </div>
      </a-spin>
    </div>
  </a-modal>

  <a-modal
    :visible="createVisible"
    modal-class="surface-modal api-key-create-modal"
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
