import { computed, ref, watch, type Ref } from "vue";
import type { Provider, ProviderCheckInRecordsResult } from "../stores/providers";
import { setLruEntry, touchLruEntry } from "../utils/lru-map.ts";
import { useLatestRequest } from "./useLatestRequest.ts";

const CHECK_IN_RECORDS_CACHE_CAPACITY = 48;

interface CheckInRecordsCacheEntry {
  /// 站点地址、协议和当前登录账号的指纹；只有这些变化才会影响签到记录。
  scope: string;
  result: ProviderCheckInRecordsResult;
}

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
  const checkInRecordsCache = new Map<string, CheckInRecordsCacheEntry>();

  const checkInRecordsProvider = computed(() =>
    options.providers.value.find(
      (provider) => provider.identity.id === checkInRecordsProviderId.value,
    ) ?? null,
  );

  const checkInRecordsResult = computed(() => {
    cacheRevision.value;
    const provider = checkInRecordsProvider.value;
    if (!provider) {
      return null;
    }
    const entry = checkInRecordsCache.get(
      checkInRecordsCacheKey(provider.identity.id, checkInRecordsMonth.value),
    );
    // 缓存跟着站点和登录账号走：额度刷新、无关卡片启停只推进全局 revision，
    // 不应该让已经读到的记录失效重读。
    return entry && entry.scope === checkInRecordsScope(provider) ? entry.result : null;
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
    const provider = options.providers.value.find(
      (candidate) => candidate.identity.id === providerId,
    );
    if (!provider) return;

    const scope = checkInRecordsScope(provider);
    const key = checkInRecordsCacheKey(providerId, month);
    if (!loadOptions.force) {
      const cached = touchLruEntry(checkInRecordsCache, key);
      if (cached && cached.scope === scope) {
        request.invalidate();
        cacheRevision.value += 1;
        return;
      }
      // 站点或登录账号已经变了：旧记录属于另一个账号，直接丢弃。
      if (cached) checkInRecordsCache.delete(key);
    }

    await request.run(() => options.loadRecords(providerId, month), (result) => {
      const current = options.providers.value.find(
        (candidate) => candidate.identity.id === providerId,
      );
      // 请求期间站点被删除或换了账号：这份记录不属于当前配置，不能写进缓存。
      if (!current || checkInRecordsScope(current) !== scope) return;
      setLruEntry(
        checkInRecordsCache,
        key,
        { scope, result },
        CHECK_IN_RECORDS_CACHE_CAPACITY,
      );
      cacheRevision.value += 1;
    });
  }

  watch(
    options.providers,
    (providers) => {
      const scopes = new Map(
        providers.map((provider) => [provider.identity.id, checkInRecordsScope(provider)] as const),
      );
      const previousSize = checkInRecordsCache.size;
      for (const [key, entry] of checkInRecordsCache) {
        const providerId = providerIdFromCheckInRecordsCacheKey(key);
        // 站点已删除，或地址、协议、登录账号变了：旧记录不再适用。
        if (providerId === null || scopes.get(providerId) !== entry.scope) {
          checkInRecordsCache.delete(key);
        }
      }
      if (checkInRecordsCache.size !== previousSize) {
        cacheRevision.value += 1;
      }
      if (
        checkInRecordsProviderId.value &&
        !scopes.has(checkInRecordsProviderId.value)
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

/// 签到记录跟着站点和登录账号走。这里只取会改变记录归属的字段：额度、
/// 运行状态和全局 revision 的更新不该让日历缓存失效。
function checkInRecordsScope(provider: Provider) {
  const auth = provider.auth;
  return JSON.stringify([
    provider.identity.baseUrl ?? "",
    provider.identity.protocol ?? "",
    auth?.mode ?? "",
    auth?.loginUsername ?? "",
    auth?.apiUser ?? "",
    auth?.apiKeyTokenId ?? "",
    auth?.browserBinding?.accountId ?? "",
  ]);
}

function providerIdFromCheckInRecordsCacheKey(key: string) {
  try {
    const value: unknown = JSON.parse(key);
    return Array.isArray(value) && typeof value[0] === "string" ? value[0] : null;
  } catch {
    return null;
  }
}
