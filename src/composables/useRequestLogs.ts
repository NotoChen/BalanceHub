import { computed, ref, watch } from "vue";
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

  async function loadRequestLogs() {
    const providerId = requestLogsProvider.value?.identity.id;
    if (!requestLogsVisible.value || !providerId) {
      return;
    }
    const query = requestLogsQuery();
    const queryKey = JSON.stringify([providerId, query]);
    if (queryKey !== resultQuery) requestLogsResult.value = null;
    await request.run(() => options.loadLogs(providerId, query), (result) => {
      requestLogsResult.value = result;
      resultQuery = queryKey;
    });
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
