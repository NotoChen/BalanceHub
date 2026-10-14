<script setup lang="ts">
import { computed, onMounted, reactive, ref, shallowRef, watch } from "vue";
import { storeToRefs } from "pinia";
import {
  ChevronDown,
  CircleAlert,
  CircleHelp,
  RefreshCw,
  X,
} from "@lucide/vue";
import { useAgentSessions } from "../../composables/useAgentSessions";
import { useAgentWorkspaceStore } from "../../stores/agent-workspace";
import { useAgentSessionResumeStore } from "../../stores/agent-session-resume";
import type {
  AgentCliKind,
  AgentInstallation,
  AgentRuntimeSession,
  AgentRuntimeSnapshot,
  CliRuntimeSnapshot,
  Provider,
} from "../../stores/provider-types";
import type {
  AgentSessionRow,
  AgentSessionSourceState,
} from "../../stores/agent-session-types";
import type { SessionListPosition } from "../../stores/session-reader-types";
import { agentSessionSourceLabels } from "../../utils/agent-session-display";
import AgentSessionList from "./AgentSessionList.vue";
import AgentSessionResumeModal from "./AgentSessionResumeModal.vue";
import AgentSessionRelocationModal from './AgentSessionRelocationModal.vue';
import AgentWorkspaceIcon from "./AgentWorkspaceIcon.vue";
import AgentRuntimeList from "../AgentRuntimeList.vue";
import SessionReader from "./SessionReader.vue";

const props = defineProps<{
  active: boolean;
  providers: Provider[];
  installations: AgentInstallation[];
  cliRuntime: CliRuntimeSnapshot;
  runtimeSnapshot: AgentRuntimeSnapshot;
  runtimeSessions: AgentRuntimeSession[];
  runtimeLoading: boolean;
  activatingId: string | null;
}>();
const emit = defineEmits<{
  refreshRuntime: [];
  activateRuntime: [session: AgentRuntimeSession];
}>();
const navigation = useAgentWorkspaceStore();
const { sessionHistoryQuery: query, sessionEnvironment } = storeToRefs(navigation);
const tasks = useAgentSessionResumeStore();
const historyActive = computed(
  () =>
    props.active &&
    navigation.page === "sessions" &&
    navigation.sessionView === "history",
);
const selectedAgentKinds = computed<AgentCliKind[]>(() =>
  navigation.sessionAgentFilter ? [navigation.sessionAgentFilter] : [],
);
const history = reactive(
  useAgentSessions({
    active: historyActive,
    agentKinds: selectedAgentKinds,
    runtimeScope: sessionEnvironment,
    query,
    initialRoleFilter: navigation.sessionRoleFilter,
  }),
);
const resumeRow = ref<AgentSessionRow | null>(null);
const relocationRow = ref<AgentSessionRow | null>(null);
const relocationRevision = ref('');
function openRelocation(row: AgentSessionRow) { if (!history.scope) return; relocationRevision.value = history.scope.revision; relocationRow.value = row; }
const listPosition = shallowRef<SessionListPosition | null>(null);
const listPositionKey = computed(() =>
  JSON.stringify([
    history.scope?.revision,
    navigation.sessionAgentFilter,
    navigation.sessionEnvironment,
    history.roleFilter,
    query.value.trim(),
  ]),
);
const environments = computed(() => [...new Map((history.scope?.sources ?? []).map((source) => [JSON.stringify(source.runtimeScope), { value: JSON.stringify(source.runtimeScope), label: source.label, scope: source.runtimeScope }])).values()]);
const environmentValue = computed({ get: () => sessionEnvironment.value ? JSON.stringify(sessionEnvironment.value) : "", set: (value: string) => { sessionEnvironment.value = environments.value.find((entry) => entry.value === value)?.scope ?? null; } });
const resumeRevision = ref("");
const issueSources = computed(() =>
  history.sourceStates.filter((source) =>
    ["partial", "unavailable", "unsupported"].includes(source.state),
  ),
);
const sourceReadSummary = computed(() => {
  if (history.cancelled) return "读取已暂停，已找到的会话仍可查看";
  if (issueSources.value.length)
    return `${issueSources.value.length} 个来源有读取问题${history.scanPending ? "，其余继续读取" : "，已保留可读会话"}`;
  return history.scanPending
    ? "正在继续检索，结果会自动补充"
    : "本次来源读取完成";
});
const hasSearchFilters = computed(
  () => Boolean(query.value.trim()) || history.roleFilter !== "all",
);
const detailRuntime = computed(() =>
  props.runtimeSnapshot.sessions.filter((session) =>
    history.detailRow?.runtimeIds.includes(session.runtimeId),
  ),
);
function sourceLabel(state: AgentSessionSourceState) {
  const source = history.scope?.sources.find(
    (candidate) => candidate.id === state.sourceId,
  );
  const agent = props.cliRuntime.agents.find(
    (candidate) => candidate.kind === source?.agentKind,
  );
  const workspace = history.scope?.workspaces.find(
    (candidate) => candidate.id === state.workspaceId,
  );
  return `${source?.label || "本机"} · ${agent?.label || source?.agentKind || "原生来源"} · ${workspace?.path || (state.workspaceId === null ? "全部目录" : "未记录目录")}`;
}
function openResume(row: AgentSessionRow) {
  if (
    !row.session.canResume ||
    !history.scope ||
    tasks.isReserved(row.sessionRef)
  )
    return;
  history.closeDetail();
  resumeRevision.value = history.scope.revision;
  resumeRow.value = row;
}
function refresh() {
  emit("refreshRuntime");
  if (historyActive.value) return history.refresh();
}
function clearSearchFilters() {
  query.value = "";
  history.roleFilter = "all";
}
watch(
  () =>
    [
      historyActive.value,
      navigation.sessionAgentFilter,
    navigation.sessionEnvironment,
      query.value,
      history.roleFilter,
      history.scope?.revision,
    ] as const,
  () => {
    resumeRow.value = null;
    relocationRow.value = null;
  },
  { flush: "sync" },
);
watch(
  () => navigation.sessionRoleFilter,
  (role) => { history.roleFilter = role; },
);
watch(
  () => history.roleFilter,
  (role) => { navigation.sessionRoleFilter = role; },
  { flush: "sync" },
);
onMounted(() => {
  void tasks.recover();
  emit("refreshRuntime");
});
defineExpose({ refresh });
</script>

