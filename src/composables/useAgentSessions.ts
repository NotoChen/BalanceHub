import { computed, getCurrentInstance, getCurrentScope, onMounted, onScopeDispose, ref, shallowRef, watch, type Ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { agentSessionQueryApi, type AgentSessionQueryApi } from "../api/agent-sessions.ts";
import type { AgentCliKind } from "../stores/provider-types";
import type { AgentSessionCount, AgentSessionDetail, AgentSessionRoleFilter, AgentSessionRow, AgentSessionScope, AgentSessionSourceState } from "../stores/agent-session-types";
import { withTimeout } from "../utils/promise-timeout.ts";
import { compareSessionRows } from "../utils/agent-session-display.ts";

const QUERY_TIMEOUT_MS = 65_000;
const DETAIL_TIMEOUT_MS = 25_000;
const SEARCH_DEBOUNCE_MS = 280;
const SCAN_CONTINUE_DELAY_MS = 150;

interface AgentSessionsOptions {
  active: Ref<boolean>;
  agentKinds: Ref<AgentCliKind[]>;
  query: Ref<string>;
  /** Explicit CLI picker scope is resolved to one workspace by Rust. */
  explicitWorkdir?: Ref<string | null>;
  /** Changes when the recorded-directory set changes, including forgetting a path. */
  scopeKey?: Ref<string>;
  pageSize?: number;
  initialWorkspaceMode?: "home" | "all";
  initialWorkspaceSelection?: string | null;
  initialRoleFilter?: AgentSessionRoleFilter;
  refreshOnIndexUpdate?: boolean;
  autoLoad?: boolean;
  autoContinue?: boolean;
  api?: AgentSessionQueryApi;
}

/** One read lifetime per surface; list and detail consumers never cancel each other. */
export function useAgentSessions(options: AgentSessionsOptions) {
  const api = options.api ?? agentSessionQueryApi;
  // A reloaded frontend must not reuse a consumer whose Rust generation is newer.
  const consumerInstance = crypto.randomUUID();
  const consumerId = `agent-session-list-${consumerInstance}`;
  const detailConsumerId = `agent-session-detail-${consumerInstance}`;
  const scope = shallowRef<AgentSessionScope | null>(null);
  const scopeLoading = ref(false);
  const scopeError = ref("");
  const workspaceSelection = ref<string | null>(options.initialWorkspaceSelection ?? null);
  const allWorkspaces = ref(options.initialWorkspaceMode !== "home");
  const roleFilter = ref<AgentSessionRoleFilter>(options.initialRoleFilter ?? "all");
  const rows = shallowRef<AgentSessionRow[]>([]);
  const loading = ref(false);
  const loadingMore = ref(false);
  const error = ref("");
  const cancelled = ref(false);
  const nextCursor = ref<string | null>(null);
  const scanPending = ref(false);
  const snapshotId = ref<string | null>(null);
  const loadedCount = ref(0);
  const total = ref<number | null>(null);
  const agentCounts = shallowRef<AgentSessionCount[]>([]);
  const sourceStates = shallowRef<AgentSessionSourceState[]>([]);
  const detailVisible = ref(false);
  const detailLoading = ref(false);
  const detailError = ref("");
  const detailResult = shallowRef<AgentSessionDetail | null>(null);
  const detailRow = computed(() => detailResult.value?.row ?? null);
  const detail = computed(() => detailResult.value?.detail ?? null);
  const hasMore = computed(() => nextCursor.value !== null);
  const busy = computed(() => scopeLoading.value || loading.value || loadingMore.value);
  const selectedWorkspaces = computed(() => {
    const choices = scope.value?.workspaces ?? [];
    if (options.explicitWorkdir) return choices;
    if (allWorkspaces.value) return choices;
    return choices.filter((workspace) => workspace.id === workspaceSelection.value);
  });
  const incomplete = computed(() => sourceStates.value.some((source) => source.state !== "complete"));
  const contextKey = computed(() => JSON.stringify([
    options.explicitWorkdir?.value ?? null, options.scopeKey?.value ?? "",
  ]));
  let disposed = false;
  let listRequestId = 0;
  let detailRequestId = 0;
  let scopeRequestId = 0;
  let pendingListId: number | null = null;
  let pendingDetailId: number | null = null;
  let scopePromise: Promise<AgentSessionScope | null> | null = null;
  let searchTimer: ReturnType<typeof globalThis.setTimeout> | null = null;
  let scanTimer: ReturnType<typeof globalThis.setTimeout> | null = null;
  let indexUnlisten: UnlistenFn | null = null;
  // Per-ref history belongs to the current list snapshot; the sequence stays
  // monotonic so an in-flight detail can detect updates even across a reset.
  const parentUpdates = new Map<string, { revision: number; key: string }>();
  let parentUpdateRevision = 0;

  function cancelRequest(id: string, requestId: number | null) {
    if (requestId === null) return;
    // Cancellation is best effort; request guards reject late reads independently.
    void withTimeout(api.cancel({ consumerId: id, requestId }), 5_000, "取消会话读取超时").catch(() => undefined);
  }

  function clearSearchTimer() {
    if (searchTimer !== null) globalThis.clearTimeout(searchTimer);
    searchTimer = null;
  }

  function clearScanTimer() {
    if (scanTimer !== null) globalThis.clearTimeout(scanTimer);
    scanTimer = null;
  }

  function scheduleContinuation() {
    clearScanTimer();
    if (options.autoContinue === false || disposed || !options.active.value || !scanPending.value || !nextCursor.value || error.value || cancelled.value) return;
    const generation = listRequestId;
    scanTimer = globalThis.setTimeout(() => {
      scanTimer = null;
      if (generation === listRequestId) void load(true);
    }, SCAN_CONTINUE_DELAY_MS);
  }

  function clearPage() {
    parentUpdates.clear();
    rows.value = [];
    nextCursor.value = null;
    scanPending.value = false;
    snapshotId.value = null;
    loadedCount.value = 0;
    total.value = null;
    agentCounts.value = [];
    sourceStates.value = [];
  }

  function stopListRequest() {
    clearSearchTimer();
    clearScanTimer();
    listRequestId += 1;
    cancelRequest(consumerId, pendingListId);
    pendingListId = null;
    loading.value = false;
    loadingMore.value = false;
  }

  function invalidateList(clear = false) {
    stopListRequest();
    scanPending.value = false;
    if (clear) clearPage();
  }

  function closeDetail() {
    detailRequestId += 1;
    cancelRequest(detailConsumerId, pendingDetailId);
    pendingDetailId = null;
    detailLoading.value = false;
    detailError.value = "";
    detailResult.value = null;
    detailVisible.value = false;
  }

  function stopScopeRequest() {
    scopeRequestId += 1;
    scopePromise = null;
    scopeLoading.value = false;
  }

  function suspend() {
    // Visibility controls work, not the identity or validity of cached results.
    stopListRequest();
    stopScopeRequest();
    closeDetail();
  }

  function invalidateQuery() {
    invalidateList(true);
    closeDetail();
    error.value = "";
    cancelled.value = false;
  }

  function invalidate() {
    invalidateQuery();
    stopScopeRequest();
    scope.value = null;
    scopeError.value = "";
  }

  async function ensureScope(force = false): Promise<AgentSessionScope | null> {
    if (disposed || !options.active.value) return null;
    if (!force && scope.value) return scope.value;
    if (!force && scopePromise) return scopePromise;
    const requestId = ++scopeRequestId;
    const context = contextKey.value;
    scopeLoading.value = true;
    scopeError.value = "";
    scopePromise = (async () => {
      try {
        const result = await withTimeout(api.scope(options.explicitWorkdir?.value ?? null), 15_000, "读取会话目录范围超时，请刷新重试");
        if (disposed || requestId !== scopeRequestId || context !== contextKey.value) return null;
        const previousRevision = scope.value?.revision;
        if (previousRevision && previousRevision !== result.revision) { clearPage(); closeDetail(); }
        scope.value = result;
        if (!workspaceSelection.value || !result.workspaces.some((workspace) => workspace.id === workspaceSelection.value)) {
          workspaceSelection.value = result.workspaces.find((workspace) => workspace.isHome)?.id ?? result.workspaces[0]?.id ?? null;
        }
        return result;
      } catch (failure) {
        if (!disposed && requestId === scopeRequestId && context === contextKey.value) scopeError.value = errorMessage(failure);
        return null;
      } finally {
        if (requestId === scopeRequestId) { scopeLoading.value = false; scopePromise = null; }
      }
    })();
    return scopePromise;
  }

  async function load(append = false, refreshScope = false) {
    if (disposed || !options.active.value || (append && (!nextCursor.value || busy.value))) return;
    const cursor = append ? nextCursor.value : null;
    const previousSnapshot = snapshotId.value;
    const continuing = append && scanPending.value;
    invalidateList(!append);
    scanPending.value = continuing;
    const requestId = ++listRequestId;
    const context = contextKey.value;
    loading.value = !append;
    loadingMore.value = append;
    error.value = "";
    cancelled.value = false;
    try {
      const currentScope = await ensureScope(refreshScope);
      if (!currentScope || disposed || requestId !== listRequestId || context !== contextKey.value) return;
      const workspaceIds = selectedWorkspaces.value.map((workspace) => workspace.id);
      if (!workspaceIds.length) { error.value = "当前没有可查询的工作目录，请刷新目录范围"; return; }
      pendingListId = requestId;
      const page = await withTimeout(api.query({
        consumerId, requestId, scopeRevision: currentScope.revision,
        agentKinds: [...options.agentKinds.value], workspaceIds,
        roleFilter: roleFilter.value, query: options.query.value.trim(), pageSize: options.pageSize ?? 50, cursor,
      }), QUERY_TIMEOUT_MS, "读取会话超时，已取消本次查询；可以刷新重试");
      if (disposed || requestId !== listRequestId || context !== contextKey.value) return;
      if (page.scopeRevision !== scope.value?.revision || (append && page.snapshotId !== previousSnapshot)) {
        throw new Error("会话范围或分页快照已变更，请刷新后继续查看");
      }
      const merged = append ? new Map(rows.value.map((row) => [row.sessionRef, row])) : new Map<string, AgentSessionRow>();
      for (const row of page.items) merged.set(row.sessionRef, row);
      for (const update of page.parentUpdates) {
        const row = merged.get(update.sessionRef);
        if (!row) continue;
        const key = parentUpdateKey(update.parent);
        const previous = parentUpdates.get(update.sessionRef);
        if (previous?.key === key) continue;
        const changed = parentUpdateKey(row.parent) !== key;
        parentUpdates.set(update.sessionRef, { key, revision: changed ? ++parentUpdateRevision : previous?.revision ?? 0 });
        if (!changed) continue;
        merged.set(update.sessionRef, { ...row, parent: update.parent });
        const currentDetail = detailResult.value;
        if (currentDetail?.row.sessionRef === update.sessionRef) {
          detailResult.value = { ...currentDetail, row: { ...currentDetail.row, parent: update.parent } };
        }
      }
      rows.value = [...merged.values()].sort(compareSessionRows);
      snapshotId.value = page.snapshotId;
      nextCursor.value = page.nextCursor;
      scanPending.value = page.scanPending;
      loadedCount.value = page.loadedCount;
      total.value = page.total;
      agentCounts.value = page.agentCounts;
      sourceStates.value = page.sourceStates;
      pendingListId = null;
    } catch (failure) {
      if (!disposed && requestId === listRequestId && context === contextKey.value) {
        error.value = errorMessage(failure);
        scanPending.value = false;
        cancelRequest(consumerId, pendingListId);
      }
    } finally {
      if (requestId === listRequestId) {
        pendingListId = null; loading.value = false; loadingMore.value = false;
        scheduleContinuation();
      }
    }
  }

  function refresh() { return load(false, true); }
  function loadMore() { return load(true); }

  async function ensureLoaded() {
    if (disposed || !options.active.value || busy.value || cancelled.value || error.value || scopeError.value) return;
    if (snapshotId.value) { scheduleContinuation(); return; }
    await load();
  }

  function cancel() {
    invalidateList();
    stopScopeRequest();
    cancelled.value = true;
  }

  function selectWorkspace(id: string | null) {
    if (id === null ? allWorkspaces.value : !allWorkspaces.value && workspaceSelection.value === id) return;
    allWorkspaces.value = id === null;
    if (id !== null) workspaceSelection.value = id;
    invalidateQuery();
    void load();
  }

  function scheduleSearch() {
    invalidateQuery();
    if (disposed || !options.active.value) return;
    loading.value = true;
    searchTimer = globalThis.setTimeout(() => { searchTimer = null; void load(); }, SEARCH_DEBOUNCE_MS);
  }

  async function openDetail(sessionRef: string) {
    if (disposed || !options.active.value || !scope.value) return;
    closeDetail();
    const requestId = ++detailRequestId;
    const revision = scope.value.revision;
    const context = contextKey.value;
    const parentRevision = parentUpdates.get(sessionRef)?.revision ?? 0;
    detailVisible.value = true;
    detailLoading.value = true;
    pendingDetailId = requestId;
    try {
      const result = await withTimeout(api.detail({ consumerId: detailConsumerId, requestId, scopeRevision: revision, sessionRef }), DETAIL_TIMEOUT_MS, "读取会话详情超时，请重新打开");
      if (disposed || requestId !== detailRequestId || !detailVisible.value || revision !== scope.value?.revision || context !== contextKey.value) return;
      if (result.row.sessionRef !== sessionRef) throw new Error("会话详情返回了不同的原生来源，请刷新重试");
      const currentRow = rows.value.find((row) => row.sessionRef === sessionRef);
      const currentParentRevision = parentUpdates.get(sessionRef)?.revision ?? 0;
      const resolved = currentRow && currentParentRevision > parentRevision
        ? { ...result, row: { ...result.row, parent: currentRow.parent } }
        : result;
      detailResult.value = resolved;
      rows.value = rows.value.map((row) => row.sessionRef === sessionRef ? resolved.row : row).sort(compareSessionRows);
      pendingDetailId = null;
    } catch (failure) {
      if (!disposed && requestId === detailRequestId && detailVisible.value) {
        detailError.value = errorMessage(failure);
        cancelRequest(detailConsumerId, pendingDetailId);
      }
    } finally {
      if (requestId === detailRequestId) { pendingDetailId = null; detailLoading.value = false; }
    }
  }

  watch(contextKey, () => {
    invalidate();
    if (options.active.value && options.autoLoad !== false) void load();
  }, { flush: "sync" });
  watch(options.active, (active) => {
    if (!active) suspend();
    else if (options.autoLoad !== false) void ensureLoaded();
    else scheduleContinuation();
  }, { immediate: options.autoLoad !== false, flush: "sync" });
  watch(() => options.agentKinds.value.join("|"), () => { invalidateQuery(); if (options.active.value && options.autoLoad !== false) void load(); }, { flush: "sync" });
  watch(roleFilter, () => { invalidateQuery(); void load(); }, { flush: "sync" });
  watch(() => options.query.value.trim(), scheduleSearch, { flush: "sync" });
  watch(detailVisible, (visible) => { if (!visible && (pendingDetailId !== null || detailResult.value)) closeDetail(); }, { flush: "sync" });

  if (getCurrentInstance() && options.refreshOnIndexUpdate !== false) onMounted(async () => {
    try {
      const unlisten = await listen<AgentCliKind | null>("cli-session-index-updated", (event) => {
        if (event.payload !== null && options.agentKinds.value.length && !options.agentKinds.value.includes(event.payload)) return;
        invalidateQuery();
        if (options.active.value && options.autoLoad !== false) void load();
      });
      if (disposed) unlisten(); else indexUnlisten = unlisten;
    } catch { /* The Tauri event bus is optional in isolated component tests. */ }
  });
  if (getCurrentScope()) onScopeDispose(() => { disposed = true; invalidate(); indexUnlisten?.(); });

  return { scope, scopeLoading, scopeError, workspaceSelection, allWorkspaces, selectedWorkspaces, roleFilter,
    rows, loading, loadingMore, busy, error, cancelled, nextCursor, scanPending, snapshotId, loadedCount, total, agentCounts, sourceStates, incomplete, hasMore,
    detailVisible, detailLoading, detailError, detailResult, detailRow, detail,
    ensureScope, ensureLoaded, load, refresh, loadMore, cancel, selectWorkspace, openDetail, closeDetail, suspend, invalidate, invalidateList };
}

function errorMessage(error: unknown) { return error instanceof Error ? error.message : String(error); }

function parentUpdateKey(parent: AgentSessionRow["parent"]) {
  return parent.kind === "known" ? JSON.stringify([parent.kind, parent.nativeId, parent.parentRef]) : parent.kind;
}
