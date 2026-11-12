<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { FolderGit2, RefreshCw } from "@lucide/vue";
import type {
  AgentProject,
  AgentProjectProgress,
  AgentProjectReport,
  AgentProjectReportRequest,
} from "../../stores/agent-project-types";
import { getAgentProjects } from "../../api/agent-projects";
import { cancelAgentSessionQuery } from "../../api/agent-sessions";
import { withTimeout } from "../../utils/promise-timeout";
import { fullAgentCliLabel } from "../../utils/cli-environment";
import { sessionTime } from "../../utils/agent-session-display";
import {
  usageCount as count,
  usageCostLabel,
  usageTokenAmount,
} from "../../utils/session-usage-display";
import {
  usagePeriodOptions,
  type UsagePeriod,
} from "../../utils/usage-trend";
import AgentCliIcon from "../AgentCliIcon.vue";
import "../../styles/modules/agent-project.css";

const props = defineProps<{ visible: boolean }>();
const emit = defineEmits<{ close: []; filter: [workspaceIds: string[]] }>();

const period = ref<UsagePeriod>("7d");
const loading = ref(false);
const error = ref("");
const report = ref<AgentProjectReport | null>(null);
const progress = ref<AgentProjectProgress | null>(null);
const consumerId = `agent-projects-${Math.random().toString(36).slice(2)}`;
let request = 0;
let disposed = false;

const periodLabel = computed(
  () =>
    usagePeriodOptions.find((option) => option.value === period.value)?.label ??
    "周期",
);
const reportPeriodLabel = computed(() => {
  const current = report.value;
  if (!current) return "周期";
  return (
    usagePeriodOptions.find((option) => option.value === current.period)
      ?.label ?? "周期"
  );
});
const title = computed(() => `${periodLabel.value}项目`);
const summary = computed(() => {
  const current = report.value;
  if (!current) return "";
  const parts = [
    `${count(current.projects.length)} 个项目`,
    `${count(current.sessions)} 个会话`,
  ];
  if (current.decodedSessions) parts.push(`计入 ${count(current.decodedSessions)} 个`);
  if (current.partial) parts.push("统计未完整");
  return parts.join(" · ");
});
const generatedAt = computed(() => {
  const value = report.value?.generatedAt;
  if (!value) return "";
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? ""
    : date.toLocaleString("zh-CN", { hour12: false });
});
const progressText = computed(
  () => progress.value?.message || "正在汇总项目与周期用量…",
);
const progressPercent = computed(() => {
  const current = progress.value;
  if (!current || current.total <= 0) return 0;
  return Math.min(100, Math.round((current.completed / current.total) * 100));
});

async function load() {
  const id = ++request;
  loading.value = true;
  error.value = "";
  progress.value = null;
  try {
    const payload: AgentProjectReportRequest = {
      consumerId,
      requestId: id,
      period: period.value,
      includeUsage: true,
    };
    const result = await withTimeout(
      getAgentProjects(payload, (update) => {
        if (disposed || id !== request) return;
        const current = progress.value;
        if (
          current &&
          current.phase === update.phase &&
          update.completed < current.completed
        ) {
          return;
        }
        progress.value = update;
      }),
      90_000,
      "项目统计超时",
    );
    if (disposed || id !== request) return;
    report.value = result;
  } catch (cause) {
    if (disposed || id !== request) return;
    report.value = null;
    progress.value = null;
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    if (!disposed && id === request) loading.value = false;
  }
}

function release() {
  const id = request;
  request += 1;
  loading.value = false;
  progress.value = null;
  if (id > 0) {
    void cancelAgentSessionQuery({ consumerId, requestId: id }).catch(() => {});
  }
}

function close() {
  release();
  report.value = null;
  error.value = "";
  emit("close");
}

function openProject(project: AgentProject) {
  if (!project.workspaceIds.length) return;
  emit("filter", project.workspaceIds);
  close();
}

function lastActivity(project: AgentProject) {
  return project.lastActivity ? sessionTime(project.lastActivity) : "无时间记录";
}

watch(
  () => props.visible,
  (visible) => {
    if (!visible) {
      release();
      return;
    }
    void load();
  },
  { immediate: true },
);
watch(period, () => {
  if (props.visible) void load();
});
onBeforeUnmount(() => {
  disposed = true;
  release();
});
</script>

