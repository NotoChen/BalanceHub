import { computed, ref, watch, type Ref } from "vue";
import type { Provider, ProviderCheckInRecordsResult } from "../stores/providers";
import { pruneLruEntries, setLruEntry, touchLruEntry } from "../utils/lru-map";

const CHECK_IN_RECORDS_CACHE_CAPACITY = 48;

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
  const cacheRevision = ref(0);
  const checkInRecordsCache = new Map<string, ProviderCheckInRecordsResult>();
  let requestSequence = 0;

  const checkInRecordsProvider = computed(() =>
    options.providers.value.find(
      (provider) => provider.identity.id === checkInRecordsProviderId.value,
    ) ?? null,
  );

  const checkInRecordsResult = computed(() => {
    cacheRevision.value;
    if (!checkInRecordsProviderId.value) {
      return null;
    }
    return (
      checkInRecordsCache.get(
        checkInRecordsCacheKey(checkInRecordsProviderId.value, checkInRecordsMonth.value),
      ) ?? null
    );
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
    const month = checkInRecordsMonth.value;
    if (!providerId || !checkInRecordsVisible.value) {
      return;
    }

    const key = checkInRecordsCacheKey(providerId, month);
    if (!loadOptions.force && touchLruEntry(checkInRecordsCache, key) !== undefined) {
      cacheRevision.value += 1;
      return;
    }

    const requestId = ++requestSequence;
    checkInRecordsLoading.value = true;
    checkInRecordsError.value = "";
    try {
      const result = await options.loadRecords(providerId, month);
      if (options.providers.value.some((provider) => provider.identity.id === providerId)) {
        setLruEntry(checkInRecordsCache, key, result, CHECK_IN_RECORDS_CACHE_CAPACITY);
        cacheRevision.value += 1;
      }
    } catch (error) {
      if (requestId === requestSequence) {
        checkInRecordsError.value = error instanceof Error ? error.message : String(error);
      }
    } finally {
      if (requestId === requestSequence) {
        checkInRecordsLoading.value = false;
      }
    }
  }

  watch(
    options.providers,
    (providers) => {
      const providerIds = new Set(providers.map((provider) => provider.identity.id));
      const previousSize = checkInRecordsCache.size;
      pruneLruEntries(checkInRecordsCache, (key) => {
        const providerId = providerIdFromCheckInRecordsCacheKey(key);
        return providerId !== null && providerIds.has(providerId);
      });
      if (checkInRecordsCache.size !== previousSize) {
        cacheRevision.value += 1;
      }
      if (
        checkInRecordsProviderId.value &&
        !providerIds.has(checkInRecordsProviderId.value)
      ) {
        requestSequence += 1;
        checkInRecordsVisible.value = false;
        checkInRecordsProviderId.value = null;
        checkInRecordsLoading.value = false;
        checkInRecordsError.value = "";
      }
    },
    { deep: false },
  );

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
  return JSON.stringify([providerId, month]);
}

function providerIdFromCheckInRecordsCacheKey(key: string) {
  try {
    const value: unknown = JSON.parse(key);
    return Array.isArray(value) && typeof value[0] === "string" ? value[0] : null;
  } catch {
    return null;
  }
}
