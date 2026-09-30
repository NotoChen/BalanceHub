<script setup lang="ts">
import { computed } from "vue";
import { IconDelete, IconPlus } from "@arco-design/web-vue/es/icon";
import type { SelectOptionData } from "@arco-design/web-vue";
import type {
  AppSettings,
  AgentCliKind,
  ProviderInput,
  ProviderNotificationMode,
} from "../../stores/providers";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import {
  agentCliLabel,
  availableCliOptions,
  registeredCliTools,
} from "../../utils/cli-environment";
import { formatDuration } from "../../utils/duration";
import DurationInput from "../DurationInput.vue";
import { MIN_LIVENESS_INTERVAL_SECONDS } from "../../utils/liveness-defaults";
import { providerProxyModeOptions, proxyModeOptions } from "../../utils/proxy-options";
import {
  livenessIntervalModeOptions,
  livenessPromptModeOptions,
  type SelectOption,
} from "../../utils/liveness-options";
import ProviderEditorCheckInSection from "./ProviderEditorCheckInSection.vue";

type LivenessMode = "global" | "custom" | "disabled";

const props = defineProps<{
  draft: ProviderInput;
  settings: AppSettings;
  availableModels: string[];
}>();

const store = useCliRuntimeStore();
const cliOptions = computed(() => availableCliOptions(store.cliEnvironmentProbe, "liveness"));
const agentEndpointOptions = computed(() =>
  registeredCliTools(store.cliEnvironmentProbe).map((tool) => ({
    kind: tool.kind,
    label: `${tool.label} Base URL`,
  })),
);

const notificationModeOptions: SelectOption<ProviderNotificationMode>[] = [
  { label: "跟随全局", value: "inherit" },
  { label: "自定义渠道", value: "custom" },
  { label: "关闭通知", value: "disabled" },
];

const livenessModeOptions: SelectOption<LivenessMode>[] = [
  { label: "跟随全局", value: "global" },
  { label: "自定义", value: "custom" },
  { label: "关闭测活", value: "disabled" },
];

const notificationChannelOptions = computed(() =>
  props.settings.notificationChannels.map((channel) => ({
    label: `${channel.name || channel.id}${channel.enabled ? "" : "（已停用）"}`,
    value: channel.id,
  })),
);

const globalLivenessSummary = computed(() => {
  const settings = props.settings;
  if (!settings.livenessEnabled) return "全局自动测活已关闭";
  const interval = settings.livenessIntervalMode === "fixed"
    ? `每 ${formatDuration(settings.livenessInterval)}`
    : `随机间隔 ${formatDuration(settings.livenessRandomMinInterval)} 至 ${formatDuration(settings.livenessRandomMaxInterval)}`;
  return [
    "全局已开启",
    agentCliLabel(store.cliEnvironmentProbe, settings.livenessCliKind),
    settings.livenessModel.trim() || "未指定模型",
    interval,
  ].join(" · ");
});

const globalProxyLabel = computed(() =>
  proxyModeOptions.find((option) => option.value === props.settings.proxyMode)?.label || "未设置",
);

const globalNotificationSummary = computed(() => {
  if (!props.settings.notificationEnabled) return "全局通知已关闭";
  const names = props.settings.notificationChannels
    .filter((channel) => channel.enabled)
    .map((channel) => channel.name || channel.id);
  return names.length ? `全局已启用渠道：${names.join("、")}` : "全局通知已开启，尚未启用渠道";
});

function modelChoices(selectedModel: string) {
  const values = [...props.availableModels, selectedModel || ""]
    .map((model) => model.trim())
    .filter(Boolean);
  return [...new Set(values)].map((model) => ({ label: model, value: model }));
}

const modelOptions = computed(() => modelChoices(props.draft.cli.preferredModel));
const livenessModelOptions = computed(() => modelChoices(props.draft.liveness.model));
const livenessModelValue = computed({
  get: () => props.draft.liveness.model,
  set: (value: string | undefined) => { props.draft.liveness.model = value || ""; },
});

function filterModelOption(inputValue: string, option: SelectOptionData) {
  const query = inputValue.trim().toLocaleLowerCase();
  if (!query) return true;
  return String(option.label ?? option.value ?? "").toLocaleLowerCase().includes(query);
}

const notificationModeModel = computed({
  get: () => props.draft.notification.mode,
  set: (mode: ProviderNotificationMode) => {
    props.draft.notification.mode = mode;
    if (mode === "custom" && props.draft.notification.channelIds.length === 0) {
      props.draft.notification.channelIds = props.settings.notificationChannels
        .filter((channel) => channel.enabled)
        .map((channel) => channel.id);
    }
  },
});

