<script setup lang="ts">
import { computed } from "vue";
import { Message } from "@arco-design/web-vue";
import { IconCopy } from "@arco-design/web-vue/es/icon";
import type { ProviderRequestLog, ProviderQuotaDisplay } from "../stores/providers";
import { logTypeLabel, logChannel, logDuration, logTokenTotal, logDetails, rawValue, rawJson, formatLogQuotaValue } from "../utils/request-log-display";
import { copyText } from "../composables/useClipboard";
import { withTimeout } from "../utils/promise-timeout";
import ContextDetails from "./ContextDetails.vue";

const props = defineProps<{ log: ProviderRequestLog | null; quotaDisplay?: ProviderQuotaDisplay }>();
const emit = defineEmits<{ close: [] }>();
const rows = computed(() => {
  const log = props.log;
  if (!log) return [];
  return [
    ["时间", log.createdAt],
    ["类型", logTypeLabel(log)],
    ["模型", log.modelName],
    ["令牌", log.tokenName],
    ["渠道", logChannel(log)],
    ["耗时", logDuration(log)],
    ["输入 Tokens", log.promptTokens.toLocaleString()],
    ["输出 Tokens", log.completionTokens.toLocaleString()],
    ["Tokens 合计", logTokenTotal(log).toLocaleString()],
    ["消耗", formatLogQuotaValue(log.quota, props.quotaDisplay)],
    ["请求 ID", log.requestId],
    ["上游请求 ID", String(rawValue(log, "upstream_request_id") || "")],
  ].filter(([, value]) => Boolean(value) && value !== "-");
});

async function copy(value: string, label: string) {
  try {
    await withTimeout(copyText(value), 5_000, "复制超时，请重试");
    Message.success(`已复制${label}`);
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  }
}
</script>

<template>
  <a-modal
    :visible="Boolean(log)"
    modal-class="surface-modal request-log-detail-modal-surface"
    title="请求详情"
    :footer="false"
    width="min(720px, calc(100vw - 32px))"
    unmount-on-close
    @update:visible="(value) => { if (!value) emit('close'); }"
  >
    <div v-if="log" class="request-log-detail-modal">
      <section class="request-log-detail-section">
        <div v-for="[label, value] in rows" :key="label" class="request-log-detail-row">
          <span>{{ label }}</span>
          <strong>{{ value }}</strong>
          <a-button v-if="label.endsWith('ID')" type="text" size="mini" :aria-label="`复制${label}`" :title="`复制${label}`" @click="copy(value, label)">
            <template #icon><IconCopy /></template>
          </a-button>
        </div>
      </section>
      <section v-if="logDetails(log)" class="request-log-detail-section">
        <header class="request-log-detail-header">
          <h4>内容</h4>
          <a-button type="text" size="mini" @click="copy(logDetails(log), '日志内容')">复制内容</a-button>
        </header>
        <p>{{ logDetails(log) }}</p>
      </section>
      <section class="request-log-detail-section">
        <ContextDetails label="原始数据" class="request-log-raw-data">
          <div><a-button type="text" size="mini" @click="copy(rawJson(log), '原始数据')"><template #icon><IconCopy /></template>复制 JSON</a-button></div>
          <pre>{{ rawJson(log) }}</pre>
        </ContextDetails>
      </section>
    </div>
  </a-modal>
</template>
