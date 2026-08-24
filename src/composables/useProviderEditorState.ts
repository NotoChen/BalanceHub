import { computed, reactive, ref, watch } from "vue";
import type {
  Provider,
  ProviderApiKeyOption,
  ProviderConnectionTestResult,
  ProviderInput,
  ProviderProtocolDetectionResult,
  ProviderSiteProbeResult,
} from "../stores/providers";
import { emptyDraft, providerToInput } from "../utils/provider-input";
import { effectiveProviderApiKeyOptions } from "../utils/provider-api-key-options";
import { normalizeProviderBaseUrl } from "./provider-editor-shared";
import type { ProtocolSelectionSource, ProviderEditorStep } from "./provider-editor-shared";

export function useProviderEditorState() {
  const drawerVisible = ref(false);
  const editorSession = ref(0);
  const editorInitialStep = ref<ProviderEditorStep>("basics");
  const editingProviderId = ref<string | null>(null);
  const completingCredentials = ref(false);
  const testingConnection = ref(false);
  const probingSite = ref(false);
  const credentialCompletionMessage = ref("");
  const credentialCompletionSteps = ref<{ name: string; ok: boolean; message: string }[]>([]);
  const apiKeyOptions = ref<ProviderApiKeyOption[]>([]);
  const connectionTestResult = ref<ProviderConnectionTestResult | null>(null);
  const siteProbeResult = ref<ProviderSiteProbeResult | null>(null);
  const protocolDetectionResult = ref<ProviderProtocolDetectionResult | null>(null);
  const protocolSelectionSource = ref<ProtocolSelectionSource>("auto");
  const protocolSelectionBaseUrl = ref("");
  const draftProvider = reactive<ProviderInput>(emptyDraft());
  // 可用模型属于运行时能力数据，不写入 ProviderInput；仅作为编辑器的候选项。
  const availableModels = ref<string[]>([]);
  const siteNameSourceBaseUrl = ref("");

  const drawerTitle = computed(() => (editingProviderId.value ? "编辑中转站" : "添加中转站"));

  watch(drawerVisible, (visible, previous) => {
    if (!visible && previous) {
      editorSession.value += 1;
    }
  });

  function resetDraft() {
    completingCredentials.value = false;
    testingConnection.value = false;
    probingSite.value = false;
    Object.assign(draftProvider, emptyDraft());
    credentialCompletionMessage.value = "";
    credentialCompletionSteps.value = [];
    apiKeyOptions.value = [];
    connectionTestResult.value = null;
    siteProbeResult.value = null;
    protocolDetectionResult.value = null;
    protocolSelectionSource.value = "auto";
    protocolSelectionBaseUrl.value = "";
    availableModels.value = [];
    siteNameSourceBaseUrl.value = "";
  }

  function openAddProvider() {
    editorSession.value += 1;
    editorInitialStep.value = "basics";
    editingProviderId.value = null;
    resetDraft();
    drawerVisible.value = true;
  }

  function openEditProvider(provider: Provider, initialStep: ProviderEditorStep = "basics") {
    editorSession.value += 1;
    editorInitialStep.value = initialStep;
    completingCredentials.value = false;
    testingConnection.value = false;
    probingSite.value = false;
    editingProviderId.value = provider.identity.id;
    Object.assign(draftProvider, providerToInput(provider));
    availableModels.value = [...(provider.capabilities.availableModels || [])];
    credentialCompletionMessage.value = "";
    credentialCompletionSteps.value = [];
    setApiKeyOptions(provider.auth.apiKeyOptions || []);
    connectionTestResult.value = null;
    siteProbeResult.value = null;
    protocolDetectionResult.value = null;
    protocolSelectionSource.value = "saved";
    protocolSelectionBaseUrl.value = normalizeProviderBaseUrl(provider.identity.baseUrl);
    siteNameSourceBaseUrl.value = normalizeProviderBaseUrl(provider.identity.baseUrl);
    drawerVisible.value = true;
  }

  function setApiKeyOptions(options: ProviderApiKeyOption[]) {
    apiKeyOptions.value = effectiveProviderApiKeyOptions(draftProvider.auth.apiKey, options);
    draftProvider.auth.apiKeyOptions = [...apiKeyOptions.value];
    if (!draftProvider.auth.apiKeyTokenId.trim() && draftProvider.auth.apiKey.trim()) {
      draftProvider.auth.apiKeyTokenId =
        apiKeyOptions.value.find((option) => option.key === draftProvider.auth.apiKey)?.tokenId || "";
    }
  }

  function syncManagedApiKeys(provider: Provider) {
    if (editingProviderId.value !== provider.identity.id) return;
    draftProvider.auth.apiKey = provider.auth.apiKey;
    draftProvider.auth.apiKeyTokenId = provider.auth.apiKeyTokenId;
    setApiKeyOptions(provider.auth.apiKeyOptions || []);
  }

  return {
    drawerVisible,
    editorSession,
    editorInitialStep,
    editingProviderId,
    completingCredentials,
    testingConnection,
    probingSite,
    credentialCompletionMessage,
    credentialCompletionSteps,
    apiKeyOptions,
    connectionTestResult,
    siteProbeResult,
    protocolDetectionResult,
    protocolSelectionSource,
    protocolSelectionBaseUrl,
    draftProvider,
    availableModels,
    siteNameSourceBaseUrl,
    drawerTitle,
    openAddProvider,
    openEditProvider,
    setApiKeyOptions,
    syncManagedApiKeys,
  };
}
