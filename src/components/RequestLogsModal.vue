<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IconFile, IconLeft, IconRefresh, IconRight, IconSearch } from "@arco-design/web-vue/es/icon";
import type {
  Provider,
  ProviderProtocolDescriptor,
  ProviderRequestLog,
  ProviderRequestLogFilters,
  ProviderRequestLogsResult,
  ProviderRequestLogTimeRange,
} from "../stores/providers";
import RequestLogDetailsModal from "./RequestLogDetailsModal.vue";
import { logIdentity, logTokenTotal, rawValue, logHasUsageData, logDuration, logDetails, logRequestId, logChannel, logTypeLabel, logStatusTone, formatLogQuotaValue, logPreview, requestLogRateHint } from "../utils/request-log-display";
import { formatNumberCompact, providerDisplayLabel } from "../utils/provider-display";
import { providerProtocolDescriptor } from "../utils/provider-protocol";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  loading: boolean;
  error: string;
  result: ProviderRequestLogsResult | null;
  filters: ProviderRequestLogFilters;
  protocols: ProviderProtocolDescriptor[];
  page: number;
  pageSize: number;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  search: [filters: ProviderRequestLogFilters];
  reset: [];
  refresh: [];
  pageChange: [page: number];
  pageSizeChange: [pageSize: number];
  reconcile: [];
}>();

const filtersDraft = ref<ProviderRequestLogFilters>({ ...props.filters });
const selectedLog = ref<ProviderRequestLog | null>(null);

watch([() => props.visible, () => props.provider?.identity.id, () => props.page, () => props.pageSize], () => {
  selectedLog.value = null;
}, { flush: "sync" });

watch(
  [() => props.visible, () => props.provider?.identity.id, () => props.filters],
  () => {
    filtersDraft.value = { ...props.filters };
  },
  { flush: "sync" },
);

/// 协议声明自己认识哪些日志筛选字段；界面只渲染这些控件，避免把站点
/// 不支持的参数发出去后仍显示成“已筛选”。
const availableFilters = computed(
  () =>
    providerProtocolDescriptor(
      props.protocols,
      props.provider?.identity.protocol ?? "newApi",
    )?.capabilities.requestLogFilters ?? [],
);

const modalTitle = computed(() =>
  props.provider ? `${providerDisplayLabel(props.provider)} · 请求日志` : "请求日志",
);

const rows = computed(() => props.result?.logs ?? []);

/// Sub2API 的 RPM / TPM / 平均耗时来自全站近 5 分钟看板，既不跟时间范围，
/// 也不跟筛选条件；同一排的其他数字却按「所选范围 + 筛选」统计。两种口径必须
/// 各自标注，否则读者会把全站瞬时值当成筛选结果。
const rateHint = computed(() =>
  requestLogRateHint(props.provider?.identity.protocol),
);

const statCards = computed(() => {
  const stats = props.result?.stats;
  const cards = [
    {
      label: "消耗",
      value:
        stats?.quota != null
          ? formatLogQuotaValue(stats.quota, props.result?.quotaDisplay)
          : "—",
      tone: "cost",
      hint: "所选时间范围和筛选条件下的消耗合计",
    },
    {
      label: "每分钟请求 · RPM",
      value: stats?.rpm != null ? formatNumberCompact(stats.rpm, 2) : "—",
      tone: "rpm",
      hint: rateHint.value,
    },
    {
      label: "每分钟 Tokens · TPM",
      value: stats?.tpm != null ? formatNumberCompact(stats.tpm, 2) : "—",
      tone: "tpm",
      hint: rateHint.value,
    },
    {
      label: props.result?.total == null ? "本页记录" : "总记录",
      value: props.result ? formatNumberCompact(props.result.total ?? rows.value.length, 0) : "—",
      tone: "count",
      hint: "按当前筛选条件统计的记录条数",
    },
  ];
  // 只有站点真的给了平均耗时才多一格；凑满四格放一个「—」没有意义。
  if (stats?.avgDurationMs != null) {
    cards.splice(3, 0, {
      label: "平均耗时",
      value: formatDuration(stats.avgDurationMs),
      tone: "duration",
      hint: rateHint.value,
    });
  }
  if (rows.value.length) {
    cards.push({
      label: "本页错误率",
      value: `${Math.round((pageErrors.value.length / rows.value.length) * 100)}%`,
      tone: "error",
      hint: `本页 ${rows.value.length} 条请求中 ${pageErrors.value.length} 条失败；只统计当前页，不代表所选时间范围`,
    });
  }
  return cards;
});

