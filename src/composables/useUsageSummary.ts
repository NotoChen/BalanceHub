import { ref, watch } from "vue";
import { useLatestRequest } from "./useLatestRequest.ts";
import type {
  Provider,
  ProviderUsageOverview,
  ProviderUsageSummary,
} from "../stores/providers";
import type { UsagePeriod } from "../utils/usage-trend";

interface UseUsageSummaryOptions {
  loadUsage: (providerId: string, period: UsagePeriod) => Promise<ProviderUsageSummary>;
  loadOverview: (period: UsagePeriod) => Promise<ProviderUsageOverview>;
}

export function useUsageSummary(options: UseUsageSummaryOptions) {
  const usageVisible = ref(false);
  const usageProvider = ref<Provider | null>(null);
  const request = useLatestRequest({ timeoutMessage: "读取用量趋势超时，请重试" });
  const usageSummary = ref<ProviderUsageSummary | null>(null);
  /// 不带中转站时读取的是全部站点的汇总，面板据此切换布局。
  const usageOverview = ref<ProviderUsageOverview | null>(null);
  const usagePeriod = ref<UsagePeriod>("24h");

  watch([usageVisible, () => usageProvider.value?.identity.id], ([visible, providerId]) => {
    request.invalidate();
    usageSummary.value = null;
    usageOverview.value = null;
    // 汇总视图没有卡片入口，只从顶栏打开、从不会被重新“切换”回来；watch 不会在
    // 同值赋值时触发，所以每次打开都要在这里重新读取。
    if (visible && !providerId) void refreshUsageSummary();
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

  /// 卡片打开的是单个中转站；不传中转站即打开全部站点的汇总视图，
  /// 后端在没有站点时聚合所有可读取的中转站。
  function openUsageSummary() {
    usageProvider.value = null;
    usageSummary.value = null;
    usageOverview.value = null;
    usageVisible.value = true;
    void refreshUsageSummary();
  }

  async function refreshUsageSummary() {
    if (!usageVisible.value) return;
    const providerId = usageProvider.value?.identity.id ?? null;
    const period = usagePeriod.value;
    if (!providerId) {
      await request.run(() => options.loadOverview(period), (overview) => {
        usageOverview.value = overview;
      });
      return;
    }
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
    usageOverview,
    usagePeriod,
    openUsage,
    openUsageSummary,
    refreshUsageSummary,
  };
}
