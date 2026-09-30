<script setup lang="ts">
import { computed } from "vue";
import { IconFolder } from "@arco-design/web-vue/es/icon";
import { ExternalLink } from "@lucide/vue";
import CardIconButton from "../workspace-card/CardIconButton.vue";
import type { AgentConfigurationSource } from "../../stores/agent-configuration-types";

const props = defineProps<{ source: AgentConfigurationSource | null; busy?: boolean }>();
const emit = defineEmits<{ action: [source: AgentConfigurationSource, action: "open" | "reveal"] }>();
const canOpen = computed(() => props.source?.actions.some((action) => action.action === "open" && action.available));
const canReveal = computed(() => props.source?.actions.some((action) => action.action === "reveal" && action.available));
</script>

<template>
  <div v-if="source && (canOpen || canReveal)" class="agent-configuration-file-actions" role="group" aria-label="配置文件操作">
    <CardIconButton v-if="canReveal" tone="automation" title="在文件管理器中显示" :disabled="busy" @click="emit('action', source, 'reveal')"><IconFolder aria-hidden="true" /><span>定位</span></CardIconButton>
    <CardIconButton v-if="canOpen" tone="models" title="使用系统应用打开文件" :disabled="busy" @click="emit('action', source, 'open')"><ExternalLink :size="14" :stroke-width="1.8" aria-hidden="true" /><span>系统打开</span></CardIconButton>
  </div>
</template>
