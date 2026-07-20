<script setup lang="ts">
import { computed } from "vue";
import type { LivenessRecord } from "../stores/providers";

const props = defineProps<{
  records: LivenessRecord[];
}>();

const dayMs = 24 * 60 * 60 * 1000;
const hourMs = 60 * 60 * 1000;

const livenessRecords = computed(() => {
  const now = Date.now();
  return props.records
    .filter((record) => record.source === "automatic")
    .filter((record) => {
      const checkedAt = Number(record.checkedAt);
      return Number.isFinite(checkedAt) && now - checkedAt >= 0 && now - checkedAt <= dayMs;
    })
    .slice(-24);
});

const livenessSlots = computed(() => {
  const slots: Array<LivenessRecord | null> = Array.from({ length: 24 }, () => null);
  const now = Date.now();

  for (const record of livenessRecords.value) {
    const checkedAt = Number(record.checkedAt);
    const hoursAgo = Math.floor((now - checkedAt) / hourMs);
    if (hoursAgo < 0 || hoursAgo >= 24) {
      continue;
    }

    const slotIndex = 23 - hoursAgo;
    const previous = slots[slotIndex];
    if (!previous || Number(previous.checkedAt) < checkedAt) {
      slots[slotIndex] = record;
    }
  }

  return slots;
});

const successfulCount = computed(() => livenessRecords.value.filter((record) => record.ok).length);

const latestRecord = computed(() => {
  return [...livenessRecords.value].sort((a, b) => Number(b.checkedAt) - Number(a.checkedAt))[0] || null;
});

const latestLatencyLabel = computed(() => {
  const latency = latestRecord.value?.latencyMs;
  return Number.isFinite(latency) ? `${latency} ms` : "";
});

const summaryLabel = computed(() => {
  if (livenessRecords.value.length === 0) {
    return "暂无记录";
  }
  return `${successfulCount.value}/${livenessRecords.value.length} 成功`;
});

const ariaLabel = computed(() => {
  const latest = latestLatencyLabel.value ? `，最近延迟 ${latestLatencyLabel.value}` : "";
  return `近24小时自动测活：${summaryLabel.value}${latest}`;
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
    :class="{ 'provider-liveness-timeline-empty': livenessRecords.length === 0 }"
  >
    <div class="provider-liveness-summary">
      <span>近24小时</span>
      <strong>{{ summaryLabel }}</strong>
      <span v-if="latestLatencyLabel" class="provider-liveness-latency">
        {{ latestLatencyLabel }}
      </span>
    </div>
    <div class="provider-liveness-strip" role="img" :aria-label="ariaLabel">
      <span
        v-for="(record, index) in livenessSlots"
        :key="record ? `${record.checkedAt}-${record.ok}` : `empty-${index}`"
        class="provider-liveness-segment"
        :class="[
          record
            ? record.ok
              ? 'provider-liveness-segment-ok'
              : 'provider-liveness-segment-error'
            : 'provider-liveness-segment-empty',
        ]"
        :title="record ? livenessPointTitle(record) : '该时段暂无测活记录'"
      />
    </div>
  </div>
</template>
