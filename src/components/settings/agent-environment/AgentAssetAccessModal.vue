<script setup lang="ts">
import { agentAssetAccessRiskLabels, type AgentAssetAccessConfirmation } from "../../../composables/useAgentAssetConsole";

defineProps<{ confirmation: AgentAssetAccessConfirmation | null }>();
const emit = defineEmits<{ close: []; confirm: [] }>();
</script>

<template>
  <a-modal :visible="Boolean(confirmation)" width="min(540px, calc(100vw - 32px))" modal-class="surface-modal" title-align="start" closable mask-closable esc-to-close unmount-on-close :footer="false" @update:visible="(value: boolean) => !value && emit('close')">
    <template #title>{{ confirmation?.action === 'reveal' ? "在文件管理器中显示" : "使用外部应用打开" }}</template>
    <div v-if="confirmation" class="agent-asset-plan">
      <strong>{{ confirmation.label }}</strong><code class="agent-asset-detail-path">{{ confirmation.path }}</code>
      <ul v-if="confirmation.risks.length" class="agent-asset-diagnostics"><li v-for="risk in confirmation.risks" :key="risk">{{ agentAssetAccessRiskLabels[risk] }}</li></ul>
      <p class="agent-asset-detail-note">本次确认仅对这一次打开有效。</p>
      <div class="agent-asset-modal-actions"><a-button @click="emit('close')">取消</a-button><a-button type="primary" @click="emit('confirm')">确认继续</a-button></div>
    </div>
  </a-modal>
</template>
