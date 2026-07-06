import { ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { Provider, ProviderUsageSummary } from "../stores/providers";
import type { UsagePeriod } from "../utils/usage-trend";

interface UseUsageSummaryOptions {
  loadUsage: (providerId: string, period: UsagePeriod) => Promise<ProviderUsageSummary>;
}

export function useUsageSummary(options: UseUsageSummaryOptions) {
  const usageVisible = ref(false);
  const usageProvider = ref<Provider | null>(null);
  const usageLoading = ref(false);
  const usageSummary = ref<ProviderUsageSummary | null>(null);
  const usagePeriod = ref<UsagePeriod>("24h");

  function openUsage(provider: Provider) {
    usageProvider.value = provider;
    usageSummary.value = null;
    usageVisible.value = true;
    void refreshUsageSummary();
  }

  async function refreshUsageSummary() {
    if (!usageProvider.value) return;
    usageLoading.value = true;
    try {
      usageSummary.value = await options.loadUsage(usageProvider.value.identity.id, usagePeriod.value);
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      usageLoading.value = false;
    }
  }

  return {
    usageVisible,
    usageProvider,
    usageLoading,
    usageSummary,
    usagePeriod,
    openUsage,
    refreshUsageSummary,
  };
}
