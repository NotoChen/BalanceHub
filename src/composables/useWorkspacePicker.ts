import { computed, ref, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type {
  CliEnvironmentProbeResult,
  LivenessCliKind,
  Provider,
  ProviderApiKeyOption,
  TemporaryCliInstance,
  TemporaryCliLaunchInput,
  TemporaryCliLaunchResult,
  TemporaryCliPreference,
  TemporaryCliSessionMode,
  TemporaryCliTerminalKind,
  Workspace,
  WorkspaceDirectoryListing,
} from "../stores/providers";
import {
  availableCliOptions,
  availableTerminalOptions,
  canNameSessionAtLaunch,
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
  listApiKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
  browse: (path?: string) => Promise<WorkspaceDirectoryListing>;
  forget: (path: string) => Promise<Workspace[]>;
  launch: (input: TemporaryCliLaunchInput) => Promise<TemporaryCliLaunchResult>;
  getInstance: (instanceId: string) => Promise<TemporaryCliInstance | null>;
}

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
  const workspaceSessionName = ref("");
  const workspaceSessionMode = ref<TemporaryCliSessionMode>("new");
  const workspaceCanNameSession = computed(() =>
    canNameSessionAtLaunch(
      options.cliEnvironmentProbe.value,
      workspacePickerCliKind.value,
      workspaceSessionMode.value,
    ),
  );
  const workspaceTerminalKind = ref<TemporaryCliTerminalKind>(options.terminalKind.value);
  const workspaceCliOptions = computed(() => availableCliOptions(options.cliEnvironmentProbe.value));
  const workspaceTerminalOptions = computed(() =>
    availableTerminalOptions(options.cliEnvironmentProbe.value),
  );
  const workspaceDirectory = ref<WorkspaceDirectoryListing | null>(null);
  const workspacePathDraft = ref("");
  const workspaceBrowsing = ref(false);
  const workspaceLaunchingPath = ref<string | null>(null);
  const workspaceLaunchProgress = ref(0);
  const workspaceLaunchStage = ref("");
  const workspaceForgettingPath = ref<string | null>(null);
  const workspaceBrowserError = ref("");
  let browseRequestId = 0;
  let apiKeyRequestId = 0;
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
    workspaceSessionName.value = "";
    workspaceSessionMode.value = "new";
    workspaceLaunchProgress.value = 0;
    workspaceLaunchStage.value = "";
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

    const selectedKey = workspaceApiKeys.value.find(
      (option) => option.tokenId === workspaceApiKeyTokenId.value,
    );
    const apiKey = selectedKey?.key || provider.auth.apiKey.trim();
    const model = workspaceSessionMode.value === "new"
      ? (workspaceSelectedModel.value.trim() || provider.cli.preferredModel.trim())
      : workspaceSelectedModel.value.trim();
    const sessionName = workspaceCanNameSession.value
      ? workspaceSessionName.value.trim()
      : "";
    if (!apiKey) {
      const message = "请选择一个可用的 API Key";
      workspaceBrowserError.value = message;
      Message.warning(message);
      workspaceLaunchingPath.value = null;
      return;
    }

    const cliLabel = workspacePickerCliKind.value === "codex" ? "Codex" : "Claude Code";
    const terminalLabel = workspaceTerminalOptions.value.find(
      (option) => option.value === workspaceTerminalKind.value,
    )?.label ?? "终端";
    const launchStage = (phase: TemporaryCliLaunchPhase) => {
      if (phase === "waiting") return `${terminalLabel} 已打开，等待 ${cliLabel} 响应`;
      if (phase === "confirming") return `正在确认 ${cliLabel} 进程`;
      return `${cliLabel} 已启动`;
    };
    let preparationTimer: number | null = null;

    workspaceLaunchingPath.value = workdir;
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
        providerId: provider.identity.id,
        cliKind: workspacePickerCliKind.value,
        workdir,
        apiKey,
        apiKeyTokenId: workspaceApiKeyTokenId.value,
        model,
        sessionMode: workspaceSessionMode.value,
        sessionName,
        terminalKind: workspaceTerminalKind.value,
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
    if (workspacePickerVisible.value && workspaceSessionMode.value !== "new") {
      workspaceSelectedModel.value = "";
    }
  });

  watch(workspaceSessionMode, (mode, previousMode) => {
    if (mode === "new") {
      workspaceSelectedModel.value = workspaceNewSessionModel.value;
      return;
    }
    if (previousMode === "new") {
      workspaceSelectedModel.value = "";
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
    workspaceBrowsing.value = false;
    workspaceApiKeyLoading.value = false;
    workspaceLaunchProgress.value = 0;
    workspaceLaunchStage.value = "";
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
    workspaceTerminalKind,
    workspaceTerminalOptions,
    workspaceDirectory,
    workspacePathDraft,
    workspaceBrowsing,
    workspaceLaunchingPath,
    workspaceLaunchProgress,
    workspaceLaunchStage,
    workspaceForgettingPath,
    workspaceBrowserError,
    openWorkspacePicker,
    browseWorkspaceDirectory,
    launchWorkspace,
    forgetWorkspace,
  };
}
