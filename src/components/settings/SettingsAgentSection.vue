<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import { open } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { IconCommand, IconDesktop, IconExperiment, IconSearch } from "@arco-design/web-vue/es/icon";
import CliIconSelector from "../CliIconSelector.vue";
import SettingsAgentPromptSection from "./SettingsAgentPromptSection.vue";
import SettingsCliManager from "./SettingsCliManager.vue";
import SettingsTerminalManager from "./SettingsTerminalManager.vue";
import { agentCliLabel, availableCliOptions } from "../../utils/cli-environment";
import { MIN_LIVENESS_INTERVAL_SECONDS } from "../../utils/liveness-defaults";
import { livenessIntervalModeOptions } from "../../utils/liveness-options";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import type { AppSettings, CliSessionIndexStatus } from "../../stores/providers";

const props = defineProps<{
  settings: AppSettings;
  expanded?: boolean;
  livenessModelOptions: string[];
  selectedLivenessModelProviders: { id: string; name: string }[];
}>();

const store = useCliRuntimeStore();
const sessionIndexStatus = ref<CliSessionIndexStatus | null>(null);
const sessionIndexStatusLoading = ref(false);
const clearingSessionIndex = ref(false);
let indexUpdatedUnlisten: UnlistenFn | null = null;
let disposed = false;

const sessionIndexUsageLabel = computed(() => {
  const status = sessionIndexStatus.value;
  if (!status) return "等待读取";
  return `${formatBytes(status.sizeBytes)} / ${status.maxSizeMiB} MiB`;
});

const sessionIndexDirectoryLabel = computed(() =>
  props.settings.sessionIndexDirectory.trim() || "系统缓存目录",
);

async function refreshSessionIndexStatus() {
  sessionIndexStatusLoading.value = true;
  try {
    sessionIndexStatus.value = await store.getSessionIndexStatus();
  } catch (error) {
    Message.error(error instanceof Error ? error.message : String(error));
  } finally {
    sessionIndexStatusLoading.value = false;
  }
}

async function chooseSessionIndexDirectory() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择会话索引存储位置",
  });
  if (typeof selected === "string") {
    props.settings.sessionIndexDirectory = selected;
    window.setTimeout(() => void refreshSessionIndexStatus(), 420);
  }
}

function resetSessionIndexDirectory() {
  props.settings.sessionIndexDirectory = "";
  window.setTimeout(() => void refreshSessionIndexStatus(), 420);
}

const sessionIndexAgentStats = computed(() =>
  (sessionIndexStatus.value?.agents ?? []).filter(
    (item) => item.sessionCount > 0 || item.sizeBytes > 0,
  ),
);

function sessionIndexAgentLabel(kind: CliSessionIndexStatus["agents"][number]["cliKind"]) {
  return store.cliRuntime.agents.find((agent) => agent.kind === kind)?.label
    ?? agentCliLabel(store.cliEnvironmentProbe, kind);
}

