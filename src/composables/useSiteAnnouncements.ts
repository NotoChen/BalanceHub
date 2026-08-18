import {
  computed,
  onBeforeUnmount,
  ref,
  watch,
  type Ref,
} from "vue";
import { Message } from "@arco-design/web-vue";
import {
  getSiteAnnouncements,
  markSiteAnnouncementRead as markSiteAnnouncementReadCommand,
} from "../api/app";
import type {
  Provider,
  SiteAnnouncement,
  SiteAnnouncementsSnapshot,
} from "../stores/providers";
import {
  latestSiteAnnouncementTimestamp,
  providerAnnouncementSourceSignature,
} from "../utils/site-announcements";

const READ_STORAGE_KEY = "balancehub.site-announcements.read.v1";
const READ_STORAGE_LIMIT = 1_000;
const REFRESH_INTERVAL_MS = 15 * 60 * 1_000;
const INITIAL_REFRESH_DELAY_MS = 800;
const SOURCE_CHANGE_REFRESH_DELAY_MS = 250;

interface UseSiteAnnouncementsOptions {
  providers: Ref<Provider[]>;
  initialized: Ref<boolean>;
  reloadProviders: () => Promise<unknown>;
}

export function useSiteAnnouncements(options: UseSiteAnnouncementsOptions) {
  const siteAnnouncementsVisible = ref(false);
  const siteAnnouncementsLoading = ref(false);
  const siteAnnouncementsFatalError = ref("");
  const siteAnnouncementsSnapshot = ref<SiteAnnouncementsSnapshot | null>(null);
  const selectedAnnouncementFingerprint = ref("");
  const markingAnnouncementFingerprints = ref<Set<string>>(new Set());
  const readFingerprints = ref<Set<string>>(loadReadFingerprints());
  let lastRefreshAt = 0;
  let refreshPromise: Promise<void> | null = null;
  let refreshTimer: ReturnType<typeof setTimeout> | null = null;
  let sourceSignature = providerAnnouncementSourceSignature(options.providers.value);

  const siteAnnouncements = computed(() =>
    [...(siteAnnouncementsSnapshot.value?.announcements ?? [])].sort(
      (left, right) =>
        latestSiteAnnouncementTimestamp(right) - latestSiteAnnouncementTimestamp(left),
    ),
  );
  const siteAnnouncementErrors = computed(
    () => siteAnnouncementsSnapshot.value?.errors ?? [],
  );
  const siteAnnouncementsLoaded = computed(() => siteAnnouncementsSnapshot.value !== null);
  const selectedSiteAnnouncement = computed(
    () =>
      siteAnnouncements.value.find(
        (item) => item.fingerprint === selectedAnnouncementFingerprint.value,
      ) ?? null,
  );
  const unreadSiteAnnouncementCount = computed(
    () => siteAnnouncements.value.filter((item) => !announcementIsRead(item)).length,
  );

  function announcementIsRead(item: SiteAnnouncement) {
    return Boolean(item.readAt) || readFingerprints.value.has(item.fingerprint);
  }

  function openSiteAnnouncements() {
    siteAnnouncementsVisible.value = true;
    if (isStale()) {
      void refreshSiteAnnouncements();
    }
  }

  async function refreshSiteAnnouncements() {
    if (refreshPromise) return refreshPromise;
    refreshPromise = (async () => {
      const requestedSourceSignature = sourceSignature;
      siteAnnouncementsLoading.value = true;
      siteAnnouncementsFatalError.value = "";
      try {
        const snapshot = await getSiteAnnouncements();
        if (requestedSourceSignature !== sourceSignature) {
          scheduleNextRefresh(SOURCE_CHANGE_REFRESH_DELAY_MS);
          return;
        }
        siteAnnouncementsSnapshot.value = snapshot;
        lastRefreshAt = Date.now();
        if (
          selectedAnnouncementFingerprint.value &&
          !snapshot.announcements.some(
            (item) => item.fingerprint === selectedAnnouncementFingerprint.value,
          )
        ) {
          selectedAnnouncementFingerprint.value = "";
        }
        // Sub2API 的公告读取可能滚动更新令牌。先结束公告加载，让顶部入口及时
        // 可用，再异步同步 Provider 快照，避免一次公告请求把弹窗拖成“卡住”。
        void options.reloadProviders().catch(() => {});
        scheduleNextRefresh(REFRESH_INTERVAL_MS);
      } catch (error) {
        if (requestedSourceSignature !== sourceSignature) {
          scheduleNextRefresh(SOURCE_CHANGE_REFRESH_DELAY_MS);
          return;
        }
        siteAnnouncementsFatalError.value = errorMessage(error);
        lastRefreshAt = Date.now();
        scheduleNextRefresh(REFRESH_INTERVAL_MS);
      } finally {
        siteAnnouncementsLoading.value = false;
      }
    })().finally(() => {
      refreshPromise = null;
    });
    return refreshPromise;
  }

  function selectSiteAnnouncement(item: SiteAnnouncement) {
    selectedAnnouncementFingerprint.value = item.fingerprint;
    if (!announcementIsRead(item)) {
      void markSiteAnnouncementRead(item);
    }
  }

  /**
   * Mark an announcement locally before attempting the optional remote sync.
   *
   * NewAPI exposes a public, read-only announcement endpoint and cannot sync
   * a read state.  The local fingerprint is therefore the source of truth for
   * the badge; a remote failure must never make the badge reappear.
   */
  function markAnnouncementReadLocally(item: SiteAnnouncement) {
    rememberReadFingerprint(item.fingerprint);
    const snapshot = siteAnnouncementsSnapshot.value;
    if (snapshot) {
      siteAnnouncementsSnapshot.value = {
        ...snapshot,
        announcements: snapshot.announcements.map((candidate) =>
          candidate.fingerprint === item.fingerprint
            ? { ...candidate, readAt: candidate.readAt ?? String(Date.now()) }
            : candidate,
        ),
      };
    }
  }

  async function markSiteAnnouncementRead(
    item: SiteAnnouncement,
    markOptions: { notifyFailure?: boolean; reloadProviders?: boolean } = {},
  ): Promise<boolean> {
    markAnnouncementReadLocally(item);
    if (!item.canMarkRead || !item.id.trim()) return true;
    if (markingAnnouncementFingerprints.value.has(item.fingerprint)) return true;

    setMarking(item.fingerprint, true);
    try {
      await markSiteAnnouncementReadCommand(item.providerId, item.id);
      if (markOptions.reloadProviders !== false) {
        void options.reloadProviders().catch(() => {});
      }
    } catch (error) {
      if (markOptions.notifyFailure !== false) {
        Message.warning(`公告已在本地标记为已读，站点同步失败：${errorMessage(error)}`);
      }
      return false;
    } finally {
      setMarking(item.fingerprint, false);
    }
    return true;
  }

  /**
   * Explicitly clear the badge for the whole currently loaded list.  This is
   * intentionally local-first so one unavailable Sub2API endpoint cannot
   * leave the user with a stale unread count.
   */
  async function markAllSiteAnnouncementsRead() {
    const unread = siteAnnouncements.value.filter((item) => !announcementIsRead(item));
    if (unread.length === 0) return;
    unread.forEach(markAnnouncementReadLocally);
    const syncable = unread.filter((item) => item.canMarkRead && item.id.trim());
    const results = await Promise.all(
      syncable.map((item) =>
        markSiteAnnouncementRead(item, {
          notifyFailure: false,
          reloadProviders: false,
        }),
      ),
    );
    const failed = results.filter((result) => !result).length;
    if (results.some(Boolean)) {
      void options.reloadProviders().catch(() => {});
    }
    if (failed > 0) {
      Message.warning(`已在本地标记 ${unread.length} 条公告为已读，${failed} 条站点状态同步失败`);
    }
  }

  function rememberReadFingerprint(fingerprint: string) {
    if (!fingerprint || readFingerprints.value.has(fingerprint)) return;
    const next = new Set(readFingerprints.value);
    next.add(fingerprint);
    const bounded = Array.from(next).slice(-READ_STORAGE_LIMIT);
    readFingerprints.value = new Set(bounded);
    try {
      window.localStorage.setItem(READ_STORAGE_KEY, JSON.stringify(bounded));
    } catch {
      // WebView storage is best-effort; the current process still keeps the read state.
    }
  }

  function setMarking(fingerprint: string, marking: boolean) {
    const next = new Set(markingAnnouncementFingerprints.value);
    if (marking) next.add(fingerprint);
    else next.delete(fingerprint);
    markingAnnouncementFingerprints.value = next;
  }

  function isStale() {
    return (
      Boolean(sourceSignature) &&
      (lastRefreshAt <= 0 || Date.now() - lastRefreshAt >= REFRESH_INTERVAL_MS)
    );
  }

  function clearAnnouncementCache() {
    siteAnnouncementsSnapshot.value = null;
    selectedAnnouncementFingerprint.value = "";
    siteAnnouncementsFatalError.value = "";
    lastRefreshAt = 0;
    if (refreshTimer) {
      clearTimeout(refreshTimer);
      refreshTimer = null;
    }
  }

  function scheduleNextRefresh(delay: number) {
    if (refreshTimer) clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => {
      refreshTimer = null;
      if (options.initialized.value && sourceSignature) {
        void refreshSiteAnnouncements();
      }
    }, delay);
  }

  watch(
    [options.initialized, () => providerAnnouncementSourceSignature(options.providers.value)],
    ([initialized, current], previous) => {
      if (!initialized) return;
      sourceSignature = current;
      if (!current) {
        clearAnnouncementCache();
        return;
      }
      const previousSignature = previous?.[1] ?? "";
      if (previousSignature && current !== previousSignature) {
        clearAnnouncementCache();
        scheduleNextRefresh(SOURCE_CHANGE_REFRESH_DELAY_MS);
        return;
      }
      if (!siteAnnouncementsSnapshot.value && !refreshPromise) {
        scheduleNextRefresh(INITIAL_REFRESH_DELAY_MS);
      }
    },
    { immediate: true },
  );

  onBeforeUnmount(() => {
    if (refreshTimer) clearTimeout(refreshTimer);
  });

  return {
    siteAnnouncementsVisible,
    siteAnnouncementsLoading,
    siteAnnouncementsFatalError,
    siteAnnouncements,
    siteAnnouncementErrors,
    siteAnnouncementsLoaded,
    selectedSiteAnnouncement,
    selectedAnnouncementFingerprint,
    markingAnnouncementFingerprints,
    unreadSiteAnnouncementCount,
    announcementIsRead,
    openSiteAnnouncements,
    refreshSiteAnnouncements,
    selectSiteAnnouncement,
    markAllSiteAnnouncementsRead,
  };
}

function loadReadFingerprints() {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(READ_STORAGE_KEY) ?? "[]");
    if (!Array.isArray(parsed)) return new Set<string>();
    return new Set(
      parsed
        .filter((value): value is string => typeof value === "string" && value.length <= 128)
        .slice(-READ_STORAGE_LIMIT),
    );
  } catch {
    return new Set<string>();
  }
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
