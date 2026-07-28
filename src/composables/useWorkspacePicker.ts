import { computed, ref, watch, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type {
  CliEnvironmentProbeResult,
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
import { supportsApiKeyManagement } from "../utils/provider-display";

interface UseWorkspacePickerOptions {
  workspaces: Ref<Workspace[]>;
  preferences: Ref<TemporaryCliPreference[]>;
  terminalKind: Ref<TemporaryCliTerminalKind>;
  cliEnvironmentProbe: Ref<CliEnvironmentProbeResult | null>;
  listApiKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
  browse: (path?: string) => Promise<WorkspaceDirectoryListing>;
  forget: (path: string) => Promise<Workspace[]>;
  launch: (input: TemporaryCliLaunchInput) => Promise<TemporaryCliLaunchResult>;
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
    workspaceSelectedModel.value =
      provider.cli.preferredModel?.trim() ||
      preference?.model ||
      provider.liveness.model ||
      "";
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
    const model =
      workspaceSelectedModel.value.trim() ||
      provider.cli.preferredModel.trim();
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
    forgetWorkspace,
  };
}
