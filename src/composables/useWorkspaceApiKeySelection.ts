import { ref, type Ref } from "vue";
import type { Provider, ProviderApiKeyOption } from "../stores/providers";
import {
  effectiveProviderApiKeyOptions,
  isProviderApiKeyUsable,
  providerApiKeyOptionMatches,
  providerApiKeyOptionSelectionValue,
} from "../utils/provider-api-key-options.ts";

interface UseWorkspaceApiKeySelectionOptions {
  currentProvider: Ref<Provider | null>;
  listApiKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
}

export function useWorkspaceApiKeySelection(options: UseWorkspaceApiKeySelectionOptions) {
  const workspaceApiKeys = ref<ProviderApiKeyOption[]>([]);
  const workspaceApiKeyLoading = ref(false);
  const workspaceApiKeyError = ref("");
  const workspaceApiKeyLocalId = ref("");
  let apiKeyRequestId = 0;

  async function loadWorkspaceApiKeys(provider: Provider) {
    const requestId = ++apiKeyRequestId;
    workspaceApiKeys.value = [];
    workspaceApiKeyError.value = "";
    workspaceApiKeyLoading.value = false;
    workspaceApiKeyLoading.value = true;
    try {
      const localOptions = provider.auth.apiKeyOptions || [];
      let apiKeys: ProviderApiKeyOption[] = [];
      // Generic API-key providers and locally managed keys have no remote
      // vault endpoint. Do not probe a non-existent endpoint merely because
      // the picker was opened; local keys should be immediately usable.
      if (provider.actions.apiKeyManagement) {
        try {
          apiKeys = await options.listApiKeys(provider.identity.id);
        } catch (error) {
          workspaceApiKeyError.value = errorMessage(error);
        }
      }
      if (
        requestId !== apiKeyRequestId
        || options.currentProvider.value?.identity.id !== provider.identity.id
      ) {
        return;
      }
      workspaceApiKeys.value = effectiveProviderApiKeyOptions(provider.auth.apiKey, [
        ...localOptions,
        ...apiKeys,
      ]);
      const providerKey = provider.auth.apiKey.trim();
      const effectiveOptions = workspaceApiKeys.value;
      if (effectiveOptions.length === 1) {
        workspaceApiKeyLocalId.value = providerApiKeyOptionSelectionValue(effectiveOptions[0]);
        return;
      }
      const preferred = effectiveOptions.find((option) =>
        providerApiKeyOptionMatches(option, workspaceApiKeyLocalId.value),
      );
      if (preferred && isProviderApiKeyUsable(preferred)) {
        workspaceApiKeyLocalId.value = providerApiKeyOptionSelectionValue(preferred);
        return;
      }
      const configured = providerKey
        ? effectiveOptions.find((option) =>
          option.key.trim() === providerKey && isProviderApiKeyUsable(option),
        )
        : undefined;
      const fallback = effectiveOptions.find(isProviderApiKeyUsable);
      workspaceApiKeyLocalId.value = configured
        ? providerApiKeyOptionSelectionValue(configured)
        : (fallback ? providerApiKeyOptionSelectionValue(fallback) : "");
    } catch (error) {
      if (requestId === apiKeyRequestId) {
        workspaceApiKeyLocalId.value = provider.auth.apiKey.trim() ? "" : workspaceApiKeyLocalId.value;
        workspaceApiKeyError.value = errorMessage(error);
      }
    } finally {
      if (requestId === apiKeyRequestId) {
        workspaceApiKeyLoading.value = false;
      }
    }
  }

  function resetWorkspaceApiKeys(preferredLocalId = "") {
    invalidateWorkspaceApiKeyRequests();
    workspaceApiKeys.value = [];
    workspaceApiKeyError.value = "";
    workspaceApiKeyLocalId.value = preferredLocalId;
  }

  function invalidateWorkspaceApiKeyRequests() {
    apiKeyRequestId += 1;
    workspaceApiKeyLoading.value = false;
  }

  return {
    workspaceApiKeys,
    workspaceApiKeyLoading,
    workspaceApiKeyError,
    workspaceApiKeyLocalId,
    loadWorkspaceApiKeys,
    resetWorkspaceApiKeys,
    invalidateWorkspaceApiKeyRequests,
  };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
