<script setup lang="ts">
import AgentUpgradeDiagnostics from "./AgentUpgradeDiagnostics.vue";
import { computed } from "vue";
import { useAgentCatalogStore } from "../../stores/agent-catalog";
import { useAgentLifecycleStore } from "../../stores/agent-lifecycle";
import { agentAssetOperationOutcomeLabels, agentAssetOperationPhaseLabels } from "../../composables/useAgentAssetConsole";
import { agentCatalogActionLabels, agentCatalogDefinitionChangeDetail } from "../../utils/agent-catalog-display";
import type { AgentCliKind } from "../../stores/provider-types";
import type { AgentOperationSelection } from "../../stores/agent-workspace";

const props = defineProps<{ selection: AgentOperationSelection; selectedPaths: Partial<Record<AgentCliKind, string>>; savingPath: AgentCliKind | null; pathErrors: Partial<Record<AgentCliKind, string>> }>();
const emit = defineEmits<{ usePath: [kind: AgentCliKind, path: string] }>();
const catalog = useAgentCatalogStore();
const lifecycle = useAgentLifecycleStore();
const catalogOperations = computed(() => Object.values(catalog.operations).filter((operation) => props.selection.kind === "catalog" && operation.id === props.selection.id));
const lifecycleOperations = computed(() => Object.values(lifecycle.operations).filter((operation) => props.selection.kind === "lifecycle" && operation.id === props.selection.id));
</script>

<template>
  <section class="agent-operation-details" aria-label="Agent 任务详情">
    <p v-if="!catalogOperations.length && !lifecycleOperations.length" class="agent-workspace-note">未找到此任务，请返回任务中心刷新。</p>
    <article v-for="operation in lifecycleOperations" :key="operation.id" class="agent-operation-result">
      <header><strong>{{ operation.agentKind }} · 升级</strong><span role="status">{{ operation.outcome ? agentAssetOperationOutcomeLabels[operation.outcome] : agentAssetOperationPhaseLabels[operation.phase] }}</span><a-button v-if="operation.canCancel" size="mini" type="text" :loading="lifecycle.canceling[operation.id]" @click="lifecycle.cancel(operation.id)">取消</a-button><a-button v-if="(operation.phase !== 'completed' || operation.outcome === 'outcomeUnknown') && !lifecycle.polling[operation.id]" size="mini" type="text" @click="lifecycle.recover">刷新状态</a-button></header>
      <p>{{ operation.channelLabel }}<template v-if="operation.observedVersion"> · 检测版本 {{ operation.observedVersion }}</template></p><p v-if="operation.message">{{ operation.message }}</p><p v-if="lifecycle.errors[operation.id]" class="agent-workspace-error">{{ lifecycle.errors[operation.id] }}</p>
      <AgentUpgradeDiagnostics :operation="operation" />
      <div v-if="operation.verifiedExecutablePath" class="agent-operation-path"><code>{{ operation.verifiedExecutablePath }}</code><span v-if="operation.nextLaunch?.usesUpgradedInstallation">应用当前使用</span><a-button v-else size="small" :loading="savingPath === operation.agentKind" :disabled="savingPath !== null && savingPath !== operation.agentKind" @click="emit('usePath', operation.agentKind, operation.verifiedExecutablePath)">采用为启动路径</a-button></div>
      <div v-if="operation.nextLaunch" class="agent-operation-path"><p>{{ operation.nextLaunch.message }}</p><span v-if="operation.nextLaunch.version">下次启动版本 {{ operation.nextLaunch.version }}</span><code v-if="operation.nextLaunch.executablePath">{{ operation.nextLaunch.executablePath }}</code></div>
      <p v-if="pathErrors[operation.agentKind]" class="agent-workspace-error" role="alert">{{ pathErrors[operation.agentKind] }}</p>
    </article>
    <article v-for="operation in catalogOperations" :key="operation.id" class="agent-operation-result">
      <header><strong>{{ agentCatalogActionLabels[operation.action] }}</strong><span role="status">{{ agentAssetOperationPhaseLabels[operation.phase] }}</span><a-button v-if="operation.canCancel" size="mini" type="text" :loading="catalog.canceling[operation.id]" @click="catalog.cancel(operation.id)">取消</a-button><a-button v-if="operation.phase !== 'completed' && !catalog.polling[operation.id]" size="mini" type="text" @click="catalog.recover">刷新状态</a-button></header>
      <p v-if="catalog.operationNames[operation.id]">{{ catalog.operationNames[operation.id] }}</p><p v-if="operation.definitionChange" role="status">{{ agentCatalogDefinitionChangeDetail(operation) }}</p>
      <p v-if="catalog.errors[operation.id]" class="agent-workspace-error">{{ catalog.errors[operation.id] }}</p><div v-for="target in operation.targets" :key="target.targetId" class="agent-operation-target"><strong>{{ target.label }}</strong><span>{{ target.outcome ? agentAssetOperationOutcomeLabels[target.outcome] : agentAssetOperationPhaseLabels[target.phase] }}</span><p v-if="target.message">{{ target.message }}</p></div>
    </article>
  </section>
</template>
