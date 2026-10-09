<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import { storeToRefs } from "pinia";
import { ChevronDown, CircleAlert, CircleHelp, Copy, FolderOpen, RefreshCw, X } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import { useAgentSessions } from "../../composables/useAgentSessions";
import { useAgentWorkspaceStore } from "../../stores/agent-workspace";
import { useAgentSessionResumeStore } from "../../stores/agent-session-resume";
import { useWorkspaceStore } from "../../stores/workspaces";
import { copyText } from "../../composables/useClipboard";
import type { AgentCliKind, AgentInstallation, AgentRuntimeSession, AgentRuntimeSnapshot, CliRuntimeSnapshot, Provider } from "../../stores/provider-types";
import type { AgentSessionRow, AgentSessionSourceState } from "../../stores/agent-session-types";
import { agentSessionSourceLabels } from "../../utils/agent-session-display";
import AgentSessionList from "./AgentSessionList.vue";
import AgentSessionResumeModal from "./AgentSessionResumeModal.vue";
import AgentWorkspaceIcon from "./AgentWorkspaceIcon.vue";
import AgentRuntimeList from "../AgentRuntimeList.vue";
import CliSessionDetailModal from "../CliSessionDetailModal.vue";

const props = defineProps<{
  active: boolean; providers: Provider[]; installations: AgentInstallation[]; cliRuntime: CliRuntimeSnapshot;
  runtimeSnapshot: AgentRuntimeSnapshot; runtimeSessions: AgentRuntimeSession[]; runtimeLoading: boolean; activatingId: string | null;
}>();
const emit = defineEmits<{ refreshRuntime: []; activateRuntime: [session: AgentRuntimeSession] }>();
const navigation = useAgentWorkspaceStore();
const { sessionHistoryQuery: query } = storeToRefs(navigation);
const workspaces = useWorkspaceStore();
const tasks = useAgentSessionResumeStore();
const historyActive = computed(() => props.active && navigation.page === "sessions" && navigation.sessionView === "history");
const selectedAgentKinds = computed<AgentCliKind[]>(() => navigation.sessionAgentFilter ? [navigation.sessionAgentFilter] : []);
const scopeKey = computed(() => JSON.stringify([...new Set(workspaces.workspaces.map((workspace) => workspace.path))].sort()));
const history = reactive(useAgentSessions({ active: historyActive, agentKinds: selectedAgentKinds, query, scopeKey,
  initialWorkspaceMode: navigation.sessionWorkspaceMode, initialWorkspaceSelection: navigation.sessionWorkspaceSelection,
  initialRoleFilter: navigation.sessionRoleFilter }));
const resumeRow = ref<AgentSessionRow | null>(null);
const resumeRevision = ref("");
const selectedDirectory = computed(() => history.selectedWorkspaces.length === 1 ? history.selectedWorkspaces[0] : null);
const directoryValue = computed(() => history.allWorkspaces ? "" : history.workspaceSelection ?? undefined);
const scopeLabel = computed(() => history.allWorkspaces
  ? `全部已记录目录（含主目录，共 ${history.selectedWorkspaces.length} 个）`
  : selectedDirectory.value ? `${selectedDirectory.value.isHome ? "主目录本身" : "工作目录"} · ${selectedDirectory.value.path}` : "正在读取目录范围");
