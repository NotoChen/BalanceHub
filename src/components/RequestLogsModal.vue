<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IconFile, IconLeft, IconRefresh, IconRight, IconSearch } from "@arco-design/web-vue/es/icon";
import type { Provider, ProviderRequestLog, ProviderRequestLogsResult } from "../stores/providers";
import RequestLogDetailsModal from "./RequestLogDetailsModal.vue";
import { logIdentity, logTokenTotal, rawValue, logHasUsageData, logDuration, logDetails, logRequestId, logChannel, logTypeLabel, logStatusTone, formatLogQuotaValue, logPreview } from "../utils/request-log-display";
import { formatNumberCompact, providerDisplayLabel } from "../utils/provider-display";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  loading: boolean;
  error: string;
  result: ProviderRequestLogsResult | null;
  keyword: string;
  page: number;
  pageSize: number;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
  search: [keyword: string];
  refresh: [];
  pageChange: [page: number];
  pageSizeChange: [pageSize: number];
}>();

const keywordDraft = ref(props.keyword);
const selectedLog = ref<ProviderRequestLog | null>(null);

watch([() => props.visible, () => props.provider?.identity.id, () => props.keyword, () => props.page, () => props.pageSize], () => {
  selectedLog.value = null;
}, { flush: "sync" });

watch(
  [() => props.visible, () => props.provider?.identity.id, () => props.keyword],
  () => {
    keywordDraft.value = props.keyword;
  },
  { flush: "sync" },
);

const modalTitle = computed(() =>
  props.provider ? `${providerDisplayLabel(props.provider)} · 请求日志` : "请求日志",
);

const rows = computed(() => props.result?.logs ?? []);

const statCards = computed(() => {
  const stats = props.result?.stats;
  return [
    {
      label: "消耗",
      value: stats ? formatLogQuotaValue(stats.quota, props.result?.quotaDisplay) : "—",
      tone: "cost",
    },
    {
      label: "每分钟请求 · RPM",
      value: stats ? formatNumberCompact(stats.rpm, 2) : "—",
      tone: "rpm",
    },
    {
      label: "每分钟 Tokens · TPM",
      value: stats ? formatNumberCompact(stats.tpm, 2) : "—",
      tone: "tpm",
    },
    {
      label: props.result?.total == null ? "本页记录" : "总记录",
      value: props.result ? formatNumberCompact(props.result.total ?? rows.value.length, 0) : "—",
      tone: "count",
    },
  ];
});

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

function submitSearch(event?: KeyboardEvent) {
  if (event?.isComposing || event?.keyCode === 229) return;
  emit("search", keywordDraft.value.trim());
}

function clearSearch() {
  keywordDraft.value = "";
  emit("search", "");
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
          v-model="keywordDraft"
          allow-clear
          placeholder="按模型名称筛选"
          aria-label="按模型名称筛选请求日志"
          @press-enter="submitSearch"
          @clear="clearSearch"
        >
          <template #prefix><icon-search /></template>
        </a-input>
        <a-button type="primary" @click="submitSearch()">
          <template #icon><icon-search /></template>
          搜索
        </a-button>
        <a-button :loading="loading" @click="emit('refresh')">
          <template #icon><icon-refresh /></template>
          刷新
        </a-button>
      </div>

      <div v-if="keyword" class="request-logs-filter-summary">
        <span>模型筛选：<strong>{{ keyword }}</strong></span>
        <a-button type="text" size="mini" @click="clearSearch">清除筛选</a-button>
      </div>
      <a-alert v-if="error" type="error" show-icon>
        <div class="panel-load-error"><span>{{ error }}</span><a-button size="small" @click="emit('refresh')">重试</a-button></div>
      </a-alert>

      <div class="request-logs-stats">
        <div v-for="card in statCards" :key="card.label" :class="`request-logs-stat-${card.tone}`">
          <span>{{ card.label }}</span>
          <strong :title="card.value">{{ card.value }}</strong>
        </div>
      </div>

      <a-spin :loading="loading">
        <div v-if="rows.length === 0" class="api-key-empty" role="status">
          {{ loading ? '正在读取请求日志…' : error ? '请求日志未能加载' : keyword ? '没有匹配的请求记录，可清除筛选后查看全部' : '暂无请求日志' }}
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
