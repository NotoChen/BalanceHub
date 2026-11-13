<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { RefreshCw } from "@lucide/vue";
import type {
  AgentUsageReport,
  AgentUsageReportRequest,
} from "../../stores/agent-usage-types";
import { getAgentUsageReport } from "../../api/agent-usage";
import { cancelAgentSessionQuery } from "../../api/agent-sessions";
import { withTimeout } from "../../utils/promise-timeout";
import { fullAgentCliLabel } from "../../utils/cli-environment";
import {
  usagePeriodOptions,
  usagePeriodTitle,
  type UsagePeriod,
} from "../../utils/usage-trend";
import {
  usageCount as count,
  usageCostLabel as costLabel,
  usageCostValue as costValue,
  usagePriceNote as priceNote,
  usageTokenAmount as tokenAmount,
  usageTokenLabel as tokenLabel,
  usageTokenTotal as tokenTotal,
} from "../../utils/session-usage-display";
import "../../styles/modules/usage-reconciliation.css";

const props = defineProps<{ visible: boolean }>();
const emit = defineEmits<{ close: [] }>();

const period = ref<UsagePeriod>("7d");
const loading = ref(false);
const error = ref("");
const report = ref<AgentUsageReport | null>(null);
const consumerId = `usage-report-${Math.random().toString(36).slice(2)}`;
let request = 0;
let disposed = false;

const title = computed(() => `${usagePeriodTitle(period.value)}对账`);
const summary = computed(() => {
  const current = report.value;
  if (!current) return "";
  const parts = [
    `计入 ${count(current.countedSessions)} / 扫描 ${count(current.scannedSessions)} 个会话`,
  ];
  if (current.skippedSessions) parts.push(`跳过 ${count(current.skippedSessions)} 个`);
  if (current.partial) parts.push("统计未完整");
  return parts.join(" · ");
});
const totals = computed(() => report.value?.totals ?? null);
const models = computed(() =>
  [...(report.value?.totals.models ?? [])].sort(
    (left, right) => tokenTotal(right.tokens) - tokenTotal(left.tokens),
  ),
);
const generatedAt = computed(() => {
  const value = report.value?.generatedAt;
  if (!value) return "";
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? ""
    : date.toLocaleString("zh-CN", { hour12: false });
});

function accountKey(account: AgentUsageReport["accounts"][number]) {
  return `${account.providerId ?? "unattributed"}:${account.accountLabel}:${account.cliKind}`;
}

function siteLabel(site: AgentUsageReport["sites"][number]) {
  return site.providerName || site.providerId;
}

function windowLabel(value: AgentUsageReport["period"]) {
  if (value === "24h") return "今日";
  return value === "7d" ? "最近 7 天" : "最近 30 天";
}

async function load() {
  const id = ++request;
  loading.value = true;
  error.value = "";
  try {
    const payload: AgentUsageReportRequest = {
      consumerId,
      requestId: id,
      period: period.value,
      includeSiteUsage: true,
    };
    const result = await withTimeout(
      getAgentUsageReport(payload),
      80_000,
      "用量对账超时",
    );
    if (disposed || id !== request) return;
    report.value = result;
  } catch (cause) {
    if (disposed || id !== request) return;
    report.value = null;
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    if (!disposed && id === request) loading.value = false;
  }
}

