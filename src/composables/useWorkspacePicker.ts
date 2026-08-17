import { computed, ref, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type {
  CliEnvironmentProbeResult,
  CliSessionSummary,
  AgentCliKind,
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
  availableCliOptions,
  availableTerminalOptions,
  canNameSessionAtLaunch,
  agentCliLabel,
  agentCliTool,
} from "../utils/cli-environment";
import { supportsApiKeyManagement } from "../utils/provider-actions";
import {
  waitForTemporaryCliStart,
  type TemporaryCliLaunchPhase,
} from "../utils/temporary-cli-launch";

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
  const workspaceApiKeys = ref<ProviderApiKeyOption[]>([]);
  const workspaceApiKeyLoading = ref(false);
  const workspaceApiKeyError = ref("");
  const workspaceApiKeyTokenId = ref("");
  const workspaceSelectedModel = ref("");
  const workspaceNewSessionModel = ref("");
  const workspaceSessionName = ref("");
  const workspaceSessionMode = ref<TemporaryCliSessionMode>("new");
  const workspaceSessions = ref<CliSessionSummary[]>([]);
  const workspaceSessionsLoading = ref(false);
  const workspaceSessionsError = ref("");
  const workspaceSelectedResumeId = ref("");
  const workspaceCanNameSession = computed(() =>
    canNameSessionAtLaunch(
      options.cliEnvironmentProbe.value,
      workspacePickerCliKind.value,
      workspaceSessionMode.value,
    ),
  );
  const selectedCliTool = computed(() =>
    agentCliTool(options.cliEnvironmentProbe.value, workspacePickerCliKind.value),
  );
  const workspaceTerminalKind = ref<TemporaryCliTerminalKind>(options.terminalKind.value);
  const workspaceCliOptions = computed(() =>
    availableCliOptions(options.cliEnvironmentProbe.value, "temporaryLaunch"),
  );
  const workspaceTerminalOptions = computed(() =>
    availableTerminalOptions(options.terminalEnvironmentProbe.value),
  );
  const workspaceDirectory = ref<WorkspaceDirectoryListing | null>(null);
  const workspacePathDraft = ref("");
  const workspaceBrowsing = ref(false);
  const workspaceLaunchingPath = ref<string | null>(null);
  const workspaceLaunchProgress = ref(0);
  const workspaceLaunchStage = ref("");
  const workspaceLaunchPreviewVisible = ref(false);
  const workspaceLaunchPreviewLoading = ref(false);
  const workspaceLaunchPreview = ref<TemporaryCliLaunchPreview | null>(null);
  const workspacePendingLaunchInput = ref<TemporaryCliLaunchInput | null>(null);
  const workspaceForgettingPath = ref<string | null>(null);
  const workspaceBrowserError = ref("");
  let browseRequestId = 0;
  let apiKeyRequestId = 0;
  let pickerRequestId = 0;
  let sessionsRequestId = 0;

  async function loadWorkspaceApiKeys(provider: Provider) {
    const requestId = ++apiKeyRequestId;
    workspaceApiKeys.value = [];
    workspaceApiKeyError.value = "";
    workspaceApiKeyLoading.value = false;
    if (!supportsApiKeyManagement(provider)) {
      workspaceApiKeyTokenId.value = provider.auth.apiKey.trim() ? "" : workspaceApiKeyTokenId.value;
      return;
    }

    workspaceApiKeyLoading.value = true;
    try {
      const apiKeys = await options.listApiKeys(provider.identity.id);
      if (
        requestId !== apiKeyRequestId ||
        workspacePickerProvider.value?.identity.id !== provider.identity.id
      ) {
        return;
      }
      workspaceApiKeys.value = apiKeys;
      const providerKey = provider.auth.apiKey.trim();
      const uniqueKeys = new Map<string, ProviderApiKeyOption>();
      if (providerKey) {
        uniqueKeys.set(providerKey, {
          name: "当前配置 API Key",
          key: providerKey,
          maskedKey: "",
          keyAvailable: true,
          tokenId: "",
          userId: "",
          status: "enabled",
          usedQuota: 0,
          remainQuota: 0,
          usedQuotaRaw: 0,
          remainQuotaRaw: 0,
          unlimitedQuota: false,
          group: "",
          crossGroupRetry: false,
          modelLimitsEnabled: false,
          modelLimits: [],
          allowIps: [],
          quotaDisplayType: "currency",
          currencySymbol: "$",
        });
      }
      for (const option of apiKeys) {
        const key = option.key.trim();
        if (key && !uniqueKeys.has(key)) {
          uniqueKeys.set(key, option);
        }
      }
      if (uniqueKeys.size === 1) {
        workspaceApiKeyTokenId.value = [...uniqueKeys.values()][0].tokenId;
        return;
      }
      const preferredKeyExists = apiKeys.some(
        (option) => option.tokenId === workspaceApiKeyTokenId.value,
      );
      if (workspaceApiKeyTokenId.value && preferredKeyExists) {
        return;
      }
      workspaceApiKeyTokenId.value = provider.auth.apiKey.trim()
        ? ""
        : (apiKeys[0]?.tokenId ?? "");
    } catch (error) {
      if (requestId === apiKeyRequestId) {
        workspaceApiKeyTokenId.value = provider.auth.apiKey.trim() ? "" : workspaceApiKeyTokenId.value;
        workspaceApiKeyError.value = error instanceof Error ? error.message : String(error);
      }
    } finally {
      if (requestId === apiKeyRequestId) {
        workspaceApiKeyLoading.value = false;
      }
    }
  }

  async function browseWorkspaceDirectory(path?: string) {
    const requestId = ++browseRequestId;
    workspaceSelectedResumeId.value = "";
    workspaceBrowsing.value = true;
    workspaceBrowserError.value = "";
    try {
      const listing = await options.browse(path?.trim() || undefined);
      if (requestId !== browseRequestId) {
        return false;
      }
      workspaceDirectory.value = listing;
      workspacePathDraft.value = listing.currentPath;
      void loadWorkspaceSessions(listing.currentPath);
      return true;
    } catch (error) {
      if (requestId === browseRequestId) {
        workspaceBrowserError.value = error instanceof Error ? error.message : String(error);
      }
      return false;
    } finally {
      if (requestId === browseRequestId) {
        workspaceBrowsing.value = false;
      }
    }
  }

  async function loadWorkspaceSessions(workdir?: string) {
    const path = (workdir || workspaceDirectory.value?.currentPath || "").trim();
    const requestId = ++sessionsRequestId;
    const previousResumeId = workspaceSelectedResumeId.value;
    workspaceSessionsError.value = "";
    workspaceSessions.value = [];
    if (!path || !workspacePickerVisible.value || workspaceSessionMode.value === "new") {
      workspaceSessionsLoading.value = false;
      return;
    }
    workspaceSessionsLoading.value = true;
    try {
      const sessions = await options.listSessions(workspacePickerCliKind.value, path);
      if (
        requestId !== sessionsRequestId
        || !workspacePickerVisible.value
        || workspaceDirectory.value?.currentPath !== path
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
        workspaceSessionsError.value = error instanceof Error ? error.message : String(error);
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
    workspaceSessionMode.value = "history";
    // 空值表示不向官方 CLI 注入模型，让它按会话自己的元数据恢复。
    workspaceSelectedModel.value = "";
  }

  async function openWorkspacePicker(provider: Provider, cliKind?: AgentCliKind) {
    const requestId = ++pickerRequestId;
    workspacePickerProvider.value = provider;
    const preference = options.preferences.value.find(
      (item) => item.providerId === provider.identity.id,
    );
    const preferredCliKind = cliKind ?? preference?.cliKind ?? "codex";
    workspaceApiKeyTokenId.value = preference?.apiKeyTokenId ?? "";
    workspaceNewSessionModel.value =
      provider.cli.preferredModel?.trim() ||
      preference?.model ||
      provider.liveness.model ||
      "";
    workspaceSelectedModel.value = workspaceNewSessionModel.value;
    workspaceSessionName.value = "";
    workspaceSessionMode.value = "new";
    workspaceSessions.value = [];
    workspaceSessionsError.value = "";
    workspaceSessionsLoading.value = false;
    workspaceSelectedResumeId.value = "";
    workspaceLaunchProgress.value = 0;
    workspaceLaunchStage.value = "";
    workspaceLaunchPreviewVisible.value = false;
    workspaceLaunchPreviewLoading.value = false;
    workspaceLaunchPreview.value = null;
    workspacePendingLaunchInput.value = null;
    workspacePickerVisible.value = true;
    workspaceDirectory.value = null;
    workspacePathDraft.value = "";
    const probes: Promise<unknown>[] = [];
    // 启动阶段只做浅探测；用户明确打开临时 CLI 时重新执行深探测，
    // 以覆盖 shell 初始化脚本、版本管理器和刚刚安装的 CLI。
    probes.push(options.probeCliTools(true).catch(() => undefined));
    if (!options.terminalEnvironmentProbe.value) {
      probes.push(options.probeTerminals().catch(() => undefined));
    }
    if (probes.length > 0) {
      await Promise.all(probes);
    }
    if (
      requestId !== pickerRequestId
      || workspacePickerProvider.value?.identity.id !== provider.identity.id
    ) {
      return;
    }
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
    if (
      requestId !== pickerRequestId
      || workspacePickerProvider.value?.identity.id !== provider.identity.id
    ) {
      return;
    }
    if (!loaded && initialPath && workspacePickerVisible.value) {
      await browseWorkspaceDirectory();
    }
    if (
      requestId !== pickerRequestId
      || workspacePickerProvider.value?.identity.id !== provider.identity.id
    ) {
      return;
    }
    void loadWorkspaceApiKeys(provider);
  }

  function buildLaunchInput(path?: string): TemporaryCliLaunchInput | null {
    const provider = workspacePickerProvider.value;
    const workdir = (path || workspaceDirectory.value?.currentPath || "").trim();
    if (!provider || !workdir || workspaceLaunchingPath.value || workspaceLaunchPreviewLoading.value) {
      return null;
    }
    if (
      !workspaceCliOptions.value.some((option) => option.value === workspacePickerCliKind.value) ||
      !workspaceTerminalOptions.value.some(
        (option) => option.value === workspaceTerminalKind.value,
      )
    ) {
      const message = "未检测到可用的 Agent 或终端";
      workspaceBrowserError.value = message;
      Message.warning(message);
      return null;
    }

    const selectedKey = workspaceApiKeys.value.find(
      (option) => option.tokenId === workspaceApiKeyTokenId.value,
    );
    const apiKey = selectedKey?.key || provider.auth.apiKey.trim();
    const model = selectedCliTool.value?.capabilities.modelSelection
      ? workspaceSessionMode.value === "new"
        ? (workspaceSelectedModel.value.trim() || provider.cli.preferredModel.trim())
        : workspaceSelectedModel.value.trim()
      : "";
    const sessionName = workspaceCanNameSession.value
      ? workspaceSessionName.value.trim()
      : "";
    const resumeId = workspaceSessionMode.value === "history"
      ? workspaceSelectedResumeId.value.trim()
      : "";
    if (!apiKey) {
      const message = "请选择一个可用的 API Key";
      workspaceBrowserError.value = message;
      Message.warning(message);
      return null;
    }
    if (workspaceSessionMode.value === "history" && !resumeId) {
      const message = "请选择一个历史会话后再启动";
      workspaceBrowserError.value = message;
      Message.warning(message);
      return null;
    }

    return {
      providerId: provider.identity.id,
      cliKind: workspacePickerCliKind.value,
      workdir,
      apiKey,
      apiKeyTokenId: workspaceApiKeyTokenId.value,
      model,
      sessionMode: workspaceSessionMode.value,
      sessionName,
      resumeId,
      terminalKind: workspaceTerminalKind.value,
    };
  }

  async function launchWorkspace(path?: string) {
    const input = buildLaunchInput(path);
    if (!input) {
      return;
    }
    workspaceLaunchPreviewLoading.value = true;
    workspaceBrowserError.value = "";
    try {
      workspaceLaunchPreview.value = await options.preview(input);
      workspacePendingLaunchInput.value = input;
      workspaceLaunchPreviewVisible.value = true;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      workspaceBrowserError.value = message;
      Message.error(message);
    } finally {
      workspaceLaunchPreviewLoading.value = false;
    }
  }

  async function confirmWorkspaceLaunch() {
    const input = workspacePendingLaunchInput.value;
    if (!input || workspaceLaunchingPath.value) {
      return;
    }
    workspaceLaunchPreviewVisible.value = false;
    workspacePendingLaunchInput.value = null;
    const provider = workspacePickerProvider.value;
    if (!provider) {
      return;
    }

    const cliLabel = agentCliLabel(options.cliEnvironmentProbe.value, input.cliKind);
    const terminalLabel = workspaceTerminalOptions.value.find(
      (option) => option.value === input.terminalKind,
    )?.label ?? "终端";
    const launchStage = (phase: TemporaryCliLaunchPhase) => {
      if (phase === "waiting") return `${terminalLabel} 已打开，等待 ${cliLabel} 响应`;
      if (phase === "confirming") return `正在确认 ${cliLabel} 进程`;
      return `${cliLabel} 已启动`;
    };
    let preparationTimer: number | null = null;

    workspaceLaunchingPath.value = input.workdir;
    workspaceBrowserError.value = "";
    workspaceLaunchProgress.value = 12;
    workspaceLaunchStage.value = `正在校验 ${cliLabel} 与 ${terminalLabel}`;
    try {
      await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
      workspaceLaunchProgress.value = 30;
      workspaceLaunchStage.value = `正在准备 ${cliLabel} 启动环境`;
      preparationTimer = window.setInterval(() => {
        workspaceLaunchProgress.value = Math.min(60, workspaceLaunchProgress.value + 2);
      }, 250);
      const result = await options.launch({
        ...input,
      });
      if (preparationTimer !== null) {
        window.clearInterval(preparationTimer);
        preparationTimer = null;
      }
      workspaceLaunchProgress.value = 68;
      workspaceLaunchStage.value = launchStage("waiting");
      await waitForTemporaryCliStart(result.instance.id, options.getInstance, {
        onProgress: (percent, phase) => {
          workspaceLaunchProgress.value = percent;
          workspaceLaunchStage.value = launchStage(phase);
        },
      });
      if (result.workspaceError) {
        Message.warning(`${cliLabel} 已启动，但工作空间记录失败：${result.workspaceError}`);
      } else {
        Message.success(`已在所选工作空间启动 ${cliLabel}`);
      }
      await new Promise<void>((resolve) => window.setTimeout(resolve, 250));
      workspacePickerVisible.value = false;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      workspaceLaunchStage.value = "启动失败";
      workspaceBrowserError.value = message;
      Message.error(message);
    } finally {
      if (preparationTimer !== null) {
        window.clearInterval(preparationTimer);
      }
      workspaceLaunchingPath.value = null;
    }
  }

  async function forgetWorkspace(path: string) {
    if (workspaceForgettingPath.value) {
      return;
    }
    workspaceForgettingPath.value = path;
    try {
      await options.forget(path);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      workspaceForgettingPath.value = null;
    }
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

  watch(workspacePickerCliKind, () => {
    workspaceSelectedResumeId.value = "";
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
    if (workspacePickerVisible.value && workspaceDirectory.value) {
      void loadWorkspaceSessions(workspaceDirectory.value.currentPath);
    }
  });

  watch(workspaceSessionMode, (mode, previousMode) => {
    if (mode === "new") {
      workspaceSelectedModel.value = workspaceNewSessionModel.value;
      workspaceSelectedResumeId.value = "";
      return;
    }
    if (mode !== "history") {
      workspaceSelectedResumeId.value = "";
    }
    if (previousMode === "new") {
      workspaceSelectedModel.value = "";
    }
    if (workspacePickerVisible.value && workspaceDirectory.value) {
      void loadWorkspaceSessions(workspaceDirectory.value.currentPath);
    }
  });

  watch(workspaceSelectedModel, (model) => {
    if (workspaceSessionMode.value === "new") {
      workspaceNewSessionModel.value = model;
    }
  });

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

    // 关闭选择器后，正在返回的目录与 API Key 请求均属于过期界面，不能再写回状态。
    // 同时复位 loading，避免用户关闭后快速重开时看到上一轮请求残留的忙碌状态。
    pickerRequestId += 1;
    browseRequestId += 1;
    apiKeyRequestId += 1;
    sessionsRequestId += 1;
    workspaceBrowsing.value = false;
    workspaceApiKeyLoading.value = false;
    workspaceSessionsLoading.value = false;
    workspaceLaunchProgress.value = 0;
    workspaceLaunchStage.value = "";
    workspaceLaunchPreviewVisible.value = false;
    workspaceLaunchPreviewLoading.value = false;
    workspaceLaunchPreview.value = null;
    workspacePendingLaunchInput.value = null;
  });

  return {
    workspacePickerVisible,
    workspacePickerProvider,
    workspacePickerCliKind,
    workspaceCliOptions,
    workspaceApiKeys,
    workspaceApiKeyLoading,
    workspaceApiKeyError,
    workspaceApiKeyTokenId,
    workspaceSelectedModel,
    workspaceSessionName,
    workspaceCanNameSession,
    workspaceSessionMode,
    workspaceSessions,
    workspaceSessionsLoading,
    workspaceSessionsError,
    workspaceSelectedResumeId,
    workspaceTerminalKind,
    workspaceTerminalOptions,
    workspaceDirectory,
    workspacePathDraft,
    workspaceBrowsing,
    workspaceLaunchingPath,
    workspaceLaunchProgress,
    workspaceLaunchStage,
    workspaceLaunchPreviewVisible,
    workspaceLaunchPreviewLoading,
    workspaceLaunchPreview,
    workspaceForgettingPath,
    workspaceBrowserError,
    openWorkspacePicker,
    browseWorkspaceDirectory,
    launchWorkspace,
    confirmWorkspaceLaunch,
    loadWorkspaceSessions,
    selectWorkspaceSession,
    forgetWorkspace,
  };
}
