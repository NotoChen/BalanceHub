import { computed, type Ref } from "vue";
import type { Provider } from "../stores/providers";
import {
  providerHasNoAvailableBalance,
  providerNeedsCheckIn,
  type ProviderCardTone,
} from "../utils/provider-display";

interface UseProviderCardToneOptions {
  providers: Ref<Provider[]>;
  checkingInProviderIds: Ref<string[]>;
  probingCapabilitiesProviderId: Ref<string | null>;
  editingProviderId: Ref<string | null>;
  probingSite: Ref<boolean>;
  testingConnection: Ref<boolean>;
  completingCredentials: Ref<boolean>;
}

export function useProviderCardTone(options: UseProviderCardToneOptions) {
  function providerIntermediateLabel(provider: Provider) {
    if (options.probingCapabilitiesProviderId.value === provider.identity.id) {
      return "探测中";
    }
    if (options.checkingInProviderIds.value.includes(provider.identity.id)) {
      return "签到中";
    }
    if (provider.runtime.status === "syncing") {
      return "同步中";
    }
    if (options.editingProviderId.value === provider.identity.id && options.probingSite.value) {
      return "探测中";
    }
    if (options.editingProviderId.value === provider.identity.id && options.testingConnection.value) {
      return "测试中";
    }
    if (options.editingProviderId.value === provider.identity.id && options.completingCredentials.value) {
      return "补全中";
    }
    return "";
  }

  function providerIsPendingSync(provider: Provider) {
    return provider.runtime.status === "warning" && !provider.automation.lastSyncedAt;
  }

  function computeProviderCardTone(provider: Provider): ProviderCardTone {
    if (providerIntermediateLabel(provider)) {
      return "syncing";
    }
    if (!provider.runtime.enabled || providerIsPendingSync(provider)) {
      return "disabled";
    }
    if (provider.runtime.status === "error") {
      return "error";
    }
    if (providerNeedsCheckIn(provider)) {
      return "warning";
    }
    if (providerHasNoAvailableBalance(provider)) {
      return "empty";
    }
    return "ok";
  }

  const providerToneMap = computed(() => {
    const map = new Map<string, ProviderCardTone>();
    for (const provider of options.providers.value) {
      map.set(provider.identity.id, computeProviderCardTone(provider));
    }
    return map;
  });

  function providerCardTone(provider: Provider): ProviderCardTone {
    return providerToneMap.value.get(provider.identity.id) ?? computeProviderCardTone(provider);
  }

  function cardStatusTooltip(provider: Provider) {
    if (providerCardTone(provider) === "error") {
      return provider.runtime.errorMessage || "接口访问异常";
    }
    return "";
  }

  return {
    providerIntermediateLabel,
    providerCardTone,
    cardStatusTooltip,
  };
}
