<script setup lang="ts">
import { computed, nextTick, onScopeDispose, ref, watch } from "vue";
import { useVirtualizer } from "@tanstack/vue-virtual";
import {
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  Copy,
  FolderOpen,
  List,
  Pause,
  RefreshCw,
  Search,
  TriangleAlert,
} from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type { CliSessionDetail } from "../../stores/provider-types";
import type { AgentSessionRow } from "../../stores/agent-session-types";
import type {
  AgentSessionSearchHit,
  SessionMessageLoadState,
  SessionReaderNavigation,
  SessionReaderSearchState,
} from "../../stores/session-reader-types";
import { copyText } from "../../composables/useClipboard";
import { formatFileSize } from "../../utils/file-size";
import {
  sessionModelLabel,
  sessionTime,
} from "../../utils/agent-session-display";
import AgentCliIcon from "../AgentCliIcon.vue";
import AgentSessionRelation from "./AgentSessionRelation.vue";
import SessionReaderMessage from "./SessionReaderMessage.vue";
import SessionUsage from "./SessionUsage.vue";
import "../../styles/modules/cli-session-detail.css";

const props = withDefaults(
  defineProps<{
    detail: CliSessionDetail | null;
    sessionRow: AgentSessionRow | null;
    loading: boolean;
    error: string;
    query: string;
    paused: boolean;
    messageStates: Record<string, SessionMessageLoadState>;
    sessionSearch: SessionReaderSearchState;
    showBack?: boolean;
  }>(),
  {
    showBack: false,
    sessionSearch: (): SessionReaderSearchState => ({
      query: "",
      hits: [],
      nextCursor: null,
      scannedWindows: 0,
      totalWindows: 0,
      complete: false,
      loading: false,
      error: "",
    }),
  },
);
const emit = defineEmits<{
  close: [];
  navigate: [navigation: SessionReaderNavigation];
  expand: [id: string, fromStart: boolean];
  pause: [];
  retry: [];
  parent: [sessionRef: string];
  searchAll: [];
  continueSearch: [];
  stopSearch: [];
  clearSearch: [];
}>();
const session = computed(
  () => props.detail?.session ?? props.sessionRow?.session ?? null,
);
const searchDraft = ref(props.query);
const outlineOpen = ref(false);
const showContext = ref(false);
const viewport = ref<HTMLElement | null>(null);
const outlineViewport = ref<HTMLElement | null>(null);
const messages = computed(() =>
  (props.detail?.messages ?? []).filter(
    (message) =>
      !message.metadata.injected || showContext.value || props.query.trim(),
  ),
);
const outline = computed(() => props.detail?.outline ?? []);
const virtual = useVirtualizer(
  computed(() => ({
    count: messages.value.length,
    getScrollElement: () => viewport.value,
    getItemKey: (index: number) => messages.value[index].id,
    estimateSize: (index: number) =>
      messages.value[index].role === "tool" ? 48 : 160,
    overscan: 5,
  })),
);
const outlineVirtual = useVirtualizer(
  computed(() => ({
    count: outline.value.length,
    getScrollElement: () => outlineViewport.value,
    getItemKey: (index: number) => outline.value[index].id,
    estimateSize: () => 72,
    overscan: 4,
  })),
);
const virtualRows = computed(() => virtual.value.getVirtualItems());
const outlineRows = computed(() => outlineVirtual.value.getVirtualItems());
const find = computed(() => props.detail?.find);
const page = computed(() => props.detail?.page);
const windowed = computed(() => Boolean(page.value?.window));
const fragmentProgress = computed(() => {
  const window = page.value?.window;
  if (!window || window.fileBytes <= 0) return 0;
  return Math.min(100, Math.round((window.end / window.fileBytes) * 100));
});
const displayedSearchHits = computed(() =>
  props.sessionSearch.hits.slice(0, 200),
);
function jumpToSearchHit(hit: AgentSessionSearchHit) {
  emit("navigate", {
    cursor: hit.cursor,
    anchor: hit.messageId,
    query: props.sessionSearch.query,
  });
}
const matchLabel = computed(() =>
  searchDraft.value.trim() !== props.query.trim()
    ? "输入中"
    : !props.query.trim()
      ? ""
      : props.loading
        ? "查找中…"
        : find.value?.total
          ? find.value.position == null
            ? `共 ${find.value.total} 处匹配`
            : `${find.value.position + 1} / ${find.value.total}`
          : page.value?.indexing
            ? "继续查找中…"
            : windowed.value
              ? "当前片段没有匹配"
              : "没有匹配",
);
let timer: ReturnType<typeof globalThis.setTimeout> | null = null;
let focusRevision = 0;
function stopSearchTimer() {
  if (timer !== null) globalThis.clearTimeout(timer);
  timer = null;
}
function search() {
  stopSearchTimer();
  // Keep the same full transcript or byte window; a bare revision cursor
  // selects the search focus without jumping back to the newest window.
  emit("navigate", {
    cursor: page.value?.revision,
    query: searchDraft.value.trim(),
    matchPosition: 0,
  });
}
function queueSearch(value: string) {
  searchDraft.value = value;
  stopSearchTimer();
  timer = globalThis.setTimeout(search, 280);
}
function match(direction: number) {
  if (searchDraft.value.trim() !== props.query.trim()) {
    search();
    return;
  }
  const total = find.value?.total ?? 0;
  const position = find.value?.position;
  if (total)
    emit("navigate", {
      cursor: page.value?.revision,
      query: props.query,
      matchPosition:
        position == null
          ? direction > 0
            ? 0
            : total - 1
          : (position + direction + total) % total,
    });
}
function measure(element: unknown) {
  virtual.value.measureElement(element instanceof HTMLElement ? element : null);
}
function measureOutline(element: unknown) {
  outlineVirtual.value.measureElement(
    element instanceof HTMLElement ? element : null,
  );
}
function navigateOutline(anchor: string) {
  if (
    outlineViewport.value &&
    getComputedStyle(outlineViewport.value).position === "absolute"
  )
    outlineOpen.value = false;
  const index = messages.value.findIndex((message) => message.id === anchor);
  if (index >= 0) {
    // Repeated clicks on the same turn must also restore its scroll position.
    virtual.value.scrollToIndex(index, { align: "start" });
  }
  emit("navigate", {
    cursor: page.value?.revision,
    anchor,
    query: props.query,
  });
}
async function copy(value: string, label: string) {
  try {
    await copyText(value);
    Message.success(`已复制${label}`);
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}
watch(
  () => props.query,
  (query) => {
    searchDraft.value = query;
  },
);
watch(
  () => props.sessionRow?.sessionRef,
  () => {
    stopSearchTimer();
    showContext.value = false;
    outlineOpen.value = false;
  },
);
watch(
  () =>
    `${props.sessionRow?.sessionRef}:${page.value?.start}:${find.value?.activeMessageId}:${find.value?.query}`,
  async () => {
    const revision = ++focusRevision;
    await nextTick();
    if (revision !== focusRevision) return;
    const index = messages.value.findIndex(
      (message) => message.id === find.value?.activeMessageId,
    );
    virtual.value.scrollToIndex(Math.max(0, index), {
      align: index < 0 ? "start" : "center",
    });
  },
  { flush: "post" },
);
onScopeDispose(() => {
  stopSearchTimer();
  focusRevision += 1;
});
</script>

<template>
  <section class="session-reader" aria-label="会话阅读器">
    <header class="session-reader-header">
      <button
        v-if="showBack"
        type="button"
        class="session-reader-icon"
        aria-label="返回会话列表"
        title="返回会话列表"
        @click="emit('close')"
      >
        <ArrowLeft :size="17" />
      </button>
      <AgentCliIcon v-if="session" :kind="session.cliKind" :size="25" />
      <div class="session-reader-heading">
        <h2>{{ session?.title || "读取会话" }}</h2>
        <div>
          <AgentSessionRelation
            v-if="sessionRow"
            :row="sessionRow"
            @parent="emit('parent', $event)"
          /><span v-if="session">{{ sessionModelLabel(session) }}</span
          ><time
            v-if="session?.updatedAt"
            :title="sessionTime(session.updatedAt)"
            >{{ sessionTime(session.updatedAt, true) }}</time
          >
        </div>
      </div>
      <button
        v-if="session"
        type="button"
        class="session-reader-icon"
        aria-label="复制会话 ID"
        title="复制会话 ID"
        @click="copy(session.id, '会话 ID')"
      >
        <Copy :size="14" />
      </button>
    </header>
    <SessionUsage v-if="detail?.usage" :usage="detail.usage" />
    <button
      v-if="session?.workdir"
      type="button"
      class="session-reader-workdir"
      :title="session.workdir"
      @click="copy(session.workdir, '工作目录')"
    >
      <FolderOpen :size="13" /><span>{{ session.workdir }}</span>
    </button>
    <div class="session-reader-toolbar">
      <button
        type="button"
        class="session-reader-outline-toggle"
        :aria-expanded="outlineOpen"
        @click="outlineOpen = !outlineOpen"
      >
        <List :size="15" />目录<span v-if="outline.length">{{
          outline.length
        }}</span>
      </button>
      <a-input
        :model-value="searchDraft"
        size="small"
        allow-clear
        :max-length="200"
        :placeholder="windowed ? '在当前片段中搜索' : '在本会话中搜索'"
        :aria-label="windowed ? '搜索当前片段' : '搜索当前会话'"
        @update:model-value="queueSearch"
        @press-enter="!$event.isComposing && match($event.shiftKey ? -1 : 1)"
        ><template #prefix><Search :size="14" /></template
      ></a-input>
      <span
        v-if="matchLabel"
        class="session-reader-match-count"
        role="status"
        >{{ matchLabel }}</span
      >
      <div class="session-reader-match-actions">
        <button
          type="button"
          class="session-reader-icon"
          aria-label="上一处匹配"
          :disabled="!find?.total || loading"
          @click="match(-1)"
        >
          <ChevronUp :size="15" /></button
        ><button
          type="button"
          class="session-reader-icon"
          aria-label="下一处匹配"
          :disabled="!find?.total || loading"
          @click="match(1)"
        >
          <ChevronDown :size="15" />
        </button>
      </div>
      <button
        v-if="windowed"
        type="button"
        class="session-reader-scope-search"
        :disabled="
          !query.trim() ||
          searchDraft.trim() !== query.trim() ||
          sessionSearch.loading
        "
        title="从最新片段开始，向更早的片段搜索整段会话"
        @click="emit('searchAll')"
      >
        <Search :size="13" />整段
      </button>
    </div>
    <section
      v-if="sessionSearch.query"
      class="session-reader-session-search"
      aria-label="整段会话搜索结果"
    >
      <header>
        <div>
          <strong>整段搜索「{{ sessionSearch.query }}」</strong
          ><span
            >{{ sessionSearch.hits.length }} 处匹配 · 已扫描
            {{ sessionSearch.scannedWindows
            }}<template v-if="sessionSearch.totalWindows">
              / 约 {{ sessionSearch.totalWindows }}</template
            >
            个片段</span
          >
        </div>
        <div class="session-reader-session-search-actions">
          <button
            v-if="sessionSearch.loading"
            type="button"
            @click="emit('stopSearch')"
          >
            停止</button
          ><button
            v-else-if="!sessionSearch.complete"
            type="button"
            @click="emit('continueSearch')"
          >
            继续</button
          ><button
            type="button"
            aria-label="关闭整段搜索结果"
            @click="emit('clearSearch')"
          >
            关闭
          </button>
        </div>
      </header>
      <div
        v-if="sessionSearch.error"
        class="session-reader-session-search-empty"
        role="alert"
      >
        {{ sessionSearch.error }}
      </div>
      <div
        v-else-if="!sessionSearch.hits.length && sessionSearch.complete"
        class="session-reader-session-search-empty"
      >
        已搜索到文件开头，没有匹配。
      </div>
      <ol v-else>
        <li v-for="hit in displayedSearchHits" :key="`${hit.cursor}:${hit.messageId}`">
          <button type="button" @click="jumpToSearchHit(hit)">
            <span class="session-reader-session-search-fragment">{{
              hit.window
                ? `片段 ${hit.window.index} / ${hit.window.total}`
                : "整段"
            }}</span
            ><span class="session-reader-session-search-snippet">{{
              hit.snippet
            }}</span>
          </button>
        </li>
      </ol>
      <p
        v-if="sessionSearch.hits.length > displayedSearchHits.length"
        class="session-reader-session-search-note"
      >
        仅显示前 {{ displayedSearchHits.length }} 处匹配。
      </p>
    </section>
    <div v-if="error" class="session-reader-error" role="alert">
      <span>{{ error }}</span
      ><button type="button" @click="emit('retry')">
        <RefreshCw :size="13" />重试
      </button>
    </div>
    <div
      v-if="loading && !detail"
      class="session-reader-skeleton"
      role="status"
      aria-label="正在读取会话"
    >
      <a-skeleton :animation="true"
        ><a-skeleton-line
          :rows="7"
          :widths="['45%', '85%', '70%', '30%', '75%', '85%', '50%']"
      /></a-skeleton>
    </div>
    <div
      v-else-if="detail"
      class="session-reader-body"
      :class="{
        'has-outline': outlineOpen,
        'has-truncation': page?.truncated || page?.window,
      }"
    >
      <div v-if="page?.truncated" class="session-reader-truncated" role="status">
        <TriangleAlert :size="14" aria-hidden="true" />
        <span>会话原文过大，仅显示最近的内容；更早的消息未加载。</span>
      </div>
      <div
        v-else-if="page?.window"
        class="session-reader-truncated session-reader-window"
        role="note"
      >
        <TriangleAlert :size="14" aria-hidden="true" />
        <span
          >会话原文较大，当前为{{
            page.window.hasLater ? "较早" : "最新"
          }}片段，已读取其中
          {{ formatFileSize(page.window.end - page.window.start) }}（共
          {{ formatFileSize(page.window.fileBytes) }}）；{{
            page.window.hasEarlier ? "可继续向更早的部分翻阅" : "已是文件开头"
          }}，输入框搜索仅覆盖当前片段，可用「整段」向前搜索。</span
        >
      </div>
      <nav
        v-if="outlineOpen"
        ref="outlineViewport"
        class="session-reader-outline"
        aria-label="会话轮次目录"
      >
        <strong
          >{{ windowed ? "本片段轮次" : "对话轮次" }}
          <span>{{ outline.length }}</span></strong
        >
        <div
          :style="{
            height: `${outlineVirtual.getTotalSize()}px`,
            position: 'relative',
          }"
        >
          <button
            v-for="item in outlineRows"
            :key="String(item.key)"
            :ref="measureOutline"
            :data-index="item.index"
            type="button"
            :title="outline[item.index].title"
            :style="{
              position: 'absolute',
              width: '100%',
              top: 0,
              transform: `translateY(${item.start}px)`,
            }"
            @click="navigateOutline(outline[item.index].messageId)"
          >
            <small>{{ String(item.index + 1).padStart(2, "0") }}</small
            ><span
              >{{ outline[item.index].title
              }}<em v-if="outline[item.index].steps"
                >{{ outline[item.index].steps }} 次调用<span
                  v-if="outline[item.index].failures"
                >
                  · {{ outline[item.index].failures }} 次失败</span
                ></em
              ></span
            >
          </button>
        </div>
      </nav>
      <div ref="viewport" class="session-reader-viewport" :aria-busy="loading">
        <div v-if="!messages.length" class="session-reader-empty">
          <strong>{{
            page?.indexing ? "正在读取更多消息…" : "没有可展示的消息"
          }}</strong
          ><span v-if="!page?.indexing">该会话的原始记录中没有正文。</span>
        </div>
        <div
          class="session-reader-timeline"
          :style="{ height: `${virtual.getTotalSize()}px` }"
        >
          <div
            v-for="item in virtualRows"
            :key="String(item.key)"
            :ref="measure"
            :data-index="item.index"
            class="session-reader-virtual-row"
            :style="{ transform: `translateY(${item.start}px)` }"
          >
            <SessionReaderMessage
              :message="messages[item.index]"
              :agent-kind="detail.session.cliKind"
              :query="query"
              :matched="messages[item.index].id === find?.activeMessageId"
              :load-state="messageStates[messages[item.index].id]"
              @expand="(id, fromStart) => emit('expand', id, fromStart)"
            />
          </div>
        </div>
      </div>
    </div>
    <div v-if="page" class="session-reader-pagebar">
      <label><input v-model="showContext" type="checkbox" />上下文</label>
      <span v-if="page.totalMessages">
        <template v-if="page.window">本片段 </template
        >{{ page.start + 1 }}–{{ page.start + detail!.messages.length }} /
        {{ page.totalMessages }} 条</span
      >
      <span v-else>0 条消息</span>
      <span
        v-if="page.window"
        class="session-reader-fragment"
        :title="`片段按 32 MiB 估算，边界会对齐到完整记录；当前已读到文件的 ${fragmentProgress}%`"
      >
        <span>片段 {{ page.window.index }} / {{ page.window.total }}</span>
        <span
          class="session-reader-fragment-meter"
          role="progressbar"
          aria-label="文件片段位置"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-valuenow="fragmentProgress"
          ><span :style="{ width: `${fragmentProgress}%` }"></span
        ></span>
      </span>
      <div v-if="page.indexing" class="session-reader-indexing" role="status">
        <span>{{ paused ? "读取已暂停" : "正在读取其余内容" }}</span
        ><button
          type="button"
          :aria-label="paused ? '继续读取会话' : '暂停读取会话'"
          @click="paused ? emit('retry') : emit('pause')"
        >
          <RefreshCw v-if="paused" :size="13" /><Pause v-else :size="13" />
        </button>
      </div>
      <div class="session-reader-page-actions">
        <button
          v-if="page.window?.hasEarlier"
          type="button"
          :disabled="loading"
          @click="
            emit('navigate', { cursor: `${page.revision}:earliest`, query })
          "
        >
          最早</button
        ><button
          v-if="page.window?.hasLater"
          type="button"
          :disabled="loading"
          @click="
            emit('navigate', { cursor: `${page.revision}:latest`, query })
          "
        >
          回到最新</button
        ><button
          type="button"
          :disabled="!page.previousCursor || loading"
          @click="emit('navigate', { cursor: page.previousCursor, query })"
        >
          上一页</button
        ><button
          type="button"
          :disabled="!page.nextCursor || loading"
          @click="emit('navigate', { cursor: page.nextCursor, query })"
        >
          下一页
        </button>
      </div>
    </div>
    <footer v-if="$slots.footer && detail" class="session-reader-footer">
      <slot name="footer" />
    </footer>
  </section>
</template>
