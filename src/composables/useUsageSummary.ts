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
    // 打开汇总或从卡片用量切回汇总时由这里发起读取；已经停在汇总视图里
    // 再次点击不会触发 watch，由 openUsageSummary 自己补一次重读。
    if (visible && !providerId) void refreshUsageSummary();
  }, { flush: "sync" });

  watch(usagePeriod, () => {
    request.invalidate();
    usageSummary.value = null;
    // 汇总视图的标题和平均 RPM 都按当前时段计算。上一个时段的汇总数字留在
    // 屏幕上，会被当成新时段的读数，所以两个视图都要在重新读取前收起。
    usageOverview.value = null;
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
    // 从关闭状态或某张卡片的用量切回汇总时，上面的 watch 已经会发起读取；
    // 只有本来就在汇总视图里再次点击才需要主动重读，否则每次打开都会发两次
    // 汇总请求。
    const rerunInPlace = usageVisible.value && usageProvider.value === null;
    usageProvider.value = null;
    usageSummary.value = null;
    usageOverview.value = null;
    usageVisible.value = true;
    if (rerunInPlace) void refreshUsageSummary();
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
