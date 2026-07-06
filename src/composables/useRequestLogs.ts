import { computed, ref } from "vue";
import { Message } from "@arco-design/web-vue";
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
  const requestLogsLoading = ref(false);
  const requestLogsKeyword = ref("");
  const requestLogsPage = ref(0);
  const requestLogsPageSize = ref(20);
  const requestLogsResult = ref<ProviderRequestLogsResult | null>(null);

  const requestLogsProvider = computed(() =>
    options.providers.value.find((provider) => provider.identity.id === requestLogsProviderId.value) ?? null,
  );

  function requestLogsQuery(): ProviderRequestLogsQuery {
    return {
      keyword: requestLogsKeyword.value.trim(),
      page: requestLogsPage.value,
      pageSize: requestLogsPageSize.value,
    };
  }

  async function loadRequestLogs() {
    if (!requestLogsProvider.value) {
      return;
    }

    requestLogsLoading.value = true;
    try {
      requestLogsResult.value = await options.loadLogs(requestLogsProvider.value.identity.id, requestLogsQuery());
    } catch (error) {
      Message.error(error instanceof Error ? error.message : String(error));
    } finally {
      requestLogsLoading.value = false;
    }
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
    requestLogsLoading,
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
