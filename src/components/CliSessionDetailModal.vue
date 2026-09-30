<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { Message } from "@arco-design/web-vue";
import {
  IconClockCircle,
  IconCommand,
  IconCopy,
  IconDown,
  IconFile,
  IconFolder,
  IconUp,
  IconSearch,
  IconUser,
} from "@arco-design/web-vue/es/icon";
import type { CliSessionDetail, CliSessionMessage } from "../stores/providers";
import type { AgentSessionRow } from "../stores/agent-session-types";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import { copyText } from "../composables/useClipboard";
import { agentCliLabel } from "../utils/cli-environment";
import AgentCliIcon from "./AgentCliIcon.vue";
import CliSessionMessageContent from "./CliSessionMessageContent.vue";
import AgentSessionRelation from "./agent-workspace/AgentSessionRelation.vue";
import { sessionModelLabel as formatSessionModel, sessionTime } from "../utils/agent-session-display";

const props = defineProps<{
  visible: boolean;
  loading: boolean;
  error: string;
  detail: CliSessionDetail | null;
  selectedResumeId: string;
  sessionRow?: AgentSessionRow | null;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  select: [];
  parent: [sessionRef: string];
}>();

type TimelineItem =
  | { type: "message"; key: string; message: CliSessionMessage; index: number }
  | {
      type: "activity";
      key: string;
      messages: Array<{ message: CliSessionMessage; index: number }>;
      startIndex: number;
      endIndex: number;
    };

const INITIAL_MESSAGE_COUNT = 140;
const MESSAGE_PAGE_SIZE = 120;
const store = useCliRuntimeStore();
const messageList = ref<HTMLElement | null>(null);
const sessionSearchQuery = ref("");
const currentMatchPosition = ref(-1);
const visibleMessageCount = ref(INITIAL_MESSAGE_COUNT);
const expandedActivityKeys = ref<Set<string>>(new Set());
let searchNavigationRevision = 0;

const session = computed(() => props.detail?.session ?? null);
const cliLabel = computed(() =>
  session.value ? agentCliLabel(store.cliEnvironmentProbe, session.value.cliKind) : "Agent CLI",
);
const selected = computed(() => session.value?.id === props.selectedResumeId);
const visibleMessages = computed(() =>
  (props.detail?.messages ?? []).slice(0, visibleMessageCount.value),
);
const hiddenMessageCount = computed(() =>
  Math.max(0, (props.detail?.messages.length ?? 0) - visibleMessages.value.length),
);
const timeline = computed<TimelineItem[]>(() => {
  const items: TimelineItem[] = [];
  let activity: Extract<TimelineItem, { type: "activity" }> | null = null;
  for (const [index, message] of visibleMessages.value.entries()) {
    if (message.role === "tool") {
      if (!activity) {
        activity = {
          type: "activity",
          key: `activity-${index}`,
          messages: [],
          startIndex: index,
          endIndex: index,
        };
        items.push(activity);
      }
      activity.messages.push({ message, index });
      activity.endIndex = index;
      continue;
    }
    activity = null;
    items.push({ type: "message", key: message.id || `message-${index}`, message, index });
  }
  return items;
});
const searchMatches = computed(() => {
  const query = sessionSearchQuery.value.trim().toLocaleLowerCase();
  if (!query) return [];
  return (props.detail?.messages ?? [])
    .map((message, index) => ({ message, index }))
    .filter(({ message }) => message.content.toLocaleLowerCase().includes(query));
});
const currentMatchedMessageIndex = computed(() =>
  currentMatchPosition.value >= 0
    ? searchMatches.value[currentMatchPosition.value]?.index ?? -1
    : -1,
);
const conversationStats = computed(() => {
  const messages = props.detail?.messages ?? [];
  const turns = messages.filter((message) => message.role === "user").length;
  const activities = messages.filter((message) => message.role === "tool").length;
  return activities > 0 ? `${turns} 轮对话 · ${activities} 项执行活动` : `${turns} 轮对话`;
});

