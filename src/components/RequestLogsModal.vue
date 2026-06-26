<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IconLeft, IconRefresh, IconRight, IconSearch } from "@arco-design/web-vue/es/icon";
import type { Provider, ProviderRequestLog, ProviderRequestLogsResult } from "../stores/providers";
import { formatNumberCompact } from "../utils/provider-display";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
  loading: boolean;
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

watch(
  () => props.keyword,
  (value) => {
    keywordDraft.value = value;
  },
);

const modalTitle = computed(() =>
  props.provider ? `${props.provider.identity.name} · 请求日志` : "请求日志",
);

const rows = computed(() => props.result?.logs ?? []);

const canPrevious = computed(() => props.page > 0);

const canNext = computed(() => {
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

const selectedLogRows = computed(() => {
  const log = selectedLog.value;
  if (!log) return [];
  const rows = [
    ["时间", log.createdAt || "-"],
    ["类型", logTypeLabel(log)],
    ["模型", log.modelName || "-"],
    ["令牌", log.tokenName || "-"],
    ["渠道", logChannel(log) || "-"],
    ["耗时", logDuration(log)],
    ["输入 Tokens", log.promptTokens > 0 ? log.promptTokens.toLocaleString() : "-"],
    ["输出 Tokens", log.completionTokens > 0 ? log.completionTokens.toLocaleString() : "-"],
    ["Tokens 合计", logTokenTotal(log) > 0 ? logTokenTotal(log).toLocaleString() : "-"],
    ["消耗", log.quota === 0 ? "-" : formatLogQuotaValue(log.quota, props.result?.quotaDisplay)],
    ["请求 ID", log.requestId || "-"],
    ["上游请求 ID", String(rawValue(log, "upstream_request_id") || "-")],
  ];
  return rows.filter(([, value]) => value !== "-");
});

function submitSearch() {
  emit("search", keywordDraft.value);
}

function openLogDetails(log: ProviderRequestLog) {
  selectedLog.value = log;
}

function logIdentity(log: ProviderRequestLog, index: number) {
  return log.id || log.requestId || `${log.createdAt}-${index}`;
}

function logTokenTotal(log: ProviderRequestLog) {
  return log.tokenUsed || log.promptTokens + log.completionTokens;
}

function isMeaningfulText(value: unknown) {
  const text = String(value ?? "").trim();
  return text !== "" && text !== "-" && text !== "—" && text.toLowerCase() !== "null";
}

function rawValue(log: ProviderRequestLog, key: string) {
  return log.raw?.[key];
}

function logHasUsageData(log: ProviderRequestLog) {
  return (
    isMeaningfulText(log.modelName) ||
    isMeaningfulText(log.tokenName) ||
    logTokenTotal(log) > 0 ||
    log.quota !== 0 ||
    (typeof log.durationMs === "number" && Number.isFinite(log.durationMs) && log.durationMs > 0)
  );
}

function logDuration(log: ProviderRequestLog) {
  if (typeof log.durationMs !== "number" || !Number.isFinite(log.durationMs) || log.durationMs <= 0) {
    return "-";
  }
  if (log.durationMs >= 1000) {
    return `${(log.durationMs / 1000).toFixed(2)}s`;
  }
  return `${log.durationMs}ms`;
}

function formatUseTime(log: ProviderRequestLog) {
  if (typeof log.durationMs !== "number" || !Number.isFinite(log.durationMs) || log.durationMs <= 0) {
    return "-";
  }
  const seconds = log.durationMs / 1000;
  if (seconds < 60) {
    return `${seconds.toFixed(1)}s`;
  }
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${(seconds % 60).toFixed(0)}s`;
}

function logDetails(log: ProviderRequestLog) {
  if (isMeaningfulText(log.content)) {
    return log.content.trim();
  }
  return "";
}

function logRequestId(log: ProviderRequestLog) {
  const upstreamRequestId = rawValue(log, "upstream_request_id");
  if (isMeaningfulText(log.requestId)) {
    return log.requestId;
  }
  if (isMeaningfulText(upstreamRequestId)) {
    return String(upstreamRequestId);
  }
  return "";
}

function logChannel(log: ProviderRequestLog) {
  if (isMeaningfulText(log.channel) && log.channel !== "0") {
    return log.channel;
  }
  const channelId = rawValue(log, "channel");
  if (isMeaningfulText(channelId) && String(channelId) !== "0") {
    return `#${channelId}`;
  }
  return "";
}

function logTypeCode(log: ProviderRequestLog) {
  const rawType = rawValue(log, "type");
  const code = Number(rawType ?? log.status);
  return Number.isFinite(code) ? code : null;
}

function logTypeLabel(log: ProviderRequestLog) {
  const code = logTypeCode(log);
  const labels: Record<number, string> = {
    0: "未知",
    1: "充值",
    2: "消耗",
    3: "管理",
    4: "系统",
    5: "错误",
    6: "退款",
    7: "登录",
  };
  if (code !== null && labels[code]) {
    return labels[code];
  }
  return isMeaningfulText(log.status) ? log.status : "未知";
}

function logStatusTone(log: ProviderRequestLog) {
  const typeCode = logTypeCode(log);
  if (typeCode === 2) return "ok";
  if (typeCode === 5) return "error";
  if (typeCode === 6) return "refund";
  if (typeCode === 1 || typeCode === 4 || typeCode === 7) return "info";

  const status = String(log.status || "").toLowerCase();
  const statusCode = Number(status);
  if (Number.isFinite(statusCode)) {
    if (statusCode >= 200 && statusCode < 400) return "ok";
    if (statusCode >= 400) return "error";
  }
  if (status.includes("success") || status === "ok" || status === "200") {
    return "ok";
  }
  if (status.includes("fail") || status.includes("error")) {
    return "error";
  }
  return "neutral";
}

function tokenSummary(log: ProviderRequestLog) {
  if (log.promptTokens <= 0 && log.completionTokens <= 0) {
    return "-";
  }
  return `${formatNumberCompact(log.promptTokens, 0)} / ${formatNumberCompact(log.completionTokens, 0)}`;
}

function formatLogQuotaValue(value: number, quotaDisplay?: ProviderRequestLogsResult["quotaDisplay"]) {
  if (!quotaDisplay) return "-";
  const displayType = quotaDisplay.quotaDisplayType || "currency";
  if (displayType.toLowerCase() === "tokens") {
    return formatPreciseNumber(value, 4, 6);
  }
  const symbol = normalizeCurrencySymbol(displayType, quotaDisplay.currencySymbol);
  return `${symbol}${formatPreciseNumber(value, 4, 6)}`;
}

function formatPreciseNumber(value: number, digitsLarge: number, digitsSmall: number) {
  if (!Number.isFinite(value)) return "-";
  const digits = Math.abs(value) >= 1 ? digitsLarge : digitsSmall;
  return value.toFixed(digits).replace(/(\.[0-9]*?)0+$/, "$1").replace(/\.$/, "");
}

function normalizeCurrencySymbol(displayType: string, value: string) {
  const symbol = value.trim();
  if (symbol && symbol !== "¤") {
    return symbol;
  }
  return displayType.trim().toUpperCase() === "USD" ? "$" : "$";
}

function logPreview(log: ProviderRequestLog) {
  return logDetails(log) || logRequestId(log) || "查看详情";
}

function rawJson(log: ProviderRequestLog) {
  return JSON.stringify(log.raw, null, 2);
}
</script>

<template>
  <a-modal
    :visible="visible"
    :title="modalTitle"
    :footer="false"
    width="min(1180px, calc(100vw - 32px))"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <div class="request-logs-panel">
      <div class="request-logs-toolbar">
        <a-input
          v-model="keywordDraft"
          allow-clear
          placeholder="搜索模型、令牌、内容或请求 ID"
          @press-enter="submitSearch"
        >
          <template #prefix><icon-search /></template>
        </a-input>
        <a-select
          :model-value="pageSize"
          :options="pageSizeOptions"
          class="request-logs-page-size"
          @update:model-value="emit('pageSizeChange', Number($event))"
        />
        <a-button type="primary" @click="submitSearch">
          <template #icon><icon-search /></template>
          搜索
        </a-button>
        <a-button :loading="loading" @click="emit('refresh')">
          <template #icon><icon-refresh /></template>
          刷新
        </a-button>
      </div>

      <a-spin :loading="loading">
        <div v-if="rows.length === 0" class="api-key-empty">
          暂无请求日志
        </div>
        <div v-else class="request-logs-table-wrap">
          <table class="request-logs-table" :class="tableClasses">
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
                  <th>Tokens</th>
                  <th>消耗</th>
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
                  <td class="request-log-timing">{{ formatUseTime(log) }}</td>
                  <td class="request-log-tokens">
                    <strong>{{ tokenSummary(log) }}</strong>
                    <span v-if="logTokenTotal(log) > 0">{{ formatNumberCompact(logTokenTotal(log), 0) }} total</span>
                  </td>
                  <td class="request-log-cost">
                    <strong>{{ log.quota === 0 ? "-" : formatLogQuotaValue(log.quota, result?.quotaDisplay) }}</strong>
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

  <a-modal
    :visible="Boolean(selectedLog)"
    title="日志详情"
    :footer="false"
    width="min(720px, calc(100vw - 32px))"
    unmount-on-close
    @update:visible="(value) => { if (!value) selectedLog = null; }"
  >
    <div v-if="selectedLog" class="request-log-detail-modal">
      <section class="request-log-detail-section">
        <div v-for="[label, value] in selectedLogRows" :key="label" class="request-log-detail-row">
          <span>{{ label }}</span>
          <strong>{{ value }}</strong>
        </div>
      </section>

      <section v-if="logDetails(selectedLog)" class="request-log-detail-section">
        <h4>内容</h4>
        <p>{{ logDetails(selectedLog) }}</p>
      </section>

      <section class="request-log-detail-section">
        <h4>原始数据</h4>
        <pre>{{ rawJson(selectedLog) }}</pre>
      </section>
    </div>
  </a-modal>
</template>
