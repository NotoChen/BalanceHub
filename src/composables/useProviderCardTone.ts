import { computed, type Ref } from "vue";
import type { Provider } from "../stores/providers";
import {
  providerCardStatusTitle,
  providerCardStatusTone,
  type ProviderCardTone,
} from "../utils/provider-display.ts";

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

  function computeProviderCardTone(provider: Provider): ProviderCardTone {
    const status = providerCardStatusTone(provider);
    return status !== "disabled" && providerIntermediateLabel(provider) ? "syncing" : status;
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

  /// 整卡和状态标共用同一条说明：只有异常消息值得补，状态词本身已经写在卡上。
  function cardStatusTooltip(provider: Provider) {
    const tone = providerCardTone(provider);
    if (tone !== "warning" && tone !== "error") return "";
    return providerCardStatusTitle(provider);
  }

  return {
    providerIntermediateLabel,
    providerCardTone,
    cardStatusTooltip,
  };
}
