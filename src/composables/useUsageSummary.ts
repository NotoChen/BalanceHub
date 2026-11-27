import { ref, watch } from "vue";
import { setLruEntry, touchLruEntry } from "../utils/lru-map.ts";
import { useLatestRequest } from "./useLatestRequest.ts";
import type {
  Provider,
  ProviderUsageOverview,
  ProviderUsageSummary,
} from "../stores/providers";
import type { UsagePeriod } from "../utils/usage-trend";

interface UseUsageSummaryOptions {
  providers: { value: Provider[] };
  loadUsage: (providerId: string, period: UsagePeriod) => Promise<ProviderUsageSummary>;
  loadOverview: (period: UsagePeriod) => Promise<ProviderUsageOverview>;
}

/// 用量趋势在一分钟内往返同一张卡片很常见；短缓存让重复打开立即出数，
/// 显式刷新仍然强制重新读取。
const USAGE_CACHE_TTL_MS = 30_000;

export function useUsageSummary(options: UseUsageSummaryOptions) {
  const usageVisible = ref(false);
  const usageProvider = ref<Provider | null>(null);
  const request = useLatestRequest({ timeoutMessage: "读取用量趋势超时，请重试" });
  const usageSummary = ref<ProviderUsageSummary | null>(null);
  /// 不带中转站时读取的是全部站点的汇总，面板据此切换布局。
  const usageOverview = ref<ProviderUsageOverview | null>(null);
  const usagePeriod = ref<UsagePeriod>("24h");
  const summaryCache = new Map<string, { at: number; value: ProviderUsageSummary }>();
  const overviewCache = new Map<string, { at: number; value: ProviderUsageOverview }>();

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
    // 已经停在汇总视图里再次点击顶栏入口属于显式重读，绕过短缓存。
    if (rerunInPlace) void refreshUsageSummary({ force: true });
  }

  /// 汇总视图合并的是多个站点：站点增删、地址、协议、认证方式或凭据版本
  /// 变化后旧合计必须失效，否则刚换完 Key 再打开会看到上一个凭据的读数。
  /// 运行状态不纳入 key，后台余额刷新不会把缓存冲掉，短暂状态变化由 TTL 兜底。
  function overviewCacheKey(period: UsagePeriod) {
    const sources = options.providers.value
      .map((provider) => [
        provider.identity.id,
        provider.identity.baseUrl,
        provider.identity.protocol,
        provider.auth?.mode ?? "",
        provider.auth?.credentialRevision ?? 0,
      ].join(":"))
      .join("|");
    return `${period}:${sources}`;
  }

  async function refreshUsageSummary(loadOptions: { force?: boolean } = {}) {
    if (!usageVisible.value) return;
    const providerId = usageProvider.value?.identity.id ?? null;
    const period = usagePeriod.value;
    if (!providerId) {
      const key = overviewCacheKey(period);
      if (!loadOptions.force) {
        const cached = readCache(overviewCache, key);
        if (cached) {
          request.invalidate();
          usageOverview.value = cached;
          return;
        }
      }
      await request.run(() => options.loadOverview(period), (overview) => {
        usageOverview.value = overview;
        writeCache(overviewCache, key, overview);
      });
      return;
    }
    const provider = usageProvider.value;
    // 缓存跟着站点和当前凭据走：同一 id 换了地址、协议或 Key 后，旧站点的
    // 用量不能在新凭据下继续复用。
    const key = [
      providerId,
      period,
      provider?.identity.baseUrl ?? "",
      provider?.identity.protocol ?? "",
      provider?.auth?.credentialRevision ?? 0,
    ].join(":");
    if (!loadOptions.force) {
      const cached = readCache(summaryCache, key);
      if (cached) {
        request.invalidate();
        usageSummary.value = cached;
        return;
      }
    }
    await request.run(() => options.loadUsage(providerId, period), (summary) => {
      usageSummary.value = summary;
      writeCache(summaryCache, key, summary);
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

const USAGE_CACHE_CAPACITY = 16;

function readCache<T>(cache: Map<string, { at: number; value: T }>, key: string) {
  // 命中先续期 LRU 顺序，容量淘汰时先丢最久没打开的组合。
  const entry = touchLruEntry(cache, key);
  if (!entry) return null;
  if (Date.now() - entry.at > USAGE_CACHE_TTL_MS) {
    cache.delete(key);
    return null;
  }
  return entry.value;
}

function writeCache<T>(cache: Map<string, { at: number; value: T }>, key: string, value: T) {
  setLruEntry(cache, key, { at: Date.now(), value }, USAGE_CACHE_CAPACITY);
}
