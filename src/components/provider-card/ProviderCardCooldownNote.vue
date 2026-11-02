<script setup lang="ts">
import { computed } from "vue";
import type { Provider } from "../../stores/providers";
import { providerCooldownNote } from "../../utils/provider-display";

/// 失败退避期间卡片说明自动刷新为什么暂停、什么时候恢复；冷却未生效时
/// 不渲染，和同步时间、备用地址一样只在需要解释当前状态时占一行。
const props = defineProps<{ provider: Provider }>();

const note = computed(() => providerCooldownNote(props.provider));
</script>

<template>
  <span v-if="note.text" class="provider-card-cooldown" :title="note.title">
    {{ note.text }}
  </span>
</template>
