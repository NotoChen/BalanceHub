<script setup lang="ts">
import { computed, ref } from "vue";
import type { SelectOptionData } from "@arco-design/web-vue";
import type {
  AppSettings,
  AgentCliKind,
  ProviderInput,
  ProviderNotificationMode,
} from "../../stores/providers";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import {
  availableCliOptions,
  registeredCliTools,
} from "../../utils/cli-environment";
import {
  durationUnitOptions,
  durationValueToSeconds,
  secondsToDurationValue,
  type DurationUnit,
} from "../../utils/duration";
import { MIN_LIVENESS_INTERVAL_SECONDS } from "../../utils/liveness-defaults";
import {
  livenessIntervalModeOptions,
  livenessPromptModeOptions,
  providerProxyModeOptions,
  type SelectOption,
} from "./options";
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

const refreshUnit = ref<DurationUnit>("minute");
const fixedLivenessUnit = ref<DurationUnit>("minute");
const randomMinLivenessUnit = ref<DurationUnit>("minute");
const randomMaxLivenessUnit = ref<DurationUnit>("minute");

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
    label: channel.name || channel.id,
    value: channel.id,
  })),
);

const modelOptions = computed(() => {
  const values = [...props.availableModels, props.draft.cli.preferredModel]
    .map((model) => model.trim())
    .filter(Boolean);
  return [...new Set(values)].map((model) => ({ label: model, value: model }));
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

const refreshAmount = computed({
  get: () => secondsToDurationValue(props.draft.automation.refreshInterval, refreshUnit.value),
  set: (value: number | undefined) => {
    props.draft.automation.refreshInterval = durationValueToSeconds(value, refreshUnit.value);
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

const fixedLivenessAmount = computed({
  get: () => secondsToDurationValue(props.draft.liveness.interval, fixedLivenessUnit.value),
  set: (value: number | undefined) => {
    props.draft.liveness.interval = Math.max(
      MIN_LIVENESS_INTERVAL_SECONDS,
      durationValueToSeconds(value, fixedLivenessUnit.value),
    );
  },
});

const randomMinLivenessAmount = computed({
  get: () =>
    secondsToDurationValue(props.draft.liveness.randomMinInterval, randomMinLivenessUnit.value),
  set: (value: number | undefined) => {
    props.draft.liveness.randomMinInterval = Math.max(
      MIN_LIVENESS_INTERVAL_SECONDS,
      durationValueToSeconds(value, randomMinLivenessUnit.value),
    );
    if (props.draft.liveness.randomMaxInterval < props.draft.liveness.randomMinInterval) {
      props.draft.liveness.randomMaxInterval = props.draft.liveness.randomMinInterval;
    }
  },
});

const randomMaxLivenessAmount = computed({
  get: () =>
    secondsToDurationValue(props.draft.liveness.randomMaxInterval, randomMaxLivenessUnit.value),
  set: (value: number | undefined) => {
    props.draft.liveness.randomMaxInterval = Math.max(
      props.draft.liveness.randomMinInterval,
      durationValueToSeconds(value, randomMaxLivenessUnit.value),
    );
  },
});

function minLivenessAmount(unit: DurationUnit) {
  if (unit === "second") return MIN_LIVENESS_INTERVAL_SECONDS;
  return 1;
}
</script>

<template>
  <div class="provider-editor-policies">
    <ProviderEditorCheckInSection :draft="draft" :settings="settings" />

    <section class="provider-form-section">
      <h3 class="provider-form-section-title">自动任务</h3>
      <div class="provider-form-section-body">
        <a-form-item class="provider-field" label="刷新间隔">
          <div class="provider-setting-controls">
            <a-checkbox v-model="refreshInheritsGlobal">跟随全局</a-checkbox>
            <div v-if="!refreshInheritsGlobal" class="duration-control">
              <a-input-number v-model="refreshAmount" :min="1" :step="1" />
              <a-select v-model="refreshUnit" :options="durationUnitOptions" />
            </div>
          </div>
        </a-form-item>
        <a-form-item class="provider-field" label="自动测活">
          <a-select v-model="livenessMode" :options="livenessModeOptions" />
          <template #extra>使用真实 CLI 请求检查可用性，会消耗少量额度。</template>
        </a-form-item>
        <template v-if="livenessMode === 'custom'">
          <a-form-item class="provider-field" label="执行 Agent">
            <a-select
              v-model="livenessCliKindModel"
              :options="cliOptions"
              :loading="store.cliEnvironmentLoading && !store.cliEnvironmentProbe"
              placeholder="未检测到可用 Agent"
            />
          </a-form-item>
          <a-form-item class="provider-field" label="测活模型">
            <a-input v-model="draft.liveness.model" placeholder="留空跟随全局" allow-clear />
          </a-form-item>
          <a-form-item class="provider-field" label="超时秒数">
            <a-input-number v-model="draft.liveness.timeout" :min="5" :max="600" :step="5" />
          </a-form-item>
          <a-form-item class="provider-field" label="周期策略">
            <a-select v-model="draft.liveness.intervalMode" :options="livenessIntervalModeOptions" />
          </a-form-item>
          <a-form-item v-if="draft.liveness.intervalMode === 'fixed'" class="provider-field" label="执行周期">
            <div class="duration-control">
              <a-input-number v-model="fixedLivenessAmount" :min="minLivenessAmount(fixedLivenessUnit)" :step="1" />
              <a-select v-model="fixedLivenessUnit" :options="durationUnitOptions" />
            </div>
          </a-form-item>
          <template v-else>
            <a-form-item class="provider-field" label="最小周期">
              <div class="duration-control">
                <a-input-number v-model="randomMinLivenessAmount" :min="minLivenessAmount(randomMinLivenessUnit)" :step="1" />
                <a-select v-model="randomMinLivenessUnit" :options="durationUnitOptions" />
              </div>
            </a-form-item>
            <a-form-item class="provider-field" label="最大周期">
              <div class="duration-control">
                <a-input-number v-model="randomMaxLivenessAmount" :min="minLivenessAmount(randomMaxLivenessUnit)" :step="1" />
                <a-select v-model="randomMaxLivenessUnit" :options="durationUnitOptions" />
              </div>
            </a-form-item>
          </template>
          <a-form-item class="provider-field" label="话术策略">
            <a-select v-model="draft.liveness.promptMode" :options="livenessPromptModeOptions" />
          </a-form-item>
          <a-form-item v-if="draft.liveness.promptMode === 'fixed'" class="provider-field" label="固定话术">
            <a-textarea
              v-model="draft.liveness.fixedPrompt"
              :auto-size="{ minRows: 2, maxRows: 4 }"
              placeholder="留空使用全局固定话术"
            />
          </a-form-item>
          <details v-if="agentEndpointOptions.length" class="provider-form-disclosure">
            <summary>按 Agent 指定地址（可选）</summary>
            <div class="provider-form-section-body">
              <a-form-item v-for="option in agentEndpointOptions" :key="option.kind" class="provider-field" :label="option.label">
                <a-input v-model="draft.liveness.agentBaseUrls[option.kind]" placeholder="留空使用中转站地址" allow-clear />
              </a-form-item>
            </div>
          </details>
        </template>
      </div>
    </section>

    <section class="provider-form-section">
      <h3 class="provider-form-section-title">网络与通知</h3>
      <div class="provider-form-section-body">
        <a-form-item class="provider-field" label="网络代理">
          <a-select v-model="draft.proxy.mode" :options="providerProxyModeOptions" />
        </a-form-item>
        <a-form-item v-if="draft.proxy.mode === 'custom'" class="provider-field" label="代理地址">
          <a-input v-model="draft.proxy.url" placeholder="http://127.0.0.1:7890" allow-clear />
        </a-form-item>
        <a-form-item class="provider-field" label="通知策略">
          <a-select v-model="notificationModeModel" :options="notificationModeOptions" />
        </a-form-item>
        <a-form-item v-if="draft.notification.mode === 'custom'" class="provider-field" label="通知渠道">
          <a-select
            v-model="draft.notification.channelIds"
            :options="notificationChannelOptions"
            multiple
            allow-clear
            placeholder="选择该中转站使用的通知渠道"
          />
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