watch(
  () => [props.visible, props.sessionRow?.sessionRef || props.detail?.session.id] as const,
  () => {
    searchNavigationRevision += 1;
    sessionSearchQuery.value = "";
    currentMatchPosition.value = -1;
    visibleMessageCount.value = INITIAL_MESSAGE_COUNT;
    expandedActivityKeys.value = new Set();
  },
);

watch(
  searchMatches,
  async (matches) => {
    currentMatchPosition.value = matches.length > 0 ? 0 : -1;
    if (matches.length > 0) {
      await scrollToSearchMatch(0, "auto");
    }
  },
  { flush: "post" },
);

function messageTime(message: CliSessionMessage) {
  return message.timestamp ? sessionTime(message.timestamp) : "";
}

const sessionModelLabel = computed(() => {
  const value = session.value;
  return value ? formatSessionModel(value) : "未记录模型";
});

function messageRoleLabel(message: CliSessionMessage) {
  if (message.role === "user") return "你";
  if (message.role === "assistant") return cliLabel.value;
  return message.toolName || "工具活动";
}

function activityLabel(item: Extract<TimelineItem, { type: "activity" }>) {
  const names = Array.from(
    new Set(item.messages.map(({ message }) => message.toolName?.trim()).filter(Boolean)),
  );
  if (names.length === 0) return "执行活动";
  const visibleNames = names.slice(0, 3).join("、");
  return names.length > 3 ? `${visibleNames} 等` : visibleNames;
}

function activityContainsCurrentMatch(item: Extract<TimelineItem, { type: "activity" }>) {
  return (
    currentMatchedMessageIndex.value >= item.startIndex
    && currentMatchedMessageIndex.value <= item.endIndex
  );
}

function activityIsOpen(item: Extract<TimelineItem, { type: "activity" }>) {
  return expandedActivityKeys.value.has(item.key);
}

function handleActivityToggle(
  item: Extract<TimelineItem, { type: "activity" }>,
  event: Event,
) {
  const target = event.currentTarget as HTMLDetailsElement;
  const next = new Set(expandedActivityKeys.value);
  if (target.open) {
    next.add(item.key);
  } else {
    next.delete(item.key);
  }
  expandedActivityKeys.value = next;
}

function loadMoreMessages() {
  visibleMessageCount.value = Math.min(
    props.detail?.messages.length ?? 0,
    visibleMessageCount.value + MESSAGE_PAGE_SIZE,
  );
}

async function navigateSearchMatch(direction: number) {
  const matches = searchMatches.value;
  if (matches.length === 0) return;
  const next =
    currentMatchPosition.value < 0
      ? 0
      : (currentMatchPosition.value + direction + matches.length) % matches.length;
  currentMatchPosition.value = next;
  await scrollToSearchMatch(next, "smooth");
}

async function scrollToSearchMatch(position: number, behavior: ScrollBehavior) {
  const revision = ++searchNavigationRevision;
  const targetIndex = searchMatches.value[position]?.index;
  if (targetIndex === undefined) return;
  visibleMessageCount.value = Math.max(visibleMessageCount.value, targetIndex + 1);
  const activity = timeline.value.find((item) => item.type === "activity"
    && targetIndex >= item.startIndex && targetIndex <= item.endIndex);
  if (activity) {
    expandedActivityKeys.value = new Set(expandedActivityKeys.value).add(activity.key);
  }
  await nextTick();
  if (revision !== searchNavigationRevision || !props.visible || targetIndex !== currentMatchedMessageIndex.value) return;
  const target = messageList.value?.querySelector<HTMLElement>(
    `[data-message-index="${targetIndex}"]`,
  );
  (target?.querySelector<HTMLElement>("mark") || target)?.scrollIntoView({ behavior, block: "center", inline: "nearest" });
}

async function copySessionId() {
  const id = session.value?.id.trim();
  if (id) await copyValue(id, "已复制会话 ID");
}

async function copyMessage(message: CliSessionMessage) {
  await copyValue(message.content, "已复制消息内容");
}

