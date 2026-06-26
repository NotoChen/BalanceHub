<script setup lang="ts">
import { computed, ref } from "vue";
import type {
  AppSettings,
  LivenessCliKind,
  LivenessHttpProtocol,
  LivenessMethod,
  ProviderInput,
  ProviderNotificationMode,
} from "../../stores/providers";
import {
  durationUnitOptions,
  durationValueToSeconds,
  secondsToDurationValue,
  type DurationUnit,
} from "../../utils/duration";
import {
  codexIntervalModeOptions,
  codexPromptModeOptions,
  livenessCliKindOptions,
  livenessHttpProtocolOptions,
  livenessMethodOptions,
  providerProxyModeOptions,
  type SelectOption,
} from "./options";
import { useLivenessConsent } from "../../composables/useLivenessConsent";

type LivenessMode = "global" | "custom" | "disabled";

const props = defineProps<{
  draft: ProviderInput;
  settings: AppSettings;
}>();

const expanded = ref(false);
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

const { ensureConsent } = useLivenessConsent();

// 选「自定义」= 本站自动测活（消耗真实额度），需先取得一次性授权；取消则保持原值不变。
async function onLivenessModeChange(value: unknown) {
  const mode = value as LivenessMode;
  if (mode === "custom" && !(await ensureConsent())) {
    return;
  }
  livenessMode.value = mode;
}

const livenessCliKindModel = computed({
  get: () => props.draft.liveness.cliKind || props.settings.livenessCliKind,
  set: (value: LivenessCliKind) => {
    props.draft.liveness.cliKind = value;
  },
});

const livenessMethodModel = computed({
  get: () => props.draft.liveness.method || props.settings.livenessMethod,
  set: (value: LivenessMethod) => {
    props.draft.liveness.method = value;
  },
});

const livenessHttpProtocolModel = computed({
  get: () => props.draft.liveness.httpProtocol || props.settings.livenessHttpProtocol,
  set: (value: LivenessHttpProtocol) => {
    props.draft.liveness.httpProtocol = value;
  },
});

const fixedLivenessAmount = computed({
  get: () => secondsToDurationValue(props.draft.liveness.interval, fixedLivenessUnit.value),
  set: (value: number | undefined) => {
    props.draft.liveness.interval = Math.max(
      45 * 60,
      durationValueToSeconds(value, fixedLivenessUnit.value),
    );
  },
});

