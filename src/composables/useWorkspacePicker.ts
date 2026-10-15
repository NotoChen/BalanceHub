import { computed, ref, watch, type Ref } from "vue";
import {
  defaultTemporaryCliRunSettings,
  copyTemporaryCliRunSettings,
  type TemporaryCliRunSettings,
} from "../stores/temporary-cli-settings-types";
import type {
  AgentCliKind,
  CliEnvironmentProbeResult,
  Provider,
  ProviderApiKeyOption,
  TemporaryCliInstance,
  TemporaryCliLaunchInput,
  TemporaryCliLaunchPreview,
  TemporaryCliLaunchResult,
  TemporaryCliPreference,
  TemporaryCliSessionMode,
  TemporaryCliTerminalKind,
  TerminalEnvironmentProbeResult,
  Workspace,
  WorkspaceDirectoryListing,
} from "../stores/providers";
import {
  agentCliTool,
  availableCliOptions,
  availableTerminalOptions,
  canNameSessionAtLaunch,
} from "../utils/cli-environment";
import { useWorkspaceApiKeySelection } from "./useWorkspaceApiKeySelection";
import { useWorkspaceDirectoryBrowser } from "./useWorkspaceDirectoryBrowser";
import { useWorkspaceLaunchFlow } from "./useWorkspaceLaunchFlow";
import type { AgentSessionQueryApi } from "../api/agent-sessions";
import { useWorkspaceSessionHistory } from "./useWorkspaceSessionHistory";
import { useAgentSessionResumeStore } from "../stores/agent-session-resume";

interface UseWorkspacePickerOptions {
  workspaces: Ref<Workspace[]>;
  preferences: Ref<TemporaryCliPreference[]>;
  terminalKind: Ref<TemporaryCliTerminalKind>;
  cliEnvironmentProbe: Ref<CliEnvironmentProbeResult | null>;
  terminalEnvironmentProbe: Ref<TerminalEnvironmentProbeResult | null>;
  probeCliTools: (deep?: boolean) => Promise<CliEnvironmentProbeResult>;
  probeTerminals: () => Promise<TerminalEnvironmentProbeResult>;
  listApiKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
  browse: (path?: string) => Promise<WorkspaceDirectoryListing>;
  forget: (path: string) => Promise<Workspace[]>;
  launch: (input: TemporaryCliLaunchInput) => Promise<TemporaryCliLaunchResult>;
  preview: (
    input: TemporaryCliLaunchInput,
  ) => Promise<TemporaryCliLaunchPreview>;
  getInstance: (instanceId: string) => Promise<TemporaryCliInstance | null>;
  sessionApi?: AgentSessionQueryApi;
}

