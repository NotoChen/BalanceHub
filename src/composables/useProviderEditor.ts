import { Message } from "@arco-design/web-vue";
import type { Provider, ProviderInput } from "../stores/providers";
import { copyText } from "./useClipboard";
import { findSavedProvider, normalizeProviderBaseUrl, type ProviderEditorStore } from "./provider-editor-shared";
import { useProviderConnectionTest } from "./useProviderConnectionTest";
import { useProviderCredentialCompletion } from "./useProviderCredentialCompletion";
import { useProviderEditorState } from "./useProviderEditorState";
import { normalizeLivenessTiming } from "../utils/liveness-defaults";
import { providerToInput } from "../utils/provider-input";

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
    if (savedProvider && connectionTestResult.value?.ok) {
      await options.store.testProviderConnection(providerToInput(savedProvider));
    }
    if (editorSession.value !== session || !drawerVisible.value) {
      return;
    }
    drawerVisible.value = false;
    refreshAfterSave(savedProvider);
  }

  async function saveDraftAndFindProvider(isCurrent: () => boolean = () => true) {
    const input = currentProviderInput();
    const savedProviders = await options.store.saveProvider(input);
    const savedProvider = findSavedProvider(savedProviders, input);
    if (savedProvider && isCurrent()) {
      editingProviderId.value = savedProvider.identity.id;
      siteNameSourceBaseUrl.value = normalizeProviderBaseUrl(savedProvider.identity.baseUrl);
      return savedProvider;
    }
    return undefined;
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