const randomMinLivenessAmount = computed({
  get: () =>
    secondsToDurationValue(props.draft.liveness.randomMinInterval, randomMinLivenessUnit.value),
  set: (value: number | undefined) => {
    props.draft.liveness.randomMinInterval = Math.max(
      45 * 60,
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
  if (unit === "hour") return 0.75;
  if (unit === "minute") return 45;
  return 45 * 60;
}
</script>

<template>
  <section class="provider-advanced-section">
    <button
      type="button"
      class="provider-advanced-title"
      @click="expanded = !expanded"
    >
      <span>
        <strong>高级配置</strong>
        <small>覆盖全局自动任务、代理、测活和通知策略</small>
      </span>
      <b>{{ expanded ? "收起" : "展开" }}</b>
    </button>

    <div v-if="expanded" class="provider-advanced-content">
      <div class="provider-advanced-group">
        <strong>自动任务</strong>
        <a-form-item label="刷新间隔">
          <a-checkbox v-model="refreshInheritsGlobal">跟随全局</a-checkbox>
          <div v-if="!refreshInheritsGlobal" class="duration-control">
            <a-input-number v-model="refreshAmount" :min="1" :step="1" />
            <a-select v-model="refreshUnit" :options="durationUnitOptions" />
          </div>
        </a-form-item>
        <a-form-item label="签到时间">
          <a-checkbox v-model="checkInInheritsGlobal">跟随全局</a-checkbox>
          <a-time-picker
            v-if="!checkInInheritsGlobal"
            v-model="draft.automation.checkInTime"
            format="HH:mm"
            value-format="HH:mm"
            placeholder="00:00"
            disable-confirm
          />
        </a-form-item>
      </div>

      <div class="provider-advanced-group">
        <strong>代理</strong>
        <a-form-item label="代理策略">
          <a-select v-model="draft.proxy.mode" :options="providerProxyModeOptions" />
        </a-form-item>
        <a-form-item v-if="draft.proxy.mode === 'custom'" label="代理地址">
          <a-input v-model="draft.proxy.url" placeholder="http://127.0.0.1:7890" />
        </a-form-item>
      </div>

      <div class="provider-advanced-group">
        <strong>测活</strong>
        <a-form-item label="测活策略">
          <a-select :model-value="livenessMode" :options="livenessModeOptions" @change="onLivenessModeChange" />
        </a-form-item>
        <template v-if="livenessMode === 'custom'">
          <a-form-item label="方式">
            <a-select v-model="livenessMethodModel" :options="livenessMethodOptions" />
          </a-form-item>
          <a-form-item v-if="livenessMethodModel === 'cli'" label="CLI">
            <a-select v-model="livenessCliKindModel" :options="livenessCliKindOptions" />
          </a-form-item>
          <a-form-item v-else label="HTTP 协议">
            <a-select v-model="livenessHttpProtocolModel" :options="livenessHttpProtocolOptions" />
          </a-form-item>
          <a-form-item label="模型">
            <a-input v-model="draft.liveness.model" placeholder="留空则使用全局模型" />
          </a-form-item>
          <a-form-item label="OpenAI Base URL">
            <a-input v-model="draft.liveness.openaiBaseUrl" placeholder="留空则使用中转站地址" />
          </a-form-item>
          <a-form-item label="Anthropic Base URL">
            <a-input v-model="draft.liveness.anthropicBaseUrl" placeholder="留空则使用中转站地址" />
          </a-form-item>
          <a-form-item label="周期模式">
            <a-select v-model="draft.liveness.intervalMode" :options="codexIntervalModeOptions" />
          </a-form-item>
          <a-form-item v-if="draft.liveness.intervalMode === 'fixed'" label="固定周期">
            <div class="duration-control">
              <a-input-number
                v-model="fixedLivenessAmount"
                :min="minLivenessAmount(fixedLivenessUnit)"
                :step="1"
              />
              <a-select v-model="fixedLivenessUnit" :options="durationUnitOptions" />
            </div>
          </a-form-item>
          <template v-else>
            <a-form-item label="最小周期">
              <div class="duration-control">
                <a-input-number
                  v-model="randomMinLivenessAmount"
                  :min="minLivenessAmount(randomMinLivenessUnit)"
                  :step="1"
                />
                <a-select v-model="randomMinLivenessUnit" :options="durationUnitOptions" />
              </div>
            </a-form-item>
            <a-form-item label="最大周期">
              <div class="duration-control">
                <a-input-number
                  v-model="randomMaxLivenessAmount"
                  :min="minLivenessAmount(randomMaxLivenessUnit)"
                  :step="1"
                />
                <a-select v-model="randomMaxLivenessUnit" :options="durationUnitOptions" />
              </div>
            </a-form-item>
          </template>
          <a-form-item label="超时秒数">
            <a-input-number v-model="draft.liveness.timeout" :min="5" :max="600" :step="5" />
          </a-form-item>
          <a-form-item label="话术模式">
            <a-select v-model="draft.liveness.promptMode" :options="codexPromptModeOptions" />
          </a-form-item>
          <a-form-item v-if="draft.liveness.promptMode === 'fixed'" label="固定话术">
            <a-textarea
              v-model="draft.liveness.fixedPrompt"
              :auto-size="{ minRows: 2, maxRows: 4 }"
              placeholder="留空则使用全局固定话术"
            />
          </a-form-item>
        </template>
      </div>

      <div class="provider-advanced-group">
        <strong>通知</strong>
        <a-form-item label="通知策略">
          <a-select v-model="notificationModeModel" :options="notificationModeOptions" />
        </a-form-item>
        <a-form-item v-if="draft.notification.mode === 'custom'" label="通知渠道">
          <a-select
            v-model="draft.notification.channelIds"
            :options="notificationChannelOptions"
            multiple
            placeholder="选择该中转站使用的通知渠道"
          />
        </a-form-item>
      </div>
    </div>
  </section>
</template>
