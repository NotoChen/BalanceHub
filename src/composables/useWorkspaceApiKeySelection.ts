import { ref, type Ref } from "vue";
import type { Provider, ProviderApiKeyOption } from "../stores/providers";
import { effectiveProviderApiKeyOptions } from "../utils/provider-api-key-options.ts";
import { supportsApiKeyManagement } from "../utils/provider-actions.ts";

interface UseWorkspaceApiKeySelectionOptions {
  currentProvider: Ref<Provider | null>;
  listApiKeys: (providerId: string) => Promise<ProviderApiKeyOption[]>;
}

export function useWorkspaceApiKeySelection(options: UseWorkspaceApiKeySelectionOptions) {
  const workspaceApiKeys = ref<ProviderApiKeyOption[]>([]);
  const workspaceApiKeyLoading = ref(false);
  const workspaceApiKeyError = ref("");
  const workspaceApiKeyTokenId = ref("");
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
        requestId !== apiKeyRequestId
        || options.currentProvider.value?.identity.id !== provider.identity.id
      ) {
        return;
      }
      workspaceApiKeys.value = apiKeys;
      const providerKey = provider.auth.apiKey.trim();
      const effectiveOptions = effectiveProviderApiKeyOptions(providerKey, apiKeys);
      if (effectiveOptions.length === 1) {
        workspaceApiKeyTokenId.value = effectiveOptions[0].tokenId;
        return;
      }
      const preferredKeyExists = apiKeys.some(
        (option) => option.tokenId === workspaceApiKeyTokenId.value,
      );
      if (workspaceApiKeyTokenId.value && preferredKeyExists) {
        return;
      }
      workspaceApiKeyTokenId.value = providerKey ? "" : (apiKeys[0]?.tokenId ?? "");
    } catch (error) {
      if (requestId === apiKeyRequestId) {
        workspaceApiKeyTokenId.value = provider.auth.apiKey.trim() ? "" : workspaceApiKeyTokenId.value;
        workspaceApiKeyError.value = errorMessage(error);
      }
    } finally {
      if (requestId === apiKeyRequestId) {
        workspaceApiKeyLoading.value = false;
      }
    }
  }

  function resetWorkspaceApiKeys(preferredTokenId = "") {
    invalidateWorkspaceApiKeyRequests();
    workspaceApiKeys.value = [];
    workspaceApiKeyError.value = "";
    workspaceApiKeyTokenId.value = preferredTokenId;
  }

  function invalidateWorkspaceApiKeyRequests() {
    apiKeyRequestId += 1;
    workspaceApiKeyLoading.value = false;
  }

  return {
    workspaceApiKeys,
    workspaceApiKeyLoading,
    workspaceApiKeyError,
    workspaceApiKeyTokenId,
    loadWorkspaceApiKeys,
    resetWorkspaceApiKeys,
    invalidateWorkspaceApiKeyRequests,
  };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
