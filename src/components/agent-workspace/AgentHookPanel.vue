<script setup lang="ts">
import { computed, ref, watch, type UnwrapNestedRefs } from "vue";
import type { useAgentEnvironmentCenter } from "../../composables/useAgentEnvironmentCenter";
import type { useAgentHookConsole } from "../../composables/useAgentHookConsole";
import type { AgentAssetCatalog, AgentCatalogAction, AgentCatalogAgentPanel } from "../../stores/agent-catalog-types";
import type { AgentCatalogAgentSelection } from "../../composables/useAgentCatalogAgentPanel";
import type { AgentAssetCategory, AgentCliDescriptor, AgentCliKind } from "../../stores/provider-types";
import { agentHookTargetKey } from "../../utils/agent-runtime";
import AgentCatalogPanel from "./AgentCatalogPanel.vue";
import AgentEnvironmentRow from "../settings/agent-environment/AgentEnvironmentRow.vue";
import AgentCatalogRow from "./AgentCatalogRow.vue";
import { agentCatalogBindingMatches, agentCatalogDefinitionMatches, agentCatalogUnresolvedMatches } from "../../utils/agent-catalog-display";
import "../../styles/modules/agent-hook-panel.css";

const props = defineProps<{
  catalog: AgentAssetCatalog | null;
  agents: AgentCliDescriptor[];
  agentFilter: AgentCliKind | null;
  query: string;
  focusedAssetId: string | null;
  loading: boolean;
  error: string;
  busy: (assetId: string) => boolean;
  rowError?: (assetId: string) => string;
  agentSelection?: AgentCatalogAgentSelection | null; agentPanel?: AgentCatalogAgentPanel | null; agentLoading?: boolean; agentError?: string;
  center: UnwrapNestedRefs<ReturnType<typeof useAgentEnvironmentCenter>>;
  hooks: UnwrapNestedRefs<ReturnType<typeof useAgentHookConsole>>;
}>();
const emit = defineEmits<{
  detail: [id: string, feature?: string]; create: [category: AgentAssetCategory];
  action: [id: string, action: AgentCatalogAction, targets?: string[]];
  manage: [id: string, kind: AgentCliKind | null]; retry: []; native: [id: string];
  installation: [kind: AgentCliKind, id: string];
}>();
const integrationAgents = computed(() => props.agents.filter((agent) => !props.agentFilter || agent.kind === props.agentFilter));
const integrationOpen = ref(false);
function toggleIntegration(event: Event) { integrationOpen.value = (event.currentTarget as HTMLDetailsElement).open; }
watch([integrationOpen, () => integrationAgents.value.map((agent) => agent.kind).join("|")], () => {
  if (!integrationOpen.value) return;
  for (const agent of integrationAgents.value) {
    if (!props.hooks.inspectionFor(agent.kind) && !props.hooks.isRowBusy(agentHookTargetKey(agent.kind))) void props.hooks.inspect(agent.kind);
  }
});
const labels = computed(() => new Map(props.agents.map((agent) => [agent.kind, agent.label])));
const statusUiRows = computed(() => (props.catalog?.assets ?? []).filter((asset) => asset.category === "statusUi" && (
  asset.bindings.some((binding) => (!props.agentFilter || binding.native.agentKind === props.agentFilter)
    && agentCatalogBindingMatches(asset, binding, props.query, labels.value, null))
  || asset.unresolvedTargets.some((target) => (!props.agentFilter || target.agentKind === props.agentFilter)
    && agentCatalogUnresolvedMatches(asset, target, props.query, labels.value))
  || (!props.agentFilter && !asset.bindings.length && !asset.unresolvedTargets.length && agentCatalogDefinitionMatches(asset, props.query))
)));
</script>

<template>
  <section class="agent-hook-panel" aria-label="Hook 管理">
    <AgentCatalogPanel :catalog="catalog" page="hook" :query="query" :agent-filter="agentFilter"
      :agents="agents" :loading="loading" :error="error" :busy="busy" :row-error="rowError" :focused-native-id="focusedAssetId"
      :agent-selection="agentSelection" :agent-panel="agentPanel" :agent-loading="agentLoading" :agent-error="agentError"
      @detail="(id, feature) => emit('detail', id, feature)"
      @create="emit('create', $event)" @action="(id, action, targets) => emit('action', id, action, targets)" @manage="(id, kind) => emit('manage', id, kind)" @retry="emit('retry')" @native="emit('native', $event)" />
    <div class="agent-hook-auxiliary">
    <details class="agent-hook-integration" aria-label="BalanceHub 会话状态接入" @toggle="toggleIntegration">
      <summary>BalanceHub 会话状态接入<span>可选辅助功能</span></summary>
      <div v-if="integrationOpen" class="agent-hook-integration-content">
        <p class="agent-workspace-note">用于在 BalanceHub 查看本机 Agent 的会话状态。接入配置位于用户级，与所选项目范围无关。</p>
        <AgentEnvironmentRow v-for="agent in integrationAgents" :key="agent.kind" :agent="agent"
          :installations="center.inventory?.installations.filter((item) => item.agentKind === agent.kind) || []"
          :inspection="hooks.inspectionFor(agent.kind)" :busy="hooks.isRowBusy(agentHookTargetKey(agent.kind))"
          :error="hooks.rowError(agentHookTargetKey(agent.kind))"
          @detail="emit('installation', agent.kind, $event)" @inspect="hooks.inspect(agent.kind, 'health')"
          @verify="hooks.inspect(agent.kind, 'verify')" @repair="hooks.requestPlan(agent.kind, 'install', true)"
          @mutate="hooks.requestPlan(agent.kind, $event)" />
      </div>
    </details>
    <section v-if="statusUiRows.length" class="agent-integration-section" aria-label="状态栏配置">
      <h3>状态栏配置</h3>
      <div role="list" aria-label="状态栏资产">
        <AgentCatalogRow v-for="asset in statusUiRows" :key="asset.id" :asset="asset" :labels="labels" :busy="busy(asset.id)" :error="rowError?.(asset.id)" :focused="asset.bindings.some((binding) => binding.id === focusedAssetId)" :agent-filter="agentFilter"
          :agent-selection="agentSelection" :agent-panel="agentPanel" :agent-loading="agentLoading" :agent-error="agentError"
          @detail="(id, feature) => emit('detail', id, feature)" @action="(id, action, targets) => emit('action', id, action, targets)"
          @manage="(id, kind) => emit('manage', id, kind)" @retry="emit('retry')" @native="emit('native', $event)" />
      </div>
    </section>
    </div>
  </section>
</template>
