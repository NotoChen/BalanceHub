<script setup lang="ts">
import { computed, type CSSProperties } from "vue";
import type { LivenessRecord } from "../stores/providers";

const props = defineProps<{
  records: LivenessRecord[];
}>();

const livenessRecords = computed(() => {
  const now = Date.now();
  const dayMs = 24 * 60 * 60 * 1000;
  return props.records
    .filter((record) => record.source === "automatic")
    .filter((record) => {
      const time = Number(record.checkedAt);
      return Number.isFinite(time) && now - time >= 0 && now - time <= dayMs;
    })
    .slice(-24);
});

const livenessHourTicks = computed(() => {
  const now = new Date();
  return Array.from({ length: 8 }, (_, index) => index * 3).map((hoursAgo) => {
    const date = new Date(now.getTime() - hoursAgo * 60 * 60 * 1000);
    return {
      label: String(date.getHours()),
      style: livenessTickStyle(hoursAgo),
    };
  });
});

const livenessHourMarks = computed(() =>
  Array.from({ length: 25 }, (_, hoursAgo) => ({
    style: livenessTickStyle(hoursAgo),
  })),
);

const livenessAgeTicks = computed(() =>
  [1, 4, 7, 10, 13, 16, 19, 22].map((hoursAgo) => ({
    label: `${hoursAgo}h前`,
    style: livenessTickStyle(hoursAgo),
  })),
);

function livenessPointTitle(record: LivenessRecord) {
  const date = new Date(Number(record.checkedAt));
  const time = Number.isNaN(date.getTime()) ? record.checkedAt : date.toLocaleString();
  return `${record.ok ? "成功" : "失败"} · ${time} · ${record.latencyMs}ms · ${record.message}`;
}

function livenessPointStyle(index: number) {
  const checkedAt = Number(livenessRecords.value[index]?.checkedAt);
  const ageMs = Date.now() - checkedAt;
  const left = Number.isFinite(ageMs)
    ? Math.max(0, Math.min(100, 100 - (ageMs / (24 * 60 * 60 * 1000)) * 100))
    : 100;
  return { "--point-left": `${left}%` } as CSSProperties;
}

function livenessTickStyle(hoursAgo: number) {
  return { "--tick-left": `${100 - (hoursAgo / 24) * 100}%` } as CSSProperties;
}
</script>

<template>
  <div
    class="provider-liveness-timeline"
    :class="{ 'provider-liveness-timeline-empty': livenessRecords.length === 0 }"
  >
    <span
      v-for="(tick, index) in livenessHourMarks"
      :key="`mark-${index}`"
      class="provider-liveness-hour-mark"
      :style="tick.style"
    />
    <span
      v-for="(tick, index) in livenessAgeTicks"
      :key="`age-${index}-${tick.label}`"
      class="provider-liveness-tick provider-liveness-age-tick"
      :style="tick.style"
    >
      {{ tick.label }}
    </span>
    <span
      v-for="(tick, index) in livenessHourTicks"
      :key="`hour-${index}-${tick.label}`"
      class="provider-liveness-tick provider-liveness-hour-tick"
      :style="tick.style"
    >
      {{ tick.label }}
    </span>
    <template v-if="livenessRecords.length > 0">
      <span
        v-for="(record, index) in livenessRecords"
        :key="`${record.checkedAt}-${record.ok}`"
        class="provider-liveness-node"
        :class="[
          record.ok ? 'provider-liveness-node-ok' : 'provider-liveness-node-error',
          index % 2 === 0 ? 'provider-liveness-node-top' : 'provider-liveness-node-bottom',
        ]"
        :style="livenessPointStyle(index)"
        :title="livenessPointTitle(record)"
      >
        <span class="provider-liveness-point" />
      </span>
    </template>
  </div>
</template>
