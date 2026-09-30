<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Message, Modal } from "@arco-design/web-vue";
import { open } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { IconDesktop, IconSearch } from "@arco-design/web-vue/es/icon";
import SettingsTerminalManager from "./SettingsTerminalManager.vue";
import { agentCliLabel } from "../../utils/cli-environment";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import { useSettingsStore } from "../../stores/settings";
import { useLatestRequest } from "../../composables/useLatestRequest";
import { withTimeout } from "../../utils/promise-timeout";
import type { AppSettings, CliSessionIndexStatus } from "../../stores/providers";

const props = defineProps<{
  settings: AppSettings;
}>();

const store = useCliRuntimeStore();
const savedSettings = useSettingsStore();
const sessionIndexStatus = ref<CliSessionIndexStatus | null>(null);
const indexRequest = useLatestRequest({ timeoutMs: 15_000, timeoutMessage: "读取会话索引状态超时，请重试" });
const sessionIndexStatusLoading = indexRequest.loading;
const sessionIndexError = indexRequest.error;
const clearingSessionIndex = ref(false);
let indexUpdatedUnlisten: UnlistenFn | null = null;
let disposed = false;

const sessionIndexUsageLabel = computed(() => {
  if (sessionIndexPending.value) return "等待保存";
  if (sessionIndexError.value) return "读取失败";
  const status = sessionIndexStatus.value;
  if (!status) return sessionIndexStatusLoading.value ? "读取中…" : "未读取";
  return `${formatBytes(status.sizeBytes)} / ${status.maxSizeMiB} MiB`;
});
const sessionIndexPending = computed(() =>
  props.settings.sessionIndexDirectory !== savedSettings.settings.sessionIndexDirectory
  || props.settings.sessionIndexEnabled !== savedSettings.settings.sessionIndexEnabled
  || props.settings.sessionIndexMaxSizeMiB !== savedSettings.settings.sessionIndexMaxSizeMiB,
);

watch(() => [
  savedSettings.settings.sessionIndexDirectory,
  savedSettings.settings.sessionIndexEnabled,
  savedSettings.settings.sessionIndexMaxSizeMiB,
], () => {
  sessionIndexStatus.value = null;
  void refreshSessionIndexStatus();
});

const sessionIndexDirectoryLabel = computed(() =>
  props.settings.sessionIndexDirectory.trim() || "系统缓存目录",
);

async function refreshSessionIndexStatus() {
  await indexRequest.run(() => store.getSessionIndexStatus(), (result) => { sessionIndexStatus.value = result; });
}

async function chooseSessionIndexDirectory() {
  try {
    const selected = await open({ directory: true, multiple: false, title: "选择会话索引存储位置" });
    if (!disposed && typeof selected === "string") props.settings.sessionIndexDirectory = selected;
  } catch (error) {
    if (!disposed) Message.error(error instanceof Error ? error.message : String(error));
  }
}

function resetSessionIndexDirectory() {
  props.settings.sessionIndexDirectory = "";
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
    onOk() { void clearSessionIndex(); },
  });
}

async function clearSessionIndex() {
  if (clearingSessionIndex.value || disposed || sessionIndexPending.value) return;
  clearingSessionIndex.value = true;
  try {
    await withTimeout(store.clearSessionIndex(), 15_000, "清理索引响应超时，请刷新状态核对");
    if (disposed) return;
    Message.success("会话索引已清理");
    void refreshSessionIndexStatus();
  } catch (error) {
    if (!disposed) Message.error(error instanceof Error ? error.message : String(error));
  } finally {
    clearingSessionIndex.value = false;
  }
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  if (value < 1024) return `${value} B`;
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
</script>

<template>
  <div class="settings-page settings-cli-page">

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
        <a-alert v-if="sessionIndexError" type="warning" show-icon>
          <div class="panel-load-error"><span>{{ sessionIndexError }}</span><a-button size="small" @click="refreshSessionIndexStatus">重试</a-button></div>
        </a-alert>
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
              :disabled="sessionIndexPending || sessionIndexStatusLoading || !sessionIndexStatus?.sizeBytes"
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


  </div>
</template>
