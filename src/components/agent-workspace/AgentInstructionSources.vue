<script setup lang="ts">
import { computed, onScopeDispose, reactive, ref, watch } from "vue";
import { FileText, Info, RefreshCw } from "@lucide/vue";
import type { AgentCliDescriptor, AgentCliKind, AgentRuntimeScope } from "../../stores/provider-types";
import type { AgentConfigurationSource } from "../../stores/agent-configuration-types";
import { useAgentConfigurationSourcesStore } from "../../stores/agent-configuration-sources";
import { useAgentWslStore } from "../../stores/agent-wsl";
import { useAgentConfiguration } from "../../composables/useAgentConfiguration";
import { agentAssetScopeLabels } from "../../composables/useAgentAssetCatalog";
import { agentConfigurationFileName, agentConfigurationReadOnlyReason, agentConfigurationSourceStatus } from "../../utils/agent-configuration-display";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentConfigurationEditorModal from "./AgentConfigurationEditorModal.vue";
import AgentConfigurationPreviewModal from "./AgentConfigurationPreviewModal.vue";
import AgentConfigurationFileActions from "./AgentConfigurationFileActions.vue";
import AgentAssetAccessModal from "../settings/agent-environment/AgentAssetAccessModal.vue";

const props = defineProps<{
  agents: AgentCliDescriptor[];
  active: boolean;
  workspace?: string;
  query: string;
}>();
const publications = useAgentConfigurationSourcesStore();
const wsl = useAgentWslStore();
const environment = ref("");
const agentFilter = ref<AgentCliKind | null>(null);
const selectedAgent = ref<AgentCliKind | null>(null);
const runtimeScope = computed<AgentRuntimeScope>(() => environment.value
  ? { kind: "wsl", distro_id: environment.value } : { kind: "native" });
const workspace = computed(() => environment.value ? undefined : props.workspace);
const cache = computed(() => publications.forRuntime(runtimeScope.value));
const model = reactive(useAgentConfiguration({ agentKind: selectedAgent, workspace, runtimeScope }));
const entries = computed(() => props.agents.map((agent) => ({ agent, entry: cache.value.get(agent.kind, workspace.value) })));
const loading = computed(() => entries.value.some(({ entry }) => entry.loading));
const query = computed(() => props.query.trim().toLocaleLowerCase());
const groups = computed(() => entries.value.map(({ agent, entry }) => {
  // Keep inaccessible candidates visible; only a confirmed missing file is omitted.
  // Search must not turn an existing file into an Agent with no rules.
  const sources = (entry.value?.sources ?? []).filter((source) => source.format === "markdown"
    && (!source.revision.isMissing || source.access.kind !== "ready"))
    .sort((left, right) => Number(left.scope === "workspace") - Number(right.scope === "workspace") || left.path.localeCompare(right.path));
  return { agent, entry, sources, ruleCount: sources.filter((source) => source.contentState === "nonEmpty").length };
}));
const selectedGroups = computed(() => groups.value.filter(({ agent }) =>
  !agentFilter.value || agentFilter.value === agent.kind));
const files = computed(() => selectedGroups.value.flatMap(({ agent, sources }) => sources
  .filter((source) => !query.value || [agent.label, agent.kind, source.label, source.path]
    .join("\n").toLocaleLowerCase().includes(query.value))
  .map((source) => ({ agent, source }))));
const ruleCount = computed(() => files.value.filter(({ source }) => source.contentState === "nonEmpty").length);
const failedGroups = computed(() => selectedGroups.value.filter(({ entry }) => entry.error));
const readingAgentNames = computed(() => selectedGroups.value
  .filter(({ entry }) => entry.loading || (!entry.value && !entry.error))
  .map(({ agent }) => agent.label).join("、"));
const staleAgentNames = computed(() => selectedGroups.value
  .filter(({ entry }) => entry.stale && entry.value && !entry.loading && !entry.error)
  .map(({ agent }) => agent.label).join("、"));
const emptyAgentNames = computed(() => selectedGroups.value
  .filter(({ sources, entry }) => !sources.length && entry.value && !entry.stale && !entry.loading && !entry.error)
  .map(({ agent }) => agent.label).join("、"));
