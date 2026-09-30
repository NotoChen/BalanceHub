import { computed, ref, watch } from "vue";
import { useLatestRequest } from "./useLatestRequest.ts";
import type {
  Provider,
  ProviderRequestLogsQuery,
  ProviderRequestLogsResult,
} from "../stores/providers";

interface UseRequestLogsOptions {
  providers: { value: Provider[] };
  loadLogs: (providerId: string, query: ProviderRequestLogsQuery) => Promise<ProviderRequestLogsResult>;
}

export function useRequestLogs(options: UseRequestLogsOptions) {
  const requestLogsVisible = ref(false);
  const requestLogsProviderId = ref<string | null>(null);
  const request = useLatestRequest({ timeoutMessage: "读取请求日志超时，请重试" });
  const requestLogsKeyword = ref("");
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
    return {
      keyword: requestLogsKeyword.value.trim(),
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
    requestLogsKeyword.value = "";
    requestLogsPage.value = 0;
    requestLogsResult.value = null;
    requestLogsVisible.value = true;
    void loadRequestLogs();
  }

  function searchRequestLogs(keyword: string) {
    requestLogsKeyword.value = keyword;
    requestLogsPage.value = 0;
    void loadRequestLogs();
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
    requestLogsKeyword,
    requestLogsPage,
    requestLogsPageSize,
    requestLogsResult,
    openRequestLogs,
    loadRequestLogs,
    searchRequestLogs,
    setRequestLogsPage,
    setRequestLogsPageSize,
  };
}
