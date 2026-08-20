<script setup lang="ts">
import { computed } from "vue";
import { Message } from "@arco-design/web-vue";
import {
  IconCopy,
  IconRefresh,
  IconSearch,
} from "@arco-design/web-vue/es/icon";
import type {
  CliSessionIndexState,
  CliSessionSearchResult,
  CliSessionSummary,
} from "../stores/providers";
import { copyText } from "../composables/useClipboard";

const props = defineProps<{
  query: string;
  results: CliSessionSearchResult[];
  loading: boolean;
  error: string;
  indexState: CliSessionIndexState;
  indexMessage: string;
  selectedResumeId: string;
  selectedSessionTitle: string;
  workdir: string;
  disabled: boolean;
}>();

const emit = defineEmits<{
  "update:query": [query: string];
  refresh: [workdir: string];
  "view-session": [session: CliSessionSummary];
}>();

const queryModel = computed({
  get: () => props.query,
  set: (value: string) => emit("update:query", value),
});
const selectedSession = computed(
  () => props.results.find((result) => result.session.id === props.selectedResumeId)?.session ?? null,
);

function sessionModelLabel(session: CliSessionSummary) {
  if (session.models.length > 1) {
    return `多模型（最近：${session.model || session.models[session.models.length - 1]}）`;
  }
  return session.model || "未记录模型";
}

function sessionTime(value: string | null) {
  if (!value) return "时间未知";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

async function copySessionId(id: string) {
  const value = id.trim();
  if (!value) return;
  try {
    await copyText(value);
    Message.success("已复制 Resume ID");
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}

</script>

<template>
  <div class="workspace-session-history">
    <div class="workspace-session-history-toolbar">
      <div>
        <strong>历史会话</strong>
        <span>搜索后先查看详情，再决定是否恢复</span>
      </div>
      <a-tooltip content="刷新历史会话">
        <a-button
          shape="circle"
          size="mini"
          :loading="loading"
          :disabled="disabled || !workdir"
          aria-label="刷新历史会话"
          @click="emit('refresh', workdir)"
        >
          <template #icon><icon-refresh /></template>
        </a-button>
      </a-tooltip>
    </div>

    <a-input
      v-model="queryModel"
      class="workspace-session-search"
      size="small"
      allow-clear
      :disabled="disabled || !workdir"
      placeholder="搜索标题、Resume ID、模型、目录或对话内容"
      aria-label="搜索历史会话"
    >
      <template #prefix><icon-search /></template>
    </a-input>

    <div
      v-if="indexMessage"
      class="workspace-session-index-state"
      :class="`is-${indexState}`"
      role="status"
    >
      <span v-if="indexState === 'building'" class="workspace-session-index-pulse" />
      <span>{{ indexMessage }}</span>
    </div>

    <a-alert v-if="error" type="warning" show-icon>
      <template #title>历史索引暂不可用</template>
      <template #default>{{ error }}。请检查 CLI 状态目录后重试。</template>
    </a-alert>

    <a-spin :loading="loading" class="workspace-session-history-spin">
      <div v-if="!loading && results.length === 0" class="workspace-session-empty">
        <strong>{{ query.trim() ? "没有找到匹配的历史会话" : "当前工作空间没有可展示的历史会话" }}</strong>
        <span>{{ query.trim() ? "可以尝试更短的关键字，或清空搜索查看最近会话。" : "请先在该工作空间创建一条有效会话。" }}</span>
      </div>

      <div v-else class="workspace-session-list">
        <div
          v-for="result in results"
          :key="result.session.id"
          class="workspace-session-item"
          :class="{
            selected: result.session.id === selectedResumeId,
            disabled: !result.session.canResume,
          }"
        >
          <button
            type="button"
            class="workspace-session-select"
            :disabled="disabled"
            :aria-pressed="result.session.id === selectedResumeId"
            :title="`查看会话详情：${result.session.title}`"
            @click="emit('view-session', result.session)"
          >
            <span class="workspace-session-item-main">
              <strong>{{ result.session.title }}</strong>
              <span class="workspace-session-meta">
                <span>模型：{{ sessionModelLabel(result.session) }}</span>
                <span>更新时间：{{ sessionTime(result.session.updatedAt) }}</span>
              </span>
            </span>
            <span class="workspace-session-item-side">
              <span class="workspace-session-id" :title="`Resume ID：${result.session.id}`">
                {{ result.session.id }}
              </span>
              <span v-if="result.session.archived" class="workspace-session-archived">已归档</span>
            </span>
          </button>
          <a-tooltip content="复制 Resume ID">
            <a-button
              class="workspace-session-copy"
              shape="circle"
              size="mini"
              :disabled="disabled || !result.session.id"
              aria-label="复制 Resume ID"
              @click.stop="copySessionId(result.session.id)"
            >
              <template #icon><icon-copy /></template>
            </a-button>
          </a-tooltip>
        </div>
      </div>
    </a-spin>

    <a-alert
      v-if="selectedSession || selectedSessionTitle"
      class="workspace-session-selected-note"
      type="success"
      show-icon
    >
      已选择：{{ selectedSession?.title || selectedSessionTitle }}。不选择模型时将沿用历史会话模型。
    </a-alert>
  </div>
</template>
