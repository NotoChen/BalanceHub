import { ref, type Ref } from "vue";
import type {
  AgentCliKind,
  CliSessionSummary,
  TemporaryCliSessionMode,
  WorkspaceDirectoryListing,
} from "../stores/providers";

interface UseWorkspaceSessionHistoryOptions {
  visible: Ref<boolean>;
  cliKind: Ref<AgentCliKind>;
  sessionMode: Ref<TemporaryCliSessionMode>;
  selectedModel: Ref<string>;
  directory: Ref<WorkspaceDirectoryListing | null>;
  listSessions: (cliKind: AgentCliKind, workdir: string) => Promise<CliSessionSummary[]>;
}

export function useWorkspaceSessionHistory(options: UseWorkspaceSessionHistoryOptions) {
  const workspaceSessions = ref<CliSessionSummary[]>([]);
  const workspaceSessionsLoading = ref(false);
  const workspaceSessionsError = ref("");
  const workspaceSelectedResumeId = ref("");
  let sessionsRequestId = 0;

  async function loadWorkspaceSessions(workdir?: string) {
    const path = (workdir || options.directory.value?.currentPath || "").trim();
    const requestId = ++sessionsRequestId;
    const previousResumeId = workspaceSelectedResumeId.value;
    workspaceSessionsError.value = "";
    workspaceSessions.value = [];
    if (!path || !options.visible.value || options.sessionMode.value === "new") {
      workspaceSessionsLoading.value = false;
      return;
    }
    workspaceSessionsLoading.value = true;
    try {
      const sessions = await options.listSessions(options.cliKind.value, path);
      if (
        requestId !== sessionsRequestId
        || !options.visible.value
        || options.directory.value?.currentPath !== path
      ) {
        return;
      }
      workspaceSessions.value = sessions;
      workspaceSelectedResumeId.value = sessions.some(
        (session) => session.id === previousResumeId && session.canResume,
      )
        ? previousResumeId
        : "";
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

  function selectWorkspaceSession(session: CliSessionSummary) {
    if (!session.canResume) return;
    workspaceSelectedResumeId.value = session.id;
    options.sessionMode.value = "history";
    // 空值表示不向官方 CLI 注入模型，让它按会话自己的元数据恢复。
    options.selectedModel.value = "";
  }

  function resetWorkspaceSessions() {
    invalidateWorkspaceSessionRequests();
    workspaceSessions.value = [];
    workspaceSessionsError.value = "";
    workspaceSelectedResumeId.value = "";
  }

  function clearWorkspaceSessionSelection() {
    workspaceSelectedResumeId.value = "";
  }

  function invalidateWorkspaceSessionRequests() {
    sessionsRequestId += 1;
    workspaceSessionsLoading.value = false;
  }

  return {
    workspaceSessions,
    workspaceSessionsLoading,
    workspaceSessionsError,
    workspaceSelectedResumeId,
    loadWorkspaceSessions,
    selectWorkspaceSession,
    resetWorkspaceSessions,
    clearWorkspaceSessionSelection,
    invalidateWorkspaceSessionRequests,
  };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
