import { computed, ref, watch } from "vue";
import { setLruEntry, touchLruEntry } from "../utils/lru-map.ts";
import { useLatestRequest } from "./useLatestRequest.ts";
import type {
  Provider,
  ProviderRequestLogFilters,
  ProviderRequestLogsQuery,
  ProviderRequestLogsResult,
  ProviderRequestLogTimeRange,
} from "../stores/providers";

interface UseRequestLogsOptions {
  providers: { value: Provider[] };
  loadLogs: (providerId: string, query: ProviderRequestLogsQuery) => Promise<ProviderRequestLogsResult>;
}

const RANGE_HOURS: Record<ProviderRequestLogTimeRange, number> = {
  "24h": 24,
  "7d": 7 * 24,
  "30d": 30 * 24,
  "90d": 90 * 24,
};

/// 同一查询短时间重复打开直接复用上次结果；「刷新」会绕过缓存重新读取。
const REQUEST_LOGS_CACHE_TTL_MS = 60_000;
const REQUEST_LOGS_CACHE_CAPACITY = 24;

interface RequestLogsCacheEntry {
  at: number;
  result: ProviderRequestLogsResult;
}

export function defaultRequestLogFilters(): ProviderRequestLogFilters {
  return {
    model: "",
    tokenName: "",
    group: "",
    requestId: "",
    logType: null,
    range: "30d",
  };
}

export function requestLogRangeTimestamps(range: ProviderRequestLogTimeRange) {
  const end = Math.floor(Date.now() / 1000);
  return { start: end - RANGE_HOURS[range] * 3600, end };
}

export function useRequestLogs(options: UseRequestLogsOptions) {
  const requestLogsVisible = ref(false);
  const requestLogsProviderId = ref<string | null>(null);
  const request = useLatestRequest({ timeoutMessage: "读取请求日志超时，请重试" });
  const requestLogsFilters = ref<ProviderRequestLogFilters>(defaultRequestLogFilters());
  const requestLogsPage = ref(0);
  const requestLogsPageSize = ref(20);
  const requestLogsResult = ref<ProviderRequestLogsResult | null>(null);
  /// 缓存 key 不带绝对时间戳：同一组筛选条件在 TTL 内重复打开沿用上次结果。
  const cache = new Map<string, RequestLogsCacheEntry>();

  const requestLogsProvider = computed(() =>
    options.providers.value.find((provider) => provider.identity.id === requestLogsProviderId.value) ?? null,
  );
  let resultQuery = "";

  watch([requestLogsVisible, () => requestLogsProvider.value?.identity.id], () => {
    request.invalidate();
    requestLogsResult.value = null;
    resultQuery = "";
  }, { flush: "sync" });

  function requestLogsQuery(): ProviderRequestLogsQuery {
    const { start, end } = requestLogRangeTimestamps(requestLogsFilters.value.range);
    return {
      model: requestLogsFilters.value.model.trim(),
      tokenName: requestLogsFilters.value.tokenName.trim(),
      group: requestLogsFilters.value.group.trim(),
      requestId: requestLogsFilters.value.requestId.trim(),
      logType: requestLogsFilters.value.logType,
      startTimestamp: start,
      endTimestamp: end,
      page: requestLogsPage.value,
      pageSize: requestLogsPageSize.value,
    };
  }

  async function loadRequestLogs(loadOptions: { force?: boolean } = {}) {
    const providerId = requestLogsProvider.value?.identity.id;
    if (!requestLogsVisible.value || !providerId) {
      return;
    }
    const query = requestLogsQuery();
    const cacheKey = requestLogsCacheKey(providerId);
    if (!loadOptions.force) {
      const cached = readCache(cacheKey);
      if (cached) {
        // 同一批筛选条件在 TTL 内重复打开：直接复用，不重新请求。
        request.invalidate();
        requestLogsResult.value = cached;
        resultQuery = JSON.stringify([providerId, query]);
        return;
      }
    }
    const queryKey = JSON.stringify([providerId, query]);
    if (queryKey !== resultQuery) requestLogsResult.value = null;
    await request.run(() => options.loadLogs(providerId, query), (result) => {
      requestLogsResult.value = result;
      resultQuery = queryKey;
      writeCache(cacheKey, result);
    });
  }

  function readCache(key: string) {
    // 命中先续期 LRU 顺序，容量淘汰时先丢最久没打开的查询。
    const entry = touchLruEntry(cache, key);
    if (!entry) return null;
    if (Date.now() - entry.at > REQUEST_LOGS_CACHE_TTL_MS) {
      cache.delete(key);
      return null;
    }
    return entry.result;
  }

  function writeCache(key: string, result: ProviderRequestLogsResult) {
    setLruEntry(cache, key, { at: Date.now(), result }, REQUEST_LOGS_CACHE_CAPACITY);
  }

  /// 缓存 key 不含绝对时间戳；时间范围只用于区分筛选档位。
  function requestLogsCacheKey(providerId: string) {
    const filters = requestLogsFilters.value;
    const provider = requestLogsProvider.value;
    return JSON.stringify([
      providerId,
      // 换地址、换协议或换 Key 后，日志归属的站点/凭据已经不同。
      provider?.identity.baseUrl ?? "",
      provider?.identity.protocol ?? "",
      provider?.auth?.credentialRevision ?? 0,
      filters.model,
      filters.tokenName,
      filters.group,
      filters.requestId,
      filters.logType,
      filters.range,
      requestLogsPage.value,
      requestLogsPageSize.value,
    ]);
  }

  function openRequestLogs(provider: Provider) {
    requestLogsProviderId.value = provider.identity.id;
    requestLogsFilters.value = defaultRequestLogFilters();
    requestLogsPage.value = 0;
    requestLogsResult.value = null;
    requestLogsVisible.value = true;
    void loadRequestLogs();
  }

  function searchRequestLogs(filters: ProviderRequestLogFilters) {
    requestLogsFilters.value = {
      ...filters,
      model: filters.model.trim(),
      tokenName: filters.tokenName.trim(),
      group: filters.group.trim(),
      requestId: filters.requestId.trim(),
    };
    requestLogsPage.value = 0;
    void loadRequestLogs();
  }

  function resetRequestLogFilters() {
    searchRequestLogs(defaultRequestLogFilters());
  }

  function setRequestLogsPage(page: number) {
    requestLogsPage.value = Math.max(0, page);
    void loadRequestLogs();
  }

  function setRequestLogsPageSize(pageSize: number) {
    requestLogsPageSize.value = pageSize;
    requestLogsPage.value = 0;
    void loadRequestLogs();
  }

  return {
    requestLogsVisible,
    requestLogsProvider,
    requestLogsLoading: request.loading,
    requestLogsError: request.error,
    requestLogsFilters,
    requestLogsPage,
    requestLogsPageSize,
    requestLogsResult,
    openRequestLogs,
    loadRequestLogs,
    searchRequestLogs,
    resetRequestLogFilters,
    setRequestLogsPage,
    setRequestLogsPageSize,
  };
}
