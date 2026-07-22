import { ref, type Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type {
  LivenessCliKind,
  Provider,
  ProviderApiKeyOption,
  TemporaryCliLaunchInput,
  TemporaryCliLaunchResult,
  TemporaryCliPreference,
  Workspace,
  WorkspaceDirectoryListing,
} from "../stores/providers";
import { supportsApiKeyManagement } from "../utils/provider-display";

interface UseWorkspacePickerOptions {
  workspaces: Ref<Workspace[]>;
  preferences: Ref<TemporaryCliPreference[]>;
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
  const workspaceDirectory = ref<WorkspaceDirectoryListing | null>(null);
  const workspacePathDraft = ref("");
  const workspaceBrowsing = ref(false);
  const workspaceLaunchingPath = ref<string | null>(null);
  const workspaceForgettingPath = ref<string | null>(null);
  const workspaceBrowserError = ref("");
  let browseRequestId = 0;
  let apiKeyRequestId = 0;

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
    workspacePickerProvider.value = provider;
    const preference = options.preferences.value.find(
      (item) => item.providerId === provider.identity.id,
    );
    workspacePickerCliKind.value = cliKind ?? preference?.cliKind ?? "codex";
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
    if (!loaded && initialPath && workspacePickerVisible.value) {
      await browseWorkspaceDirectory();
    }
    void loadWorkspaceApiKeys(provider);
  }

  async function launchWorkspace(path?: string) {
    const provider = workspacePickerProvider.value;
    const workdir = (path || workspaceDirectory.value?.currentPath || "").trim();
    if (!provider || !workdir || workspaceLaunchingPath.value) {
      return;
    }

    workspaceLaunchingPath.value = workdir;
    workspaceBrowserError.value = "";
    const selectedKey = workspaceApiKeys.value.find(
      (option) => option.tokenId === workspaceApiKeyTokenId.value,
    );
    const apiKey = selectedKey?.key || provider.auth.apiKey.trim();
    const model =
      provider.cli.preferredModel.trim() ||
      workspaceSelectedModel.value.trim();
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

  return {
    workspacePickerVisible,
    workspacePickerProvider,
    workspacePickerCliKind,
    workspaceApiKeys,
    workspaceApiKeyLoading,
    workspaceApiKeyError,
    workspaceApiKeyTokenId,
    workspaceSelectedModel,
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
