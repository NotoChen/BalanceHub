import { computed, ref, watch, type Ref } from "vue";
import type { Provider, ProviderCheckInRecordsResult } from "../stores/providers";
import { pruneLruEntries, setLruEntry, touchLruEntry } from "../utils/lru-map.ts";
import { useLatestRequest } from "./useLatestRequest.ts";

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
  const request = useLatestRequest({ timeoutMessage: "读取签到记录超时，请重试" });
  const cacheRevision = ref(0);
  const checkInRecordsCache = new Map<string, ProviderCheckInRecordsResult>();

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
    checkInRecordsVisible.value = false;
    checkInRecordsProviderId.value = provider.identity.id;
    checkInRecordsMonth.value = currentMonthValue();
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
      request.invalidate();
      cacheRevision.value += 1;
      return;
    }

    await request.run(() => options.loadRecords(providerId, month), (result) => {
      if (options.providers.value.some((provider) => provider.identity.id === providerId)) {
        setLruEntry(checkInRecordsCache, key, result, CHECK_IN_RECORDS_CACHE_CAPACITY);
        cacheRevision.value += 1;
      }
    });
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
        checkInRecordsVisible.value = false;
        checkInRecordsProviderId.value = null;
      }
    },
    { deep: false },
  );

  /// 签到成功会改变当月记录：对应站点的缓存必须立即失效，打开的日历读新值，
  /// 不能让用户再看到签到前的缓存。
  function invalidateCheckInRecords(providerIds: readonly string[]) {
    if (providerIds.length === 0) return;
    const ids = new Set(providerIds);
    let changed = false;
    for (const key of [...checkInRecordsCache.keys()]) {
      const providerId = providerIdFromCheckInRecordsCacheKey(key);
      if (providerId && ids.has(providerId)) {
        checkInRecordsCache.delete(key);
        changed = true;
      }
    }
    if (!changed) return;
    cacheRevision.value += 1;
    if (
      checkInRecordsVisible.value &&
      checkInRecordsProviderId.value &&
      ids.has(checkInRecordsProviderId.value)
    ) {
      void loadCheckInRecords({ force: true });
    }
  }

  watch([checkInRecordsVisible, checkInRecordsProviderId], request.invalidate, { flush: "sync" });
  watch(checkInRecordsMonth, () => {
    request.invalidate();
    if (checkInRecordsVisible.value) void loadCheckInRecords();
  }, { flush: "sync" });

  return {
    checkInRecordsVisible,
    checkInRecordsProviderId,
    checkInRecordsMonth,
    checkInRecordsLoading: request.loading,
    checkInRecordsError: request.error,
    checkInRecordsProvider,
    checkInRecordsResult,
    openCheckInRecords,
    loadCheckInRecords,
    invalidateCheckInRecords,
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
