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
    editingProviderId,
    completingCredentials,
    testingConnection,
    probingSite,
    credentialCompletionMessage,
    credentialCompletionSteps,
    connectionTestResult,
    siteProbeResult,
    draftProvider,
    siteNameSourceBaseUrl,
    setApiKeyOptions,
  } = state;

  const { testConnection } = useProviderConnectionTest({
    draftProvider,
    editingProviderId,
    testingConnection,
    connectionTestResult,
    testProviderConnection: (input) => options.store.testProviderConnection(input),
  });

  const credentialAssistant = useProviderCredentialCompletion({
    draftProvider,
    editingProviderId,
    probingSite,
    siteProbeResult,
    completingCredentials,
    credentialCompletionMessage,
    credentialCompletionSteps,
    siteNameSourceBaseUrl,
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
    const savedProvider = await saveDraftAndFindProvider();
    if (savedProvider && connectionTestResult.value?.ok) {
      await options.store.testProviderConnection(providerToInput(savedProvider));
    }
    drawerVisible.value = false;
    refreshAfterSave(savedProvider);
  }

  async function saveDraftAndFindProvider() {
    const input = currentProviderInput();
    const savedProviders = await options.store.saveProvider(input);
    const savedProvider = findSavedProvider(savedProviders, input);
    if (savedProvider) {
      editingProviderId.value = savedProvider.identity.id;
      siteNameSourceBaseUrl.value = normalizeProviderBaseUrl(savedProvider.identity.baseUrl);
    }
    return savedProvider;
  }

  function currentProviderInput(): ProviderInput {
    normalizeLivenessTiming(draftProvider.liveness);
    return {
      ...draftProvider,
      identity: {
        ...draftProvider.identity,
        name:
          normalizeProviderBaseUrl(draftProvider.identity.baseUrl) === siteNameSourceBaseUrl.value
            ? draftProvider.identity.name
            : "",
      },
      id: editingProviderId.value ?? undefined,
    };
  }

  function refreshAfterSave(provider: Provider | undefined) {
    if (!provider?.runtime.enabled) {
      return;
    }
    void options.store.refreshByIds([provider.identity.id]);
    void options.store.probeCapabilities(provider.identity.id).catch(() => undefined);
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