const refreshInheritsGlobal = computed({
  get: () => props.draft.automation.refreshInterval <= 0,
  set: (inherits: boolean) => {
    props.draft.automation.refreshInterval = inherits ? 0 : props.settings.refreshInterval || 300;
  },
});

const livenessMode = computed<LivenessMode>({
  get: () => {
    if (props.draft.liveness.useGlobal) return "global";
    return props.draft.liveness.enabled ? "custom" : "disabled";
  },
  set: (mode) => {
    props.draft.liveness.useGlobal = mode === "global";
    props.draft.liveness.enabled = mode === "custom";
  },
});

const livenessCliKindModel = computed({
  get: () => props.draft.liveness.cliKind || props.settings.livenessCliKind,
  set: (value: AgentCliKind) => {
    props.draft.liveness.cliKind = value;
  },
});

function updateRandomMinimum(seconds: number) {
  props.draft.liveness.randomMinInterval = seconds;
  props.draft.liveness.randomMaxInterval = Math.max(seconds, props.draft.liveness.randomMaxInterval);
}
</script>

<template>
  <div class="provider-editor-policies">
    <section class="provider-form-section">
      <h3 class="provider-form-section-title">
        备用地址
        <a-button type="text" size="small" @click="draft.identity.backupUrls.push('')">
          <template #icon><IconPlus /></template>添加地址
        </a-button>
      </h3>
      <div v-if="draft.identity.backupUrls.length" class="provider-backup-url-list">
        <div v-for="(_, index) in draft.identity.backupUrls" :key="index" class="provider-backup-url-row">
          <span class="provider-backup-url-index">{{ index + 1 }}</span>
          <a-input v-model="draft.identity.backupUrls[index]" :aria-label="`备用地址 ${index + 1}`" placeholder="https://backup.example.com" allow-clear />
          <a-button type="text" status="danger" :aria-label="`删除备用地址 ${index + 1}`" @click="draft.identity.backupUrls.splice(index, 1)"><template #icon><IconDelete /></template></a-button>
        </div>
      </div>
      <p v-else class="provider-empty-note">尚未添加备用地址</p>
    </section>
    <ProviderEditorCheckInSection :draft="draft" :settings="settings" />

    <section class="provider-form-section">
      <h3 class="provider-form-section-title">自动任务</h3>
      <div class="provider-form-section-body">
        <a-form-item class="provider-field" label="刷新间隔">
          <div class="provider-setting-controls">
            <a-checkbox v-model="refreshInheritsGlobal">跟随全局</a-checkbox>
            <span v-if="refreshInheritsGlobal" class="provider-inherited-value">{{ formatDuration(settings.refreshInterval) }}</span>
            <DurationInput v-if="!refreshInheritsGlobal" v-model="draft.automation.refreshInterval" :min="30" label="刷新间隔" />
          </div>
          <template v-if="!settings.autoRefreshEnabled" #extra>全局自动刷新已关闭</template>
        </a-form-item>
        <a-form-item class="provider-field" label="自动测活">
          <a-select v-model="livenessMode" :options="livenessModeOptions" />
          <template #extra>
            <p v-if="livenessMode === 'global'" class="provider-setting-summary">{{ globalLivenessSummary }}</p>
            <span v-if="livenessMode !== 'disabled'">使用真实 CLI 请求检查可用性，会消耗少量额度。</span>
          </template>
        </a-form-item>
        <div v-if="livenessMode === 'custom'" class="provider-liveness-fields">
          <div class="provider-field-grid">
            <a-form-item class="provider-field" label="执行 Agent">
              <a-select
                v-model="livenessCliKindModel"
                :options="cliOptions"
                :loading="store.cliEnvironmentLoading && !store.cliEnvironmentProbe"
                placeholder="未检测到可用 Agent"
              />
            </a-form-item>
            <a-form-item class="provider-field" label="测活模型">
              <a-select
                v-model="livenessModelValue"
                :options="livenessModelOptions"
                :filter-option="filterModelOption"
                allow-search
                allow-create
                allow-clear
                placeholder="选择或输入模型，留空跟随全局"
              />
              <template v-if="!draft.liveness.model" #extra>全局模型：{{ settings.livenessModel || '未设置' }}</template>
            </a-form-item>
            <a-form-item class="provider-field" label="周期策略">
              <a-select v-model="draft.liveness.intervalMode" :options="livenessIntervalModeOptions" />
            </a-form-item>
            <a-form-item class="provider-field" label="超时（秒）">
              <a-input-number v-model="draft.liveness.timeout" :min="5" :max="600" :step="5" />
            </a-form-item>
            <a-form-item v-if="draft.liveness.intervalMode === 'fixed'" class="provider-field provider-field-wide" label="执行周期">
              <DurationInput v-model="draft.liveness.interval" :min="MIN_LIVENESS_INTERVAL_SECONDS" label="执行周期" />
            </a-form-item>
            <template v-else>
              <a-form-item class="provider-field" label="最短周期">
                <DurationInput :model-value="draft.liveness.randomMinInterval" :min="MIN_LIVENESS_INTERVAL_SECONDS" label="最短周期" @update:model-value="updateRandomMinimum" />
              </a-form-item>
              <a-form-item class="provider-field" label="最长周期">
                <DurationInput v-model="draft.liveness.randomMaxInterval" :min="Math.max(MIN_LIVENESS_INTERVAL_SECONDS, draft.liveness.randomMinInterval)" label="最长周期" />
              </a-form-item>
            </template>
            <a-form-item class="provider-field provider-field-wide" label="话术策略">
              <a-select v-model="draft.liveness.promptMode" :options="livenessPromptModeOptions" />
            </a-form-item>
            <a-form-item v-if="draft.liveness.promptMode === 'fixed'" class="provider-field provider-field-wide" label="固定话术">
              <a-textarea
                v-model="draft.liveness.fixedPrompt"
                :auto-size="{ minRows: 2, maxRows: 4 }"
                placeholder="留空使用全局固定话术"
              />
            </a-form-item>
          </div>
          <details v-if="agentEndpointOptions.length" class="provider-form-disclosure">
            <summary>按 Agent 指定地址（可选）</summary>
            <div class="provider-form-section-body">
              <a-form-item v-for="option in agentEndpointOptions" :key="option.kind" class="provider-field" :label="option.label">
                <a-input v-model="draft.liveness.agentBaseUrls[option.kind]" placeholder="留空使用中转站地址" allow-clear />
              </a-form-item>
            </div>
          </details>
        </div>
      </div>
    </section>

    <section class="provider-form-section">
      <h3 class="provider-form-section-title">网络与通知</h3>
      <div class="provider-form-section-body">
        <a-form-item class="provider-field" label="网络代理">
          <a-select v-model="draft.proxy.mode" :options="providerProxyModeOptions" />
          <template v-if="draft.proxy.mode === 'inherit'" #extra>全局设置：{{ globalProxyLabel }}</template>
        </a-form-item>
        <a-form-item v-if="draft.proxy.mode === 'custom'" class="provider-field" label="代理地址">
          <a-input v-model="draft.proxy.url" placeholder="http://127.0.0.1:7890" allow-clear />
        </a-form-item>
        <a-form-item class="provider-field" label="通知策略">
          <a-select v-model="notificationModeModel" :options="notificationModeOptions" />
          <template v-if="draft.notification.mode === 'inherit'" #extra>{{ globalNotificationSummary }}</template>
          <template v-else-if="draft.notification.mode === 'custom' && !settings.notificationEnabled" #extra>全局通知已关闭</template>
        </a-form-item>
        <a-form-item v-if="draft.notification.mode === 'custom'" class="provider-field" label="通知渠道">
          <a-select
            v-model="draft.notification.channelIds"
            :options="notificationChannelOptions"
            multiple
            allow-clear
            :disabled="notificationChannelOptions.length === 0"
            placeholder="选择该中转站使用的通知渠道"
          />
          <template v-if="!notificationChannelOptions.length || !draft.notification.channelIds.length" #extra>
            <span v-if="!notificationChannelOptions.length">尚未添加渠道，请先在「设置 → 通知」中添加。</span>
            <span v-else>尚未选择通知渠道。</span>
          </template>
        </a-form-item>
      </div>
    </section>

    <section class="provider-form-section">
      <h3 class="provider-form-section-title">临时 CLI</h3>
      <div class="provider-form-section-body">
        <a-form-item class="provider-field" label="首选模型">
          <a-select
            v-model="draft.cli.preferredModel"
            :options="modelOptions"
            allow-search
            allow-create
            allow-clear
            :filter-option="filterModelOption"
            placeholder="搜索模型或直接输入"
          />
          <template #extra>{{ availableModels.length ? `已获取 ${availableModels.length} 个模型` : "暂未获取模型，可直接输入" }}</template>
        </a-form-item>
      </div>
    </section>
  </div>
</template>