function confirmClearSessionIndex() {
  Modal.confirm({
    title: "清理会话索引",
    content: "只删除 BalanceHub 生成的可再生索引，不会修改任何 Agent 的原始会话。下次搜索会在后台重建。",
    okText: "清理索引",
    cancelText: "取消",
    async onOk() {
      clearingSessionIndex.value = true;
      try {
        await store.clearSessionIndex();
        Message.success("会话索引已清理");
        await refreshSessionIndexStatus();
      } catch (error) {
        Message.error(error instanceof Error ? error.message : String(error));
        throw error;
      } finally {
        clearingSessionIndex.value = false;
      }
    },
  });
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / 1024 / 1024).toFixed(1)} MiB`;
}

onMounted(async () => {
  disposed = false;
  void refreshSessionIndexStatus();
  try {
    const unlisten = await listen("cli-session-index-updated", () => {
      void refreshSessionIndexStatus();
    });
    if (disposed) {
      unlisten();
      return;
    }
    indexUpdatedUnlisten = unlisten;
  } catch {
    // Browser preview has no Tauri event bus.
  }
});

onUnmounted(() => {
  disposed = true;
  indexUpdatedUnlisten?.();
  indexUpdatedUnlisten = null;
});
const cliOptions = computed(() => availableCliOptions(store.cliEnvironmentProbe, "liveness"));

const livenessModelSelectOptions = computed(() =>
  Array.from(
    new Set(
      [props.settings.livenessModel.trim(), ...props.livenessModelOptions.map((model) => model.trim())].filter(
        Boolean,
      ),
    ),
  ).map((model) => ({ label: model, value: model })),
);

const minimumRandomMaxInterval = computed(() =>
  Math.max(MIN_LIVENESS_INTERVAL_SECONDS, Number(props.settings.livenessRandomMinInterval) || 0),
);
</script>

<template>
  <div class="settings-page settings-cli-page">
    <section class="settings-card settings-cli-card">
      <header class="settings-card-header">
        <span class="settings-card-icon"><IconCommand /></span>
        <div><strong>Agent</strong></div>
      </header>
      <SettingsCliManager :settings="settings" />
    </section>

    <section class="settings-card settings-terminal-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-amber"><IconDesktop /></span>
        <div><strong>终端</strong></div>
      </header>
      <SettingsTerminalManager :settings="settings" />
    </section>

    <section class="settings-card settings-session-index-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-green"><IconSearch /></span>
        <div><strong>会话索引</strong></div>
        <span class="settings-card-state" :class="{ active: settings.sessionIndexEnabled }">
          {{ settings.sessionIndexEnabled ? sessionIndexUsageLabel : "已关闭" }}
        </span>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row">
          <div class="settings-setting-copy">
            <strong>启用历史会话全文检索</strong>
            <span>仅索引用户输入与 Agent 可见回复，工具调用和输出不进入索引。</span>
          </div>
          <a-switch v-model="settings.sessionIndexEnabled" />
        </div>
      </div>

      <div v-if="settings.sessionIndexEnabled" class="settings-session-index-config">
        <a-form-item label="存储位置">
          <div class="settings-session-index-path">
            <span :title="sessionIndexStatus?.directory || sessionIndexDirectoryLabel">
              {{ sessionIndexDirectoryLabel }}
            </span>
            <a-space>
              <a-button size="small" @click="chooseSessionIndexDirectory">选择目录</a-button>
              <a-button
                v-if="settings.sessionIndexDirectory"
                size="small"
                type="text"
                @click="resetSessionIndexDirectory"
              >
                恢复默认
              </a-button>
            </a-space>
          </div>
        </a-form-item>

        <div class="settings-session-index-controls">
          <a-form-item label="总容量上限（MiB）">
            <a-input-number
              v-model="settings.sessionIndexMaxSizeMiB"
              :min="8"
              :max="4096"
              :step="8"
            />
          </a-form-item>
          <div class="settings-session-index-actions">
            <span v-if="sessionIndexStatus">
              {{ sessionIndexStatus.agents.reduce((sum, item) => sum + item.sessionCount, 0) }} 个会话 · {{ formatBytes(sessionIndexStatus.sizeBytes) }}
            </span>
            <a-button
              size="small"
              status="danger"
              :loading="clearingSessionIndex"
              :disabled="sessionIndexStatusLoading || !sessionIndexStatus?.sizeBytes"
              @click="confirmClearSessionIndex"
            >
              清理索引
            </a-button>
          </div>
        </div>
        <div v-if="sessionIndexAgentStats.length > 0" class="settings-session-index-agent-stats">
          <span v-for="item in sessionIndexAgentStats" :key="item.cliKind">
            <strong>{{ sessionIndexAgentLabel(item.cliKind) }}</strong>
            {{ item.sessionCount }} 个 · {{ formatBytes(item.sizeBytes) }}
          </span>
        </div>
      </div>
    </section>

    <section class="settings-card settings-liveness-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-green"><IconExperiment /></span>
        <div><strong>自动测活</strong></div>
        <span class="settings-card-state" :class="{ active: settings.livenessEnabled }">
          {{ settings.livenessEnabled ? "运行中" : "已关闭" }}
        </span>
      </header>

      <div class="settings-setting-list">
        <div class="settings-setting-row">
          <div class="settings-setting-copy"><strong>启用自动测活</strong></div>
          <a-switch v-model="settings.livenessEnabled" />
        </div>
      </div>

      <div v-if="settings.livenessEnabled" class="settings-liveness-config">
        <div class="settings-field-grid">
          <a-form-item label="执行 Agent">
            <CliIconSelector
              v-model="settings.livenessCliKind"
              :options="cliOptions"
              :loading="store.cliEnvironmentLoading && !store.cliEnvironmentProbe"
            />
          </a-form-item>
          <a-form-item label="默认模型">
            <a-select
              v-model="settings.livenessModel"
              :options="livenessModelSelectOptions"
              allow-create
              allow-search
              placeholder="选择或输入模型"
            />
          </a-form-item>
        </div>

        <div v-if="selectedLivenessModelProviders.length > 0" class="model-support-tags">
          <span>支持当前模型</span>
          <a-tag
            v-for="provider in selectedLivenessModelProviders"
            :key="`${settings.livenessModel}-${provider.id}`"
            color="blue"
          >
            {{ provider.name }}
          </a-tag>
        </div>

        <div class="settings-field-grid settings-field-grid-three settings-liveness-timing-grid">
          <a-form-item label="周期策略">
            <a-select
              v-model="settings.livenessIntervalMode"
              :options="livenessIntervalModeOptions"
            />
          </a-form-item>
          <a-form-item v-if="settings.livenessIntervalMode === 'fixed'" label="执行周期（秒）">
            <a-input-number
              v-model="settings.livenessInterval"
              :min="MIN_LIVENESS_INTERVAL_SECONDS"
              :step="1"
            />
          </a-form-item>
          <template v-else>
            <a-form-item label="最短周期（秒）">
              <a-input-number
                v-model="settings.livenessRandomMinInterval"
                :min="MIN_LIVENESS_INTERVAL_SECONDS"
                :step="1"
              />
            </a-form-item>
            <a-form-item label="最长周期（秒）">
              <a-input-number
                v-model="settings.livenessRandomMaxInterval"
                :min="minimumRandomMaxInterval"
                :step="1"
              />
            </a-form-item>
          </template>
          <a-form-item label="超时（秒）">
            <a-input-number v-model="settings.livenessTimeout" :min="10" :max="600" :step="5" />
          </a-form-item>
        </div>

        <div class="settings-liveness-prompt">
          <SettingsAgentPromptSection :settings="settings" />
        </div>
      </div>
    </section>
  </div>
</template>
