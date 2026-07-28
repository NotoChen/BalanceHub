<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { IconLoading, IconRefresh } from "@arco-design/web-vue/es/icon";
import { useProviderStore, type AppSettings } from "../../stores/providers";
import {
  applyCliEnvironmentProbeResult,
  availableTerminalOptions,
  availableTerminalResults,
  captureCliEnvironmentSettings,
} from "../../utils/cli-environment";
import TerminalBrandIcon from "../TerminalBrandIcon.vue";
import TerminalIconSelector from "../TerminalIconSelector.vue";
import SettingsDetectionGrid from "./SettingsDetectionGrid.vue";
import SettingsDetectionItem from "./SettingsDetectionItem.vue";

const props = defineProps<{
  settings: AppSettings;
}>();

const store = useProviderStore();
const probeError = ref("");

const terminals = computed(() => availableTerminalResults(store.cliEnvironmentProbe));
const terminalOptions = computed(() => availableTerminalOptions(store.cliEnvironmentProbe));

async function runProbe() {
  if (store.cliEnvironmentLoading) return;
  probeError.value = "";
  const settingsAtStart = captureCliEnvironmentSettings(props.settings);
  try {
    const result = await store.probeCliEnvironment();
    applyCliEnvironmentProbeResult(props.settings, result, settingsAtStart);
  } catch (error) {
    probeError.value = error instanceof Error ? error.message : String(error);
  }
}

watch(
  terminalOptions,
  (options) => {
    if (
      options.length > 0 &&
      !options.some((option) => option.value === props.settings.temporaryCliTerminalKind)
    ) {
      props.settings.temporaryCliTerminalKind = options[0].value;
    }
  },
  { immediate: true },
);

onMounted(() => {
  if (!store.cliEnvironmentProbe) void runProbe();
});

</script>

<template>
  <div class="settings-terminal-panel">
    <header class="settings-terminal-head">
      <span class="settings-terminal-mode">
        <IconLoading v-if="store.cliEnvironmentLoading" />
        <i v-else />
        自动检测
      </span>
      <span class="settings-terminal-summary">
        {{ terminals.length }} 个可用
        <a-tooltip content="重新扫描 Agent 与终端">
          <a-button
            shape="circle"
            size="mini"
            :loading="store.cliEnvironmentLoading"
            aria-label="重新扫描 Agent 与终端"
            @click="runProbe"
          >
            <template #icon><IconRefresh /></template>
          </a-button>
        </a-tooltip>
      </span>
    </header>

    <SettingsDetectionGrid v-if="terminals.length > 0">
      <SettingsDetectionItem
        v-for="terminal in terminals"
        :key="terminal.kind"
        state="ok"
        :name="terminal.name"
        :detail="terminal.version || '已检测'"
      >
        <template #icon>
          <TerminalBrandIcon :kind="terminal.kind" :name="terminal.name" :size="26" />
        </template>
      </SettingsDetectionItem>
    </SettingsDetectionGrid>
    <div v-else class="settings-terminal-empty">
      {{ store.cliEnvironmentLoading ? "正在扫描本机终端" : "未检测到可用终端" }}
    </div>

    <div v-if="terminalOptions.length > 0" class="settings-terminal-preference">
      <span>启动终端</span>
      <TerminalIconSelector
        v-model="settings.temporaryCliTerminalKind"
        :options="terminalOptions"
        :loading="store.cliEnvironmentLoading && !store.cliEnvironmentProbe"
      />
    </div>
    <div v-if="probeError" class="settings-terminal-error">{{ probeError }}</div>
  </div>
</template>

<style scoped>
.settings-terminal-panel {
  display: grid;
  min-width: 0;
  gap: 12px;
  padding: 12px 14px 14px;
}

.settings-terminal-head,
.settings-terminal-mode,
.settings-terminal-summary,
.settings-terminal-preference {
  display: flex;
  min-width: 0;
  align-items: center;
}

.settings-terminal-head {
  justify-content: space-between;
  color: var(--color-text-3);
  font-size: 11px;
}

.settings-terminal-mode {
  gap: 6px;
  color: var(--color-text-2);
  font-weight: 650;
}

.settings-terminal-summary {
  gap: 7px;
}

.settings-terminal-mode > i {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: rgb(var(--green-6));
}

.settings-terminal-mode > svg {
  color: rgb(var(--arcoblue-6));
  animation: terminal-probe-spin 0.9s linear infinite;
}

.settings-terminal-preference {
  width: 100%;
  max-width: 510px;
  gap: 10px;
}

.settings-terminal-preference {
  border-top: 1px solid var(--color-border-2);
  padding-top: 11px;
}

.settings-terminal-preference > span {
  flex: 0 0 auto;
  color: var(--color-text-2);
  font-size: 11px;
  font-weight: 650;
}

.settings-terminal-preference > :deep(.environment-icon-selector) {
  flex: 1;
}

.settings-terminal-empty {
  display: flex;
  min-height: 74px;
  align-items: center;
  justify-content: center;
  border: 1px dashed var(--color-border-2);
  border-radius: 6px;
  color: var(--color-text-3);
  font-size: 12px;
}

.settings-terminal-error {
  color: rgb(var(--red-6));
  font-size: 11px;
}

@keyframes terminal-probe-spin {
  to { transform: rotate(360deg); }
}

@media (max-width: 620px) {
  .settings-terminal-preference {
    align-items: stretch;
    flex-direction: column;
  }

}
</style>
