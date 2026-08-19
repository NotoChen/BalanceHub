import { computed, ref, watch, type Ref } from "vue";
import type {
  AgentCliKind,
  CliEnvironmentProbeResult,
  CliSessionSummary,
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
import { useWorkspaceSessionHistory } from "./useWorkspaceSessionHistory";

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
  preview: (input: TemporaryCliLaunchInput) => Promise<TemporaryCliLaunchPreview>;
  getInstance: (instanceId: string) => Promise<TemporaryCliInstance | null>;
  listSessions: (cliKind: AgentCliKind, workdir: string) => Promise<CliSessionSummary[]>;
}

export function useWorkspacePicker(options: UseWorkspacePickerOptions) {
  const workspacePickerVisible = ref(false);
  const workspacePickerProvider = ref<Provider | null>(null);
  const workspacePickerCliKind = ref<AgentCliKind>("codex");
  const workspaceSelectedModel = ref("");
  const workspaceNewSessionModel = ref("");
  const workspaceSessionName = ref("");
  const workspaceSessionMode = ref<TemporaryCliSessionMode>("new");
  const workspaceTerminalKind = ref<TemporaryCliTerminalKind>(options.terminalKind.value);
  const workspaceCliOptions = computed(() =>
    availableCliOptions(options.cliEnvironmentProbe.value, "temporaryLaunch"),
  );
  const workspaceTerminalOptions = computed(() =>
    availableTerminalOptions(options.terminalEnvironmentProbe.value),
  );
  const selectedCliTool = computed(() =>
    agentCliTool(options.cliEnvironmentProbe.value, workspacePickerCliKind.value),
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
    directory: directoryBrowser.workspaceDirectory,
    listSessions: options.listSessions,
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
    apiKeyTokenId: apiKeySelection.workspaceApiKeyTokenId,
    selectedModel: workspaceSelectedModel,
    sessionMode: workspaceSessionMode,
    sessionName: workspaceSessionName,
    canNameSession: workspaceCanNameSession,
    selectedResumeId: sessionHistory.workspaceSelectedResumeId,
    selectedSessionTitle: sessionHistory.workspaceSelectedSessionTitle,
    error: directoryBrowser.workspaceBrowserError,
    launch: options.launch,
    preview: options.preview,
    getInstance: options.getInstance,
  });

  async function browseWorkspaceDirectory(path?: string) {
    sessionHistory.resetWorkspaceSessions();
    const loaded = await directoryBrowser.browseWorkspaceDirectory(path);
    const currentPath = directoryBrowser.workspaceDirectory.value?.currentPath;
    if (loaded && currentPath) {
      void sessionHistory.loadWorkspaceSessions(currentPath);
    }
    return loaded;
  }

  async function openWorkspacePicker(provider: Provider, cliKind?: AgentCliKind) {
    const requestId = ++pickerRequestId;
    workspacePickerProvider.value = provider;
    const preference = options.preferences.value.find(
      (item) => item.providerId === provider.identity.id,
    );
    const preferredCliKind = cliKind ?? preference?.cliKind ?? "codex";
    apiKeySelection.resetWorkspaceApiKeys(preference?.apiKeyTokenId ?? "");
    workspaceNewSessionModel.value =
      provider.cli.preferredModel?.trim()
      || preference?.model
      || provider.liveness.model
      || "";
    workspaceSelectedModel.value = workspaceNewSessionModel.value;
    workspaceSessionName.value = "";
    workspaceSessionMode.value = "new";
    sessionHistory.resetWorkspaceSessions();
    launchFlow.resetWorkspaceLaunch();
    directoryBrowser.resetWorkspaceDirectory();
    workspacePickerVisible.value = true;

    const probes: Promise<unknown>[] = [
      // 用户明确打开临时 CLI 时执行深探测，覆盖 shell 初始化脚本、版本管理器和新安装 CLI。
      options.probeCliTools(true).catch(() => undefined),
    ];
    if (!options.terminalEnvironmentProbe.value) {
      probes.push(options.probeTerminals().catch(() => undefined));
    }
    await Promise.all(probes);
    if (!isCurrentPickerRequest(requestId, provider)) return;

    workspacePickerCliKind.value = workspaceCliOptions.value.some(
      (option) => option.value === preferredCliKind,
    )
      ? preferredCliKind
      : (workspaceCliOptions.value[0]?.value ?? preferredCliKind);
    workspaceTerminalKind.value = workspaceTerminalOptions.value.some(
      (option) => option.value === options.terminalKind.value,
    )
      ? options.terminalKind.value
      : (workspaceTerminalOptions.value[0]?.value ?? options.terminalKind.value);

    const initialPath = preference?.workspacePath || options.workspaces.value[0]?.path;
    const loaded = await browseWorkspaceDirectory(initialPath);
    if (!isCurrentPickerRequest(requestId, provider)) return;
    if (!loaded && initialPath && workspacePickerVisible.value) {
      await browseWorkspaceDirectory();
    }
    if (!isCurrentPickerRequest(requestId, provider)) return;
    void apiKeySelection.loadWorkspaceApiKeys(provider);
  }

  function isCurrentPickerRequest(requestId: number, provider: Provider) {
    return requestId === pickerRequestId
      && workspacePickerProvider.value?.identity.id === provider.identity.id;
  }

  watch(workspaceCliOptions, (available) => {
    if (
      workspacePickerVisible.value
      && available.length > 0
      && !available.some((option) => option.value === workspacePickerCliKind.value)
    ) {
      workspacePickerCliKind.value = available[0].value;
    }
  });

  watch(workspacePickerCliKind, () => {
    sessionHistory.clearWorkspaceSessionSelection();
    if (
      workspaceSessionMode.value === "history"
      && (!selectedCliTool.value?.capabilities.sessionHistory
        || !selectedCliTool.value?.capabilities.sessionResume)
    ) {
      workspaceSessionMode.value = "new";
    }
    if (workspacePickerVisible.value && workspaceSessionMode.value !== "new") {
      workspaceSelectedModel.value = "";
    }
    const currentPath = directoryBrowser.workspaceDirectory.value?.currentPath;
    if (workspacePickerVisible.value && currentPath) {
      void sessionHistory.loadWorkspaceSessions(currentPath);
    }
  });

  watch(workspaceSessionMode, (mode, previousMode) => {
    if (mode === "new") {
      workspaceSelectedModel.value = workspaceNewSessionModel.value;
      sessionHistory.clearWorkspaceSessionSelection();
    } else if (previousMode === "new") {
      workspaceSelectedModel.value = "";
    }
    const currentPath = directoryBrowser.workspaceDirectory.value?.currentPath;
    if (workspacePickerVisible.value && currentPath) {
      void sessionHistory.loadWorkspaceSessions(currentPath);
    }
  });

  watch(workspaceSelectedModel, (model) => {
    if (workspaceSessionMode.value === "new") {
      workspaceNewSessionModel.value = model;
    }
  });

  watch(workspaceTerminalOptions, (available) => {
    if (
      workspacePickerVisible.value
      && available.length > 0
      && !available.some((option) => option.value === workspaceTerminalKind.value)
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
    workspaceApiKeyTokenId: apiKeySelection.workspaceApiKeyTokenId,
    workspaceSelectedModel,
    workspaceSessionName,
    workspaceCanNameSession,
    workspaceSessionMode,
    workspaceSessions: sessionHistory.workspaceSessions,
    workspaceSessionsLoading: sessionHistory.workspaceSessionsLoading,
    workspaceSessionsError: sessionHistory.workspaceSessionsError,
    workspaceSelectedResumeId: sessionHistory.workspaceSelectedResumeId,
    workspaceSelectedSessionTitle: sessionHistory.workspaceSelectedSessionTitle,
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
    selectWorkspaceSession: sessionHistory.selectWorkspaceSession,
    forgetWorkspace: directoryBrowser.forgetWorkspace,
  };
}
