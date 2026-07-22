<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { IconLoading } from "@arco-design/web-vue/es/icon";
import { hostPlatform } from "../../api/app";
import { useProviderStore, type AppSettings, type TemporaryCliTerminalKind } from "../../stores/providers";
import { temporaryCliTerminalOptionsForPlatform } from "../../utils/liveness-options";
import TerminalBrandIcon from "../TerminalBrandIcon.vue";
import SettingsDetectionGrid from "./SettingsDetectionGrid.vue";
import SettingsDetectionItem from "./SettingsDetectionItem.vue";

const props = defineProps<{
  settings: AppSettings;
}>();

const store = useProviderStore();
const platform = ref("macos");
const probeError = ref("");
let probeTimer: ReturnType<typeof setTimeout> | null = null;

const terminal = computed(() => store.cliEnvironmentProbe?.terminal ?? null);
const terminalOptions = computed(() => temporaryCliTerminalOptionsForPlatform(platform.value));
const terminalIconKind = computed<TemporaryCliTerminalKind>(() =>
  terminal.value?.available ? terminal.value.kind : props.settings.temporaryCliTerminalKind,
);
const terminalName = computed(() => {
  if (terminal.value?.name) return terminal.value.name;
  return (
    terminalOptions.value.find((option) => option.value === props.settings.temporaryCliTerminalKind)
      ?.label ?? "终端"
  );
});
const terminalStateLabel = computed(() => {
  if (store.cliEnvironmentLoading) return "检测中";
  return terminal.value?.available ? "可用" : "未检测到";
});

function itemState() {
  if (store.cliEnvironmentLoading) return "checking";
  return terminal.value?.available ? "ok" : "error";
}

function resultText() {
  if (store.cliEnvironmentLoading) return "正在检测";
  if (terminal.value?.available) return terminal.value.version || "已检测";
  return terminal.value?.message || "未检测到";
}

async function runProbe() {
  if (store.cliEnvironmentLoading) return;
  probeError.value = "";
  try {
    await store.probeCliEnvironment(
      props.settings.temporaryCliTerminalKind,
      props.settings.temporaryCliTerminalCommand,
    );
  } catch (error) {
    probeError.value = error instanceof Error ? error.message : String(error);
  }
}

function scheduleProbe() {
  if (probeTimer) clearTimeout(probeTimer);
  probeTimer = setTimeout(() => {
    probeTimer = null;
    void runProbe();
  }, 350);
}

watch(
  () => [props.settings.temporaryCliTerminalKind, props.settings.temporaryCliTerminalCommand],
  scheduleProbe,
);

onMounted(async () => {
  try {
    platform.value = await hostPlatform();
  } catch {
    platform.value = "macos";
  }
  if (!store.cliEnvironmentProbe) void runProbe();
});

onUnmounted(() => {
  if (probeTimer) clearTimeout(probeTimer);
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
      <span>{{ terminalStateLabel }}</span>
    </header>

    <SettingsDetectionGrid>
      <SettingsDetectionItem
        :state="itemState()"
        :name="terminalName"
        :detail="resultText()"
      >
        <template #icon>
          <TerminalBrandIcon :kind="terminalIconKind" :name="terminalName" :size="26" />
        </template>
      </SettingsDetectionItem>
    </SettingsDetectionGrid>

    <div class="settings-terminal-preference">
      <span>启动终端</span>
      <a-select
        v-model="settings.temporaryCliTerminalKind"
        :options="terminalOptions"
        size="small"
      />
      <a-input
        v-if="settings.temporaryCliTerminalKind === 'custom'"
        v-model="settings.temporaryCliTerminalCommand"
        size="small"
        placeholder="输入终端启动命令"
        allow-clear
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
  border-top: 1px solid var(--color-border-2);
  padding-top: 11px;
}

.settings-terminal-preference > span {
  flex: 0 0 auto;
  color: var(--color-text-2);
  font-size: 11px;
  font-weight: 650;
}

.settings-terminal-preference > .arco-select {
  width: 180px;
  flex: 0 0 auto;
}

.settings-terminal-preference > .arco-input-wrapper {
  min-width: 180px;
  flex: 1;
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

  .settings-terminal-preference > .arco-select,
  .settings-terminal-preference > .arco-input-wrapper {
    width: 100%;
  }
}
</style>
