<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from "vue";
import { ChevronDown } from "@lucide/vue";
import { Message } from "@arco-design/web-vue";
import type { Provider } from "../../stores/provider-types";
import type {
  CpaGatewaySettings,
  CpaGatewaySettingsUpdate,
} from "../../stores/oauth-types";
import {
  getCpaGatewaySettings,
  updateCpaGatewaySettings,
} from "../../api/oauth-accounts";
import CpaConfigDialog from "./CpaConfigDialog.vue";

const props = defineProps<{ provider: Provider; revision: number }>();

const settings = ref<CpaGatewaySettings | null>(null);
const settingsOpen = ref(false);
/// The document editor is the escape hatch for values this section does not
/// model, so it is opened from here rather than from a separate page.
const configOpen = ref(false);
const settingsLoading = ref(false);
const settingsSaving = ref(false);
const settingsError = ref("");
interface SettingsDraft {
  routingStrategy: string;
  proxyUrl: string;
  requestRetry?: number;
  maxRetryCredentials?: number;
  maxRetryInterval?: number;
  /// The gateway always reports these switches, so a draft keeps them boolean
  /// and a value it did not report reads as off.
  switchProject: boolean;
  switchPreviewModel: boolean;
  requestLog: boolean;
  usageStatisticsEnabled: boolean;
  debug: boolean;
  forceModelPrefix: boolean;
  wsAuth: boolean;
  logsMaxTotalSizeMb?: number;
  errorLogsMaxFiles?: number;
}
/// Editing happens in a draft so a half-typed proxy URL or retry count never
/// reaches the gateway until the whole change is saved.
const settingsDraft = ref<SettingsDraft | null>(null);
let settingsRequest = 0;
let disposed = false;
onScopeDispose(() => {
  disposed = true;
});
/// The three strategies CPA accepts, spelled the way its config stores them.
const routingStrategies = [
  {
    value: "round-robin",
    label: "轮询",
    hint: "按顺序轮流使用各授权，请求分布最均匀",
  },
  {
    value: "weighted-round-robin",
    label: "加权轮询",
    hint: "按各授权配置的权重分配请求量",
  },
  {
    value: "fill-first",
    label: "优先填满",
    hint: "先用满一个授权，再切换到下一个",
  },
];
/// The selector edits a draft, so the hint follows the pending choice while
/// the collapsed summary keeps the value the gateway has confirmed.
const routingStrategy = computed(
  () =>
    settingsDraft.value?.routingStrategy ||
    settings.value?.routingStrategy ||
    "round-robin",
);
const routingHint = computed(
  () =>
    routingStrategies.find((item) => item.value === routingStrategy.value)
      ?.hint || "",
);
/// An unset strategy means CPA is already doing round-robin, so the summary
/// names the effective value instead of leaving the line blank.
const routingSummary = computed(() => {
  if (settingsLoading.value) return "读取网关设置…";
  if (!settings.value) return "查看路由策略与重试设置";
  const label =
    routingStrategies.find(
      (item) =>
        item.value === (settings.value?.routingStrategy || "round-robin"),
    )?.label || settings.value.routingStrategy;
  return `${label} · 请求重试 ${settings.value.requestRetry ?? "网关默认"}`;
});
/// Only fields that differ from the confirmed settings are sent, and a value
/// the user cleared is left untouched: CPA has no "unset" write for an
/// integer, and sending 0 would mean "no retries" instead of "default".
const settingsPatch = computed<CpaGatewaySettingsUpdate>(() => {
  const draft = settingsDraft.value;
  const saved = settings.value;
  if (!draft || !saved) return {};
  const patch: CpaGatewaySettingsUpdate = {};
  if (draft.routingStrategy !== (saved.routingStrategy || "round-robin")) {
    patch.routingStrategy = draft.routingStrategy;
  }
  if (draft.proxyUrl.trim() !== saved.proxyUrl) {
    patch.proxyUrl = draft.proxyUrl.trim();
  }
  if (
    draft.requestRetry !== undefined &&
    draft.requestRetry !== saved.requestRetry
  ) {
    patch.requestRetry = draft.requestRetry;
  }
  if (
    draft.maxRetryCredentials !== undefined &&
    draft.maxRetryCredentials !== saved.maxRetryCredentials
  ) {
    patch.maxRetryCredentials = draft.maxRetryCredentials;
  }
  if (
    draft.maxRetryInterval !== undefined &&
    draft.maxRetryInterval !== saved.maxRetryInterval
  ) {
    patch.maxRetryInterval = draft.maxRetryInterval;
  }
  for (const key of [
    "switchProject",
    "switchPreviewModel",
    "requestLog",
    "usageStatisticsEnabled",
    "debug",
    "forceModelPrefix",
    "wsAuth",
  ] as const) {
    if (draft[key] !== (saved[key] ?? false)) patch[key] = draft[key];
  }
  for (const key of ["logsMaxTotalSizeMb", "errorLogsMaxFiles"] as const) {
    if (draft[key] !== undefined && draft[key] !== saved[key]) {
      patch[key] = draft[key];
    }
  }
  return patch;
});
const settingsDirty = computed(
  () => Object.keys(settingsPatch.value).length > 0,
);
watch(
  () => [props.provider.identity.id, props.revision],
  () => {
    settingsRequest++;
    settings.value = null;
    settingsDraft.value = null;
    settingsError.value = "";
    settingsOpen.value = false;
    configOpen.value = false;
    settingsLoading.value = false;
  },
);
function draftFrom(value: CpaGatewaySettings): SettingsDraft {
  return {
    routingStrategy: value.routingStrategy || "round-robin",
    proxyUrl: value.proxyUrl,
    requestRetry: value.requestRetry ?? undefined,
    maxRetryCredentials: value.maxRetryCredentials ?? undefined,
    maxRetryInterval: value.maxRetryInterval ?? undefined,
    switchProject: value.switchProject ?? false,
    switchPreviewModel: value.switchPreviewModel ?? false,
    requestLog: value.requestLog ?? false,
    usageStatisticsEnabled: value.usageStatisticsEnabled ?? false,
    debug: value.debug ?? false,
    forceModelPrefix: value.forceModelPrefix ?? false,
    wsAuth: value.wsAuth ?? false,
    logsMaxTotalSizeMb: value.logsMaxTotalSizeMb ?? undefined,
    errorLogsMaxFiles: value.errorLogsMaxFiles ?? undefined,
  };
}
function toggleSettings() {
  settingsOpen.value = !settingsOpen.value;
  if (settingsOpen.value && !settings.value && !settingsLoading.value) {
    void loadSettings();
  }
}
async function loadSettings() {
  const sequence = ++settingsRequest;
  const id = props.provider.identity.id;
  settingsLoading.value = true;
  settingsError.value = "";
  try {
    const result = await getCpaGatewaySettings(id);
    if (!disposed && sequence === settingsRequest) {
      settings.value = result;
      settingsDraft.value = draftFrom(result);
    }
  } catch (cause) {
    if (!disposed && sequence === settingsRequest)
      settingsError.value = String(cause);
  } finally {
    if (sequence === settingsRequest) settingsLoading.value = false;
  }
}
function resetDraft() {
  settingsDraft.value = settings.value ? draftFrom(settings.value) : null;
  settingsError.value = "";
}
async function saveSettings() {
  const saved = settings.value;
  const patch = settingsPatch.value;
  if (!saved || settingsSaving.value || !Object.keys(patch).length) return;
  const id = props.provider.identity.id;
  settingsSaving.value = true;
  settingsError.value = "";
  try {
    await updateCpaGatewaySettings(id, patch, props.revision);
    if (disposed || settings.value !== saved) return;
    /// Only the confirmed fields move; anything CPA normalized stays as the
    /// gateway reports it on the next read.
    const next: CpaGatewaySettings = {
      ...saved,
      routingStrategy: patch.routingStrategy ?? saved.routingStrategy,
      proxyUrl: patch.proxyUrl ?? saved.proxyUrl,
      requestRetry: patch.requestRetry ?? saved.requestRetry,
      maxRetryCredentials:
        patch.maxRetryCredentials ?? saved.maxRetryCredentials,
      maxRetryInterval: patch.maxRetryInterval ?? saved.maxRetryInterval,
      switchProject: patch.switchProject ?? saved.switchProject,
      switchPreviewModel: patch.switchPreviewModel ?? saved.switchPreviewModel,
      requestLog: patch.requestLog ?? saved.requestLog,
      usageStatisticsEnabled:
        patch.usageStatisticsEnabled ?? saved.usageStatisticsEnabled,
      debug: patch.debug ?? saved.debug,
      forceModelPrefix: patch.forceModelPrefix ?? saved.forceModelPrefix,
      wsAuth: patch.wsAuth ?? saved.wsAuth,
      logsMaxTotalSizeMb:
        patch.logsMaxTotalSizeMb ?? saved.logsMaxTotalSizeMb,
      errorLogsMaxFiles: patch.errorLogsMaxFiles ?? saved.errorLogsMaxFiles,
    };
    settings.value = next;
    settingsDraft.value = draftFrom(next);
    Message.success("网关调度设置已保存");
  } catch (cause) {
    if (!disposed) settingsError.value = String(cause);
  } finally {
    settingsSaving.value = false;
  }
}
</script>
<template>
  <section class="cpa-scheduling" aria-label="网关调度设置">
    <button
      type="button"
      class="cpa-scheduling-head"
      :aria-expanded="settingsOpen"
      @click="toggleSettings"
    >
      <span
        ><strong>调度与重试</strong><small>{{ routingSummary }}</small></span
      ><ChevronDown
        :size="13"
        class="cpa-scheduling-caret"
        :class="{ open: settingsOpen }"
      />
    </button>
    <div v-if="settingsOpen" class="cpa-scheduling-body">
      <a-alert v-if="settingsError" type="warning">{{ settingsError }}</a-alert>
      <a-spin v-if="settingsLoading" tip="读取网关调度设置" />
      <template v-else-if="settings && settingsDraft">
        <div class="cpa-scheduling-field">
          <span>路由策略</span>
          <a-select
            v-model="settingsDraft.routingStrategy"
            aria-label="路由策略"
            :options="routingStrategies"
            :disabled="settingsSaving"
          />
        </div>
        <p class="cpa-scheduling-hint">{{ routingHint }}</p>
        <div class="cpa-scheduling-fields">
          <label>
            <span>请求重试</span>
            <a-input-number
              v-model="settingsDraft.requestRetry"
              :precision="0"
              :min="0"
              placeholder="网关默认"
              :disabled="settingsSaving"
            />
          </label>
          <label>
            <span>最多重试账号</span>
            <a-input-number
              v-model="settingsDraft.maxRetryCredentials"
              :precision="0"
              :min="0"
              placeholder="网关默认"
              :disabled="settingsSaving"
            />
          </label>
          <label>
            <span>重试间隔（秒）</span>
            <a-input-number
              v-model="settingsDraft.maxRetryInterval"
              :precision="0"
              :min="0"
              placeholder="网关默认"
              :disabled="settingsSaving"
            />
          </label>
          <label class="cpa-scheduling-proxy-field">
            <span>出站代理</span>
            <a-input
              v-model="settingsDraft.proxyUrl"
              placeholder="留空表示直连"
              allow-clear
              :disabled="settingsSaving"
            />
          </label>
        </div>
        <div class="cpa-scheduling-group">
          <span class="cpa-scheduling-group-title">配额耗尽</span>
          <label class="cpa-scheduling-toggle">
            <span
              >切换项目<small
                >同一账号额度用尽时，换用它的另一个项目重试</small
              ></span
            ><a-switch
              v-model="settingsDraft.switchProject"
              aria-label="配额耗尽时切换项目"
              :disabled="settingsSaving"
            />
          </label>
          <label class="cpa-scheduling-toggle">
            <span
              >切换预览模型<small
                >额度用尽时回退到预览模型，而不是让请求直接失败</small
              ></span
            ><a-switch
              v-model="settingsDraft.switchPreviewModel"
              aria-label="配额耗尽时切换预览模型"
              :disabled="settingsSaving"
            />
          </label>
        </div>
        <div class="cpa-scheduling-group">
          <span class="cpa-scheduling-group-title">路由</span>
          <label class="cpa-scheduling-toggle">
            <span
              >强制模型前缀<small
                >开启后请求必须带授权文件配置的模型前缀，不会落到别的授权上</small
              ></span
            ><a-switch
              v-model="settingsDraft.forceModelPrefix"
              aria-label="强制模型前缀"
              :disabled="settingsSaving"
            />
          </label>
          <label class="cpa-scheduling-toggle">
            <span
              >AI Studio WebSocket 鉴权<small
                >让 AI Studio 授权同时为 WebSocket 请求提供鉴权</small
              ></span
            ><a-switch
              v-model="settingsDraft.wsAuth"
              aria-label="AI Studio WebSocket 鉴权"
              :disabled="settingsSaving"
            />
          </label>
        </div>
        <div class="cpa-scheduling-group">
          <span class="cpa-scheduling-group-title">日志与用量</span>
          <label class="cpa-scheduling-toggle">
            <span
              >记录调试日志<small
                >把调试级别的日志一并写入日志文件，排查问题时更详细</small
              ></span
            ><a-switch
              v-model="settingsDraft.debug"
              aria-label="记录调试日志"
              :disabled="settingsSaving"
            />
          </label>
          <label class="cpa-scheduling-toggle">
            <span
              >单独记录请求日志<small
                >为每个请求写一个独立日志文件，需同时开启「写入日志文件」</small
              ></span
            ><a-switch
              v-model="settingsDraft.requestLog"
              aria-label="单独记录请求日志"
              :disabled="settingsSaving"
            />
          </label>
          <label class="cpa-scheduling-toggle">
            <span
              >记录用量统计<small
                >网关按用量队列统计请求与额度消耗</small
              ></span
            ><a-switch
              v-model="settingsDraft.usageStatisticsEnabled"
              aria-label="记录用量统计"
              :disabled="settingsSaving"
            />
          </label>
          <div class="cpa-scheduling-fields">
            <label>
              <span>日志总量上限（MB）</span>
              <a-input-number
                v-model="settingsDraft.logsMaxTotalSizeMb"
                :precision="0"
                :min="0"
                placeholder="网关默认"
                :disabled="settingsSaving"
              />
            </label>
            <label>
              <span>错误日志保留文件数</span>
              <a-input-number
                v-model="settingsDraft.errorLogsMaxFiles"
                :precision="0"
                :min="0"
                placeholder="网关默认"
                :disabled="settingsSaving"
              />
            </label>
          </div>
        </div>
        <div class="cpa-scheduling-actions">
          <span v-if="settingsDirty" class="cpa-scheduling-dirty"
            >有未保存的修改</span
          ><a-button
            size="mini"
            :disabled="!settingsDirty || settingsSaving"
            @click="resetDraft"
            >撤销</a-button
          ><a-button
            size="mini"
            type="primary"
            :disabled="!settingsDirty"
            :loading="settingsSaving"
            @click="saveSettings"
            >保存调度设置</a-button
          >
        </div>
      </template>
      <div class="cpa-scheduling-more">
        <a-button size="mini" type="text" @click="configOpen = true"
          >编辑完整配置</a-button
        >
      </div>
    </div>
    <CpaConfigDialog
      :visible="configOpen"
      :provider="provider"
      :revision="revision"
      @close="configOpen = false"
    />
  </section>
