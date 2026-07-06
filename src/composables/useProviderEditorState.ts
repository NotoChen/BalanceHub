import { computed, reactive, ref } from "vue";
import type {
  Provider,
  ProviderApiKeyOption,
  ProviderConnectionTestResult,
  ProviderInput,
  ProviderSiteProbeResult,
} from "../stores/providers";
import { emptyDraft, providerToInput } from "../utils/provider-input";
import { normalizeProviderBaseUrl } from "./provider-editor-shared";

export function useProviderEditorState() {
  const drawerVisible = ref(false);
  const editingProviderId = ref<string | null>(null);
  const completingCredentials = ref(false);
  const testingConnection = ref(false);
  const probingSite = ref(false);
  const credentialCompletionMessage = ref("");
  const credentialCompletionSteps = ref<{ name: string; ok: boolean; message: string }[]>([]);
  const apiKeyOptions = ref<ProviderApiKeyOption[]>([]);
  const connectionTestResult = ref<ProviderConnectionTestResult | null>(null);
  const siteProbeResult = ref<ProviderSiteProbeResult | null>(null);
  const draftProvider = reactive<ProviderInput>(emptyDraft());
  const siteNameSourceBaseUrl = ref("");

  const drawerTitle = computed(() => (editingProviderId.value ? "编辑中转站" : "添加中转站"));

  function resetDraft() {
    Object.assign(draftProvider, emptyDraft());
    credentialCompletionMessage.value = "";
    credentialCompletionSteps.value = [];
    apiKeyOptions.value = [];
    connectionTestResult.value = null;
    siteProbeResult.value = null;
    siteNameSourceBaseUrl.value = "";
  }

  function openAddProvider() {
    editingProviderId.value = null;
    resetDraft();
    drawerVisible.value = true;
  }

  function openEditProvider(provider: Provider) {
    editingProviderId.value = provider.identity.id;
    Object.assign(draftProvider, providerToInput(provider));
    credentialCompletionMessage.value = "";
    credentialCompletionSteps.value = [];
    setApiKeyOptions(provider.auth.apiKey ? [currentApiKeyOption(provider.auth.apiKey)] : []);
    connectionTestResult.value = null;
    siteProbeResult.value = null;
    siteNameSourceBaseUrl.value = normalizeProviderBaseUrl(provider.identity.baseUrl);
    drawerVisible.value = true;
  }

  function setApiKeyOptions(options: ProviderApiKeyOption[]) {
    const items = [...options];
    if (draftProvider.auth.apiKey.trim()) {
      items.unshift(currentApiKeyOption(draftProvider.auth.apiKey.trim()));
    }

    const seen = new Set<string>();
    apiKeyOptions.value = items.filter((option) => {
      const key = option.key.trim();
      if (!key || seen.has(key)) {
        return false;
      }
      seen.add(key);
      return true;
    });
  }

  function currentApiKeyOption(key: string): ProviderApiKeyOption {
    return {
      name: "当前 API 密钥",
      key,
      tokenId: "",
      status: "",
      usedQuota: 0,
      remainQuota: 0,
      unlimitedQuota: false,
      group: "",
      modelLimitsEnabled: false,
      modelLimits: [],
      allowIps: [],
      createdTime: null,
      accessedTime: null,
      expiredTime: null,
    };
  }

  return {
    drawerVisible,
    editingProviderId,
    completingCredentials,
    testingConnection,
    probingSite,
    credentialCompletionMessage,
    credentialCompletionSteps,
    apiKeyOptions,
    connectionTestResult,
    siteProbeResult,
    draftProvider,
    siteNameSourceBaseUrl,
    drawerTitle,
    openAddProvider,
    openEditProvider,
    setApiKeyOptions,
  };
}
