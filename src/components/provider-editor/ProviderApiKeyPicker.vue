<script setup lang="ts">
import { computed } from "vue";
import { IconCheck } from "@arco-design/web-vue/es/icon";
import type { ProviderApiKeyOption } from "../../stores/providers";
import { formatQuotaValue, maskApiKey } from "../../utils/provider-display";

const props = withDefaults(
  defineProps<{
    options: ProviderApiKeyOption[];
    currentKey: string;
    currentTokenId: string;
    selectable?: boolean;
  }>(),
  { selectable: true },
);

const emit = defineEmits<{
  select: [option: ProviderApiKeyOption];
}>();

const singleOption = computed(() => (props.options.length === 1 ? props.options[0] : null));

function selected(option: ProviderApiKeyOption) {
  if (props.currentTokenId.trim() && option.tokenId.trim()) {
    return props.currentTokenId.trim() === option.tokenId.trim();
  }
  return Boolean(props.currentKey.trim()) && props.currentKey.trim() === option.key.trim();
}

function keyDisplay(option: ProviderApiKeyOption) {
  return option.maskedKey?.trim() || maskApiKey(option.key) || "完整 Key 不可读取";
}

function statusLabel(status: string) {
  const value = String(status || "").trim().toLowerCase();
  if (value === "1" || value === "enabled") return "启用";
  if (value === "2" || value === "disabled") return "停用";
  if (value === "3" || value === "expired") return "过期";
  if (value === "4" || value === "exhausted") return "耗尽";
  return value || "未知";
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
  if (option.unlimitedQuota) {
    return `无限额度 · 已用 ${formatKeyQuota(option.usedQuota, option)}`;
  }
  return `剩余 ${formatKeyQuota(option.remainQuota, option)} · 已用 ${formatKeyQuota(option.usedQuota, option)}`;
}

function formatKeyQuota(value: number, option: ProviderApiKeyOption) {
  return formatQuotaValue(value || 0, {
    quotaDisplayType: option.quotaDisplayType || "currency",
    currencySymbol: option.currencySymbol || "$",
  });
}

function restrictionText(option: ProviderApiKeyOption) {
  const parts = [option.group ? `分组 ${option.group}` : "默认分组"];
  if (option.modelLimitsEnabled) {
    parts.push(`模型 ${option.modelLimits.length || 0}`);
  } else {
    parts.push("模型不限");
  }
  parts.push(option.allowIps.length > 0 ? `IP ${option.allowIps.length}` : "IP 不限");
  if (option.crossGroupRetry) parts.push("跨组重试");
  return parts.join(" · ");
}

function timeText(option: ProviderApiKeyOption) {
  const created = formatUnixTime(option.createdTime);
  const accessed = formatUnixTime(option.accessedTime);
  const expired = option.expiredTime === -1 ? "永不过期" : formatUnixTime(option.expiredTime);
  return `创建 ${created} · 访问 ${accessed} · ${expired === "-" ? "未设置过期" : `过期 ${expired}`}`;
}

function formatUnixTime(value?: number | null) {
  if (!value || value < 0) return value === -1 ? "永不过期" : "-";
  const timestamp = value > 1_000_000_000_000 ? value : value * 1000;
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(new Date(timestamp));
}
</script>

<template>
  <div
    v-if="!selectable && singleOption"
    class="provider-api-key-single"
    :class="{ unavailable: !singleOption.keyAvailable }"
  >
    <span class="provider-api-key-single-identity">
      <strong>{{ singleOption.name || "未命名 API Key" }}</strong>
      <code>{{ keyDisplay(singleOption) }}</code>
    </span>
    <span class="provider-api-key-single-quota">{{ quotaText(singleOption) }}</span>
    <span class="provider-api-key-single-restrictions" :title="restrictionText(singleOption)">
      {{ restrictionText(singleOption) }}
    </span>
    <span class="provider-api-key-single-time" :title="timeText(singleOption)">
      {{ timeText(singleOption) }}
    </span>
    <span class="provider-api-key-status" :class="`is-${statusTone(singleOption.status)}`">
      {{ singleOption.keyAvailable ? statusLabel(singleOption.status) : "不可读取" }}
    </span>
  </div>
  <div v-else class="provider-api-key-picker" role="radiogroup" aria-label="选择主 API Key">
    <button
      v-for="(option, index) in options"
      :key="option.tokenId || option.key || option.maskedKey || `api-key-${index}`"
      type="button"
      class="provider-api-key-option"
      :class="{ selected: selected(option), unavailable: !option.keyAvailable }"
      :disabled="!option.keyAvailable"
      :aria-checked="selected(option)"
      role="radio"
      @click="emit('select', option)"
    >
      <span class="provider-api-key-radio"><IconCheck v-if="selected(option)" /></span>
      <span class="provider-api-key-identity">
        <strong>{{ option.name || "未命名 API Key" }}</strong>
        <code>{{ keyDisplay(option) }}</code>
      </span>
      <span class="provider-api-key-quota">{{ quotaText(option) }}</span>
      <span class="provider-api-key-restrictions" :title="restrictionText(option)">
        {{ restrictionText(option) }}
      </span>
      <span class="provider-api-key-time" :title="timeText(option)">{{ timeText(option) }}</span>
      <span class="provider-api-key-status" :class="`is-${statusTone(option.status)}`">
        {{ option.keyAvailable ? statusLabel(option.status) : "不可读取" }}
      </span>
    </button>
  </div>
</template>
