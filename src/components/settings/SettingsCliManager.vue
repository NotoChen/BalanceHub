<script setup lang="ts">
import { computed, ref } from "vue";
import { IconLoading, IconRefresh } from "@arco-design/web-vue/es/icon";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import type { AppSettings, CliToolProbeResult } from "../../stores/providers";
import {
  agentCliVersionLabel,
  applyCliEnvironmentProbeResult,
  availableCliKinds,
  captureCliEnvironmentSettings,
} from "../../utils/cli-environment";
import AgentCliIcon from "../AgentCliIcon.vue";
import SettingsDetectionGrid from "./SettingsDetectionGrid.vue";
import SettingsDetectionItem from "./SettingsDetectionItem.vue";

const store = useCliRuntimeStore();
const props = defineProps<{
  settings: AppSettings;
}>();

const probe = computed(() => store.cliEnvironmentProbe);
const registeredTools = computed(() => probe.value?.tools || []);
const detectedKinds = computed(() => availableCliKinds(probe.value));
const probeError = ref("");

function itemState(result: CliToolProbeResult | null) {
  if (store.cliEnvironmentLoading) return "checking";
  return result?.available ? "ok" : "error";
}

function resultText(result: CliToolProbeResult | null) {
  if (store.cliEnvironmentLoading) {
    return result ? "正在重新检测" : "正在检测";
  }
  if (!result) return "尚未扫描";
  if (!result.available) return result.message || "未检测到可用 CLI";
  return agentCliVersionLabel(result.version) || "已检测";
}

function resultTooltip(result: CliToolProbeResult | null) {
  if (!result) return "尚未扫描";
  if (!result.available) return result.message || "未检测到可用 CLI";
  return [result.version.trim(), result.path.trim()].filter(Boolean).join("\n") || "已检测";
}

async function runProbe() {
  if (store.cliEnvironmentLoading) return;
  probeError.value = "";
  const settingsAtStart = captureCliEnvironmentSettings(props.settings);
  try {
    const result = await store.probeCliTools(true);
    applyCliEnvironmentProbeResult(props.settings, result, settingsAtStart);
  } catch (error) {
    probeError.value = error instanceof Error ? error.message : String(error);
  }
}
</script>

<template>
  <div class="settings-detection-panel">
    <header class="settings-detection-head">
      <span class="settings-detection-mode">
        <IconLoading v-if="store.cliEnvironmentLoading" />
        <i v-else />
        自动检测
      </span>
      <span class="settings-detection-summary">
        {{ detectedKinds.length }} 个可用
        <a-tooltip content="重新扫描 Agent">
          <a-button
            shape="circle"
            size="mini"
            :loading="store.cliEnvironmentLoading"
            aria-label="重新扫描 Agent"
            @click="runProbe"
          >
            <template #icon><IconRefresh /></template>
          </a-button>
        </a-tooltip>
      </span>
    </header>

    <SettingsDetectionGrid wide>
      <SettingsDetectionItem
        v-for="tool in registeredTools"
        :key="tool.kind"
        :state="itemState(tool)"
        :name="tool.label"
        :detail="resultText(tool)"
        :tooltip="resultTooltip(tool)"
        compact
      >
        <template #icon>
          <AgentCliIcon :kind="tool.kind" :size="26" />
        </template>
      </SettingsDetectionItem>
    </SettingsDetectionGrid>
    <div v-if="probeError" class="settings-detection-error">{{ probeError }}</div>
  </div>
</template>

<style scoped>
.settings-detection-panel {
  display: grid;
  min-width: 0;
  gap: 12px;
  padding: 12px 14px 14px;
}

.settings-detection-head,
.settings-detection-mode,
.settings-detection-summary {
  display: flex;
  min-width: 0;
  align-items: center;
}

.settings-detection-head {
  justify-content: space-between;
  color: var(--color-text-3);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}

.settings-detection-mode {
  gap: 6px;
  color: var(--color-text-2);
  font-weight: 650;
}

.settings-detection-summary {
  gap: 7px;
}

.settings-detection-mode > i {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: rgb(var(--green-6));
}

.settings-detection-mode > svg {
  color: rgb(var(--arcoblue-6));
  animation: cli-probe-spin 0.9s linear infinite;
}

.settings-detection-error {
  color: rgb(var(--red-6));
  font-size: 11px;
}

@keyframes cli-probe-spin {
  to { transform: rotate(360deg); }
}

</style>
