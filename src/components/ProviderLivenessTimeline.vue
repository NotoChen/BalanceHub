<script setup lang="ts">
import { computed } from "vue";
import type { LivenessRecord } from "../stores/providers";
import { livenessTimelineSlots } from "../utils/provider-liveness";

const props = defineProps<{
  records: LivenessRecord[];
}>();

/// 每一格是一次自动测活，最近的一次在最右端。次数不足 24 次时左侧留空，
/// 不按小时合并，避免「24 格只亮 2 格」和成功率计数对不上。
const livenessSlots = computed(() => livenessTimelineSlots(props.records ?? []));

const recentRecords = computed(
  () => livenessSlots.value.filter((record): record is LivenessRecord => record !== null),
);

const successfulCount = computed(() => recentRecords.value.filter((record) => record.ok).length);

/// 延迟是最近一次自动测活的耗时：单写一个「3125 ms」读者无法判断这是延迟、
/// 超时时间还是别的；加两个字说清它是什么，仍然只占同一行。
const latestLatencyLabel = computed(() => {
  const latest = recentRecords.value[recentRecords.value.length - 1];
  const latency = latest?.latencyMs;
  return Number.isFinite(latency) ? `延迟 ${latency} ms` : "";
});

const summaryLabel = computed(() => {
  if (recentRecords.value.length === 0) {
    return "暂无记录";
  }
  return `${successfulCount.value}/${recentRecords.value.length} 成功`;
});

const ariaLabel = computed(() => {
  const latest = latestLatencyLabel.value ? `，最近一次${latestLatencyLabel.value}` : "";
  return `最近24次自动测活：${summaryLabel.value}${latest}`;
});

function livenessPointTitle(record: LivenessRecord) {
  const date = new Date(Number(record.checkedAt));
  const time = Number.isNaN(date.getTime()) ? record.checkedAt : date.toLocaleString();
  const result = record.ok ? "成功" : "失败";
  return `${result}，${time}，${record.latencyMs} ms，${record.message}`;
}
</script>

<template>
  <div
    class="provider-liveness-timeline"
    :class="{ 'provider-liveness-timeline-empty': recentRecords.length === 0 }"
  >
    <div class="provider-liveness-summary">
      <span>最近24次</span>
      <strong>{{ summaryLabel }}</strong>
      <span v-if="latestLatencyLabel" class="provider-liveness-latency">
        {{ latestLatencyLabel }}
      </span>
    </div>
    <div class="provider-liveness-strip" role="img" :aria-label="ariaLabel">
      <span
        v-for="(record, index) in livenessSlots"
        :key="record ? `${record.checkedAt}-${index}` : `empty-${index}`"
        class="provider-liveness-segment"
        :class="[
          record
            ? record.ok
              ? 'provider-liveness-segment-ok'
              : 'provider-liveness-segment-error'
            : 'provider-liveness-segment-empty',
        ]"
        :title="record ? livenessPointTitle(record) : '暂无更早的自动测活记录'"
      />
    </div>
  </div>
</template>
