import { computed, ref, shallowRef, type Ref } from "vue";
import type { AgentSessionQueryApi } from "../api/agent-sessions.ts";
import type {
  AgentSessionDetail,
  AgentSessionRow,
  AgentSessionScope,
} from "../stores/agent-session-types";
import type {
  AgentSessionMessageContent,
  SessionMessageLoadState,
  SessionReaderNavigation,
} from "../stores/session-reader-types";
import { withTimeout } from "../utils/promise-timeout.ts";

const READ_TIMEOUT = 25_000;
interface ReaderOptions {
  api: AgentSessionQueryApi;
  instance: string;
  active: Ref<boolean>;
  scope: Ref<AgentSessionScope | null>;
  rows: Ref<AgentSessionRow[]>;
  query: Ref<string>;
  contextKey: Ref<string>;
  parentRevision: (sessionRef: string) => number;
}

/** Independent, cancellable lifetime shared by the history page and CLI picker. */
export function useSessionReader(options: ReaderOptions) {
  const detailVisible = ref(false);
  const detailLoading = ref(false);
  const detailError = ref("");
  const detailPaused = ref(false);
  const detailQuery = ref("");
  const detailResult = shallowRef<AgentSessionDetail | null>(null);
  const selectedRow = shallowRef<AgentSessionRow | null>(null);
  const detailRow = computed(
    () => detailResult.value?.row ?? selectedRow.value,
  );
  const detail = computed(() => detailResult.value?.detail ?? null);
  const messageStates = shallowRef<Record<string, SessionMessageLoadState>>({});
  const consumerId = `agent-session-detail-${options.instance}`;
  let sequence = 0;
  let generation = 0;
  let pending: number | null = null;
  let sessionRef = "";
  let navigation: SessionReaderNavigation = {};
  let timer: ReturnType<typeof globalThis.setTimeout> | null = null;
  const messageRequests = new Map<
    string,
    { consumerId: string; requestId: number }
  >();
  const expandedMessages = new Set<string>();

  function cancel(consumerId: string, requestId: number) {
    void withTimeout(
      options.api.cancel({ consumerId, requestId }),
      5_000,
      "取消读取超时",
    ).catch(() => undefined);
  }
  function stop() {
    generation += 1;
    if (timer !== null) globalThis.clearTimeout(timer);
    timer = null;
    if (pending !== null) cancel(consumerId, pending);
    pending = null;
    for (const request of messageRequests.values())
      cancel(request.consumerId, request.requestId);
    messageRequests.clear();
    messageStates.value = {};
    detailLoading.value = false;
  }
  function closeDetail() {
    stop();
    expandedMessages.clear();
    sessionRef = "";
    detailVisible.value = false;
    detailResult.value = null;
    selectedRow.value = null;
    detailError.value = "";
    detailQuery.value = "";
    detailPaused.value = false;
  }
  function pauseDetail() {
    stop();
    detailPaused.value = true;
  }

  async function read(background = false) {
    const scopeRevision = options.scope.value?.revision;
    if (
      !scopeRevision ||
      !sessionRef ||
      !options.active.value ||
      !detailVisible.value
    )
      return;
    if (timer !== null) globalThis.clearTimeout(timer);
    timer = null;
    if (pending !== null) cancel(consumerId, pending);
    const requestId = ++sequence;
    const currentGeneration = generation;
    const target = sessionRef;
    const context = options.contextKey.value;
    const parentRevision = options.parentRevision(target);
    const current = () =>
      currentGeneration === generation &&
      requestId === pending &&
      target === sessionRef &&
      scopeRevision === options.scope.value?.revision &&
      context === options.contextKey.value &&
      detailVisible.value;
    pending = requestId;
    detailLoading.value = !background;
    detailError.value = "";
    try {
      const result = await withTimeout(
        options.api.detail({
          consumerId,
          requestId,
          scopeRevision,
          sessionRef: target,
          cursor: navigation.cursor ?? null,
          anchor: navigation.anchor ?? null,
          query: detailQuery.value,
          matchPosition: navigation.matchPosition ?? null,
        }),
        READ_TIMEOUT,
        "读取会话超时，可以重试或关闭阅读器",
      );
      if (!current()) return;
      if (result.row.sessionRef !== target)
        throw new Error("会话详情返回了不同的原生来源，请刷新重试");
      const row = options.rows.value.find((row) => row.sessionRef === target);
      if (row && options.parentRevision(target) > parentRevision)
        result.row = { ...result.row, parent: row.parent };
      if (
        detailResult.value?.detail.page.revision === result.detail.page.revision
      ) {
        const previous = new Map(
          detailResult.value.detail.messages.map((message) => [
            message.id,
            message,
          ]),
        );
        result.detail.messages = result.detail.messages.map((message) => {
          const cached = previous.get(message.id);
          if (
            !cached ||
            cached.contentVersion !== message.contentVersion ||
            cached.contentOffset !== 0 ||
            // A search can deliberately return a later fragment as its anchor.
            message.contentOffset !== 0
          ) {
            expandedMessages.delete(message.id);
            return message;
          }
          return expandedMessages.has(message.id) &&
            cached.content.length > message.content.length
            ? {
                ...message,
                content: cached.content,
                contentOffset: 0,
                hasMoreContent: cached.hasMoreContent,
                changes: cached.changes,
              }
            : message;
        });
        const visibleIds = new Set(
          result.detail.messages.map((message) => message.id),
        );
        for (const id of expandedMessages) {
          if (!visibleIds.has(id)) expandedMessages.delete(id);
        }
      } else {
        expandedMessages.clear();
      }
      for (const [id, request] of messageRequests) {
        const previous = detailResult.value?.detail.messages.find(
          (message) => message.id === id,
        );
        const next = result.detail.messages.find(
          (message) => message.id === id,
        );
        if (
          result.detail.page.revision !==
            detailResult.value?.detail.page.revision ||
          previous?.contentVersion !== next?.contentVersion
        ) {
          cancel(request.consumerId, request.requestId);
          messageRequests.delete(id);
          const states = { ...messageStates.value };
          delete states[id];
          messageStates.value = states;
        }
      }
      detailResult.value = result;
      options.rows.value = options.rows.value.map((row) =>
        row.sessionRef === target
          ? { ...result.row, searchHit: row.searchHit }
          : row,
      );
      if (result.detail.page.indexing && !detailPaused.value) {
        timer = globalThis.setTimeout(() => {
          timer = null;
          if (currentGeneration === generation) void read(true);
        }, 250);
      }
    } catch (error) {
      if (current()) {
        detailError.value =
          error instanceof Error ? error.message : String(error);
        cancel(consumerId, requestId);
      }
    } finally {
      if (requestId === pending) {
        pending = null;
        detailLoading.value = false;
      }
    }
  }

  async function openDetail(target: string) {
    if (!options.active.value || !options.scope.value) return;
    closeDetail();
    sessionRef = target;
    selectedRow.value =
      options.rows.value.find((row) => row.sessionRef === target) ?? null;
    detailQuery.value = options.query.value.trim();
    navigation = { anchor: selectedRow.value?.searchHit?.messageId ?? null };
    detailVisible.value = true;
    await read();
  }
  async function navigateDetail(next: SessionReaderNavigation) {
    stop();
    detailPaused.value = false;
    navigation = next;
    if (next.query !== undefined) detailQuery.value = next.query;
    await read();
  }
  function refreshDetail() {
    return navigateDetail({
      anchor:
        detail.value?.find.activeMessageId ?? detail.value?.messages[0]?.id,
      query: detailQuery.value,
    });
  }

  async function expandMessage(messageId: string, fromStart = false) {
    const result = detailResult.value;
    const message = result?.detail.messages.find(
      (message) => message.id === messageId,
    );
    const scopeRevision = options.scope.value?.revision;
    if (!result || !message || !scopeRevision || messageRequests.has(messageId))
      return;
    const target = sessionRef;
    const currentGeneration = generation;
    const revision = result.detail.page.revision;
    let offset = fromStart
      ? 0
      : message.contentOffset + Array.from(message.content).length;
    if (offset >= message.contentLength && !fromStart) return;
    const automatic =
      message.metadata.kind === "image" ||
      message.metadata.toolKind === "edit" ||
      message.metadata.toolKind === "write";
    const parts = fromStart ? [] : [message.content];
    let lastContent: AgentSessionMessageContent | null = null;
    let request = {
      consumerId: `session-message-${options.instance}-${messageId.slice(0, 64)}`,
      requestId: ++sequence,
    };
    messageRequests.set(messageId, request);
    messageStates.value = {
      ...messageStates.value,
      [messageId]: { loading: true, error: "" },
    };
    const current = () =>
      currentGeneration === generation &&
      sessionRef === target &&
      detailVisible.value &&
      options.active.value &&
      detailResult.value?.detail.page.revision === revision &&
      detailResult.value?.detail.messages.some(
        (entry) =>
          entry.id === messageId &&
          entry.contentVersion === message.contentVersion,
      ) &&
      options.scope.value?.revision === scopeRevision;
    const commitContent = () => {
      if (!lastContent || !current()) return;
      const content = lastContent;
      const latest = detailResult.value!;
      detailResult.value = {
        ...latest,
        detail: {
          ...latest.detail,
          messages: latest.detail.messages.map((entry) =>
            entry.id !== messageId
              ? entry
              : {
                  ...entry,
                  content: parts.join(""),
                  contentOffset: fromStart ? 0 : entry.contentOffset,
                  contentLength: content.totalChars,
                  hasMoreContent: content.nextOffset !== null,
                  changes: content.changes.length
                    ? content.changes
                    : entry.changes,
                },
          ),
        },
      };
      expandedMessages.add(messageId);
    };
    try {
      while (current()) {
        const content = await withTimeout(
          options.api.message({
            ...request,
            scopeRevision,
            sessionRef: target,
            revision,
            messageId,
            contentVersion: message.contentVersion,
            offset,
          }),
          READ_TIMEOUT,
          "展开消息超时，可以重试",
        );
        if (!current()) return;
        const end = offset + Array.from(content.content).length;
        if (
          content.messageId !== messageId ||
          content.offset !== offset ||
          content.totalChars !== message.contentLength ||
          end > content.totalChars ||
          (content.nextOffset !== null &&
            (content.nextOffset !== end || end <= offset)) ||
          (content.nextOffset === null && end !== content.totalChars)
        ) {
          throw new Error("消息位置已变化，请重新读取");
        }
        parts.push(content.content);
        lastContent = content;
        if (!automatic || content.nextOffset === null) break;
        offset = content.nextOffset;
        request = { ...request, requestId: ++sequence };
        messageRequests.set(messageId, request);
      }
      commitContent();
    } catch (error) {
      if (current()) {
        commitContent();
        cancel(request.consumerId, request.requestId);
        messageStates.value = {
          ...messageStates.value,
          [messageId]: {
            loading: false,
            error: error instanceof Error ? error.message : String(error),
          },
        };
      }
    } finally {
      if (messageRequests.get(messageId)?.requestId === request.requestId) {
        messageRequests.delete(messageId);
        messageStates.value = {
          ...messageStates.value,
          [messageId]: {
            loading: false,
            error: messageStates.value[messageId]?.error ?? "",
          },
        };
      }
    }
  }
  return {
    detailVisible,
    detailLoading,
    detailError,
    detailPaused,
    detailQuery,
    detailResult,
    detailRow,
    detail,
    messageStates,
    openDetail,
    closeDetail,
    navigateDetail,
    refreshDetail,
    pauseDetail,
    expandMessage,
  };
}
