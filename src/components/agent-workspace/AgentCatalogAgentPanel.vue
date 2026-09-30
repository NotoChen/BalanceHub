<script setup lang="ts">
import { computed } from "vue";
import AgentCatalogConfigurationActions from "./AgentCatalogConfigurationActions.vue";
import { X, RefreshCw } from "@lucide/vue";
import type { AgentCatalogAction, AgentCatalogAgentPanel } from "../../stores/agent-catalog-types";
import { agentAssetScopeLabels, agentAssetStateLabels } from "../../composables/useAgentAssetCatalog";
import { formatAgentAssetDiagnostics } from "../../utils/agent-environment-diagnostics";

const props = defineProps<{ panel: AgentCatalogAgentPanel | null; label: string; name: string; loading: boolean; error: string; busy: boolean }>();
const emit = defineEmits<{ close: []; retry: []; detail: [targetId?: string]; native: [id: string]; action: [action: AgentCatalogAction, targets: string[]] }>();
const entries = computed(() => props.panel?.entries.map((entry) => {
  const diagnostics = [...new Set(formatAgentAssetDiagnostics(entry.diagnostics))];
  return {
    ...entry, diagnostics,
    notes: [entry.reason].filter((note): note is string => note !== null && note.length > 0 && note !== props.panel?.observation?.reason && !diagnostics.includes(note)),
  };
}) ?? []);
const groups = computed(() => {
  const values = new Map<string, typeof entries.value>();
  for (const entry of entries.value) {
    const key = entry.usage?.groupId ?? `${entry.targetKind}:${entry.targetId}`;
    values.set(key, [...(values.get(key) ?? []), entry]);
  }
  return [...values].map(([id, entries]) => ({ id, entries, usage: entries[0].usage }));
});
</script>

<template>
  <section class="agent-catalog-agent-panel" role="dialog" :aria-label="`${name} 在 ${label} 中的使用`" @keydown.esc.stop="emit('close')">
    <header><div><strong>{{ label }}</strong><span>{{ name }}</span></div><button type="button" class="agent-catalog-icon-action" aria-label="关闭 Agent 使用面板" @click="emit('close')"><X :size="15" aria-hidden="true" /></button></header>
    <p v-if="loading" role="status" class="agent-workspace-note">正在读取状态与可用操作…</p>
    <div v-else-if="error" class="agent-workspace-error" role="alert"><p>{{ error }}</p><a-button size="mini" @click="emit('retry')">重新读取</a-button></div>
    <template v-else-if="panel">
      <p v-if="!entries.length && panel.observation?.reason" class="agent-workspace-note">{{ panel.observation.reason }}</p>
      <p v-if="!entries.length" class="agent-workspace-note">未找到可操作的配置，请打开详情查看来源。</p>
      <div class="agent-catalog-agent-scopes">
        <div v-for="group in groups" :key="group.id">
          <strong v-if="group.usage && group.entries.length > 1">{{ agentAssetStateLabels[group.usage.state] }} · {{ group.entries.length }} 处配置</strong>
          <section v-for="(entry, index) in group.entries" :key="`${entry.targetKind}:${entry.targetId}`" class="agent-catalog-agent-scope" :data-agent-target="entry.targetId" :aria-label="entry.label">
            <header><strong>{{ agentAssetScopeLabels[entry.scope] }}</strong><span>{{ entry.stateLabel }}</span></header>
            <p class="agent-catalog-scope-label">{{ entry.label }}</p><code v-if="entry.path" :title="entry.path">{{ entry.path }}</code>
            <p v-for="diagnostic in entry.diagnostics" :key="diagnostic" class="agent-workspace-note" role="status">{{ diagnostic }}</p>
            <AgentCatalogConfigurationActions :actions="entry.actions" :sync-state="entry.syncState" :busy="busy" :excluded-reasons="entry.diagnostics" :details-notes="[...entry.notes, ...(index === 0 ? [panel.observation?.reason, group.usage?.detail] : [])]" details-label="配置说明" @action="(action, targets) => emit('action', action, targets)" @native="emit('native', $event)">
              <template v-if="entry.targetKind !== 'destination'" #secondary><button type="button" class="agent-catalog-text-action" @click="emit('detail', entry.targetId)">查看来源</button></template>
              <template v-if="panel.syncSource" #details><p>同步来源：{{ panel.syncSource }}</p></template>
            </AgentCatalogConfigurationActions>
          </section>
        </div>
      </div>
      <p v-if="busy" class="agent-workspace-note">此资源正在后台处理，仍可查看详情。</p>
    </template>
    <footer><button type="button" class="agent-catalog-text-action" @click="emit('detail')">打开详情</button><button type="button" class="agent-catalog-icon-action" aria-label="重新读取 Agent 使用详情" :disabled="loading" @click="emit('retry')"><RefreshCw :size="13" aria-hidden="true" /></button></footer>
  </section>
</template>
