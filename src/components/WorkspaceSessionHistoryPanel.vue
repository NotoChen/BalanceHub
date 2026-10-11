<script setup lang="ts">
import { computed } from "vue";
import { IconRefresh, IconSearch } from "@arco-design/web-vue/es/icon";
import type { CliSessionIndexState } from "../stores/providers";
import type {
  AgentSessionRoleFilter,
  AgentSessionRow,
} from "../stores/agent-session-types";
import { useCliRuntimeStore } from "../stores/cli-runtime";
import AgentSessionList from "./agent-workspace/AgentSessionList.vue";

const props = defineProps<{
  query: string;
  results: AgentSessionRow[];
  loading: boolean;
  loadingMore: boolean;
  error: string;
  indexState: CliSessionIndexState;
  indexMessage: string;
  roleFilter: AgentSessionRoleFilter;
  selectedResumeId: string;
  selectedSessionRef: string;
  selectedSessionTitle: string;
  workdir: string;
  disabled: boolean;
  total: number | null;
  hasMore: boolean;
}>();
const emit = defineEmits<{
  "update:query": [query: string];
  "update:roleFilter": [role: AgentSessionRoleFilter];
  refresh: [workdir: string];
  "load-more": [];
  "view-session": [row: AgentSessionRow];
  "view-parent": [sessionRef: string];
}>();
const store = useCliRuntimeStore();
const queryModel = computed({
  get: () => props.query,
  set: (value: string) => emit("update:query", value),
});
const roleModel = computed({
  get: () => props.roleFilter,
  set: (value: AgentSessionRoleFilter) => emit("update:roleFilter", value),
});
const selectedSession = computed(
  () =>
    props.results.find((row) => row.sessionRef === props.selectedSessionRef)
      ?.session ?? null,
);
</script>

<template>
  <div class="workspace-session-history" role="region" aria-label="历史会话">
    <div class="workspace-session-history-toolbar">
      <div class="agent-session-filters workspace-session-filters">
        <a-select
          v-model="roleModel"
          aria-label="历史会话角色"
          :disabled="disabled"
        >
          <a-option value="all">全部会话</a-option>
          <a-option value="main">主会话</a-option>
          <a-option value="subagent">子 Agent 会话</a-option>
        </a-select>
        <a-input
          v-model="queryModel"
          size="small"
          allow-clear
          :disabled="disabled || !workdir"
          placeholder="搜索标题、会话 ID 或正文"
          aria-label="搜索历史会话"
        >
          <template #prefix><icon-search /></template>
        </a-input>
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
    <div
      v-if="indexMessage"
      class="workspace-session-index-state"
      :class="`is-${indexState}`"
      role="status"
    >
      <span>{{ indexMessage }}</span>
    </div>
    <a-alert v-if="error" type="warning" show-icon
      ><template #title>历史会话读取失败</template>{{ error }}</a-alert
    >
    <p class="agent-session-result-summary" role="status">
      <span
        >已显示 {{ results.length }} 条<template v-if="total !== null">
          / 共 {{ total }} 条</template
        ><template v-if="loading"> · 正在读取…</template></span
      >
    </p>
    <AgentSessionList
      v-if="results.length"
      :rows="results"
      :agents="store.cliRuntime.agents"
      :selected-session-ref="selectedSessionRef"
      :disabled="disabled"
      @detail="emit('view-session', $event)"
      @parent="emit('view-parent', $event)"
    />
    <div v-else class="workspace-session-empty" role="status">
      <strong>{{
        loading
          ? "正在读取历史会话…"
          : error
            ? "暂时无法读取历史会话"
            : query.trim()
              ? "没有匹配的会话"
              : "当前目录暂无历史会话"
      }}</strong>
      <span v-if="!loading">可切换目录、会话角色或刷新后查看。</span>
    </div>
    <div v-if="hasMore" class="agent-session-pagination">
      <a-button
        size="small"
        :loading="loadingMore"
        :disabled="disabled || (loading && !loadingMore)"
        @click="emit('load-more')"
        >继续加载</a-button
      >
    </div>
    <a-alert
      v-if="selectedSession || selectedSessionTitle"
      class="workspace-session-selected-note"
      type="success"
      show-icon
      >已选择：{{
        selectedSession?.title || selectedSessionTitle
      }}。不选择模型时将沿用历史会话模型。</a-alert
    >
  </div>
</template>
