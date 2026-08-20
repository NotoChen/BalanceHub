<script setup lang="ts">
import { computed } from "vue";
import { IconCheck, IconLock } from "@arco-design/web-vue/es/icon";
import type { ProviderApiKeyOption } from "../../stores/providers";
import { formatQuotaValue, maskApiKey } from "../../utils/provider-display";

const props = withDefaults(
  defineProps<{
    options: ProviderApiKeyOption[];
    currentKey: string;
    currentTokenId: string;
    remoteManaged?: boolean;
    selectable?: boolean;
  }>(),
  {
    remoteManaged: true,
    selectable: true,
  },
);

const emit = defineEmits<{
  select: [option: ProviderApiKeyOption];
}>();

const singleOption = computed(() => (props.options.length === 1 ? props.options[0] : null));

function selected(option: ProviderApiKeyOption) {
  if (option.localId.trim() && props.currentKey.trim()) {
    const current = props.options.find((candidate) => candidate.key.trim() === props.currentKey.trim());
    if (current?.localId.trim()) {
      return current.localId.trim() === option.localId.trim();
    }
  }
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

function hasRemoteMetadata(option: ProviderApiKeyOption) {
  return Boolean(
    option.tokenId.trim() ||
      option.userId.trim() ||
      option.unlimitedQuota ||
      option.usedQuotaRaw ||
      option.remainQuotaRaw ||
      option.createdTime ||
      option.accessedTime ||
      option.expiredTime,
  );
}

function displayStatus(option: ProviderApiKeyOption) {
  if (!option.keyAvailable) return "不可读取";
  if (!props.remoteManaged) return "已保存";
  if (!hasRemoteMetadata(option) && !option.status.trim()) return "待同步";
  return statusLabel(option.status);
}

function displayStatusTone(option: ProviderApiKeyOption) {
  if (!option.keyAvailable) return "unknown";
  if (!props.remoteManaged) return "ready";
  if (!hasRemoteMetadata(option) && !option.status.trim()) return "pending";
  return statusTone(option.status);
}

function quotaText(option: ProviderApiKeyOption) {
  if (!props.remoteManaged) {
    return "服务商未提供额度";
  }
  if (!hasRemoteMetadata(option)) {
    return "额度未同步";
  }
  if (option.unlimitedQuota) {
    return "无限额度";
  }
  return `剩余 ${formatKeyQuota(option.remainQuota, option)}`;
}

function quotaSupplement(option: ProviderApiKeyOption) {
  if (!props.remoteManaged) {
    return "仅验证模型接口连通性";
  }
  if (!hasRemoteMetadata(option)) {
    return "刷新后同步 Key 维度额度";
  }
  return `已用 ${formatKeyQuota(option.usedQuota, option)}`;
}

function formatKeyQuota(value: number, option: ProviderApiKeyOption) {
  return formatQuotaValue(value || 0, {
    quotaDisplayType: option.quotaDisplayType || "currency",
    currencySymbol: option.currencySymbol || "$",
  });
}

function restrictionText(option: ProviderApiKeyOption) {
  if (!props.remoteManaged) {
    return "模型与 IP 策略由接口服务商控制";
  }
  if (!hasRemoteMetadata(option)) {
    return "限制信息未同步";
  }
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
  if (!props.remoteManaged) {
    return "已保存到本机配置";
  }
  if (!hasRemoteMetadata(option)) {
    return "时间信息未同步";
  }
  const created = formatUnixTime(option.createdTime);
  const accessed = formatUnixTime(option.accessedTime);
  const expiration = option.expiredTime === -1
    ? "永不过期"
    : option.expiredTime
      ? `过期 ${formatUnixTime(option.expiredTime)}`
      : "未设置过期";
  return `创建 ${created} · 访问 ${accessed} · ${expiration}`;
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
  <article
    v-if="!selectable && singleOption"
    class="provider-api-key-card provider-api-key-single"
    :class="{ unavailable: !singleOption.keyAvailable }"
  >
    <span class="provider-api-key-card-marker provider-api-key-card-marker-static" aria-hidden="true">
      <IconLock />
    </span>
    <span class="provider-api-key-identity">
      <span class="provider-api-key-name-row">
        <strong>{{ singleOption.localName || singleOption.name || "未命名 API Key" }}</strong>
        <small>当前主 Key</small>
      </span>
      <code>{{ keyDisplay(singleOption) }}</code>
    </span>
    <span class="provider-api-key-status" :class="`is-${displayStatusTone(singleOption)}`">
      {{ displayStatus(singleOption) }}
    </span>
    <span class="provider-api-key-details">
      <span class="provider-api-key-detail provider-api-key-detail-quota">
        <small>Key 额度</small>
        <strong>{{ quotaText(singleOption) }}</strong>
        <span>{{ quotaSupplement(singleOption) }}</span>
      </span>
      <span class="provider-api-key-detail" :title="restrictionText(singleOption)">
        <small>调用范围</small>
        <span>{{ restrictionText(singleOption) }}</span>
      </span>
      <span class="provider-api-key-detail" :title="timeText(singleOption)">
        <small>使用记录</small>
        <span>{{ timeText(singleOption) }}</span>
      </span>
    </span>
  </article>
  <div v-else class="provider-api-key-picker" role="radiogroup" aria-label="选择主 API Key">
    <button
      v-for="(option, index) in options"
      :key="option.localId || option.tokenId || option.key || option.maskedKey || `api-key-${index}`"
      type="button"
      class="provider-api-key-card provider-api-key-option"
      :class="{ selected: selected(option), unavailable: !option.keyAvailable }"
      :disabled="!option.keyAvailable"
      :aria-checked="selected(option)"
      role="radio"
      @click="emit('select', option)"
    >
      <span class="provider-api-key-card-marker provider-api-key-radio">
        <IconCheck v-if="selected(option)" />
      </span>
      <span class="provider-api-key-identity">
        <span class="provider-api-key-name-row">
            <strong>{{ option.localName || option.name || "未命名 API Key" }}</strong>
          <small v-if="selected(option)">当前主 Key</small>
        </span>
        <code>{{ keyDisplay(option) }}</code>
      </span>
      <span class="provider-api-key-status" :class="`is-${displayStatusTone(option)}`">
        {{ displayStatus(option) }}
      </span>
      <span class="provider-api-key-details">
        <span class="provider-api-key-detail provider-api-key-detail-quota">
          <small>Key 额度</small>
          <strong>{{ quotaText(option) }}</strong>
          <span>{{ quotaSupplement(option) }}</span>
        </span>
        <span class="provider-api-key-detail" :title="restrictionText(option)">
          <small>调用范围</small>
          <span>{{ restrictionText(option) }}</span>
        </span>
        <span class="provider-api-key-detail" :title="timeText(option)">
          <small>使用记录</small>
          <span>{{ timeText(option) }}</span>
        </span>
      </span>
    </button>
  </div>
</template>
