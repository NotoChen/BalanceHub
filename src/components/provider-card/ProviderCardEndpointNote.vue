<script setup lang="ts">
import { computed } from "vue";
import type { Provider } from "../../stores/providers";
import { providerEndpointNote } from "../../utils/provider-display";

/// 主地址失败、请求改走备用地址时，卡片在同步时间同一位置说明当前用的是哪个
/// 地址；主地址正常时这一行不出现，不占卡片容量。
const props = defineProps<{ provider: Provider }>();

const note = computed(() => providerEndpointNote(props.provider));
</script>

<template>
  <span v-if="note.text" class="provider-card-endpoint" :title="note.title">
    {{ note.text }}
  </span>
</template>
