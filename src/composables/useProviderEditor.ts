import { Message } from "@arco-design/web-vue";
import type { Provider, ProviderInput } from "../stores/providers";
import { copyText } from "./useClipboard";
import { normalizeProviderBaseUrl, type ProviderEditorStore } from "./provider-editor-shared";
import { useProviderConnectionTest } from "./useProviderConnectionTest";
import { useProviderCredentialCompletion } from "./useProviderCredentialCompletion";
import { useProviderEditorState } from "./useProviderEditorState";
import { normalizeLivenessTiming } from "../utils/liveness-defaults";
import { providerToInput } from "../utils/provider-input";
import { confirmAction } from "./provider-credential-dialogs";
import type { ProviderSaveOptions } from "../stores/provider-types";

interface UseProviderEditorOptions {
  store: ProviderEditorStore;
}

export function useProviderEditor(options: UseProviderEditorOptions) {
  const state = useProviderEditorState();
  const {
    drawerVisible,
    editorSession,
    editingProviderId,
    completingCredentials,
    testingConnection,
    probingSite,
    credentialCompletionMessage,
    credentialCompletionSteps,
    connectionTestResult,
    siteProbeResult,
    protocolDetectionResult,
    protocolSelectionSource,
    protocolSelectionBaseUrl,
    draftProvider,
    availableModels,
    siteNameSourceBaseUrl,
    setApiKeyOptions,
  } = state;

  const { testConnection } = useProviderConnectionTest({
    draftProvider,
    drawerVisible,
    editorSession,
    editingProviderId,
    testingConnection,
    connectionTestResult,
    testProviderConnection: (input) => options.store.testProviderConnection(input),
  });

  const credentialAssistant = useProviderCredentialCompletion({
    draftProvider,
    providerProtocols: () => options.store.providerProtocols,
    drawerVisible,
    editorSession,
    editingProviderId,
    probingSite,
    siteProbeResult,
    protocolDetectionResult,
    protocolSelectionSource,
    protocolSelectionBaseUrl,
    completingCredentials,
    credentialCompletionMessage,
    credentialCompletionSteps,
    siteNameSourceBaseUrl,
    detectProviderProtocol: (input) => options.store.detectProviderProtocol(input),
    probeProviderSite: (input) => options.store.probeProviderSite(input),
    completeProviderCredentials: (input) => options.store.completeProviderCredentials(input),
    createApiKeyForInput: (input, name) => options.store.createApiKeyForInput(input, name),
    generateAccessTokenForInput: (input) => options.store.generateAccessTokenForInput(input),
    setApiKeyOptions,
    saveDraftAndFindProvider,
    refreshAfterSave,
  });

  function openAddProvider() {
    state.openAddProvider();
    credentialAssistant.resetCredentialAssistant();
  }

  function openEditProvider(provider: Provider) {
    state.openEditProvider(provider);
    credentialAssistant.resetCredentialAssistant();
  }

  async function copyDraftApiKey() {
    const value = draftProvider.auth.apiKey.trim();
    if (!value) {
      Message.warning("API 密钥为空");
      return;
    }

    try {
      await copyText(value);
      Message.success("已复制 API 密钥");
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  async function saveProvider() {
    try {
      const session = editorSession.value;
      await credentialAssistant.ensureProtocolSelection();
      if (editorSession.value !== session || !drawerVisible.value) {
        return;
      }
      const savedProvider = await saveDraftAndFindProvider(
        () => editorSession.value === session && drawerVisible.value,
      );
      if (editorSession.value !== session || !drawerVisible.value) {
        return;
      }
      if (!savedProvider) {
        return;
      }
      if (savedProvider && connectionTestResult.value?.ok) {
        await options.store.testProviderConnection(providerToInput(savedProvider));
      }
      if (editorSession.value !== session || !drawerVisible.value) {
        return;
      }
      drawerVisible.value = false;
      refreshAfterSave(savedProvider);
    } catch (error) {
      // Keep the editor open so the user can correct a duplicate or invalid value in place.
      Message.error(error instanceof Error ? error.message : String(error));
    }
  }

  async function saveDraftAndFindProvider(
    isCurrent: () => boolean = () => true,
    saveOptions: ProviderSaveOptions = {},
  ) {
    const input = currentProviderInput();
    const result = await options.store.saveProvider(input, saveOptions);
    if (!result.saved) {
      const conflict = result.conflict;
      if (!conflict || !isCurrent()) {
        return undefined;
      }
      const confirmed = await confirmDuplicateConflict(conflict.kind, conflict.existingProviderName);
      if (!confirmed || !isCurrent()) {
        return undefined;
      }
      const retryOptions: ProviderSaveOptions = conflict.kind === "sameUrlDifferentApiKey"
        ? { mergeApiKeyIntoProviderId: conflict.existingProviderId }
        : { overwriteProviderId: conflict.existingProviderId };
      return saveDraftAndFindProvider(isCurrent, retryOptions);
    }

    const savedProvider = result.provider ?? undefined;
    if (savedProvider && isCurrent()) {
      editingProviderId.value = savedProvider.identity.id;
      siteNameSourceBaseUrl.value = normalizeProviderBaseUrl(savedProvider.identity.baseUrl);
      return savedProvider;
    }
    return undefined;
  }

  function confirmDuplicateConflict(
    kind: "sameAccount" | "sameApiKey" | "sameUrlDifferentApiKey",
    existingName: string,
  ) {
    if (kind === "sameUrlDifferentApiKey") {
      return confirmAction(
        "站点已有中转站配置",
        `检测到同一站点已有“${existingName}”。是否将当前 API Key 添加到该中转站，而不是创建新的卡片？`,
        "添加 API Key",
        "normal",
      );
    }
    if (kind === "sameApiKey") {
      return confirmAction(
        "API Key 已存在",
        `“${existingName}”已经保存了相同的 API Key。是否覆盖已有中转站配置？`,
        "覆盖配置",
        "warning",
      );
    }
    return confirmAction(
      "账号已存在",
      `检测到“${existingName}”是同一站点的同一账号。是否覆盖已有中转站配置？`,
      "覆盖配置",
      "warning",
    );
  }

  function currentProviderInput(): ProviderInput {
    normalizeLivenessTiming(draftProvider.liveness);
    return {
      ...draftProvider,
      identity: {
        ...draftProvider.identity,
        backupUrls: normalizeBackupUrls(draftProvider.identity.backupUrls),
        name:
          normalizeProviderBaseUrl(draftProvider.identity.baseUrl) === siteNameSourceBaseUrl.value
            ? draftProvider.identity.name
            : "",
      },
      cli: {
        preferredModel: draftProvider.cli.preferredModel.trim(),
      },
      id: editingProviderId.value ?? undefined,
    };
  }

  function normalizeBackupUrls(values: string[]) {
    const normalized: string[] = [];
    for (const value of values) {
      const url = value.trim().replace(/\/+$/, "");
      if (url && !normalized.includes(url)) {
        normalized.push(url);
      }
    }
    return normalized;
  }

  function refreshAfterSave(provider: Provider | undefined) {
    if (!provider?.runtime.enabled) {
      return;
    }
    void options.store.refreshByIds([provider.identity.id]).then((error) => {
      if (error) {
        Message.error(`保存后刷新失败：${error}`);
      }
    });
    void options.store
      .probeCapabilities(provider.identity.id)
      .then((result) => {
        if (editingProviderId.value === provider.identity.id) {
          availableModels.value = [...(result.provider.capabilities.availableModels || [])];
        }
      })
      .catch(() => undefined);
  }

  return {
    ...state,
    openAddProvider,
    openEditProvider,
    copyDraftApiKey,
    testConnection,
    saveProvider,
    ...credentialAssistant,
  };
}
