import { computed, ref, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type {
  CliEnvironmentProbeResult,
  CliSessionSummary,
  LivenessCliKind,
  Provider,
  ProviderApiKeyOption,
  TemporaryCliLaunchInput,
  TemporaryCliLaunchResult,
  TemporaryCliPreference,
  TemporaryCliTerminalKind,
  Workspace,
  WorkspaceDirectoryListing,
} from "../stores/providers";
import { availableCliOptions, availableTerminalOptions } from "../utils/cli-environment";
import { supportsApiKeyManagement } from "../utils/provider-actions";

interface UseWorkspacePickerOptions {
  workspaces: Ref<Workspace[]>;
  preferences: Ref<TemporaryCliPreference[]>;
  terminalKind: Ref<TemporaryCliTerminalKind>;
  cliEnvironmentProbe: Ref<CliEnvironmentProbeResult | null>;
  listApiKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
  browse: (path?: string) => Promise<WorkspaceDirectoryListing>;
  forget: (path: string) => Promise<Workspace[]>;
  launch: (input: TemporaryCliLaunchInput) => Promise<TemporaryCliLaunchResult>;
  listSessions: (cliKind: LivenessCliKind, workdir: string) => Promise<CliSessionSummary[]>;
}

export type WorkspaceSessionMode = "new" | "resume";

export function useWorkspacePicker(options: UseWorkspacePickerOptions) {
  const workspacePickerVisible = ref(false);
  const workspacePickerProvider = ref<Provider | null>(null);
  const workspacePickerCliKind = ref<LivenessCliKind>("codex");
  const workspaceApiKeys = ref<ProviderApiKeyOption[]>([]);
  const workspaceApiKeyLoading = ref(false);
  const workspaceApiKeyError = ref("");
  const workspaceApiKeyTokenId = ref("");
  const workspaceSelectedModel = ref("");
  const workspaceNewSessionModel = ref("");
  const workspaceSessionMode = ref<WorkspaceSessionMode>("new");
  const workspaceSessions = ref<CliSessionSummary[]>([]);
  const workspaceSessionsLoading = ref(false);
  const workspaceSessionError = ref("");
  const workspaceSelectedSessionId = ref("");
  const workspaceTerminalKind = ref<TemporaryCliTerminalKind>(options.terminalKind.value);
  const workspaceCliOptions = computed(() => availableCliOptions(options.cliEnvironmentProbe.value));
  const workspaceTerminalOptions = computed(() =>
    availableTerminalOptions(options.cliEnvironmentProbe.value),
  );
  const workspaceDirectory = ref<WorkspaceDirectoryListing | null>(null);
  const workspacePathDraft = ref("");
  const workspaceBrowsing = ref(false);
  const workspaceLaunchingPath = ref<string | null>(null);
  const workspaceForgettingPath = ref<string | null>(null);
  const workspaceBrowserError = ref("");
  let browseRequestId = 0;
  let apiKeyRequestId = 0;
  let sessionRequestId = 0;
  let pickerRequestId = 0;

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
    workspaceBrowsing.value = true;
    workspaceBrowserError.value = "";
    try {
      const listing = await options.browse(path?.trim() || undefined);
      if (requestId !== browseRequestId) {
        return false;
      }
      workspaceDirectory.value = listing;
      workspacePathDraft.value = listing.currentPath;
      if (workspaceSessionMode.value === "resume") {
        void loadWorkspaceSessions(listing.currentPath, workspacePickerCliKind.value);
      } else {
        sessionRequestId += 1;
        workspaceSessions.value = [];
        workspaceSelectedSessionId.value = "";
        workspaceSessionError.value = "";
        workspaceSessionsLoading.value = false;
      }
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

  async function loadWorkspaceSessions(path?: string, cliKind = workspacePickerCliKind.value) {
    const requestId = ++sessionRequestId;
    const workdir = (path || workspaceDirectory.value?.currentPath || "").trim();
    workspaceSessions.value = [];
    workspaceSelectedSessionId.value = "";
    workspaceSessionError.value = "";
    if (!workspacePickerVisible.value || !workdir) {
      workspaceSessionsLoading.value = false;
      return;
    }

    workspaceSessionsLoading.value = true;
    try {
      const sessions = await options.listSessions(cliKind, workdir);
      if (
        requestId !== sessionRequestId
        || !workspacePickerVisible.value
        || workspacePickerCliKind.value !== cliKind
        || workspaceDirectory.value?.currentPath !== workdir
      ) {
        return;
      }
      workspaceSessions.value = sessions;
      if (sessions.length === 0) {
        workspaceSessionMode.value = "new";
        return;
      }
      selectWorkspaceSession(sessions[0].id);
    } catch (error) {
      if (requestId === sessionRequestId) {
        workspaceSessionError.value = error instanceof Error ? error.message : String(error);
        workspaceSessionMode.value = "new";
      }
    } finally {
      if (requestId === sessionRequestId) {
        workspaceSessionsLoading.value = false;
      }
    }
  }

  function selectWorkspaceSession(sessionId: string) {
    const session = workspaceSessions.value.find((item) => item.id === sessionId);
    workspaceSelectedSessionId.value = session?.id ?? "";
    if (workspaceSessionMode.value === "resume") {
      workspaceSelectedModel.value = session?.model?.trim() || "";
    }
  }

  async function openWorkspacePicker(provider: Provider, cliKind?: LivenessCliKind) {
    const requestId = ++pickerRequestId;
    workspacePickerProvider.value = provider;
    const preference = options.preferences.value.find(
      (item) => item.providerId === provider.identity.id,
    );
    const preferredCliKind = cliKind ?? preference?.cliKind ?? "codex";
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
    workspaceApiKeyTokenId.value = preference?.apiKeyTokenId ?? "";
    workspaceNewSessionModel.value =
      provider.cli.preferredModel?.trim() ||
      preference?.model ||
      provider.liveness.model ||
      "";
    workspaceSelectedModel.value = workspaceNewSessionModel.value;
    workspaceSessionMode.value = "new";
    workspaceSessions.value = [];
    workspaceSelectedSessionId.value = "";
    workspaceSessionError.value = "";
    workspaceSessionsLoading.value = false;
    workspacePickerVisible.value = true;
    workspaceDirectory.value = null;
    workspacePathDraft.value = "";
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

  async function launchWorkspace(path?: string) {
    const provider = workspacePickerProvider.value;
    const workdir = (path || workspaceDirectory.value?.currentPath || "").trim();
    if (!provider || !workdir || workspaceLaunchingPath.value) {
      return;
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
      return;
    }

    workspaceLaunchingPath.value = workdir;
    workspaceBrowserError.value = "";
    const selectedKey = workspaceApiKeys.value.find(
      (option) => option.tokenId === workspaceApiKeyTokenId.value,
    );
    const apiKey = selectedKey?.key || provider.auth.apiKey.trim();
    const resumeId = workspaceSessionMode.value === "resume"
      ? workspaceSelectedSessionId.value.trim()
      : "";
    if (workspaceSessionMode.value === "resume" && !resumeId) {
      const message = "请选择要继续的历史会话";
      workspaceBrowserError.value = message;
      Message.warning(message);
      workspaceLaunchingPath.value = null;
      return;
    }
    const selectedSession = workspaceSessions.value.find((session) => session.id === resumeId);
    const model = workspaceSessionMode.value === "resume"
      ? (selectedSession?.model?.trim() || "")
      : (workspaceSelectedModel.value.trim() || provider.cli.preferredModel.trim());
    if (!apiKey) {
      const message = "请选择一个可用的 API Key";
      workspaceBrowserError.value = message;
      Message.warning(message);
      workspaceLaunchingPath.value = null;
      return;
    }

    try {
      const result = await options.launch({
        providerId: provider.identity.id,
        cliKind: workspacePickerCliKind.value,
        workdir,
        apiKey,
        apiKeyTokenId: workspaceApiKeyTokenId.value,
        model,
        terminalKind: workspaceTerminalKind.value,
        resumeId: resumeId || null,
      });
      const cliLabel = workspacePickerCliKind.value === "codex" ? "Codex" : "Claude Code";
      if (result.workspaceError) {
        Message.warning(`${cliLabel} 已启动，但工作空间记录失败：${result.workspaceError}`);
      } else {
        Message.success(`已在所选工作空间启动 ${cliLabel}`);
      }
      workspacePickerVisible.value = false;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      workspaceBrowserError.value = message;
      Message.error(message);
    } finally {
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

  watch(workspacePickerCliKind, (cliKind) => {
    if (
      workspacePickerVisible.value
      && workspaceDirectory.value
      && workspaceSessionMode.value === "resume"
    ) {
      void loadWorkspaceSessions(workspaceDirectory.value.currentPath, cliKind);
    } else {
      sessionRequestId += 1;
      workspaceSessions.value = [];
      workspaceSelectedSessionId.value = "";
      workspaceSessionError.value = "";
      workspaceSessionsLoading.value = false;
    }
  });

  watch(workspaceSessionMode, (mode) => {
    if (mode === "new") {
      workspaceSelectedModel.value = workspaceNewSessionModel.value;
      return;
    }
    if (workspaceSelectedSessionId.value) {
      selectWorkspaceSession(workspaceSelectedSessionId.value);
    } else if (workspaceSessions.value.length > 0) {
      selectWorkspaceSession(workspaceSessions.value[0].id);
    } else if (workspacePickerVisible.value && workspaceDirectory.value) {
      void loadWorkspaceSessions(
        workspaceDirectory.value.currentPath,
        workspacePickerCliKind.value,
      );
    }
  });

  watch(workspaceSelectedSessionId, (sessionId) => {
    if (workspaceSessionMode.value === "resume") {
      selectWorkspaceSession(sessionId);
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
    sessionRequestId += 1;
    workspaceBrowsing.value = false;
    workspaceApiKeyLoading.value = false;
    workspaceSessionsLoading.value = false;
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
    workspaceSessionMode,
    workspaceSessions,
    workspaceSessionsLoading,
    workspaceSessionError,
    workspaceSelectedSessionId,
    workspaceTerminalKind,
    workspaceTerminalOptions,
    workspaceDirectory,
    workspacePathDraft,
    workspaceBrowsing,
    workspaceLaunchingPath,
    workspaceForgettingPath,
    workspaceBrowserError,
    openWorkspacePicker,
    browseWorkspaceDirectory,
    launchWorkspace,
    loadWorkspaceSessions,
    selectWorkspaceSession,
    forgetWorkspace,
  };
}
