<script setup lang="ts">
import { computed } from "vue";
import { IconExperiment } from "@arco-design/web-vue/es/icon";
import type { LivenessRecord, Provider } from "../stores/providers";

const props = defineProps<{
  visible: boolean;
  provider: Provider | null;
}>();

const emit = defineEmits<{
  "update:visible": [visible: boolean];
}>();

const cumulative = computed(() => ({
  runCount: props.provider?.liveness.runCount ?? 0,
  inputTokens: props.provider?.liveness.totalInputTokens ?? 0,
  outputTokens: props.provider?.liveness.totalOutputTokens ?? 0,
  totalTokens: props.provider?.liveness.totalTokens ?? 0,
  costUsd: props.provider?.liveness.totalCostUsd ?? 0,
}));

const records = computed(() =>
  [...(props.provider?.liveness.records ?? [])].sort(
    (left, right) => Number(right.checkedAt || 0) - Number(left.checkedAt || 0),
  ),
);

const modalTitle = computed(() =>
  props.provider ? `${props.provider.identity.name} · 测活明细` : "测活明细",
);

function formatDateTime(value: string) {
  const timestamp = Number(value);
  if (!Number.isFinite(timestamp) || timestamp <= 0) {
    return "--";
  }
  const date = new Date(timestamp);
  const pad = (item: number) => String(item).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

function sourceLabel(source: string | undefined) {
  if (source === "automatic") return "自动测活";
  if (source === "manual") return "单次测活";
  return source || "未知来源";
}

function cliLabel(record: LivenessRecord) {
  if (record.cliKind === "claudeCode") return "Claude Code CLI";
  if (record.cliKind === "codex") return "Codex CLI";
  const command = record.commandPreview.toLowerCase();
  if (command.includes("anthropic_api_key") || command.includes("claude")) {
    return "Claude Code CLI";
  }
  if (command.includes("openai_api_key") || command.includes("codex")) {
    return "Codex CLI";
  }
  return "未知方式";
}

function durationLabel(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return "--";
  }
  if (value < 1000) {
    return `${Math.round(value)} ms`;
  }
  return `${(value / 1000).toFixed(2)} s`;
}

function numberLabel(value: number | null | undefined) {
  if (value === null || value === undefined || !Number.isFinite(value)) {
    return "--";
  }
  return Math.round(value).toLocaleString();
}

function costLabel(value: number | null | undefined) {
  if (value === null || value === undefined || !Number.isFinite(value)) {
    return "--";
  }
  return `$${value.toFixed(6)}`;
}

function responseText(record: LivenessRecord) {
  return record.responseRaw?.trim() || record.responsePreview?.trim() || record.message || "--";
}
</script>

<template>
  <a-modal
    :visible="visible"
    modal-class="surface-modal liveness-details-modal"
    :footer="false"
    :width="860"
    unmount-on-close
    @update:visible="emit('update:visible', $event)"
  >
    <template #title>
      <div class="surface-modal-title liveness-details-title">
        <span class="surface-modal-title-icon"><icon-experiment /></span>
        <span class="surface-modal-title-copy">
          <strong>{{ modalTitle }}</strong>
        </span>
      </div>
    </template>
    <div class="liveness-details">
      <section class="liveness-summary">
        <div class="liveness-summary-stats">
          <div>
            <span>累计测活</span>
            <strong>{{ cumulative.runCount.toLocaleString() }} 次</strong>
          </div>
          <div>
            <span>累计 Token</span>
            <strong>{{ numberLabel(cumulative.totalTokens) }}</strong>
          </div>
          <div>
            <span>累计费用</span>
            <strong>{{ costLabel(cumulative.costUsd) }}</strong>
          </div>
          <div>
            <span>输入 / 输出</span>
            <strong>{{ numberLabel(cumulative.inputTokens) }} / {{ numberLabel(cumulative.outputTokens) }}</strong>
          </div>
        </div>
      </section>

      <a-empty v-if="records.length === 0" description="还没有测活记录" />
      <div v-else class="liveness-record-list">
        <section
          v-for="(record, index) in records"
          :key="`${record.checkedAt}-${index}`"
          class="liveness-record"
        >
          <div class="liveness-record-summary">
            <a-tag :color="record.ok ? 'green' : 'red'">
              {{ record.ok ? "成功" : "失败" }}
            </a-tag>
            <strong>{{ formatDateTime(record.checkedAt) }}</strong>
            <span>{{ sourceLabel(record.source) }}</span>
            <span>{{ cliLabel(record) }}</span>
            <span>{{ record.model || "--" }}</span>
            <span>{{ durationLabel(record.latencyMs) }}</span>
            <span>Token {{ numberLabel(record.totalTokens) }}</span>
          </div>

          <div class="liveness-record-grid">
            <div>
              <span>Base URL</span>
              <code>{{ record.baseUrl || "--" }}</code>
            </div>
            <div>
              <span>状态消息</span>
              <code>{{ record.message || "--" }}</code>
            </div>
            <div>
              <span>Token 用量</span>
              <code>
                输入 {{ numberLabel(record.inputTokens) }} /
                缓存 {{ numberLabel(record.cachedInputTokens) }} /
                输出 {{ numberLabel(record.outputTokens) }} /
                推理 {{ numberLabel(record.reasoningOutputTokens) }} /
                合计 {{ numberLabel(record.totalTokens) }}
              </code>
            </div>
            <div>
              <span>费用</span>
              <code>{{ costLabel(record.totalCostUsd) }}</code>
            </div>
          </div>

          <a-collapse :bordered="false" class="liveness-record-collapse">
            <a-collapse-item header="测活话术" key="prompt">
              <pre>{{ record.prompt || "--" }}</pre>
            </a-collapse-item>
            <a-collapse-item header="响应数据" key="response">
              <pre>{{ responseText(record) }}</pre>
            </a-collapse-item>
            <a-collapse-item header="完整命令" key="command">
              <pre>{{ record.commandPreview || "--" }}</pre>
            </a-collapse-item>
          </a-collapse>
        </section>
      </div>
    </div>
  </a-modal>
</template>

<style scoped>
.liveness-summary {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 14px;
  margin-bottom: 14px;
  border: 1px solid var(--color-border-2);
  border-radius: 8px;
  background: var(--color-fill-1);
}

.liveness-summary-stats {
  display: flex;
  flex-wrap: wrap;
  gap: 18px;
}

.liveness-summary-stats > div {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.liveness-summary-stats span {
  font-size: 12px;
  color: var(--color-text-3);
}

.liveness-summary-stats strong {
  font-size: 15px;
  color: var(--color-text-1);
}
</style>
