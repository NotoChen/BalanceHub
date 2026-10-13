<script setup lang="ts">
import { computed, onMounted, onScopeDispose, ref, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type {
  SessionTokenCounts,
  SessionUsageSummary,
} from "../../stores/session-usage-types";
import { withTimeout } from "../../utils/promise-timeout";
const props = defineProps<{ usage: SessionUsageSummary }>();
const estimate = ref<SessionUsageSummary | null>(null);
const estimating = ref(false);
const error = ref("");
const expanded = ref(false);
let revision = 0;
let disposed = false;
let unlisten: UnlistenFn | null = null;
const count = (value: number | null) =>
  value == null ? "未知" : value.toLocaleString();
const rows: { key: keyof SessionTokenCounts; label: string }[] = [
  { key: "inputTokens", label: "输入" },
  { key: "outputTokens", label: "输出" },
  { key: "cacheReadTokens", label: "缓存命中" },
  { key: "cacheWriteTokens", label: "缓存写入" },
  { key: "reasoningTokens", label: "推理" },
];
const price = computed(() =>
  estimate.value?.estimatedCostMicrousd == null
    ? "价格未知"
    : `$${(estimate.value.estimatedCostMicrousd / 1_000_000).toLocaleString("en-US", { maximumFractionDigits: 6 })}`,
);
async function estimateCost() {
  const current = ++revision;
  error.value = "";
  estimating.value = true;
  try {
    const result = await withTimeout(
      invoke<SessionUsageSummary>("estimate_session_usage", {
        usage: props.usage,
      }),
      35_000,
      "费用估算超时",
    );
    if (!disposed && current === revision) estimate.value = result;
  } catch {
    if (!disposed && current === revision) error.value = "模型价格暂不可用";
  } finally {
    if (current === revision) estimating.value = false;
  }
}
watch(
  () => props.usage,
  () => {
    revision += 1;
    estimating.value = false;
    estimate.value = null;
    error.value = "";
    if (expanded.value && props.usage.records) void estimateCost();
  },
);
function toggle(event: Event) {
  expanded.value = (event.currentTarget as HTMLDetailsElement).open;
  if (
    expanded.value &&
    !estimate.value &&
    !estimating.value &&
    props.usage.records
  )
    void estimateCost();
}
onMounted(async () => {
  try {
    const stop = await listen("model-metadata-updated", () => {
      if (expanded.value && !estimating.value && props.usage.records)
        void estimateCost();
    });
    if (disposed) stop();
    else unlisten = stop;
  } catch {
    /* No native event bus in isolated components. */
  }
});
onScopeDispose(() => {
  disposed = true;
  revision += 1;
  unlisten?.();
});
</script>
<template>
  <details class="session-usage" @toggle="toggle">
    <summary>
      <span>用量</span
      ><template v-if="usage.records"
        ><span>输入 {{ count(usage.tokens.inputTokens) }}</span
        ><span>输出 {{ count(usage.tokens.outputTokens) }}</span
        ><small v-if="usage.partial">统计未完整</small></template
      ><span v-else>日志未记录 token 用量</span>
    </summary>
    <div v-if="usage.records" class="session-usage-body">
      <dl>
        <div v-for="row in rows" :key="row.key">
          <dt>{{ row.label }}</dt>
          <dd>{{ count(usage.tokens[row.key]) }}</dd>
        </div>
      </dl>
      <p v-if="usage.partial">
        索引仍在读取或原生计数不完整，当前数据仅反映已确认的部分。
      </p>
      <p v-else-if="usage.nativeCumulative">
        使用原生日志累计值；重复的统计事件已合并。
      </p>
      <div v-if="usage.models.length > 1" class="session-usage-models">
        <div v-for="model in usage.models" :key="model.model ?? ''">
          <strong>{{ model.model || "未记录模型" }}</strong
          ><span
            >输入 {{ count(model.tokens.inputTokens) }} · 输出
            {{ count(model.tokens.outputTokens) }}</span
          >
        </div>
      </div>
      <p>输入含缓存 token，输出含推理 token。未记录的字段显示为未知。</p>
      <p class="session-usage-price">
        {{
          estimating
            ? "正在读取参考价格…"
            : error ||
              `${estimate?.unpricedRecords ? "已知部分参考估算" : "参考估算"} ${price}`
        }}<span v-if="estimate?.unpricedRecords">
          · {{ estimate.unpricedRecords }} 条用量缺少价格资料</span
        ><a-button v-if="error" type="text" size="mini" @click="estimateCost"
          >重试</a-button
        >
      </p>
      <p>
        按 models.dev
        公开价格估算，未记录缓存时按普通输入计算；实际费用以中转站账单为准。
      </p>
    </div>
  </details>
</template>
<style scoped>
.session-usage {
  margin: 0 18px 8px;
  border-bottom: 1px solid var(--color-border-2);
  color: var(--color-text-2);
  font-size: 12px;
}
summary {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px 16px;
  padding: 8px 0;
  cursor: pointer;
}
summary::before {
  content: "›";
  transition: transform 0.15s;
}
details[open] > summary::before {
  transform: rotate(90deg);
}
summary small {
  color: rgb(var(--warning-6));
}
.session-usage-body {
  padding: 2px 0 12px;
  max-height: 260px;
  overflow: auto;
}
dl {
  display: flex;
  gap: 10px 24px;
  flex-wrap: wrap;
  margin: 6px 0 10px;
}
dt {
  color: var(--color-text-3);
}
dd {
  margin: 4px 0 0;
  font-variant-numeric: tabular-nums;
  color: var(--color-text-1);
}
p {
  margin: 6px 0;
  color: var(--color-text-3);
  font-size: 11px;
}
.session-usage-price {
  color: var(--color-text-1);
}
.session-usage-models > div {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 18px;
  margin: 6px 0;
}
.session-usage-models strong {
  font-weight: 500;
  overflow-wrap: anywhere;
}
</style>
