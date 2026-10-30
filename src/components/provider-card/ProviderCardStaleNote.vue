<script setup lang="ts">
import { computed } from "vue";
import {
  formatProviderSyncTime,
  providerStaleSyncNote,
} from "../../utils/provider-display";

/// 数据新鲜时卡片不写同步时间；过期了才说一句，避免拿旧数字做判断。四类卡片
/// 共用这一块，说明文字和提示里的准确时刻只有一份实现。
const props = defineProps<{
  syncedAt: string | number | null | undefined;
}>();

const note = computed(() => providerStaleSyncNote(props.syncedAt));
const title = computed(() => {
  const time = formatProviderSyncTime(props.syncedAt);
  return time ? `最近一次同步：${time}` : "";
});
</script>

<template>
  <span v-if="note" class="provider-card-stale" :title="title">{{ note }}</span>
</template>