export function useWorkspacePicker(options: UseWorkspacePickerOptions) {
  const sessionResume = useAgentSessionResumeStore();
  const workspacePickerVisible = ref(false);
  const workspacePickerProvider = ref<Provider | null>(null);
  const workspacePickerCliKind = ref<AgentCliKind>("codex");
  const workspaceSelectedModel = ref("");
  const workspaceNewSessionModels = new Map<AgentCliKind, string>();
  const workspaceSessionName = ref("");
  const workspaceRunSettings = ref(defaultTemporaryCliRunSettings());
  const runDrafts = new Map<AgentCliKind, TemporaryCliRunSettings>();
  const workspaceSessionMode = ref<TemporaryCliSessionMode>("new");
  const workspaceTerminalKind = ref<TemporaryCliTerminalKind>(
    options.terminalKind.value,
  );
  const workspaceCliOptions = computed(() =>
    availableCliOptions(options.cliEnvironmentProbe.value, "temporaryLaunch"),
  );
  const workspaceTerminalOptions = computed(() =>
    availableTerminalOptions(options.terminalEnvironmentProbe.value),
  );
  const selectedCliTool = computed(() =>
    agentCliTool(
      options.cliEnvironmentProbe.value,
      workspacePickerCliKind.value,
    ),
  );
  const workspaceCanNameSession = computed(() =>
    canNameSessionAtLaunch(
      options.cliEnvironmentProbe.value,
      workspacePickerCliKind.value,
      workspaceSessionMode.value,
    ),
  );
  let pickerRequestId = 0;

  const directoryBrowser = useWorkspaceDirectoryBrowser({
    browse: options.browse,
    forget: options.forget,
  });
  const apiKeySelection = useWorkspaceApiKeySelection({
    currentProvider: workspacePickerProvider,
    listApiKeys: options.listApiKeys,
  });
  const sessionHistory = useWorkspaceSessionHistory({
    visible: workspacePickerVisible,
    cliKind: workspacePickerCliKind,
    sessionMode: workspaceSessionMode,
    selectedModel: workspaceSelectedModel,
    sessionApi: options.sessionApi,
  });
  const launchFlow = useWorkspaceLaunchFlow({
    visible: workspacePickerVisible,
    provider: workspacePickerProvider,
    cliKind: workspacePickerCliKind,
    cliOptions: workspaceCliOptions,
    cliTool: selectedCliTool,
    cliProbe: options.cliEnvironmentProbe,
    terminalKind: workspaceTerminalKind,
    terminalOptions: workspaceTerminalOptions,
    directory: directoryBrowser.workspaceDirectory,
    apiKeys: apiKeySelection.workspaceApiKeys,
    apiKeyLocalId: apiKeySelection.workspaceApiKeyLocalId,
    selectedModel: workspaceSelectedModel,
    sessionMode: workspaceSessionMode,
    sessionName: workspaceSessionName,
    canNameSession: workspaceCanNameSession,
    runSettings: workspaceRunSettings,
    selectedResumeId: sessionHistory.workspaceSelectedResumeId,
    selectedSessionRef: sessionHistory.workspaceSelectedSessionRef,
    sessionScopeRevision: sessionHistory.workspaceSessionScopeRevision,
    selectedSessionTitle: sessionHistory.workspaceSelectedSessionTitle,
    selectedSessionWorkdir: sessionHistory.workspaceSelectedSessionWorkdir,
    selectedSessionModel: sessionHistory.workspaceSelectedSessionModel,
    error: directoryBrowser.workspaceBrowserError,
    launch: options.launch,
    preview: options.preview,
    getInstance: options.getInstance,
    resume: sessionResume.submit,
  });

  async function browseWorkspaceDirectory(path?: string) {
    return directoryBrowser.browseWorkspaceDirectory(path);
  }

  async function openWorkspacePicker(
    provider: Provider,
    cliKind?: AgentCliKind,
  ) {
    const requestId = ++pickerRequestId;
    workspacePickerProvider.value = provider;
    const preference = options.preferences.value.find(
      (item) => item.providerId === provider.identity.id,
    );
    const preferredCliKind = cliKind ?? preference?.cliKind ?? "codex";
    const initialCliKind = workspaceCliOptions.value.some(
      (option) => option.value === preferredCliKind,
    )
      ? preferredCliKind
      : (workspaceCliOptions.value[0]?.value ?? preferredCliKind);
    apiKeySelection.resetWorkspaceApiKeys(preference?.apiKeyLocalId ?? "");
    workspaceNewSessionModels.clear();
    if (preference) {
      workspaceNewSessionModels.set(preference.cliKind, preference.model);
    }
    workspaceNewSessionModels.set(
      initialCliKind,
      preference?.cliKind === initialCliKind
        ? preference.model
        : provider.cli.preferredModel?.trim() || "",
    );
    workspaceSessionName.value = "";
    runDrafts.clear();
    workspaceRunSettings.value = defaultTemporaryCliRunSettings();
    workspaceSessionMode.value = "new";
    sessionHistory.resetWorkspaceSessions();
    sessionHistory.workspaceSessionRoleFilter.value = "main";
    launchFlow.resetWorkspaceLaunch();
    directoryBrowser.resetWorkspaceDirectory();
    workspacePickerVisible.value = true;

    workspacePickerCliKind.value = initialCliKind;
    workspaceSelectedModel.value =
      workspaceNewSessionModels.get(initialCliKind) ?? "";
    workspaceTerminalKind.value = workspaceTerminalOptions.value.some(
      (option) => option.value === options.terminalKind.value,
    )
      ? options.terminalKind.value
      : (workspaceTerminalOptions.value[0]?.value ??
        options.terminalKind.value);

    // Opening the picker reuses the current probe. Directory/API-key loading
    // must not wait for shell startup or installation discovery.
    if (!options.cliEnvironmentProbe.value) {
      void options.probeCliTools().catch(() => undefined);
    }
    if (!options.terminalEnvironmentProbe.value) {
      void options
        .probeTerminals()
        .then(() => {
          if (!isCurrentPickerRequest(requestId, provider)) return;
          if (
            !workspaceTerminalOptions.value.some(
              (option) => option.value === workspaceTerminalKind.value,
            )
          ) {
            workspaceTerminalKind.value =
              workspaceTerminalOptions.value[0]?.value ??
              options.terminalKind.value;
          }
        })
        .catch(() => undefined);
    }
    void apiKeySelection.loadWorkspaceApiKeys(provider);

    const initialPath =
      preference?.workspacePath || options.workspaces.value[0]?.path;
    const loaded = await browseWorkspaceDirectory(initialPath);
    if (!isCurrentPickerRequest(requestId, provider)) return;
    if (!loaded && initialPath && workspacePickerVisible.value) {
      await browseWorkspaceDirectory();
    }
    if (!isCurrentPickerRequest(requestId, provider)) return;
  }

  function isCurrentPickerRequest(requestId: number, provider: Provider) {
    return (
      requestId === pickerRequestId &&
      workspacePickerProvider.value?.identity.id === provider.identity.id
    );
  }

  watch(workspaceCliOptions, (available) => {
    if (
      workspacePickerVisible.value &&
      available.length > 0 &&
      !available.some((option) => option.value === workspacePickerCliKind.value)
    ) {
      workspacePickerCliKind.value = available[0].value;
    }
  });

  watch(
    workspacePickerCliKind,
    (kind, previous) => {
      runDrafts.set(
        previous,
        copyTemporaryCliRunSettings(workspaceRunSettings.value),
      );
      workspaceRunSettings.value = copyTemporaryCliRunSettings(
        runDrafts.get(kind) ?? defaultTemporaryCliRunSettings(),
      );
      sessionHistory.resetWorkspaceSessions();
      if (
        workspaceSessionMode.value === "history" &&
        (!selectedCliTool.value?.capabilities.sessionHistory ||
          !selectedCliTool.value?.capabilities.sessionResume)
      ) {
        workspaceSessionMode.value = "new";
      }
      workspaceSelectedModel.value =
        workspaceSessionMode.value === "new"
          ? (workspaceNewSessionModels.get(kind) ?? "")
          : "";
      if (workspacePickerVisible.value) {
        void sessionHistory.loadWorkspaceSessions();
      }
    },
    { flush: "sync" },
  );

  watch(
    workspaceSessionMode,
    (mode, previousMode) => {
      if (mode === "new") {
        workspaceSelectedModel.value =
          workspaceNewSessionModels.get(workspacePickerCliKind.value) ?? "";
        sessionHistory.clearWorkspaceSessionSelection();
        sessionHistory.closeWorkspaceSessionDetail();
      } else if (previousMode === "new") {
        workspaceSelectedModel.value = "";
      }
      if (workspacePickerVisible.value) {
        void sessionHistory.loadWorkspaceSessions();
      }
    },
    { flush: "sync" },
  );

  watch(
    workspaceSelectedModel,
    (model) => {
      if (workspaceSessionMode.value === "new") {
        workspaceNewSessionModels.set(workspacePickerCliKind.value, model);
      }
    },
    { flush: "sync" },
  );

  watch(workspaceTerminalOptions, (available) => {
    if (
      workspacePickerVisible.value &&
      available.length > 0 &&
      !available.some((option) => option.value === workspaceTerminalKind.value)
    ) {
      workspaceTerminalKind.value = available[0].value;
    }
  });

  watch(workspacePickerVisible, (visible) => {
    if (visible) return;

    // 所有返回中的异步请求都属于已关闭界面，禁止继续回写状态。
    pickerRequestId += 1;
    directoryBrowser.invalidateWorkspaceDirectoryRequests();
    apiKeySelection.invalidateWorkspaceApiKeyRequests();
    sessionHistory.invalidateWorkspaceSessionRequests();
    launchFlow.resetWorkspaceLaunch();
  });

  return {
    workspacePickerVisible,
    workspacePickerProvider,
    workspacePickerCliKind,
    workspaceCliOptions,
    workspaceApiKeys: apiKeySelection.workspaceApiKeys,
    workspaceApiKeyLoading: apiKeySelection.workspaceApiKeyLoading,
    workspaceApiKeyError: apiKeySelection.workspaceApiKeyError,
    workspaceApiKeyLocalId: apiKeySelection.workspaceApiKeyLocalId,
    workspaceSelectedModel,
    workspaceSessionName,
    workspaceRunSettings,
    workspaceCanNameSession,
    workspaceSessionMode,
    workspaceSessionQuery: sessionHistory.workspaceSessionQuery,
    workspaceSessionResults: sessionHistory.workspaceSessionResults,
    workspaceSessionsLoading: sessionHistory.workspaceSessionsLoading,
    workspaceSessionsLoadingMore: sessionHistory.workspaceSessionsLoadingMore,
    workspaceSessionRoleFilter: sessionHistory.workspaceSessionRoleFilter,
    workspaceSessionTotal: sessionHistory.workspaceSessionTotal,
    workspaceSessionHasMore: sessionHistory.workspaceSessionHasMore,
    workspaceSelectedSessionRef: sessionHistory.workspaceSelectedSessionRef,
    workspaceSessionsError: sessionHistory.workspaceSessionsError,
    workspaceSessionIndexState: sessionHistory.workspaceSessionIndexState,
    workspaceSessionIndexMessage: sessionHistory.workspaceSessionIndexMessage,
    workspaceSelectedResumeId: sessionHistory.workspaceSelectedResumeId,
    workspaceSelectedSessionTitle: sessionHistory.workspaceSelectedSessionTitle,
    workspaceSelectedSessionWorkdir:
      sessionHistory.workspaceSelectedSessionWorkdir,
    workspaceSessionDetailVisible: sessionHistory.workspaceSessionDetailVisible,
    workspaceSessionDetailLoading: sessionHistory.workspaceSessionDetailLoading,
    workspaceSessionDetailError: sessionHistory.workspaceSessionDetailError,
    workspaceSessionDetail: sessionHistory.workspaceSessionDetail,
    workspaceSessionDetailRow: sessionHistory.workspaceSessionDetailRow,
    workspaceSessionDetailQuery: sessionHistory.workspaceSessionDetailQuery,
    workspaceSessionDetailPaused: sessionHistory.workspaceSessionDetailPaused,
    workspaceSessionMessageStates: sessionHistory.workspaceSessionMessageStates,
    navigateWorkspaceSessionDetail:
      sessionHistory.navigateWorkspaceSessionDetail,
    expandWorkspaceSessionMessage: sessionHistory.expandWorkspaceSessionMessage,
    pauseWorkspaceSessionDetail: sessionHistory.pauseWorkspaceSessionDetail,
    refreshWorkspaceSessionDetail: sessionHistory.refreshWorkspaceSessionDetail,

    workspaceTerminalKind,
    workspaceTerminalOptions,
    workspaceDirectory: directoryBrowser.workspaceDirectory,
    workspacePathDraft: directoryBrowser.workspacePathDraft,
    workspaceBrowsing: directoryBrowser.workspaceBrowsing,
    workspaceLaunchPreviewVisible: launchFlow.workspaceLaunchPreviewVisible,
    workspaceLaunchPreviewLoading: launchFlow.workspaceLaunchPreviewLoading,
    workspaceLaunchPreview: launchFlow.workspaceLaunchPreview,
    temporaryCliLaunchTasks: launchFlow.temporaryCliLaunchTasks,
    workspaceForgettingPath: directoryBrowser.workspaceForgettingPath,
    workspaceBrowserError: directoryBrowser.workspaceBrowserError,
    openWorkspacePicker,
    browseWorkspaceDirectory,
    launchWorkspace: launchFlow.launchWorkspace,
    confirmWorkspaceLaunch: launchFlow.confirmWorkspaceLaunch,
    loadWorkspaceSessions: sessionHistory.loadWorkspaceSessions,
    refreshWorkspaceSessions: sessionHistory.refreshWorkspaceSessions,
    loadMoreWorkspaceSessions: sessionHistory.loadMoreWorkspaceSessions,
    openWorkspaceSessionParent: sessionHistory.openWorkspaceSessionParent,
    openWorkspaceSessionDetail: sessionHistory.openWorkspaceSessionDetail,
    closeWorkspaceSessionDetail: sessionHistory.closeWorkspaceSessionDetail,
    selectWorkspaceSession: sessionHistory.selectWorkspaceSession,
    forgetWorkspace: directoryBrowser.forgetWorkspace,
  };
}
