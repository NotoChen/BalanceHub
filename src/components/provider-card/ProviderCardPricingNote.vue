<script setup lang="ts">
import { computed } from "vue";
import { formatProviderSyncTime, providerSyncInstant } from "../../utils/provider-display";
import type { Provider } from "../../stores/providers";

/// 站点价格和余额来自不同接口：余额新鲜不代表价格新鲜。价格超过一周或
/// 上次读取失败时，卡片单独说明，别让旧报价参与「哪家便宜」的判断。
const PRICE_STALE_DAYS = 7;

const props = defineProps<{ provider: Provider }>();

const note = computed(() => {
  const pricing = props.provider.capabilities.pricing;
  if (!pricing) return "";
  if (pricing.error?.trim()) {
    const time = formatProviderSyncTime(pricing.updatedAt);
    return time ? `价格读取失败，沿用 ${time} 的读数` : "价格读取失败";
  }
  // updatedAt 是 Rust 写入的毫秒数字串，Date.parse 会得到 NaN；统一走
  // providerSyncInstant，和卡片其他同步时间保持同一套解析。
  const updated = providerSyncInstant(pricing.updatedAt);
  if (!updated) return "";
  const days = Math.floor((Date.now() - updated.getTime()) / 86_400_000);
  return days >= PRICE_STALE_DAYS
    ? `价格已 ${days} 天未更新`
    : "";
});

const title = computed(() => {
  const time = formatProviderSyncTime(props.provider.capabilities.pricing?.updatedAt);
  return time ? `站点价格最近一次读取：${time}` : "";
});
</script>

<template>
  <span v-if="note" class="provider-card-stale" :title="title">{{ note }}</span>
</template>
