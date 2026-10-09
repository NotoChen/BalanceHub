import { computed, ref, watch, type Ref } from "vue";
import type { AgentSessionQueryApi } from "../api/agent-sessions.ts";
import type { AgentSessionRow } from "../stores/agent-session-types.ts";
import type { AgentCliKind, CliSessionIndexState, TemporaryCliSessionMode, WorkspaceDirectoryListing } from "../stores/provider-types.ts";
import { useAgentSessions } from "./useAgentSessions.ts";

interface UseWorkspaceSessionHistoryOptions {
  visible: Ref<boolean>;
  cliKind: Ref<AgentCliKind>;
  sessionMode: Ref<TemporaryCliSessionMode>;
  selectedModel: Ref<string>;
  directory: Ref<WorkspaceDirectoryListing | null>;
  sessionApi?: AgentSessionQueryApi;
}

/** The temporary CLI picker shares the workbench query and detail lifetimes. */
export function useWorkspaceSessionHistory(options: UseWorkspaceSessionHistoryOptions) {
  const workspaceSessionQuery = ref("");
  const active = computed(() => options.visible.value && options.sessionMode.value === "history" && Boolean(options.directory.value?.currentPath));
  const explicitWorkdir = computed(() => options.directory.value?.currentPath || null);
  const agentKinds = computed(() => [options.cliKind.value]);
  const sessions = useAgentSessions({ active, agentKinds, query: workspaceSessionQuery, explicitWorkdir, autoLoad: false, api: options.sessionApi });
  const workspaceSelectedResumeId = ref("");
  const workspaceSelectedSessionRef = ref("");
  const workspaceSelectedSessionTitle = ref("");
  const workspaceSessionIndexState = computed<CliSessionIndexState>(() => {
    const states = sessions.sourceStates.value;
    if (states.some((state) => state.indexState === "fallback")) return "fallback";
    return states.length && states.every((state) => state.indexState === "disabled") ? "disabled" : "ready";
  });
  const workspaceSessionIndexMessage = computed(() => {
    const messages = sessions.sourceStates.value.flatMap((state) => state.message ? [state.message] : []);
    if (sessions.incomplete.value) messages.unshift("部分来源读取受限，已保留可读会话");
    if (sessions.cancelled.value) messages.push("本次查询已取消");
    return [...new Set(messages)].join("；");
  });
  const workspaceSessionsError = computed(() => sessions.scopeError.value || sessions.error.value);

  async function loadWorkspaceSessions(workdir?: string) {
    if (workdir && workdir.trim() !== options.directory.value?.currentPath) return;
    await sessions.ensureLoaded();
  }
  function refreshWorkspaceSessions(workdir?: string) {
    if (workdir && workdir.trim() !== options.directory.value?.currentPath) return;
    return sessions.refresh();
  }
  function openWorkspaceSessionDetail(row: AgentSessionRow) { return sessions.openDetail(row.sessionRef); }
  function selectWorkspaceSession(row: AgentSessionRow) {
    if (!row.session.canResume) return;
    workspaceSelectedResumeId.value = row.session.id;
    workspaceSelectedSessionRef.value = row.sessionRef;
    workspaceSelectedSessionTitle.value = row.session.title;
    options.sessionMode.value = "history";
    options.selectedModel.value = "";
  }
  function selectWorkspaceSessionFromDetail() {
    if (!sessions.detailRow.value) return;
    selectWorkspaceSession(sessions.detailRow.value);
    sessions.closeDetail();
  }
  function clearWorkspaceSessionSelection() {
    workspaceSelectedResumeId.value = "";
    workspaceSelectedSessionRef.value = "";
    workspaceSelectedSessionTitle.value = "";
  }
  function resetWorkspaceSessions() {
    sessions.invalidate();
    workspaceSessionQuery.value = "";
    clearWorkspaceSessionSelection();
  }
  function invalidateWorkspaceSessionRequests() { sessions.suspend(); }
  watch(() => [options.cliKind.value, explicitWorkdir.value] as const, clearWorkspaceSessionSelection, { flush: "sync" });
  watch(() => sessions.scope.value?.revision, (revision, previous) => {
    if (previous && revision !== previous) clearWorkspaceSessionSelection();
  }, { flush: "sync" });

  return {
    workspaceSessionQuery,
    workspaceSessionResults: sessions.rows,
    workspaceSessionsLoading: sessions.busy,
    workspaceSessionsLoadingMore: sessions.loadingMore,
    workspaceSessionsError,
    workspaceSessionIndexState,
    workspaceSessionIndexMessage,
    workspaceSessionRoleFilter: sessions.roleFilter,
    workspaceSessionTotal: sessions.total,
    workspaceSessionHasMore: sessions.hasMore,
    workspaceSelectedResumeId,
    workspaceSelectedSessionRef,
    workspaceSessionScopeRevision: computed(() => sessions.scope.value?.revision ?? ""),
    workspaceSelectedSessionTitle,
    workspaceSessionDetailVisible: sessions.detailVisible,
    workspaceSessionDetailLoading: sessions.detailLoading,
    workspaceSessionDetailError: sessions.detailError,
    workspaceSessionDetail: sessions.detail,
    workspaceSessionDetailRow: sessions.detailRow,
    loadWorkspaceSessions,
    refreshWorkspaceSessions,
    loadMoreWorkspaceSessions: sessions.loadMore,
    openWorkspaceSessionDetail,
    openWorkspaceSessionParent: sessions.openDetail,
    closeWorkspaceSessionDetail: sessions.closeDetail,
    selectWorkspaceSession,
    selectWorkspaceSessionFromDetail,
    resetWorkspaceSessions,
    clearWorkspaceSessionSelection,
    invalidateWorkspaceSessionRequests,
  };
}