</template>
<style scoped>
.cpa-scheduling {
  border: 1px solid var(--color-border-2);
  border-radius: 10px;
  background: var(--color-fill-1);
}
.cpa-scheduling-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  width: 100%;
  padding: 10px 13px;
  border: 0;
  background: none;
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.cpa-scheduling-head > span {
  display: flex;
  align-items: baseline;
  gap: 9px;
  min-width: 0;
}
.cpa-scheduling-head strong {
  color: var(--color-text-1);
  font-size: 12px;
  font-weight: 550;
  white-space: nowrap;
}
.cpa-scheduling-head small {
  overflow: hidden;
  color: var(--color-text-3);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cpa-scheduling-caret {
  flex: 0 0 auto;
  color: var(--color-text-3);
  transition: transform 0.15s ease;
}
.cpa-scheduling-caret.open {
  transform: rotate(180deg);
}
.cpa-scheduling-body {
  display: grid;
  gap: 9px;
  padding: 0 13px 13px;
}
.cpa-scheduling-field {
  display: grid;
  grid-template-columns: 84px minmax(0, 1fr);
  align-items: center;
  gap: 10px;
  font-size: 12px;
}
.cpa-scheduling-field > span {
  color: var(--color-text-2);
}
.cpa-scheduling-hint {
  margin: 0;
  color: var(--color-text-3);
  font-size: 11px;
  line-height: 1.6;
}
.cpa-scheduling-fields {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 9px 14px;
}
.cpa-scheduling-fields label {
  display: grid;
  gap: 4px;
  min-width: 0;
}
.cpa-scheduling-fields label > span {
  color: var(--color-text-3);
  font-size: 11px;
}
.cpa-scheduling-proxy-field {
  grid-column: 1 / -1;
}
.cpa-scheduling-group {
  display: grid;
  gap: 9px;
  padding-top: 9px;
  border-top: 1px solid var(--color-border-2);
}
.cpa-scheduling-group-title {
  color: var(--color-text-3);
  font-size: 11px;
  font-weight: 550;
}
.cpa-scheduling-toggle {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  cursor: pointer;
}
.cpa-scheduling-toggle > span {
  display: grid;
  gap: 2px;
  min-width: 0;
  color: var(--color-text-2);
  font-size: 12px;
}
.cpa-scheduling-toggle small {
  color: var(--color-text-3);
  font-size: 11px;
  line-height: 1.5;
}
.cpa-scheduling-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}
.cpa-scheduling-dirty {
  margin-right: auto;
  color: #b26a00;
  font-size: 11px;
}
.cpa-scheduling-more {
  display: flex;
  justify-content: flex-end;
  padding-top: 9px;
  border-top: 1px solid var(--color-border-2);
}
</style>