const pageErrors = computed(() => rows.value.filter((log) => logStatusTone(log) === "error"));

/// 站点统计接口不按模型或 Key 汇总，这里用当前页记录给出可见的集中度。
/// 明确写「本页」，避免读者把一页样本当成完整时间范围。
const pageAggregates = computed(() => {
  const logs = rows.value;
  if (!logs.length) return [];
  const top = (values: string[]) => {
    const counts = new Map<string, number>();
    for (const value of values.map((item) => item.trim()).filter(Boolean)) {
      counts.set(value, (counts.get(value) || 0) + 1);
    }
    return [...counts.entries()].sort((left, right) => right[1] - left[1])[0];
  };
  const items: Array<{ label: string; value: string }> = [];
  const model = top(logs.map((log) => log.modelName));
  if (model) items.push({ label: "主要模型", value: `${model[0]} · ${model[1]} 次` });
  const key = top(logs.map((log) => log.tokenName));
  if (key) items.push({ label: "主要 Key", value: `${key[0]} · ${key[1]} 次` });
  const durations = logs
    .map((log) => log.durationMs)
    .filter((value): value is number => typeof value === "number" && Number.isFinite(value) && value > 0);
  if (durations.length) {
    const average = durations.reduce((total, value) => total + value, 0) / durations.length;
    items.push({ label: "本页平均耗时", value: formatDuration(average) });
  }
  return items;
});

function formatDuration(value: number) {
  return value >= 1000 ? `${(value / 1000).toFixed(1)} s` : `${Math.round(value)} ms`;
}

const canPrevious = computed(() => props.page > 0);

const canNext = computed(() => {
  if (!props.result) return false;
  const total = props.result?.total;
  if (typeof total === "number" && total >= 0) {
    return (props.page + 1) * props.pageSize < total;
  }
  return rows.value.length >= props.pageSize;
});

const pageLabel = computed(() => {
  const total = props.result?.total;
  if (typeof total === "number" && total >= 0) {
    return `第 ${props.page + 1} 页 / 共 ${formatNumberCompact(total, 0)} 条`;
  }
  return `第 ${props.page + 1} 页`;
});

const pageSizeOptions = [10, 20, 50, 100].map((value) => ({ label: `${value} 条`, value }));

const showUsageColumns = computed(() => rows.value.some((log) => logHasUsageData(log)));

const tableClasses = computed(() => ({
  "request-logs-table-usage": showUsageColumns.value,
  "request-logs-table-simple": !showUsageColumns.value,
}));

const RANGE_OPTIONS: { label: string; value: ProviderRequestLogTimeRange }[] = [
  { label: "最近 24 小时", value: "24h" },
  { label: "最近 7 天", value: "7d" },
  { label: "最近 30 天", value: "30d" },
  { label: "最近 90 天", value: "90d" },
];

const LOG_TYPE_OPTIONS = [
  { label: "消费", value: 2 },
  { label: "充值", value: 1 },
  { label: "管理", value: 3 },
  { label: "系统", value: 4 },
  { label: "错误", value: 5 },
];

// 表格结果来自上一次「查询」提交的 filters；筛选条和空结果说明也必须
// 描述这份已生效的条件，不能拿正在输入、尚未提交的草稿冒充筛选结果。
const appliedFilters = computed(() => props.filters);

const rangeLabel = computed(() => {
  const range = appliedFilters.value.range;
  return RANGE_OPTIONS.find((option) => option.value === range)?.label ?? "";
});

const activeFilterSummary = computed(() => {
  const filters = appliedFilters.value;
  const parts: string[] = [];
  if (filters.model) parts.push(`模型 ${filters.model}`);
  if (filters.tokenName) parts.push(`Key ${filters.tokenName}`);
  if (filters.group) parts.push(`分组 ${filters.group}`);
  if (filters.requestId) parts.push(`请求 ${filters.requestId}`);
  if (filters.logType != null) {
    const logType = filters.logType;
    const type = LOG_TYPE_OPTIONS.find((option) => option.value === logType);
    if (type) parts.push(type.label);
  }
  parts.push(rangeLabel.value);
  return parts.join(" · ");
});

