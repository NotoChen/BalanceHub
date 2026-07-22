<script setup lang="ts">
import { computed } from "vue";
import {
  useProviderStore,
  type CliToolProbeResult,
  type LivenessCliKind,
} from "../../stores/providers";
import BrandIcon, { type BrandIconName } from "../BrandIcon.vue";
import SettingsDetectionGrid from "./SettingsDetectionGrid.vue";
import SettingsDetectionItem from "./SettingsDetectionItem.vue";

const store = useProviderStore();

const CLI_KINDS: LivenessCliKind[] = ["codex", "claudeCode"];
const cliMeta: Record<LivenessCliKind, { label: string; brand: BrandIconName }> = {
  codex: { label: "Codex", brand: "codex" },
  claudeCode: { label: "Claude Code", brand: "claude" },
};

const probe = computed(() => store.cliEnvironmentProbe);
const detectedCount = computed(
  () => CLI_KINDS.filter((kind) => toolResult(kind)?.available).length,
);

function toolResult(kind: LivenessCliKind): CliToolProbeResult | null {
  if (!probe.value) return null;
  return kind === "codex" ? probe.value.codex : probe.value.claudeCode;
}

function itemState(available?: boolean) {
  if (store.cliEnvironmentLoading && !probe.value) return "checking";
  return available ? "ok" : "error";
}

function resultText(result: CliToolProbeResult | null) {
  if (store.cliEnvironmentLoading && !result) return "正在检测";
  if (result?.available) return result.version || "已检测";
  return result?.message || "未检测到";
}
</script>

<template>
  <div class="settings-detection-panel">
    <header class="settings-detection-head">
      <span class="settings-detection-mode">
        <IconLoading v-if="store.cliEnvironmentLoading && !probe" />
        <i v-else />
        自动检测
      </span>
      <span>{{ detectedCount }} / 2 可用</span>
    </header>

    <SettingsDetectionGrid>
      <SettingsDetectionItem
        v-for="kind in CLI_KINDS"
        :key="kind"
        :state="itemState(toolResult(kind)?.available)"
        :name="cliMeta[kind].label"
        :detail="resultText(toolResult(kind))"
      >
        <template #icon>
          <BrandIcon :brand="cliMeta[kind].brand" :size="26" />
        </template>
      </SettingsDetectionItem>
    </SettingsDetectionGrid>
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
.settings-detection-mode {
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

@keyframes cli-probe-spin {
  to { transform: rotate(360deg); }
}

</style>