const issueSources = computed(() => history.sourceStates.filter((source) => ["partial", "unavailable", "unsupported"].includes(source.state)));
const sourceReadSummary = computed(() => {
  if (history.cancelled) return "读取已暂停，已找到的会话仍可查看";
  if (issueSources.value.length) return `${issueSources.value.length} 个来源有读取问题${history.scanPending ? '，其余继续读取' : '，已保留可读会话'}`;
  return history.scanPending ? "正在继续检索，结果会自动补充" : "本次来源读取完成";
});
const hasSearchFilters = computed(() => Boolean(query.value.trim()) || history.roleFilter !== "all");
const detailRuntime = computed(() => props.runtimeSnapshot.sessions.filter((session) => history.detailRow?.runtimeIds.includes(session.runtimeId)));
function selectDirectory(value: unknown) { if (typeof value === "string") history.selectWorkspace(value || null); }
function sourceLabel(state: AgentSessionSourceState) {
  const source = history.scope?.sources.find((candidate) => candidate.id === state.sourceId);
  const agent = props.cliRuntime.agents.find((candidate) => candidate.kind === source?.agentKind);
  const workspace = history.scope?.workspaces.find((candidate) => candidate.id === state.workspaceId);
  return `${agent?.label || source?.agentKind || "原生来源"} · ${workspace?.path || "工作目录"}`;
}
function openResume(row: AgentSessionRow) {
  if (!row.session.canResume || !history.scope || tasks.isReserved(row.sessionRef)) return;
  history.closeDetail();
  resumeRevision.value = history.scope.revision;
  resumeRow.value = row;
}
async function copyDirectory() {
  if (!selectedDirectory.value) return;
  try { await copyText(selectedDirectory.value.path); Message.success("已复制完整目录"); }
  catch (failure) { Message.error(failure instanceof Error ? failure.message : String(failure)); }
}
function refresh() { emit("refreshRuntime"); if (historyActive.value) return history.refresh(); }
function clearSearchFilters() { query.value = ""; history.roleFilter = "all"; }
watch(() => [historyActive.value, navigation.sessionAgentFilter, query.value, history.workspaceSelection, history.allWorkspaces, history.scope?.revision] as const, () => { resumeRow.value = null; }, { flush: "sync" });
watch(() => [navigation.sessionWorkspaceSelection, navigation.sessionWorkspaceMode, navigation.sessionRoleFilter] as const, ([workspace, mode, role]) => {
  if (mode === "all" || workspace !== null) history.selectWorkspace(mode === "all" ? null : workspace);
  history.roleFilter = role;
});
watch(() => [history.workspaceSelection, history.allWorkspaces, history.roleFilter] as const, ([workspace, all, role]) => {
  navigation.sessionWorkspaceSelection = workspace;
  navigation.sessionWorkspaceMode = all ? "all" : "home";
  navigation.sessionRoleFilter = role;
}, { flush: "sync" });
onMounted(() => { void tasks.recover(); emit("refreshRuntime"); });
defineExpose({ refresh });
</script>