let subscriptions: (() => void)[] = [];
function release() { for (const unsubscribe of subscriptions) unsubscribe(); subscriptions = []; }
watch(() => wsl.supported, (supported) => { if (supported === false) environment.value = ""; });
watch([() => props.active, workspace, environment, () => props.agents.map((agent) => agent.kind).join("|")], () => {
  release();
  if (!props.active) { selectedAgent.value = null; model.invalidate(); return; }
  void wsl.ensure();
  subscriptions = props.agents.map((agent) => cache.value.subscribe(agent.kind, workspace.value));
}, { immediate: true, flush: "sync" });
onScopeDispose(release);
function refresh() {
  return Promise.allSettled(entries.value.filter(({ agent }) => !agentFilter.value || agentFilter.value === agent.kind)
    .map(({ agent }) => cache.value.refresh(agent.kind, workspace.value)));
}
function available(source: AgentConfigurationSource, action: "read" | "edit") {
  return source.access.kind === "ready" && source.actions.some((item) => item.action === action && item.available);
}
function open(source: AgentConfigurationSource, action: "read" | "edit") {
  selectedAgent.value = source.agentKind;
  model.action(source.sourceId, action, source);
}
function unavailableReason(source: AgentConfigurationSource) {
  return source.actions.find((action) => action.action === "read")?.reason
    || source.diagnostics.find((diagnostic) => diagnostic.severity !== "info")?.message
    || "此规则文件当前无法读取，请检查路径和权限后重新读取。";
}
defineExpose({ refresh });
</script>

