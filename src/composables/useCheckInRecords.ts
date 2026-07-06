import { computed, ref, type Ref } from "vue";
import type { Provider, ProviderCheckInRecordsResult } from "../stores/providers";

interface UseCheckInRecordsOptions {
  providers: Ref<Provider[]>;
  loadRecords: (providerId: string, month: string) => Promise<ProviderCheckInRecordsResult>;
}

export function currentMonthValue() {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`;
}

export function useCheckInRecords(options: UseCheckInRecordsOptions) {
  const checkInRecordsVisible = ref(false);
  const checkInRecordsProviderId = ref<string | null>(null);
  const checkInRecordsMonth = ref(currentMonthValue());
  const checkInRecordsLoading = ref(false);
  const checkInRecordsError = ref("");
  const checkInRecordsCache = ref<Map<string, ProviderCheckInRecordsResult>>(new Map());

  const checkInRecordsProvider = computed(() =>
    options.providers.value.find((provider) => provider.identity.id === checkInRecordsProviderId.value) ?? null,
  );

  const checkInRecordsResult = computed(() => {
    if (!checkInRecordsProviderId.value) {
      return null;
    }
    return checkInRecordsCache.value.get(checkInRecordsCacheKey(checkInRecordsProviderId.value, checkInRecordsMonth.value)) ?? null;
  });

  function openCheckInRecords(provider: Provider) {
    checkInRecordsProviderId.value = provider.identity.id;
    checkInRecordsMonth.value = currentMonthValue();
    checkInRecordsError.value = "";
    checkInRecordsVisible.value = true;
    void loadCheckInRecords();
  }

  async function loadCheckInRecords(loadOptions: { force?: boolean } = {}) {
    const providerId = checkInRecordsProviderId.value;
    if (!providerId || !checkInRecordsVisible.value) {
      return;
    }

    const key = checkInRecordsCacheKey(providerId, checkInRecordsMonth.value);
    if (!loadOptions.force && checkInRecordsCache.value.has(key)) {
      return;
    }

    checkInRecordsLoading.value = true;
    checkInRecordsError.value = "";
    try {
      const result = await options.loadRecords(providerId, checkInRecordsMonth.value);
      const next = new Map(checkInRecordsCache.value);
      next.set(key, result);
      checkInRecordsCache.value = next;
    } catch (error) {
      checkInRecordsError.value = error instanceof Error ? error.message : String(error);
    } finally {
      checkInRecordsLoading.value = false;
    }
  }

  return {
    checkInRecordsVisible,
    checkInRecordsProviderId,
    checkInRecordsMonth,
    checkInRecordsLoading,
    checkInRecordsError,
    checkInRecordsProvider,
    checkInRecordsResult,
    openCheckInRecords,
    loadCheckInRecords,
  };
}

function checkInRecordsCacheKey(providerId: string, month: string) {
  return `${providerId}:${month}`;
}
