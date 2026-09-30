import { ref, watch } from "vue";
import { useLatestRequest } from "./useLatestRequest.ts";
import type { Provider, ProviderUsageSummary } from "../stores/providers";
import type { UsagePeriod } from "../utils/usage-trend";

interface UseUsageSummaryOptions {
  loadUsage: (providerId: string, period: UsagePeriod) => Promise<ProviderUsageSummary>;
}

export function useUsageSummary(options: UseUsageSummaryOptions) {
  const usageVisible = ref(false);
  const usageProvider = ref<Provider | null>(null);
  const request = useLatestRequest({ timeoutMessage: "读取用量趋势超时，请重试" });
  const usageSummary = ref<ProviderUsageSummary | null>(null);
  const usagePeriod = ref<UsagePeriod>("24h");

  watch([usageVisible, () => usageProvider.value?.identity.id], () => {
    request.invalidate();
    usageSummary.value = null;
  }, { flush: "sync" });

  watch(usagePeriod, () => {
    request.invalidate();
    usageSummary.value = null;
    if (usageVisible.value) void refreshUsageSummary();
  }, { flush: "sync" });

  function openUsage(provider: Provider) {
    usageProvider.value = provider;
    usageSummary.value = null;
    usageVisible.value = true;
    void refreshUsageSummary();
  }

  async function refreshUsageSummary() {
    const providerId = usageProvider.value?.identity.id;
    if (!usageVisible.value || !providerId) return;
    const period = usagePeriod.value;
    await request.run(() => options.loadUsage(providerId, period), (summary) => {
      usageSummary.value = summary;
    });
  }

  return {
    usageVisible,
    usageProvider,
    usageLoading: request.loading,
    usageError: request.error,
    usageSummary,
    usagePeriod,
    openUsage,
    refreshUsageSummary,
  };
}