<template>
  <section class="instruction-sources" aria-labelledby="instruction-sources-title" :aria-busy="loading || undefined">
    <header class="instruction-sources-header">
      <div class="instruction-sources-heading">
        <div class="instruction-sources-title">
          <h2 id="instruction-sources-title">Agent 规则</h2>
          <span v-if="files.length || !loading" class="instruction-source-count">{{ ruleCount }} 份已填写</span>
          <a-popover trigger="click" position="bottom" title="规则读取范围">
            <a-button type="text" size="mini" shape="circle" class="instruction-help-button" aria-label="了解规则读取范围"><Info :size="14" /></a-button>
            <template #content><div class="instruction-help-copy"><p>读取各 Agent 原有的规则文件。切换右上角的配置范围，可查看项目规则。</p><p>空文件不计入已填写数量，可直接编写规则。</p></div></template>
          </a-popover>
        </div>
        <a-button type="text" size="small" :loading="loading" @click="refresh"><template #icon><RefreshCw :size="14" /></template>刷新</a-button>
      </div>
      <div class="instruction-source-filters">
        <div class="instruction-agent-filters" role="group" aria-label="筛选已有规则的 Agent">
          <button type="button" :aria-pressed="!agentFilter" @click="agentFilter = null">全部</button>
          <button v-for="group in groups" :key="group.agent.kind" type="button" :aria-pressed="agentFilter === group.agent.kind" @click="agentFilter = group.agent.kind">
            <AgentCliIcon :kind="group.agent.kind" :size="15" />{{ group.agent.label }}
            <span v-if="group.entry.value && !group.entry.error" class="instruction-filter-count">{{ group.ruleCount }}</span>
          </button>
        </div>
        <a-select v-if="wsl.supported === true && wsl.inventory?.distributions.length" v-model="environment" size="small" aria-label="已有规则的运行环境">
          <a-option value="">本机</a-option>
          <a-option v-for="distro in wsl.inventory.distributions" :key="distro.id" :value="distro.id" :disabled="!distro.running || Boolean(distro.error)">WSL · {{ distro.name }}{{ distro.running ? '' : '（已停止）' }}</a-option>
        </a-select>
      </div>
      <p v-if="environment" class="instruction-source-note">当前查看所选 WSL 发行版的用户规则。</p>
    </header>
    <ul v-if="files.length" class="instruction-source-list" aria-label="规则文件">
      <li v-for="{ agent, source } in files" :key="`${agent.kind}:${source.sourceId}`" class="instruction-source-file" :class="{ 'is-empty': source.contentState === 'empty' }">
        <AgentCliIcon class="instruction-source-icon" :kind="agent.kind" :size="22" />
        <div class="instruction-source-info">
          <div class="instruction-source-name">
            <h3>{{ agentConfigurationFileName(source) }}</h3>
            <span>{{ agent.label }} · {{ agentAssetScopeLabels[source.scope] }}级</span>
            <span v-if="agentConfigurationSourceStatus(source).presence !== '已存在'" class="instruction-source-state">{{ agentConfigurationSourceStatus(source).presence }}</span>
            <span v-else-if="agentConfigurationReadOnlyReason(source)" class="instruction-source-state">只读</span>
            <a-popover v-if="available(source, 'read')" trigger="click" position="bottom" title="规则读取说明">
              <a-button type="text" size="mini" shape="circle" class="instruction-help-button" :aria-label="`${agent.label} ${agentConfigurationFileName(source)} 的读取说明`"><Info :size="14" /></a-button>
              <template #content><div class="instruction-help-copy"><p>{{ source.effect.message }}</p><p v-if="source.reloadHint">{{ source.reloadHint }}</p></div></template>
            </a-popover>
          </div>
          <code :title="source.path">{{ source.path }}</code>
          <p v-if="!available(source, 'read')" class="agent-workspace-error">{{ unavailableReason(source) }}</p>
        </div>
        <div class="instruction-source-actions">
          <a-button size="small" :disabled="!available(source, 'read')" :aria-label="`查看 ${agent.label} 的 ${agentConfigurationFileName(source)}`" @click="open(source, 'read')">查看</a-button>
          <a-button size="small" :type="source.contentState === 'empty' ? 'outline' : 'text'" :disabled="!available(source, 'edit')" :aria-label="`${source.contentState === 'empty' ? '编写' : '编辑'} ${agent.label} 的 ${agentConfigurationFileName(source)}`" :title="agentConfigurationReadOnlyReason(source)" @click="open(source, 'edit')">{{ source.contentState === 'empty' ? '编写规则' : '编辑' }}</a-button>
        </div>
      </li>
    </ul>
    <div v-else-if="!readingAgentNames && !failedGroups.length && !staleAgentNames" class="instruction-source-empty" role="status">
      <FileText :size="22" aria-hidden="true" />
      <div>
        <strong>{{ query ? '没有匹配的规则文件' : agents.length ? '当前范围内还没有规则文件' : '当前没有可读取的 Agent' }}</strong>
        <p>{{ query ? '可按 Agent、文件名或路径搜索。' : '已有文件会显示在这里，也可切换配置范围查看项目规则。' }}</p>
      </div>
    </div>
    <div v-if="readingAgentNames || failedGroups.length || staleAgentNames || (files.length && !query && emptyAgentNames)" class="instruction-source-feedback">
      <p v-if="readingAgentNames" class="instruction-source-note" role="status">正在读取 {{ readingAgentNames }} 的规则…</p>
      <div v-for="group in failedGroups" :key="group.agent.kind" class="instruction-source-error agent-workspace-error" role="alert">
        <div><strong>{{ group.agent.label }} · 读取失败</strong><p>{{ group.entry.error }}</p></div>
        <a-button size="small" :loading="group.entry.loading" @click="cache.refresh(group.agent.kind, workspace)">重试</a-button>
      </div>
      <p v-if="staleAgentNames" class="instruction-source-note">{{ staleAgentNames }} 的文件信息待刷新，打开时会重新核对。</p>
      <p v-if="files.length && !query && emptyAgentNames" class="instruction-source-note">{{ emptyAgentNames }}：当前范围内没有规则文件。</p>
    </div>
    <AgentConfigurationEditorModal :model="model.editor">
      <template #file-actions="{ sourceId }"><AgentConfigurationFileActions :source="model.editorSource(sourceId)" :busy="model.opening[sourceId]" @action="(source, action) => model.action(source.sourceId, action, source)" /></template>
    </AgentConfigurationEditorModal>
    <AgentConfigurationPreviewModal :model="model" />
    <AgentAssetAccessModal :confirmation="model.accessConfirmation" @close="model.closeAccessConfirmation" @confirm="model.confirmAccess" />
  </section>
</template>