<template>
  <section class="agent-session-panel" aria-label="Agent 会话">
    <header class="agent-session-toolbar">
      <div class="agent-session-tabs" role="group" aria-label="会话视图">
        <button type="button" :aria-pressed="navigation.sessionView === 'history'" @click="navigation.selectSessionView('history')"><AgentWorkspaceIcon page="history" :size="15" />历史会话</button>
        <button type="button" :aria-pressed="navigation.sessionView === 'active'" @click="navigation.selectSessionView('active')"><AgentWorkspaceIcon page="activeSessions" :size="15" />活动会话</button>
      </div>
      <a-tooltip v-if="navigation.sessionView === 'active'" content="显示活动及状态未知的会话；活动数量只统计已确认的会话。" :trigger="['hover', 'focus']"><button type="button" class="agent-catalog-icon-action" aria-label="活动状态说明"><CircleHelp :size="15" /></button></a-tooltip>
      <div class="agent-inline-actions"><a-button v-if="navigation.sessionView === 'history'" size="small" :loading="history.busy" aria-label="刷新会话" @click="refresh"><template #icon><RefreshCw :size="14" /></template>刷新</a-button><a-button v-if="historyActive && (history.busy || history.scanPending)" size="small" @click="history.cancel">取消读取</a-button></div>
    </header>
    <template v-if="navigation.sessionView === 'history'">
      <div class="agent-session-filters">
        <div class="agent-session-directory" :title="scopeLabel"><a-select :model-value="directoryValue" :loading="history.scopeLoading" aria-label="会话工作目录" @change="selectDirectory"><template #prefix><FolderOpen :size="14" aria-hidden="true" /></template><a-option value="">全部已记录目录（含主目录）</a-option><a-option v-for="workspace in history.scope?.workspaces || []" :key="workspace.id" :value="workspace.id">{{ workspace.isHome ? '主目录 · ' : '' }}{{ workspace.path }}{{ workspace.exists ? '' : ' · 不存在或未挂载' }}</a-option></a-select><button v-if="selectedDirectory" type="button" class="agent-catalog-icon-action" aria-label="复制当前会话目录" title="复制完整目录" @click="copyDirectory"><Copy :size="14" /></button></div>
        <a-select v-model="history.roleFilter" aria-label="会话角色"><a-option value="all">全部会话角色</a-option><a-option value="main">主会话</a-option><a-option value="subagent">子 Agent 会话</a-option></a-select>
      </div>
      <p v-if="selectedDirectory && !selectedDirectory.exists" class="agent-workspace-note">目录不存在或未挂载；原生存储可读时仍可查看历史，继续前需要恢复目录。</p>
      <p v-if="history.scopeError || history.error" class="agent-workspace-error" role="alert">{{ history.scopeError || history.error }}</p>
      <div class="agent-session-result-summary">
        <span role="status">已显示 {{ history.rows.length }} 条<span v-if="history.total !== null"> / 共 {{ history.total }} 条</span><span v-else-if="history.scanPending"> · 搜索中</span><span v-else-if="history.incomplete"> · 部分结果</span></span>
        <div class="agent-session-result-actions">
          <span v-if="history.cancelled" role="status">本次查询已取消</span><span v-else-if="history.scanPending" role="status">正在继续检索…</span><span v-else-if="history.loading" role="status">正在读取原生会话…</span><span v-else-if="history.hasMore">还有后续会话可加载</span>
          <button v-if="!history.allWorkspaces" type="button" class="agent-session-clear-filters" @click="history.selectWorkspace(null)">搜索全部目录</button>
          <button v-if="hasSearchFilters" type="button" class="agent-session-clear-filters" title="清除搜索和角色筛选，保留当前 Agent 与目录范围" @click="clearSearchFilters"><X :size="12" aria-hidden="true" />清除筛选</button>
        </div>
      </div>
      <details v-if="history.sourceStates.length" class="agent-session-source-states" :class="{ 'has-warning': issueSources.length > 0 }">
        <summary>
          <CircleAlert v-if="issueSources.length" :size="14" aria-hidden="true" /><RefreshCw v-else-if="history.scanPending" :size="14" aria-hidden="true" /><CircleHelp v-else :size="14" aria-hidden="true" />
          <strong>来源读取情况</strong>
          <span>{{ sourceReadSummary }}</span>
          <ChevronDown class="agent-session-source-chevron" :size="14" aria-hidden="true" />
        </summary>
        <ul tabindex="0" aria-label="各来源读取结果"><li v-for="state in history.sourceStates" :key="`${state.sourceId}:${state.workspaceId}`"><strong>{{ sourceLabel(state) }}</strong><span>{{ agentSessionSourceLabels[state.state] }} · 已读取 {{ state.loadedCount }} 条{{ state.indexState === 'fallback' ? ' · 未使用索引缓存' : '' }}</span><p v-if="state.message">{{ state.message }}</p></li></ul>
      </details>
      <div v-if="!history.busy && !history.rows.length && !history.scopeError && !history.error" class="agent-workspace-empty"><span>{{ history.cancelled ? '读取已取消，可以继续读取' : history.scanPending ? '正在检索会话正文，找到后会自动显示' : history.incomplete ? '本次未读取到可展示会话，请查看来源状态' : query.trim() ? '当前范围没有匹配的会话' : '当前范围没有可展示的原生会话' }}</span></div>
      <AgentSessionList :rows="history.rows" :agents="cliRuntime.agents" allow-resume :resume-busy="tasks.isReserved" :runtime-sessions="runtimeSnapshot.sessions" :activating-id="activatingId" @detail="history.openDetail($event.sessionRef)" @parent="history.openDetail" @resume="openResume" @activate="emit('activateRuntime', $event)" />
      <div v-if="history.hasMore" class="agent-session-pagination"><a-button :loading="history.loadingMore" :disabled="history.loading" @click="history.loadMore">{{ history.scanPending ? '继续检索' : '继续加载' }}</a-button><span>已显示 {{ history.rows.length }} 条</span></div>
    </template>
    <AgentRuntimeList v-else :provider="null" :cli-kind="navigation.agentFilter" :cli-runtime="cliRuntime" :instances="runtimeSessions" :loading="runtimeLoading" :activating-id="activatingId" @refresh="emit('refreshRuntime')" @activate="emit('activateRuntime', $event)" />
    <p v-if="tasks.recoveryError" class="agent-workspace-error" role="alert">{{ tasks.recoveryError }}<a-button type="text" size="small" :loading="tasks.recovering" @click="tasks.recover">重新读取任务状态</a-button></p>
      <CliSessionDetailModal :visible="history.detailVisible" :loading="history.detailLoading" :error="history.detailError" :detail="history.detail" :session-row="history.detailRow" selected-resume-id="" @update:visible="!$event && history.closeDetail()" @parent="history.openDetail"><template #footer><span v-if="history.detailRow">{{ history.detailRow.resumeReason || (history.detailRow.activityState === 'active' ? '此原生会话有已确认的活动实例。' : '尚未确认是否正在运行，可手动选择继续方式。') }}</span><div class="agent-session-detail-actions"><template v-for="runtime in detailRuntime" :key="runtime.runtimeId"><a-button v-if="runtime.actions.canActivateTerminal" :loading="activatingId === runtime.runtimeId" @click="emit('activateRuntime', runtime)">切回终端</a-button></template><a-button v-if="history.detailRow?.activityState !== 'active'" type="primary" :disabled="!history.detailRow?.session.canResume || Boolean(history.detailRow && tasks.isReserved(history.detailRow.sessionRef))" @click="history.detailRow && openResume(history.detailRow)">继续会话</a-button></div></template></CliSessionDetailModal>
    <AgentSessionResumeModal :visible="Boolean(resumeRow)" :row="resumeRow" :scope-revision="resumeRevision" :providers="providers" :installations="installations" :agents="cliRuntime.agents" @close="resumeRow = null" />
  </section>
</template>