const hasNarrowedFilters = computed(() => {
  const filters = appliedFilters.value;
  return Boolean(
    filters.model || filters.tokenName || filters.group || filters.requestId,
  ) || filters.logType != null || filters.range !== "30d";
});

function submitSearch(event?: KeyboardEvent) {
  if (event?.isComposing || event?.keyCode === 229) return;
  emit("search", { ...filtersDraft.value });
}

function clearFilters() {
  filtersDraft.value = {
    model: "",
    tokenName: "",
    group: "",
    requestId: "",
    logType: null,
    range: "30d",
  };
  emit("reset");
}

function setLogType(value: unknown) {
  filtersDraft.value.logType = typeof value === "number" ? value : null;
}

function openLogDetails(log: ProviderRequestLog) {
  selectedLog.value = log;
}

</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal request-logs-modal"
    :footer="false"
    width="min(1180px, calc(100vw - 32px))"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title request-logs-title">
        <span class="surface-modal-title-icon"><icon-file /></span>
        <span class="surface-modal-title-copy">
          <strong>{{ modalTitle }}</strong>
        </span>
      </div>
    </template>
    <div class="request-logs-panel">
      <div class="request-logs-toolbar">
        <a-input
          v-if="availableFilters.includes('model')"
          v-model="filtersDraft.model"
          allow-clear
          placeholder="按模型名称筛选"
          aria-label="按模型名称筛选请求日志"
          @press-enter="submitSearch"
        >
          <template #prefix><icon-search /></template>
        </a-input>
        <a-select
          v-if="availableFilters.includes('timeRange')"
          v-model="filtersDraft.range"
          :options="RANGE_OPTIONS"
          class="request-logs-range"
          aria-label="请求日志时间范围"
        />
        <a-button type="primary" @click="submitSearch()">
          <template #icon><icon-search /></template>
          搜索
        </a-button>
        <a-button :loading="loading" @click="emit('refresh')">
          <template #icon><icon-refresh /></template>
          刷新
        </a-button>
        <!-- 站点日志只能回答“站点记了多少”；本地会话对账回答“本机 Agent 用了
             多少”。两个入口放在同一排，方便带着当前时间范围去核对。 -->
        <a-button @click="emit('reconcile')">本地会话对账</a-button>
      </div>

      <div
        v-if="
          availableFilters.includes('tokenName') ||
          availableFilters.includes('group') ||
          availableFilters.includes('requestId') ||
          availableFilters.includes('logType')
        "
        class="request-logs-filter-grid"
      >
        <a-input
          v-if="availableFilters.includes('tokenName')"
          v-model="filtersDraft.tokenName"
          allow-clear
          placeholder="Key 名称"
          aria-label="按 Key 名称筛选请求日志"
          @press-enter="submitSearch"
        />
        <a-input
          v-if="availableFilters.includes('group')"
          v-model="filtersDraft.group"
          allow-clear
          placeholder="分组"
          aria-label="按分组筛选请求日志"
          @press-enter="submitSearch"
        />
        <a-input
          v-if="availableFilters.includes('requestId')"
          v-model="filtersDraft.requestId"
          allow-clear
          placeholder="请求 ID"
          aria-label="按请求 ID 筛选请求日志"
          @press-enter="submitSearch"
        />
        <a-select
          v-if="availableFilters.includes('logType')"
          :model-value="filtersDraft.logType ?? undefined"
          :options="LOG_TYPE_OPTIONS"
          allow-clear
          placeholder="全部类型"
          aria-label="按日志类型筛选请求日志"
          @update:model-value="setLogType"
        />
      </div>

      <div v-if="hasNarrowedFilters" class="request-logs-filter-summary">
        <span>筛选：<strong>{{ activeFilterSummary }}</strong></span>
        <a-button type="text" size="mini" @click="clearFilters">清除筛选</a-button>
      </div>
      <a-alert v-if="error" type="error" show-icon>
        <div class="panel-load-error"><span>{{ error }}</span><a-button size="small" @click="emit('refresh')">重试</a-button></div>
      </a-alert>

      <div class="request-logs-stats">
        <div
          v-for="card in statCards"
          :key="card.label"
          :class="`request-logs-stat-${card.tone}`"
          :title="card.hint"
        >
          <span>{{ card.label }}</span>
          <strong :title="card.value">{{ card.value }}</strong>
        </div>
      </div>
      <p v-if="pageAggregates.length" class="request-logs-aggregates" aria-label="本页请求分布">
        <span v-for="item in pageAggregates" :key="item.label">
          <small>{{ item.label }}</small>
          <strong>{{ item.value }}</strong>
        </span>
      </p>

      <a-spin :loading="loading">
        <div v-if="rows.length === 0" class="api-key-empty" role="status">
          {{ loading ? '正在读取请求日志…' : error ? '请求日志未能加载' : hasNarrowedFilters ? '没有匹配的请求记录，可清除筛选后查看全部' : '暂无请求日志' }}
        </div>
        <div v-else class="request-logs-table-wrap">
          <table class="request-logs-table" :class="tableClasses" aria-label="请求日志">
            <colgroup>
              <col class="request-log-col-time" />
              <col class="request-log-col-type" />
              <template v-if="showUsageColumns">
                <col class="request-log-col-token" />
                <col class="request-log-col-model" />
                <col class="request-log-col-timing" />
                <col class="request-log-col-tokens" />
                <col class="request-log-col-cost" />
              </template>
              <col class="request-log-col-details" />
            </colgroup>
            <thead>
              <tr>
                <th>时间</th>
                <th>类型</th>
                <template v-if="showUsageColumns">
                  <th>令牌</th>
                  <th>模型</th>
                  <th>耗时</th>
                  <th class="request-log-number">Tokens</th>
                  <th class="request-log-number">消耗</th>
                </template>
                <th>详情</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(log, index) in rows" :key="logIdentity(log, index)">
                <td class="request-log-time">{{ log.createdAt || "-" }}</td>
                <td>
                  <span class="request-log-status" :class="`request-log-status-${logStatusTone(log)}`">
                    {{ logTypeLabel(log) }}
                  </span>
                </td>
                <template v-if="showUsageColumns">
                  <td class="request-log-token" :title="log.tokenName || '-'">
                    <strong>{{ log.tokenName || "-" }}</strong>
                    <span v-if="rawValue(log, 'group')">{{ rawValue(log, "group") }}</span>
                  </td>
                  <td class="request-log-model" :title="log.modelName || '-'">
                    <strong>{{ log.modelName || "-" }}</strong>
                    <span v-if="logChannel(log)" :title="logChannel(log)">{{ logChannel(log) }}</span>
                  </td>
                  <td class="request-log-timing">{{ logDuration(log) }}</td>
                  <td class="request-log-tokens request-log-number">
                    <strong>{{ logTokenTotal(log) > 0 ? formatNumberCompact(logTokenTotal(log), 0) : "—" }}</strong>
                    <span v-if="logTokenTotal(log) > 0" :title="`输入 ${log.promptTokens.toLocaleString()} · 输出 ${log.completionTokens.toLocaleString()}`">
                      入 {{ log.promptTokens.toLocaleString() }} · 出 {{ log.completionTokens.toLocaleString() }}
                    </span>
                  </td>
                  <td class="request-log-cost request-log-number">
                    <strong>{{ formatLogQuotaValue(log.quota, result?.quotaDisplay) }}</strong>
                  </td>
                </template>
                <td class="request-log-details" :title="logPreview(log)">
                  <button type="button" class="request-log-detail-button" @click="openLogDetails(log)">
                    <span>{{ logPreview(log) }}</span>
                    <small v-if="logRequestId(log) && logDetails(log)">{{ logRequestId(log) }}</small>
                  </button>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </a-spin>

      <div class="request-logs-pagination">
        <span>{{ pageLabel }}</span>
        <div>
          <a-select
            :model-value="pageSize"
            :options="pageSizeOptions"
            class="request-logs-page-size"
            aria-label="每页请求日志数量"
            @update:model-value="emit('pageSizeChange', Number($event))"
          />
          <a-button :disabled="!canPrevious || loading" @click="emit('pageChange', page - 1)">
            <template #icon><icon-left /></template>
            上一页
          </a-button>
          <a-button :disabled="!canNext || loading" @click="emit('pageChange', page + 1)">
            下一页
            <template #icon><icon-right /></template>
          </a-button>
        </div>
      </div>
    </div>
  </a-modal>

  <RequestLogDetailsModal
    :log="visible ? selectedLog : null"
    :quota-display="result?.quotaDisplay"
    @close="selectedLog = null"
  />
</template>
