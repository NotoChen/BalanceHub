import { getCurrentInstance, onMounted, onUnmounted, ref, watch, type Ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AgentCliKind,
  CliSessionDetail,
  CliSessionIndexState,
  CliSessionSearchResponse,
  CliSessionSearchResult,
  CliSessionSummary,
  TemporaryCliSessionMode,
  WorkspaceDirectoryListing,
} from "../stores/providers";

const SESSION_SEARCH_DEBOUNCE_MS = 280;

interface UseWorkspaceSessionHistoryOptions {
  visible: Ref<boolean>;
  cliKind: Ref<AgentCliKind>;
  sessionMode: Ref<TemporaryCliSessionMode>;
  selectedModel: Ref<string>;
  directory: Ref<WorkspaceDirectoryListing | null>;
  searchSessions: (
    cliKind: AgentCliKind,
    workdir: string,
    query: string,
    forceRefresh?: boolean,
  ) => Promise<CliSessionSearchResponse>;
  getSessionDetail: (
    cliKind: AgentCliKind,
    workdir: string,
    sessionId: string,
  ) => Promise<CliSessionDetail>;
}

export function useWorkspaceSessionHistory(options: UseWorkspaceSessionHistoryOptions) {
  const workspaceSessionQuery = ref("");
  const workspaceSessionResults = ref<CliSessionSearchResult[]>([]);
  const workspaceSessionsLoading = ref(false);
  const workspaceSessionsError = ref("");
  const workspaceSessionIndexState = ref<CliSessionIndexState>("ready");
  const workspaceSessionIndexMessage = ref("");
  const workspaceSelectedResumeId = ref("");
  const workspaceSelectedSessionTitle = ref("");
  const workspaceSessionDetailVisible = ref(false);
  const workspaceSessionDetailLoading = ref(false);
  const workspaceSessionDetailError = ref("");
  const workspaceSessionDetail = ref<CliSessionDetail | null>(null);
  let sessionsRequestId = 0;
  let detailRequestId = 0;
  let searchTimer: ReturnType<typeof globalThis.setTimeout> | null = null;
  let indexUpdatedUnlisten: UnlistenFn | null = null;
  let disposed = false;

  async function loadWorkspaceSessions(workdir?: string, forceRefresh = false) {
    clearSearchTimer();
    const path = (workdir || options.directory.value?.currentPath || "").trim();
    const query = workspaceSessionQuery.value.trim();
    const cliKind = options.cliKind.value;
    const requestId = ++sessionsRequestId;
    workspaceSessionsError.value = "";
    workspaceSessionResults.value = [];
    if (!path || !options.visible.value || options.sessionMode.value === "new") {
      workspaceSessionsLoading.value = false;
      return;
    }
    workspaceSessionsLoading.value = true;
    try {
      const response = await options.searchSessions(cliKind, path, query, forceRefresh);
      if (
        requestId !== sessionsRequestId
        || !options.visible.value
        || options.directory.value?.currentPath !== path
        || options.cliKind.value !== cliKind
        || workspaceSessionQuery.value.trim() !== query
      ) {
        return;
      }
      workspaceSessionResults.value = response.results;
      workspaceSessionIndexState.value = response.indexState;
      workspaceSessionIndexMessage.value = response.indexMessage || "";
      if (
        !query
        && workspaceSelectedResumeId.value
        && !response.results.some((result) => result.session.id === workspaceSelectedResumeId.value)
      ) {
        clearWorkspaceSessionSelection();
      }
    } catch (error) {
      if (requestId === sessionsRequestId) {
        workspaceSessionsError.value = errorMessage(error);
      }
    } finally {
      if (requestId === sessionsRequestId) {
        workspaceSessionsLoading.value = false;
      }
    }
  }

  function refreshWorkspaceSessions(workdir?: string) {
    return loadWorkspaceSessions(workdir, true);
  }

  function scheduleWorkspaceSessionSearch() {
    clearSearchTimer();
    sessionsRequestId += 1;
    workspaceSessionsError.value = "";
    workspaceSessionIndexMessage.value = "";
    workspaceSessionIndexState.value = "ready";
    workspaceSessionResults.value = [];
    if (
      !options.visible.value
      || options.sessionMode.value === "new"
      || !options.directory.value?.currentPath
    ) {
      workspaceSessionsLoading.value = false;
      return;
    }
    workspaceSessionsLoading.value = true;
    searchTimer = globalThis.setTimeout(() => {
      searchTimer = null;
      void loadWorkspaceSessions();
    }, SESSION_SEARCH_DEBOUNCE_MS);
  }

  async function openWorkspaceSessionDetail(session: CliSessionSummary) {
    const path = options.directory.value?.currentPath?.trim() || "";
    if (!path) return;
    const cliKind = options.cliKind.value;
    const requestId = ++detailRequestId;
    workspaceSessionDetailVisible.value = true;
    workspaceSessionDetailLoading.value = true;
    workspaceSessionDetailError.value = "";
    workspaceSessionDetail.value = null;
    try {
      const detail = await options.getSessionDetail(cliKind, path, session.id);
      if (
        requestId !== detailRequestId
        || !workspaceSessionDetailVisible.value
        || options.directory.value?.currentPath !== path
        || options.cliKind.value !== cliKind
      ) {
        return;
      }
      workspaceSessionDetail.value = detail;
    } catch (error) {
      if (requestId === detailRequestId) {
        workspaceSessionDetailError.value = errorMessage(error);
      }
    } finally {
      if (requestId === detailRequestId) {
        workspaceSessionDetailLoading.value = false;
      }
    }
  }

  function closeWorkspaceSessionDetail() {
    detailRequestId += 1;
    workspaceSessionDetailVisible.value = false;
    workspaceSessionDetailLoading.value = false;
    workspaceSessionDetailError.value = "";
    workspaceSessionDetail.value = null;
  }

  function selectWorkspaceSession(session: CliSessionSummary) {
    if (!session.canResume) return;
    workspaceSelectedResumeId.value = session.id;
    workspaceSelectedSessionTitle.value = session.title;
    options.sessionMode.value = "history";
    // 空值表示不向官方 CLI 注入模型，让它按会话自己的元数据恢复。
    options.selectedModel.value = "";
  }

  function selectWorkspaceSessionFromDetail() {
    const session = workspaceSessionDetail.value?.session;
    if (!session) return;
    selectWorkspaceSession(session);
    closeWorkspaceSessionDetail();
  }

  function resetWorkspaceSessions() {
    invalidateWorkspaceSessionRequests();
    closeWorkspaceSessionDetail();
    workspaceSessionQuery.value = "";
    workspaceSessionResults.value = [];
    workspaceSessionsError.value = "";
    workspaceSelectedResumeId.value = "";
    workspaceSelectedSessionTitle.value = "";
  }

  function clearWorkspaceSessionSelection() {
    workspaceSelectedResumeId.value = "";
    workspaceSelectedSessionTitle.value = "";
  }

  function invalidateWorkspaceSessionRequests() {
    clearSearchTimer();
    sessionsRequestId += 1;
    workspaceSessionsLoading.value = false;
  }

  function clearSearchTimer() {
    if (searchTimer !== null) {
      globalThis.clearTimeout(searchTimer);
      searchTimer = null;
    }
  }

  watch(workspaceSessionQuery, scheduleWorkspaceSessionSearch);
  watch(workspaceSessionDetailVisible, (visible) => {
    if (visible) return;
    detailRequestId += 1;
    workspaceSessionDetailLoading.value = false;
    workspaceSessionDetailError.value = "";
    workspaceSessionDetail.value = null;
  });

  if (getCurrentInstance()) {
    onMounted(async () => {
      disposed = false;
      try {
        const unlisten = await listen<string>("cli-session-index-updated", (event) => {
          if (
            event.payload === options.cliKind.value
            && options.visible.value
            && options.sessionMode.value === "history"
            && options.directory.value?.currentPath
          ) {
            void loadWorkspaceSessions();
          }
        });
        if (disposed) {
          unlisten();
          return;
        }
        indexUpdatedUnlisten = unlisten;
      } catch {
        // Vite/browser preview does not expose the Tauri event bus.
      }
    });

    onUnmounted(() => {
      disposed = true;
      clearSearchTimer();
      indexUpdatedUnlisten?.();
      indexUpdatedUnlisten = null;
    });
  }

  return {
    workspaceSessionQuery,
    workspaceSessionResults,
    workspaceSessionsLoading,
    workspaceSessionsError,
    workspaceSessionIndexState,
    workspaceSessionIndexMessage,
    workspaceSelectedResumeId,
    workspaceSelectedSessionTitle,
    workspaceSessionDetailVisible,
    workspaceSessionDetailLoading,
    workspaceSessionDetailError,
    workspaceSessionDetail,
    loadWorkspaceSessions,
    refreshWorkspaceSessions,
    openWorkspaceSessionDetail,
    closeWorkspaceSessionDetail,
    selectWorkspaceSession,
    selectWorkspaceSessionFromDetail,
    resetWorkspaceSessions,
    clearWorkspaceSessionSelection,
    invalidateWorkspaceSessionRequests,
  };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
