<script setup lang="ts">
import { computed } from "vue";
import type { AccountQuotaSample } from "../../stores/oauth-types";
const props = defineProps<{ samples: AccountQuotaSample[] }>();
/// The series is scaled to its own range instead of a fixed 0–100 axis: a move
/// from 4% to 9% matters to the user even though it is tiny on a full meter.
/// A flat series sits in the middle so it reads as "no movement", not "empty".
const points = computed(() => {
  const samples = props.samples;
  if (samples.length < 2) return "";
  const values = samples.map((sample) =>
    Math.min(100, Math.max(0, sample.usedPercent)),
  );
  const low = Math.min(...values);
  const high = Math.max(...values);
  const span = high - low;
  return values
    .map((value, index) => {
      const x = (index / (samples.length - 1)) * 100;
      const y = span ? 88 - ((value - low) / span) * 76 : 50;
      return `${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");
});
</script>

<template>
  <svg
    v-if="points"
    class="quota-spark"
    viewBox="0 0 100 100"
    preserveAspectRatio="none"
    aria-hidden="true"
    focusable="false"
  >
    <polygon class="quota-spark-area" :points="`0,100 ${points} 100,100`" />
    <polyline
      class="quota-spark-line"
      :points="points"
      vector-effect="non-scaling-stroke"
    />
  </svg>
</template>

<style scoped>
.quota-spark {
  display: block;
  width: 40px;
  height: 14px;
  overflow: visible;
  color: inherit;
}
.quota-spark-area {
  fill: currentColor;
  opacity: 0.16;
  stroke: none;
}
.quota-spark-line {
  fill: none;
  stroke: currentColor;
  stroke-width: 1.4;
  stroke-linecap: round;
  stroke-linejoin: round;
  opacity: 0.85;
}
</style>
