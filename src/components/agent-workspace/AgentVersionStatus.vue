<script setup lang="ts">
import { computed } from "vue";
import { RefreshCw } from "@lucide/vue";
import type { AgentLifecycleVersion } from "../../stores/agent-lifecycle-types";
import { agentLifecycleVersionLabels } from "../../utils/agent-catalog-display";

const props = defineProps<{ version: AgentLifecycleVersion | null; compact?: boolean; checking?: boolean; error?: string }>();
const state = computed(() => props.error ? "checkFailed" : props.version?.state ?? "notChecked");
const checking = computed(() => props.checking && state.value !== "unsupported");
const label = computed(() => checking.value && !props.version?.checkedAt ? "检查中" : agentLifecycleVersionLabels[state.value]);
const latestLabel = computed(() => props.error || props.version?.stale ? "上次查到" : "最新版本");
function formatTime(value: string | null | undefined) {
  if (!value) return "";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleString("zh-CN", { hour12: false });
}
const checkedAt = computed(() => formatTime(props.version?.checkedAt));
const lastSuccessAt = computed(() => props.version?.lastSuccessAt !== props.version?.checkedAt ? formatTime(props.version?.lastSuccessAt) : "");
const detail = computed(() => props.error || props.version?.message || "");
const tooltip = computed(() => [
  label.value,
  props.version?.latestVersion ? `${latestLabel.value} ${props.version.latestVersion}` : "",
  checkedAt.value ? `最近检查 ${checkedAt.value}` : "",
  lastSuccessAt.value ? `上次成功 ${lastSuccessAt.value}` : "",
  detail.value,
].filter(Boolean).join("；"));
</script>

<template>
  <span class="agent-version-status" :class="{ compact }" :title="tooltip" :aria-busy="checking || undefined">
    <span class="agent-version-state" :class="`is-${state}`" role="status"><RefreshCw v-if="checking" :size="12" class="agent-version-loading" aria-hidden="true" />{{ label }}</span>
    <template v-if="!compact">
      <span v-if="version?.latestVersion" class="agent-version-latest">{{ latestLabel }} {{ version.latestVersion }}</span>
      <span v-if="checkedAt" class="agent-version-checked">最近检查 {{ checkedAt }}<template v-if="lastSuccessAt"> · 上次成功 {{ lastSuccessAt }}</template></span>
      <span v-if="detail" class="agent-version-message">{{ detail }}</span>
    </template>
  </span>
</template>

<style scoped>
.agent-version-status { display: flex; min-width: 0; flex-wrap: wrap; align-items: center; gap: 4px 10px; color: var(--color-text-3); font-size: 12px; line-height: 1.6; font-variant-numeric: tabular-nums; }
.agent-version-status.compact { display: inline-flex; flex-wrap: nowrap; font-size: 11px; line-height: 18px; }
.agent-version-state { display: inline-flex; align-items: center; gap: 4px; white-space: nowrap; }
.agent-version-state.is-upToDate { color: var(--surface-success); }
.agent-version-state.is-updateAvailable { color: var(--surface-accent); }
.agent-version-state.is-checkFailed { color: var(--surface-warning); }
.compact .agent-version-state.is-updateAvailable { border-radius: 4px; padding: 3px 7px; background: var(--surface-accent-soft); font-weight: 500; }
.agent-version-checked, .agent-version-message { flex-basis: 100%; font-size: 11px; overflow-wrap: anywhere; }
.agent-version-loading { flex: 0 0 auto; animation: agent-version-spin 1s linear infinite; }
@keyframes agent-version-spin { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .agent-version-loading { animation: none; } }
</style>