<template>
  <section class="agent-session-panel" aria-label="Agent 会话">
    <header class="agent-session-toolbar">
      <div class="agent-session-tabs" role="group" aria-label="会话视图">
        <button
          type="button"
          :aria-pressed="navigation.sessionView === 'history'"
          @click="navigation.selectSessionView('history')"
        >
          <AgentWorkspaceIcon page="history" :size="15" />历史会话
        </button>
        <button
          type="button"
          :aria-pressed="navigation.sessionView === 'active'"
          @click="navigation.selectSessionView('active')"
        >
          <AgentWorkspaceIcon page="activeSessions" :size="15" />活动会话
        </button>
      </div>
      <a-tooltip
        v-if="navigation.sessionView === 'active'"
        content="显示活动及状态未知的会话；活动数量只统计已确认的会话。"
        :trigger="['hover', 'focus']"
        ><button
          type="button"
          class="agent-catalog-icon-action"
          aria-label="活动状态说明"
        >
          <CircleHelp :size="15" /></button
      ></a-tooltip>
      <div class="agent-inline-actions">
        <a-button
          v-if="navigation.sessionView === 'history'"
          size="small"
          :loading="history.busy"
          aria-label="刷新会话"
          @click="refresh"
          ><template #icon><RefreshCw :size="14" /></template>刷新</a-button
        ><a-button
          v-if="historyActive && (history.busy || history.scanPending)"
          size="small"
          @click="history.cancel"
          >取消读取</a-button
        >
      </div>
    </header>
    <div
      v-if="historyActive"
      class="agent-session-workbench"
      :class="{ 'is-reading': history.detailVisible }"
    >
      <aside class="agent-session-browser" aria-label="会话检索">
        <div class="agent-session-filters">
          <a-select v-if="environments.length > 1" v-model="environmentValue" aria-label="会话运行环境"><a-option value="">全部环境</a-option><a-option v-for="environment in environments" :key="environment.value" :value="environment.value">{{ environment.label }}</a-option></a-select>
          <a-select v-model="history.roleFilter" aria-label="会话角色"
            ><a-option value="all">全部会话</a-option
            ><a-option value="main">主会话</a-option
            ><a-option value="subagent">子 Agent 会话</a-option></a-select
          >
          <span class="agent-session-order">最近更新优先</span>
        </div>
        <p
          v-if="history.scopeError || history.error"
          class="agent-workspace-error"
          role="alert"
        >
          {{ history.scopeError || history.error }}
        </p>
        <div class="agent-session-result-summary">
          <span role="status"
            >已显示 {{ history.rows.length }} 条<span
              v-if="history.total !== null"
            >
              / 共 {{ history.total }} 条</span
            ><span v-else-if="history.scanPending"> · 搜索中</span
            ><span v-else-if="history.incomplete"> · 部分结果</span></span
          >
          <div class="agent-session-result-actions">
            <span v-if="history.cancelled" role="status">本次查询已取消</span
            ><span v-else-if="history.scanPending" role="status"
              >正在继续检索…</span
            ><span v-else-if="history.loading" role="status"
              >正在读取原生会话…</span
            ><span v-else-if="history.hasMore">还有后续会话可加载</span>
            <button
              v-if="hasSearchFilters"
              type="button"
              class="agent-session-clear-filters"
              title="清除搜索和角色筛选，保留当前 Agent"
              @click="clearSearchFilters"
            >
              <X :size="12" aria-hidden="true" />清除筛选
            </button>
          </div>
        </div>
        <details
          v-if="
            history.sourceStates.length &&
            (history.incomplete || history.cancelled)
          "
          class="agent-session-source-states"
          :class="{ 'has-warning': issueSources.length > 0 }"
        >
          <summary>
            <CircleAlert
              v-if="issueSources.length"
              :size="14"
              aria-hidden="true"
            /><RefreshCw
              v-else-if="history.scanPending"
              :size="14"
              aria-hidden="true"
            /><CircleHelp v-else :size="14" aria-hidden="true" />
            <strong>来源读取情况</strong>
            <span>{{ sourceReadSummary }}</span>
            <ChevronDown
              class="agent-session-source-chevron"
              :size="14"
              aria-hidden="true"
            />
          </summary>
          <ul tabindex="0" aria-label="各来源读取结果">
            <li
              v-for="state in history.sourceStates"
              :key="`${state.sourceId}:${state.workspaceId}`"
            >
              <strong>{{ sourceLabel(state) }}</strong
              ><span
                >{{ agentSessionSourceLabels[state.state] }} · 已读取
                {{ state.loadedCount }} 条{{
                  state.indexState === "fallback" ? " · 未使用索引缓存" : ""
                }}</span
              >
              <p v-if="state.message">{{ state.message }}</p>
            </li>
          </ul>
        </details>
        <div
          v-if="
            !history.busy &&
            !history.rows.length &&
            !history.scopeError &&
            !history.error
          "
          class="agent-workspace-empty"
        >
          <span>{{
            history.cancelled
              ? "读取已取消，可以继续读取"
              : history.scanPending
                ? "正在检索会话正文，找到后会自动显示"
                : history.incomplete
                  ? "本次未读取到可展示会话，请查看来源状态"
                  : query.trim()
                    ? "没有匹配的会话"
                    : "暂无可展示的原生会话"
          }}</span>
        </div>
        <AgentSessionList
          :key="listPositionKey"
          :position-key="listPositionKey"
          :position="listPosition"
          :selected-session-ref="history.detailRow?.sessionRef"
          :rows="history.rows"
          :agents="cliRuntime.agents"
          allow-resume
          allow-relocation
          :resume-busy="tasks.isReserved"
          :runtime-sessions="runtimeSnapshot.sessions"
          :activating-id="activatingId"
          @detail="history.openDetail($event.sessionRef)"
          @parent="history.openDetail"
          @resume="openResume"
          @relocate="openRelocation"
          @activate="emit('activateRuntime', $event)"
          @position="listPosition = $event"
        />
        <div v-if="history.hasMore" class="agent-session-pagination">
          <a-button
            :loading="history.loadingMore"
            :disabled="history.loading"
            @click="history.loadMore"
            >{{ history.scanPending ? "继续检索" : "继续加载" }}</a-button
          ><span>已显示 {{ history.rows.length }} 条</span>
        </div>
      </aside>
      <SessionReader
        v-if="history.detailVisible"
        :detail="history.detail"
        :session-row="history.detailRow"
        :loading="history.detailLoading"
        :error="history.detailError"
        :query="history.detailQuery"
        :paused="history.detailPaused"
        :message-states="history.messageStates"
        show-back
        @close="history.closeDetail"
        @parent="history.openDetail"
        @navigate="history.navigateDetail"
        @expand="history.expandMessage"
        @pause="history.pauseDetail"
        @retry="history.refreshDetail"
      >
        <template #footer
          ><span v-if="history.detailRow?.resumeReason">{{
            history.detailRow.resumeReason
          }}</span>
          <div class="agent-session-detail-actions">
            <template v-for="runtime in detailRuntime" :key="runtime.runtimeId"
              ><a-button
                v-if="runtime.actions.canActivateTerminal"
                :loading="activatingId === runtime.runtimeId"
                @click="emit('activateRuntime', runtime)"
                >切回终端</a-button
              ></template
            ><a-button
              v-if="history.detailRow?.activityState !== 'active'"
              type="primary"
              :disabled="
                !history.detailRow?.session.canResume ||
                Boolean(
                  history.detailRow &&
                  tasks.isReserved(history.detailRow.sessionRef),
                )
              "
              @click="history.detailRow && openResume(history.detailRow)"
              >继续会话</a-button
            >
          </div></template
        >
      </SessionReader>
    </div>
    <AgentRuntimeList
      v-else-if="navigation.sessionView === 'active'"
      :provider="null"
      :cli-kind="navigation.agentFilter"
      :cli-runtime="cliRuntime"
      :instances="runtimeSessions"
      :loading="runtimeLoading"
      :activating-id="activatingId"
      @refresh="emit('refreshRuntime')"
      @activate="emit('activateRuntime', $event)"
    />
    <p v-if="tasks.recoveryError" class="agent-workspace-error" role="alert">
      {{ tasks.recoveryError
      }}<a-button
        type="text"
        size="small"
        :loading="tasks.recovering"
        @click="tasks.recover"
        >重新读取任务状态</a-button
      >
    </p>

    <AgentSessionRelocationModal :visible="Boolean(relocationRow)" :row="relocationRow" :scope-revision="relocationRevision" @close="relocationRow = null" />
    <AgentSessionResumeModal
      :visible="Boolean(resumeRow)"
      :row="resumeRow"
      :scope-revision="resumeRevision"
      :providers="providers"
      :installations="installations"
      :agents="cliRuntime.agents"
      @close="resumeRow = null"
    />
  </section>
</template>
