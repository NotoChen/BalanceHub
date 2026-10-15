import { computed, ref, shallowRef, watch, type Ref } from "vue";
import type { AgentSessionQueryApi } from "../api/agent-sessions.ts";
import type { AgentSessionRow } from "../stores/agent-session-types.ts";
import type {
  AgentCliKind,
  CliSessionIndexState,
  TemporaryCliSessionMode,
} from "../stores/provider-types.ts";
import { useAgentSessions } from "./useAgentSessions.ts";

interface UseWorkspaceSessionHistoryOptions {
  visible: Ref<boolean>;
  cliKind: Ref<AgentCliKind>;
  sessionMode: Ref<TemporaryCliSessionMode>;
  selectedModel: Ref<string>;
  sessionApi?: AgentSessionQueryApi;
}

/** Reuse session queries and the optional reader; launch selection stays independent. */
export function useWorkspaceSessionHistory(
  options: UseWorkspaceSessionHistoryOptions,
) {
  const workspaceSessionQuery = ref("");
  const active = computed(
    () => options.visible.value && options.sessionMode.value === "history",
  );
  const agentKinds = computed(() => [options.cliKind.value]);
  const sessions = useAgentSessions({
    active,
    agentKinds,
    query: workspaceSessionQuery,
    initialRoleFilter: "main",
    autoLoad: false,
    api: options.sessionApi,
  });
  const selectedSession = shallowRef<AgentSessionRow | null>(null);
  const workspaceSelectedResumeId = computed(
    () => selectedSession.value?.session.id ?? "",
  );
  const workspaceSelectedSessionRef = computed(
    () => selectedSession.value?.sessionRef ?? "",
  );
  const workspaceSelectedSessionTitle = computed(
    () => selectedSession.value?.session.title ?? "",
  );
  const workspaceSelectedSessionWorkdir = computed(
    () => selectedSession.value?.session.workdir ?? "",
  );
  const workspaceSelectedSessionModel = computed(
    () => selectedSession.value?.session.model ?? "",
  );
  const workspaceSessionIndexState = computed<CliSessionIndexState>(() => {
    const states = sessions.sourceStates.value;
    if (states.some((state) => state.indexState === "fallback"))
      return "fallback";
    return states.length &&
      states.every((state) => state.indexState === "disabled")
      ? "disabled"
      : "ready";
  });
  const workspaceSessionIndexMessage = computed(() => {
    const messages = sessions.sourceStates.value.flatMap((state) =>
      state.message ? [state.message] : [],
    );
    if (
      sessions.sourceStates.value.some((state) =>
        ["partial", "unavailable", "unsupported"].includes(state.state),
      )
    )
      messages.unshift("部分来源读取受限，已保留可读会话");
    if (sessions.cancelled.value) messages.push("本次查询已取消");
    return [...new Set(messages)].join("；");
  });
  const workspaceSessionsError = computed(
    () => sessions.scopeError.value || sessions.error.value,
  );

  async function loadWorkspaceSessions() {
    await sessions.ensureLoaded();
  }
  function refreshWorkspaceSessions() {
    return sessions.refresh();
  }
  function openWorkspaceSessionDetail(row: AgentSessionRow) {
    return sessions.openDetail(row.sessionRef);
  }
  function selectWorkspaceSession(row: AgentSessionRow) {
    if (!row.session.canResume) return;
    const changed = selectedSession.value?.sessionRef !== row.sessionRef;
    selectedSession.value = row;
    options.sessionMode.value = "history";
    if (changed) options.selectedModel.value = "";
  }
  function clearWorkspaceSessionSelection() {
    selectedSession.value = null;
  }
  function resetWorkspaceSessions() {
    sessions.invalidate();
    workspaceSessionQuery.value = "";
    clearWorkspaceSessionSelection();
  }
  function invalidateWorkspaceSessionRequests() {
    sessions.suspend();
  }
  watch(
    () =>
      [
        options.cliKind.value,
        sessions.roleFilter.value,
        workspaceSessionQuery.value,
      ] as const,
    clearWorkspaceSessionSelection,
    { flush: "sync" },
  );
  watch(
    () => sessions.scope.value?.revision,
    (revision, previous) => {
      if (previous && revision !== previous) clearWorkspaceSessionSelection();
    },
    { flush: "sync" },
  );

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
    workspaceSessionScopeRevision: computed(
      () => sessions.scope.value?.revision ?? "",
    ),
    workspaceSelectedSessionTitle,
    workspaceSelectedSessionWorkdir,
    workspaceSelectedSessionModel,
    workspaceSessionDetailVisible: sessions.detailVisible,
    workspaceSessionDetailLoading: sessions.detailLoading,
    workspaceSessionDetailError: sessions.detailError,
    workspaceSessionDetail: sessions.detail,
    workspaceSessionDetailRow: sessions.detailRow,
    workspaceSessionDetailQuery: sessions.detailQuery,
    workspaceSessionDetailPaused: sessions.detailPaused,
    workspaceSessionMessageStates: sessions.messageStates,
    navigateWorkspaceSessionDetail: sessions.navigateDetail,
    expandWorkspaceSessionMessage: sessions.expandMessage,
    pauseWorkspaceSessionDetail: sessions.pauseDetail,
    refreshWorkspaceSessionDetail: sessions.refreshDetail,
    loadWorkspaceSessions,
    refreshWorkspaceSessions,
    loadMoreWorkspaceSessions: sessions.loadMore,
    openWorkspaceSessionDetail,
    openWorkspaceSessionParent: sessions.openDetail,
    closeWorkspaceSessionDetail: sessions.closeDetail,
    selectWorkspaceSession,
    resetWorkspaceSessions,
    clearWorkspaceSessionSelection,
    invalidateWorkspaceSessionRequests,
  };
}