<style scoped>
.instruction-sources { min-width: 0; border: 1px solid var(--surface-line); border-radius: 12px; background: var(--surface-panel); }
.instruction-sources-header { padding: 14px 18px; }
.instruction-sources-heading, .instruction-sources-title { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
.instruction-sources-heading { justify-content: space-between; gap: 12px; }
.instruction-sources-heading h2 { margin: 0; color: var(--surface-ink); font-size: 15px; font-weight: 650; }
.instruction-source-count { color: var(--surface-ink-soft); font-size: 12px; font-variant-numeric: tabular-nums; }
.instruction-help-button { color: var(--surface-ink-soft); }
.instruction-help-copy { max-width: min(320px, calc(100vw - 72px)); color: var(--surface-ink-soft); font-size: 12px; line-height: 1.75; overflow-wrap: anywhere; }
.instruction-help-copy p { margin: 0; }
.instruction-help-copy p + p { margin-top: 8px; }
.instruction-source-note { margin: 0; color: var(--surface-ink-soft); font-size: 12px; line-height: 1.7; overflow-wrap: anywhere; }
.instruction-sources-header > .instruction-source-note { margin-top: 10px; }
.instruction-source-filters { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 10px; margin-top: 12px; }
.instruction-agent-filters { display: flex; min-width: 0; flex-wrap: wrap; gap: 4px; }
.instruction-agent-filters button { display: inline-flex; align-items: center; gap: 6px; border: 0; border-radius: 6px; padding: 6px 10px; background: transparent; color: var(--surface-ink-soft); font: inherit; font-size: 12px; cursor: pointer; }
.instruction-agent-filters button:hover { background: var(--surface-panel-soft); }
.instruction-agent-filters button[aria-pressed="true"] { background: var(--surface-accent-soft); color: var(--surface-accent); font-weight: 600; }
.instruction-agent-filters button:focus-visible { outline: 2px solid var(--surface-accent); outline-offset: 2px; }
.instruction-filter-count { font-size: 11px; font-variant-numeric: tabular-nums; }
.instruction-source-filters :deep(.arco-select-view) { width: min(240px, 100%); }
.instruction-source-list { margin: 0; padding: 0; list-style: none; }
.instruction-source-file { display: grid; min-width: 0; grid-template-columns: 24px minmax(0, 1fr) auto; align-items: center; gap: 10px 12px; border-top: 1px solid var(--surface-line); padding: 14px 18px; }
.instruction-source-file:hover { background: var(--surface-panel-soft); }
.instruction-source-file.is-empty { background: var(--surface-panel-soft); }
.instruction-source-file:last-child { border-radius: 0 0 12px 12px; }
.instruction-source-icon { align-self: start; margin-top: 3px; }
.instruction-source-info { display: grid; min-width: 0; gap: 4px; }
.instruction-source-name { display: flex; min-width: 0; flex-wrap: wrap; align-items: center; gap: 4px 10px; }
.instruction-source-name h3 { margin: 0; color: var(--surface-ink); font-size: 14px; font-weight: 600; overflow-wrap: anywhere; }
.instruction-source-name > span { color: var(--surface-ink-soft); font-size: 12px; }
.instruction-source-name > .instruction-source-state { border: 1px solid var(--surface-line-strong); border-radius: 4px; padding: 1px 5px; font-size: 11px; }
.instruction-source-info > code { overflow: hidden; color: var(--surface-ink-soft); font-size: 12px; line-height: 1.7; text-overflow: ellipsis; white-space: nowrap; }
.instruction-source-info > .agent-workspace-error { margin: 4px 0 0; }
.instruction-source-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; }
.instruction-source-feedback { display: grid; gap: 8px; border-top: 1px solid var(--surface-line); padding: 10px 18px; }
.instruction-source-error { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 16px; margin: 0; }
.instruction-source-error > div { flex: 1 1 220px; min-width: 0; }
.instruction-source-error strong { font-weight: 600; }
.instruction-source-error p { margin: 3px 0 0; }
.instruction-source-empty { display: flex; align-items: center; gap: 12px; border-top: 1px solid var(--surface-line); padding: 20px 18px; color: var(--surface-ink-soft); font-size: 12px; }
.instruction-source-empty > svg { flex: 0 0 auto; }
.instruction-source-empty strong { color: var(--surface-ink); font-size: 13px; font-weight: 500; }
.instruction-source-empty p { margin: 4px 0 0; line-height: 1.7; }
@container agent-instructions (max-width: 640px) {
  .instruction-source-file { grid-template-columns: 24px minmax(0, 1fr); }
  .instruction-source-actions { grid-column: 2; justify-self: start; }
  .instruction-sources-header, .instruction-source-file, .instruction-source-empty, .instruction-source-feedback { padding-right: 14px; padding-left: 14px; }
}
</style>