<template>
  <a-modal
    :visible="visible"
    width="min(980px, calc(100vw - 32px))"
    modal-class="surface-modal agent-project-modal"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @cancel="close"
  >
    <template #title>
      <div class="agent-project-heading">
        <strong>项目视图</strong>
        <span class="agent-project-title">{{ title }}</span>
        <span v-if="summary" class="agent-project-summary">{{ summary }}</span>
      </div>
    </template>
    <div class="agent-project-body">
      <p class="agent-project-note">
        按原生会话的工作目录聚合跨 Agent 会话。会话数与最近活动来自原生元数据；
        周期消耗读取会话正文估算，实际费用以中转站账单为准。
      </p>
      <div class="agent-project-controls">
        <a-radio-group v-model="period" type="button" :options="usagePeriodOptions" />
        <a-button :loading="loading" aria-label="重新统计项目" @click="load">
          <template v-if="!loading" #icon><RefreshCw :size="14" /></template>
          重新统计
        </a-button>
      </div>
      <p v-if="error" class="agent-project-error" role="alert">{{ error }}</p>
      <div v-if="loading && !report" class="agent-project-loading" role="status">
        <p class="agent-project-state">
          <a-spin :size="14" /> {{ progressText }}
          <span
            v-if="progress && progress.total > 0"
            class="agent-project-progress-value"
            >{{ progressPercent }}%</span
          >
        </p>
        <a-progress
          v-if="progress && progress.total > 0"
          :percent="progressPercent"
          :show-text="false"
        />
      </div>
      <template v-else-if="report">
        <div class="agent-project-cards">
          <div class="agent-project-card">
            <span>项目</span>
            <strong>{{ count(report.projects.length) }}</strong>
            <small>{{ count(report.sessions) }} 个会话</small>
          </div>
          <div class="agent-project-card">
            <span>本周期计入</span>
            <strong>{{ count(report.decodedSessions) }}</strong>
            <small v-if="report.skippedSessions">跳过 {{ count(report.skippedSessions) }} 个</small>
            <small v-else-if="report.decodedSessions">解码成功</small>
            <small v-else>未读取到用量</small>
          </div>
          <div class="agent-project-card">
            <span>缺少时间</span>
            <strong>{{ count(report.undatedSessions) }}</strong>
            <small>无法判断活动时间</small>
          </div>
          <div class="agent-project-card">
            <span>统计窗口</span>
            <strong>{{ reportPeriodLabel }}</strong>
            <small>按会话最近更新时间筛选</small>
          </div>
        </div>
        <p v-if="report.partial" class="agent-project-warning">
          统计未完整：部分来源只读取到部分会话，或会话正文仍在变化。
        </p>
        <p v-if="report.truncated" class="agent-project-warning">
          本周期用量达到单次读取上限，较早的会话未纳入消耗统计。
        </p>
        <p v-if="report.pricingError" class="agent-project-warning">
          {{ report.pricingError }}
        </p>
        <div v-if="report.projects.length" class="agent-project-list">
          <article
            v-for="project in report.projects"
            :key="project.path"
            class="agent-project-row"
          >
            <div class="agent-project-main">
              <strong :title="project.path">{{ project.path }}</strong>
              <div class="agent-project-meta">
                <span><FolderGit2 :size="12" /> {{ count(project.sessions) }} 个会话</span>
                <span v-if="project.undatedSessions">{{ count(project.undatedSessions) }} 个缺少时间</span>
                <span>{{ lastActivity(project) }}</span>
              </div>
              <div class="agent-project-agents">
                <span v-for="agent in project.agents" :key="agent.cliKind">
                  <AgentCliIcon :kind="agent.cliKind" :size="13" />
                  {{ fullAgentCliLabel(agent.cliKind) }} · {{ count(agent.sessions) }}
                </span>
              </div>
              <div class="agent-project-accounts">
                <span v-if="!project.accounts.length">未关联 BalanceHub 启动记录</span>
                <span v-for="account in project.accounts" :key="`${account.providerId}:${account.accountLabel}:${account.cliKind}`">
                  {{ account.providerName || account.providerId }}<template v-if="account.accountLabel"> · {{ account.accountLabel }}</template>
                </span>
              </div>
            </div>
            <div class="agent-project-usage">
              <strong>{{
                project.usageSessions
                  ? usageTokenAmount(project.usage.tokens)
                  : "—"
              }}</strong>
              <small v-if="project.usageSessions">
                {{ count(project.usageSessions) }} 个会话 · {{ usageCostLabel(project.usage) }}
              </small>
              <small v-else>本周期无用量</small>
            </div>
            <div class="agent-project-actions">
              <a-button
                size="small"
                :disabled="!project.workspaceIds.length"
                @click="openProject(project)"
                >查看会话</a-button
              >
            </div>
          </article>
        </div>
        <p v-else class="agent-project-state">没有扫描到可用的原生会话。</p>
        <section v-if="report.diagnostics.length" class="agent-project-section">
          <header><h4>读取说明</h4></header>
          <ul class="agent-project-diagnostics">
            <li v-for="item in report.diagnostics" :key="item">{{ item }}</li>
          </ul>
        </section>
      </template>
      <p v-else-if="!loading" class="agent-project-state">尚未读取项目视图。</p>
      <footer class="agent-project-actions-footer">
        <span v-if="generatedAt">统计时间 {{ generatedAt }}</span>
        <a-button @click="close">关闭</a-button>
      </footer>
    </div>
  </a-modal>
</template>