async function copyValue(value: string, successMessage: string) {
  try {
    await copyText(value);
    Message.success(successMessage);
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(1040px, calc(100vw - 32px))"
    modal-class="surface-modal cli-session-detail-modal"
    title-align="start"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title cli-session-detail-title">
        <AgentCliIcon v-if="session" :kind="session.cliKind" :size="26" :decorative="false" :label="cliLabel" />
        <span v-else class="surface-modal-title-icon"><icon-file /></span>
        <span class="surface-modal-title-copy">
          <strong>{{ session?.title || "会话详情" }}</strong>
        </span>
      </div>
    </template>

    <div class="cli-session-detail-shell">
      <div v-if="loading" class="cli-session-detail-loading" aria-live="polite">
        <a-skeleton :animation="true">
          <a-space direction="vertical" size="large" fill>
            <a-skeleton-line :rows="2" :widths="['48%', '76%']" />
            <a-skeleton-line :rows="3" :widths="['82%', '72%', '38%']" />
            <a-skeleton-line :rows="4" :widths="['70%', '88%', '64%', '42%']" />
          </a-space>
        </a-skeleton>
      </div>

      <a-alert v-else-if="error" class="cli-session-detail-error" type="error" show-icon>
        <template #title>无法读取会话详情</template>
        <template #default>{{ error }}</template>
      </a-alert>

      <template v-else-if="detail && session">
        <header class="cli-session-detail-meta">
          <div class="cli-session-detail-meta-main">
            <span class="cli-session-detail-agent">
              {{ cliLabel }}
            </span>
            <AgentSessionRelation v-if="sessionRow" :row="sessionRow" @parent="emit('parent', $event)" />
            <span>{{ sessionModelLabel }}</span>
            <span>{{ conversationStats }}</span>
            <span><icon-clock-circle /> {{ sessionTime(session.updatedAt) }}</span>
            <span v-if="sessionRow" class="agent-session-activity" :class="{ 'is-active': sessionRow.activityState === 'active' }">{{ sessionRow.activityState === 'active' ? '活动中' : '待确认活动' }}</span>
            <span v-if="session.archived" class="agent-session-archived">已归档</span>
          </div>
          <div class="cli-session-detail-locations">
            <button v-if="session.workdir" type="button" class="cli-session-detail-path" :title="`复制完整目录：${session.workdir}`" aria-label="复制会话目录" @click="copyValue(session.workdir, '已复制会话目录')"><icon-folder /><span>{{ session.workdir }}</span><icon-copy /></button>
            <button
              type="button"
              class="cli-session-detail-id"
              :title="`复制会话 ID：${session.id}`"
              aria-label="复制会话 ID"
              @click="copySessionId"
            >
              <span>会话 ID</span><code>{{ session.id }}</code>
              <icon-copy />
            </button>
          </div>
        </header>

        <div class="cli-session-detail-searchbar">
          <a-input
            v-model="sessionSearchQuery"
            size="small"
            allow-clear
            placeholder="搜索对话与执行活动"
            aria-label="搜索当前会话中的对话与执行活动"
            title="Enter 查找下一条匹配消息，Shift+Enter 查找上一条"
            @press-enter="!$event.isComposing && navigateSearchMatch($event.shiftKey ? -1 : 1)"
          >
            <template #prefix><icon-search /></template>
          </a-input>
          <span class="cli-session-detail-search-count" role="status" aria-live="polite">
            {{ searchMatches.length > 0 ? `${currentMatchPosition + 1} / ${searchMatches.length} 条匹配` : sessionSearchQuery.trim() ? "未找到匹配" : `${detail.messages.length} 条消息` }}
          </span>
          <a-button-group>
            <a-button
              size="mini"
              :disabled="searchMatches.length === 0"
              aria-label="上一条匹配消息"
              title="上一条匹配消息（Shift+Enter）"
              @click="navigateSearchMatch(-1)"
            >
              <template #icon><icon-up /></template>
            </a-button>
            <a-button
              size="mini"
              :disabled="searchMatches.length === 0"
              aria-label="下一条匹配消息"
              title="下一条匹配消息（Enter）"
              @click="navigateSearchMatch(1)"
            >
              <template #icon><icon-down /></template>
            </a-button>
          </a-button-group>
        </div>

        <a-alert v-if="detail.truncated" class="cli-session-detail-truncated" type="warning">
          本次读取的会话内容不完整<span v-if="detail.omittedMessageCount > 0">，另有 {{ detail.omittedMessageCount }} 条消息未载入</span>；搜索仅覆盖已读取的内容。
        </a-alert>

        <div v-if="detail.messages.length === 0" class="cli-session-detail-empty">
          <strong>没有可展示的对话消息</strong>
          <span>会话摘要存在，但该 Agent 没有留下可还原的正文。</span>
        </div>

        <div v-else ref="messageList" class="cli-session-message-list">
          <div class="cli-session-conversation">
            <template v-for="item in timeline" :key="item.key">
              <details
                v-if="item.type === 'activity'"
                class="cli-session-activity-group"
                :class="{ 'is-match': activityContainsCurrentMatch(item) }"
                :open="activityIsOpen(item)"
                @toggle="handleActivityToggle(item, $event)"
              >
                <summary>
                  <span class="cli-session-activity-icon"><icon-command /></span>
                  <span class="cli-session-activity-copy">
                    <strong>执行活动</strong>
                    <span>{{ activityLabel(item) }}</span>
                  </span>
                  <small>{{ item.messages.length }} 项</small>
                  <icon-down class="cli-session-activity-chevron" />
                </summary>
                <div v-if="activityIsOpen(item)" class="cli-session-activity-list">
                  <article
                    v-for="entry in item.messages"
                    :key="entry.message.id || entry.index"
                    class="cli-session-activity-item"
                    :class="{ 'is-match': entry.index === currentMatchedMessageIndex }"
                    :data-message-index="entry.index"
                  >
                    <header>
                      <strong>{{ messageRoleLabel(entry.message) }}</strong>
                      <time v-if="messageTime(entry.message)">{{ messageTime(entry.message) }}</time>
                      <button
                        type="button"
                        title="复制活动内容"
                        aria-label="复制活动内容"
                        @click="copyMessage(entry.message)"
                      >
                        <icon-copy />
                      </button>
                    </header>
                    <CliSessionMessageContent
                      :content="entry.message.content"
                      :query="sessionSearchQuery"
                    />
                  </article>
                </div>
              </details>

              <article
                v-else
                class="cli-session-message"
                :class="[
                  `cli-session-message-${item.message.role}`,
                  { 'is-match': item.index === currentMatchedMessageIndex },
                ]"
                :data-message-index="item.index"
              >
                <header class="cli-session-message-header">
                  <span class="cli-session-message-avatar" aria-hidden="true">
                    <icon-user v-if="item.message.role === 'user'" />
                    <AgentCliIcon
                      v-else-if="item.message.role === 'assistant'"
                      :kind="session.cliKind"
                      :size="17"
                    />
                    <icon-file v-else />
                  </span>
                  <strong>{{ messageRoleLabel(item.message) }}</strong>
                  <span v-if="item.message.model" :title="item.message.model">{{ item.message.model }}</span>
                  <div class="cli-session-message-actions">
                    <time v-if="messageTime(item.message)">{{ messageTime(item.message) }}</time>
                    <button
                      type="button"
                      title="复制消息"
                      aria-label="复制消息"
                      @click="copyMessage(item.message)"
                    >
                      <icon-copy />
                    </button>
                  </div>
                </header>
                <div class="cli-session-message-content">
                  <CliSessionMessageContent
                    :content="item.message.content"
                    :query="sessionSearchQuery"
                  />
                </div>
              </article>
            </template>

            <button
              v-if="hiddenMessageCount > 0"
              type="button"
              class="cli-session-load-more"
              @click="loadMoreMessages"
            >
              继续载入后续消息 · 还剩 {{ hiddenMessageCount }} 条
            </button>
          </div>
        </div>

        <footer class="cli-session-detail-footer">
          <slot name="footer">
            <span v-if="selected">该会话已选中</span>
            <span v-else>{{ sessionRow?.resumeReason || '选择后将使用该 Resume ID 启动 CLI。' }}</span>
            <a-button type="primary" :disabled="selected || !session.canResume" @click="emit('select')">{{ selected ? "已选中" : "选中并返回" }}</a-button>
          </slot>
        </footer>
      </template>
    </div>
  </a-modal>
</template>
