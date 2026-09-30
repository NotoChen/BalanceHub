<script setup lang="ts">
import { computed } from "vue";
import { IconFile } from "@arco-design/web-vue/es/icon";
import { agentAssetScopeLabels } from "../../composables/useAgentAssetCatalog";
import { agentConfigurationFileLabel, agentConfigurationOpenAction, agentConfigurationReadOnlyReason, agentConfigurationSourceStatus } from "../../utils/agent-configuration-display";
import type { AgentConfigurationSnapshot, AgentConfigurationSource } from "../../stores/agent-configuration-types";

const props = defineProps<{ label: string; snapshot: AgentConfigurationSnapshot | null; loading: boolean; error: string; stale: boolean }>();
const emit = defineEmits<{ openFile: [sourceId: string]; refresh: [] }>();
const rank = (source: AgentConfigurationSource) => source.format === "markdown" ? 2
  : source.actions.some((action) => action.action === "edit" && action.available) ? 0 : 1;
const rows = computed(() => (props.snapshot?.sources ?? []).filter((source) => !source.revision.isMissing)
  .sort((left, right) => rank(left) - rank(right) || left.path.localeCompare(right.path)));
const issues = computed(() => [...new Set([
  ...(props.snapshot?.diagnostics ?? []).filter((diagnostic) => diagnostic.severity !== "info").map((diagnostic) => diagnostic.message),
  ...rows.value.flatMap((source) => source.diagnostics.filter((diagnostic) => diagnostic.severity !== "info")
    .map((diagnostic) => `${agentConfigurationFileLabel(source, rows.value)}：${diagnostic.message}`)),
  ...rows.value.filter((source) => !canOpen(source)).map((source) => `${agentConfigurationFileLabel(source, rows.value)}：${unavailableReason(source)}`),
])]);
function canOpen(source: AgentConfigurationSource) { return source.access.kind === "ready" && Boolean(agentConfigurationOpenAction(source)); }
function unavailableReason(source: AgentConfigurationSource) {
  return source.actions.find((action) => ["read", "edit"].includes(action.action) && action.reason)?.reason || "文件当前不可访问，请刷新后重试";
}
function detail(source: AgentConfigurationSource) {
  const reason = canOpen(source) ? agentConfigurationReadOnlyReason(source) : unavailableReason(source);
  return `${source.path}\n配置范围：${agentAssetScopeLabels[source.scope]}${source.profile && source.profile !== "default" ? ` · ${source.profile}` : ""}${reason ? `\n${reason}` : ""}`;
}
function statusLabel(source: AgentConfigurationSource) {
  const presence = agentConfigurationSourceStatus(source).presence;
  if (presence !== "已存在") return presence;
  return agentConfigurationReadOnlyReason(source) ? "只读" : "";
}
</script>

<template>
  <section class="agent-card-configuration" :aria-label="`${label} 配置文件`" :aria-busy="loading || undefined">
    <h3 class="agent-card-section-heading">配置文件</h3>
    <div v-if="rows.length" class="agent-card-file-list">
      <button v-for="source in rows" :key="source.sourceId" type="button" class="workspace-card-chip agent-card-file" :disabled="!canOpen(source)" :title="detail(source)" :aria-label="`打开 ${agentConfigurationFileLabel(source, rows)}${statusLabel(source) ? '，' + statusLabel(source) : ''}`" @click="emit('openFile', source.sourceId)">
        <IconFile :size="13" aria-hidden="true" /><span class="agent-card-file-name">{{ agentConfigurationFileLabel(source, rows) }}</span><span v-if="statusLabel(source)" class="agent-card-file-state">{{ statusLabel(source) }}</span>
      </button>
    </div>
    <p v-else-if="!error" class="agent-card-file-empty" role="status">{{ loading ? '正在读取配置文件…' : snapshot ? '暂无本地配置文件' : '尚未读取配置文件' }}</p>
    <div v-if="error || issues.length || (stale && snapshot) || (!snapshot && !loading)" class="agent-card-configuration-issues">
      <p v-if="error" role="alert">{{ error }}</p><p v-else-if="stale && snapshot" role="status">配置文件信息待刷新。</p>
      <p v-for="issue in issues" :key="issue">{{ issue }}</p>
      <button type="button" :disabled="loading" @click="emit('refresh')">{{ loading ? '正在重试…' : '重新读取' }}</button>
    </div>
  </section>
</template>
