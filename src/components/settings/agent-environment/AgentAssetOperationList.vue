<script setup lang="ts">
import { IconLoading } from "@arco-design/web-vue/es/icon";
import type { AgentAssetOperation } from "../../../stores/provider-types";
import { agentAssetOperationOutcomeLabels, agentAssetOperationPhaseLabels } from "../../../composables/useAgentAssetConsole";
import { agentAssetActionLabels } from "../../../composables/useAgentAssetCatalog";

defineProps<{
  operations: AgentAssetOperation[];
  assetLabels: Record<string, string>;
  busy: (id: string) => boolean;
  error: string | null;
}>();
const emit = defineEmits<{ cancel: [id: string]; verify: [id: string]; refresh: [] }>();
</script>

<template>
  <details v-if="operations.length || error" class="agent-asset-operation-list" open>
    <summary>后台资产操作 · {{ operations.length }}</summary>
    <p v-if="error" class="agent-environment-stale-error">{{ error }}</p>
    <div v-for="operation in operations" :key="operation.id" class="agent-asset-operation-item">
      <div><strong>{{ agentAssetActionLabels[operation.action] }} · {{ assetLabels[operation.assetId] || "资产操作" }}</strong><span role="status"><IconLoading v-if="busy(operation.id)" class="agent-version-loading" />{{ operation.outcome ? agentAssetOperationOutcomeLabels[operation.outcome] : agentAssetOperationPhaseLabels[operation.phase] }}</span><p v-if="operation.message">{{ operation.message }}</p><p v-if="operation.reloadEffect">{{ operation.reloadEffect }}</p></div>
      <a-button v-if="operation.canCancel" type="text" size="small" @click="emit('cancel', operation.id)">取消</a-button>
      <a-button v-if="operation.phase === 'completed' && (operation.outcome === 'appliedUnverified' || operation.outcome === 'outcomeUnknown')" type="text" size="small" :disabled="busy(operation.id)" @click="emit('verify', operation.id)">检查结果</a-button>
      <a-button v-else-if="operation.phase !== 'completed' && !busy(operation.id)" type="text" size="small" @click="emit('refresh')">刷新状态</a-button>
    </div>
  </details>
</template>
