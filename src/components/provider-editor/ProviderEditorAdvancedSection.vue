<script setup lang="ts">
import { computed, ref } from "vue";
import {
  IconCalendarClock,
  IconCommand,
  IconExperiment,
  IconInfoCircle,
  IconNotification,
  IconWifi,
} from "@arco-design/web-vue/es/icon";
import type { SelectOptionData } from "@arco-design/web-vue";
import type {
  AppSettings,
  LivenessCliKind,
  ProviderInput,
  ProviderNotificationMode,
} from "../../stores/providers";
import {
  durationUnitOptions,
  durationValueToSeconds,
  secondsToDurationValue,
  type DurationUnit,
} from "../../utils/duration";
import { MIN_LIVENESS_INTERVAL_SECONDS } from "../../utils/liveness-defaults";
import {
  codexIntervalModeOptions,
  codexPromptModeOptions,
  livenessCliKindOptions,
  providerProxyModeOptions,
  type SelectOption,
} from "./options";

type LivenessMode = "global" | "custom" | "disabled";

const props = defineProps<{
  draft: ProviderInput;
  settings: AppSettings;
  availableModels: string[];
  initiallyExpanded?: boolean;
}>();

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

const checkInInheritsGlobal = computed({
  get: () => !props.draft.automation.checkInTime.trim(),
  set: (inherits: boolean) => {
    props.draft.automation.checkInTime = inherits ? "" : props.settings.checkInTime || "00:00";
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

function onLivenessModeChange(value: unknown) {
  const mode = value as LivenessMode;
  livenessMode.value = mode;
}

const livenessCliKindModel = computed({
  get: () => props.draft.liveness.cliKind || props.settings.livenessCliKind,
  set: (value: LivenessCliKind) => {
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
  <div class="provider-form-page provider-policy-page">
    <section class="provider-policy-block provider-cli-policy">
      <header class="provider-policy-header">
        <span class="provider-form-block-icon"><IconCommand /></span>
        <div><strong>临时 CLI 默认值</strong></div>
        <span class="provider-form-block-meta">{{ availableModels.length ? `${availableModels.length} 个模型` : "待同步" }}</span>
      </header>
      <div class="provider-policy-body provider-policy-cli-body">
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
        </a-form-item>
      </div>
    </section>

    <section class="provider-policy-block provider-automation-policy">
      <header class="provider-policy-header">
        <span class="provider-form-block-icon"><IconCalendarClock /></span>
        <div><strong>自动任务</strong></div>
      </header>
      <div class="provider-policy-body provider-policy-rows">
        <div class="provider-policy-row">
          <div class="provider-policy-copy"><strong>刷新间隔</strong></div>
          <div class="provider-policy-control">
            <label class="provider-inherit-switch"><span>跟随全局</span><a-switch v-model="refreshInheritsGlobal" size="small" /></label>
            <div v-if="!refreshInheritsGlobal" class="duration-control">
              <a-input-number v-model="refreshAmount" :min="1" :step="1" />
              <a-select v-model="refreshUnit" :options="durationUnitOptions" />
            </div>
          </div>
        </div>
        <div class="provider-policy-row">
          <div class="provider-policy-copy"><strong>签到时间</strong></div>
          <div class="provider-policy-control">
            <label class="provider-inherit-switch"><span>跟随全局</span><a-switch v-model="checkInInheritsGlobal" size="small" /></label>
            <a-time-picker
              v-if="!checkInInheritsGlobal"
              v-model="draft.automation.checkInTime"
              format="HH:mm"
              value-format="HH:mm"
              placeholder="00:00"
              disable-confirm
            />
          </div>
        </div>
      </div>
    </section>

    <section class="provider-policy-block provider-liveness-policy">
      <header class="provider-policy-header">
        <span class="provider-form-block-icon provider-form-block-icon-warning"><IconExperiment /></span>
        <div><strong>自动测活</strong></div>
        <a-tooltip content="使用真实 CLI 请求检查可用性，会消耗少量额度">
          <span class="provider-policy-info" aria-label="自动测活说明"><IconInfoCircle /></span>
        </a-tooltip>
      </header>
      <div class="provider-policy-body">
        <div class="provider-option-segment" role="radiogroup" aria-label="测活策略">
          <button
            v-for="option in livenessModeOptions"
            :key="option.value"
            type="button"
            :class="{ active: livenessMode === option.value }"
            role="radio"
            :aria-checked="livenessMode === option.value"
            @click="onLivenessModeChange(option.value)"
          >
            {{ option.label }}
          </button>
        </div>

        <div v-if="livenessMode === 'custom'" class="provider-policy-reveal provider-liveness-fields">
          <div class="provider-field-grid provider-field-grid-three">
            <a-form-item class="provider-field" label="执行 CLI">
              <a-select v-model="livenessCliKindModel" :options="livenessCliKindOptions" />
            </a-form-item>
            <a-form-item class="provider-field" label="模型">
              <a-input v-model="draft.liveness.model" placeholder="留空跟随全局" allow-clear />
            </a-form-item>
            <a-form-item class="provider-field" label="超时秒数">
              <a-input-number v-model="draft.liveness.timeout" :min="5" :max="600" :step="5" />
            </a-form-item>
          </div>

          <div class="provider-field-grid">
            <a-form-item class="provider-field" label="OpenAI Base URL">
              <a-input v-model="draft.liveness.openaiBaseUrl" placeholder="留空使用中转站地址" allow-clear />
            </a-form-item>
            <a-form-item class="provider-field" label="Anthropic Base URL">
              <a-input v-model="draft.liveness.anthropicBaseUrl" placeholder="留空使用中转站地址" allow-clear />
            </a-form-item>
          </div>

          <div class="provider-field-grid provider-field-grid-three">
            <a-form-item class="provider-field" label="周期策略">
              <a-select v-model="draft.liveness.intervalMode" :options="codexIntervalModeOptions" />
            </a-form-item>
            <a-form-item v-if="draft.liveness.intervalMode === 'fixed'" class="provider-field provider-field-span-two" label="执行周期">
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
          </div>

          <div class="provider-field-grid">
            <a-form-item class="provider-field" label="话术策略">
              <a-select v-model="draft.liveness.promptMode" :options="codexPromptModeOptions" />
            </a-form-item>
            <a-form-item v-if="draft.liveness.promptMode === 'fixed'" class="provider-field" label="固定话术">
              <a-textarea
                v-model="draft.liveness.fixedPrompt"
                :auto-size="{ minRows: 2, maxRows: 4 }"
                placeholder="留空使用全局固定话术"
              />
            </a-form-item>
          </div>
        </div>
      </div>
    </section>

    <section class="provider-policy-block provider-proxy-policy">
      <header class="provider-policy-header">
        <span class="provider-form-block-icon provider-form-block-icon-neutral"><IconWifi /></span>
        <div><strong>网络代理</strong></div>
      </header>
      <div class="provider-policy-body">
        <div class="provider-option-segment" role="radiogroup" aria-label="代理策略">
          <button
            v-for="option in providerProxyModeOptions"
            :key="option.value"
            type="button"
            :class="{ active: draft.proxy.mode === option.value }"
            role="radio"
            :aria-checked="draft.proxy.mode === option.value"
            @click="draft.proxy.mode = option.value"
          >
            {{ option.label }}
          </button>
        </div>
        <a-form-item v-if="draft.proxy.mode === 'custom'" class="provider-field provider-policy-reveal" label="代理地址">
          <a-input v-model="draft.proxy.url" placeholder="http://127.0.0.1:7890" allow-clear />
        </a-form-item>
      </div>
    </section>

    <section class="provider-policy-block provider-notification-policy">
      <header class="provider-policy-header">
        <span class="provider-form-block-icon provider-form-block-icon-neutral"><IconNotification /></span>
        <div><strong>通知策略</strong></div>
      </header>
      <div class="provider-policy-body">
        <div class="provider-option-segment" role="radiogroup" aria-label="通知策略">
          <button
            v-for="option in notificationModeOptions"
            :key="option.value"
            type="button"
            :class="{ active: notificationModeModel === option.value }"
            role="radio"
            :aria-checked="notificationModeModel === option.value"
            @click="notificationModeModel = option.value"
          >
            {{ option.label }}
          </button>
        </div>
        <a-form-item v-if="draft.notification.mode === 'custom'" class="provider-field provider-policy-reveal" label="通知渠道">
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
  </div>
</template>