function release() {
  const id = request;
  request += 1;
  loading.value = false;
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
    width="min(960px, calc(100vw - 32px))"
    modal-class="surface-modal usage-reconciliation-modal"
    :footer="false"
    closable
    mask-closable
    esc-to-close
    unmount-on-close
    @cancel="close"
  >
    <template #title>
      <div class="usage-reconciliation-heading">
        <strong>用量对账</strong>
        <span class="usage-reconciliation-title">{{ title }}</span>
        <span v-if="summary" class="usage-reconciliation-summary">{{ summary }}</span>
      </div>
    </template>
    <div class="usage-reconciliation-body">
      <p class="usage-reconciliation-note">
        本地统计读取本机原生会话日志；账号归属来自 BalanceHub
        的启动记录。参考费用优先使用站点价目，缺失时回退到 models.dev
        公开参考价；实际费用以中转站账单为准。
      </p>
      <div class="usage-reconciliation-controls">
        <a-radio-group v-model="period" type="button" :options="usagePeriodOptions" />
        <a-button :loading="loading" aria-label="重新读取用量对账" @click="load">
          <template v-if="!loading" #icon><RefreshCw :size="14" /></template>
          重新统计
        </a-button>
      </div>
      <p v-if="error" class="usage-reconciliation-error" role="alert">{{ error }}</p>
      <p
        v-if="loading && !report"
        class="usage-reconciliation-state"
        role="status"
      >
        <a-spin :size="14" /> 正在解码本机会话并读取站点账单…
      </p>
      <template v-else-if="report && totals">
        <div class="usage-reconciliation-cards">
          <div class="usage-reconciliation-card">
            <span>计入会话</span>
            <strong>{{ count(report.countedSessions) }}</strong>
            <small>
              扫描 {{ count(report.scannedSessions) }}
              <template v-if="report.skippedSessions">
                · 跳过 {{ count(report.skippedSessions) }}
              </template>
              <template v-if="report.undatedSessions">
                · {{ count(report.undatedSessions) }} 个缺少时间
              </template>
            </small>
          </div>
          <div class="usage-reconciliation-card">
            <span>Token 总量</span>
            <strong>{{ tokenAmount(totals.tokens) }}</strong>
            <small>
              输入 {{ count(totals.tokens.inputTokens) }} · 输出
              {{ count(totals.tokens.outputTokens) }}
            </small>
          </div>
          <div class="usage-reconciliation-card">
            <span>参考费用</span>
            <strong>{{ costLabel(totals) }}</strong>
            <small>{{
              totals.unpricedRecords
                ? `${count(totals.unpricedRecords)} 条用量缺少价格资料`
                : totals.records
                  ? "全部模型均有价格资料"
                  : "窗口内没有用量记录"
            }}</small>
          </div>
          <div class="usage-reconciliation-card">
            <span>统计窗口</span>
            <strong>{{ windowLabel(report.period) }}</strong>
            <small>按会话最近更新时间筛选</small>
          </div>
        </div>
        <p v-if="report.partial" class="usage-reconciliation-warning">
          统计未完整：部分会话只读取到片段、正文仍在变化，或原生计数不完整。
        </p>
        <p v-if="report.truncated" class="usage-reconciliation-warning">
          达到单次读取上限，较早的会话未纳入本次统计。
        </p>
        <p v-if="report.pricingError" class="usage-reconciliation-warning">
          {{ report.pricingError }}
        </p>
        <section class="usage-reconciliation-section">
          <header>
            <h4>账号归属</h4>
            <span>启动记录保留 {{ report.accountRetentionDays }} 天</span>
          </header>
          <div v-if="report.accounts.length" class="usage-reconciliation-list">
            <div
              v-for="account in report.accounts"
              :key="accountKey(account)"
              class="usage-reconciliation-row is-account"
            >
              <div class="usage-reconciliation-row-main">
                <strong>{{
                  account.providerName ||
                  (account.providerId ? account.providerId : "未关联账号")
                }}</strong>
                <span>{{
                  account.accountLabel ||
                  (account.providerId ? "未记录账号标签" : "未经过 BalanceHub 启动")
                }}</span>
              </div>
              <span class="usage-reconciliation-meta">{{ fullAgentCliLabel(account.cliKind) }}</span>
              <span class="usage-reconciliation-meta">{{ count(account.sessions) }} 个会话</span>
              <span class="usage-reconciliation-tokens">{{ tokenLabel(account.usage.tokens) }}</span>
              <span class="usage-reconciliation-cost">{{ costLabel(account.usage) }}</span>
            </div>
          </div>
          <p v-else class="usage-reconciliation-state">窗口内没有可解码的会话用量。</p>
        </section>
        <section class="usage-reconciliation-section">
          <header>
            <h4>项目</h4>
            <span>{{ report.projects.length }} 个目录</span>
          </header>
          <div v-if="report.projects.length" class="usage-reconciliation-list">
            <div
              v-for="project in report.projects"
              :key="project.path"
              class="usage-reconciliation-row is-project"
            >
              <div class="usage-reconciliation-row-main">
                <strong :title="project.path">{{ project.path || "未记录项目" }}</strong>
                <span>{{ count(project.sessions) }} 个会话</span>
              </div>
              <span class="usage-reconciliation-tokens">{{ tokenLabel(project.usage.tokens) }}</span>
              <span class="usage-reconciliation-cost">{{ costLabel(project.usage) }}</span>
              <span v-if="priceNote(project.usage)" class="usage-reconciliation-meta">{{
                priceNote(project.usage)
              }}</span>
            </div>
          </div>
          <p v-else class="usage-reconciliation-state">窗口内没有项目用量。</p>
        </section>
        <section class="usage-reconciliation-section">
          <header>
            <h4>模型</h4>
            <span>按 token 排序</span>
          </header>
          <div v-if="models.length" class="usage-reconciliation-list">
            <div
              v-for="model in models"
              :key="model.model ?? 'unknown'"
              class="usage-reconciliation-row is-model"
            >
              <div class="usage-reconciliation-row-main">
                <strong>{{ model.model || "未记录模型" }}</strong>
                <span>{{ count(model.records) }} 条用量记录</span>
              </div>
              <span class="usage-reconciliation-tokens">{{ tokenLabel(model.tokens) }}</span>
              <span class="usage-reconciliation-cost">{{
                costValue(model.estimatedCostMicrousd, model.records)
              }}</span>
            </div>
          </div>
          <p v-else class="usage-reconciliation-state">窗口内没有模型用量。</p>
        </section>
        <section v-if="report.sites.length" class="usage-reconciliation-section">
          <header>
            <h4>站点账单对比</h4>
            <span>本地仅覆盖本机经 BalanceHub 启动的会话；站点包含账号全部流量</span>
          </header>
          <div class="usage-reconciliation-sites">
            <div
              v-for="site in report.sites"
              :key="site.providerId"
              class="usage-reconciliation-site"
            >
              <div class="usage-reconciliation-site-head">
                <strong>{{ siteLabel(site) }}</strong>
                <span v-if="site.error" class="usage-reconciliation-site-error">{{ site.error }}</span>
              </div>
              <div class="usage-reconciliation-site-numbers">
                <span>本地估算 {{ count(site.localTokens) }} token</span>
                <span v-if="site.usage">站点账单 {{ count(site.usage.tokenUsed) }} token</span>
                <span v-if="site.usage">请求 {{ count(site.usage.requestCount) }}</span>
              </div>
            </div>
          </div>
        </section>
        <section v-if="report.diagnostics.length" class="usage-reconciliation-section">
          <header><h4>读取说明</h4></header>
          <ul class="usage-reconciliation-diagnostics">
            <li v-for="item in report.diagnostics" :key="item">{{ item }}</li>
          </ul>
        </section>
        <p class="usage-reconciliation-footnote">
          按会话最近更新时间筛选：窗口开始前产生、窗口内又活跃的会话会整体计入；
          未记录 token 的字段显示为未知。
        </p>
      </template>
      <p v-else-if="!loading" class="usage-reconciliation-state">
        尚未读取用量对账。
      </p>
      <footer class="usage-reconciliation-actions">
        <span v-if="generatedAt">统计时间 {{ generatedAt }}</span>
        <a-button @click="close">关闭</a-button>
      </footer>
    </div>
  </a-modal>
</template>
