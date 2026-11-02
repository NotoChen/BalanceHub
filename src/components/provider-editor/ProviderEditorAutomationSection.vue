<script setup lang="ts">
import { computed } from "vue";
import type { SelectOptionData } from "@arco-design/web-vue";
import type {
  AppSettings,
  AgentCliKind,
  ProviderInput,
  ProviderProtocolDescriptor,
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
import { providerProtocolDescriptor } from "../../utils/provider-protocol";
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
  providerProtocols: ProviderProtocolDescriptor[];
}>();

/// 签到只对实现了签到能力的协议出现。网关和授权账号没有账号签到接口，
/// 摆一组按下去只会提示「当前协议不支持账号签到」的开关没有意义。
const checkInSupported = computed(
  () =>
    providerProtocolDescriptor(props.providerProtocols, props.draft.identity.protocol)
      ?.capabilities.checkIn ?? false,
);

const store = useCliRuntimeStore();
const cliOptions = computed(() => availableCliOptions(store.cliEnvironmentProbe, "liveness"));
const agentEndpointOptions = computed(() =>
  registeredCliTools(store.cliEnvironmentProbe).map((tool) => ({
    kind: tool.kind,
    label: `${tool.label} Base URL`,
  })),
);

const livenessModeOptions: SelectOption<LivenessMode>[] = [
  { label: "跟随全局", value: "global" },
  { label: "自定义", value: "custom" },
  { label: "关闭测活", value: "disabled" },
];

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

function modelChoices(selectedModel: string) {
  const values = [...props.availableModels, selectedModel || ""]
    .map((model) => model.trim())
    .filter(Boolean);
  return [...new Set(values)].map((model) => ({ label: model, value: model }));
}

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
  <section class="provider-form-section">
    <h3 class="provider-form-section-title">刷新与测活</h3>
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
        </div>
        <details class="provider-form-disclosure">
          <summary>更多测活设置</summary>
          <div class="provider-form-disclosure-body">
            <div class="provider-field-grid">
              <a-form-item class="provider-field" label="话术策略">
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
            <template v-if="agentEndpointOptions.length">
              <div class="provider-form-subsection-header"><span>按 Agent 指定地址</span></div>
              <a-form-item v-for="option in agentEndpointOptions" :key="option.kind" class="provider-field" :label="option.label">
                <a-input v-model="draft.liveness.agentBaseUrls[option.kind]" placeholder="留空使用中转站地址" allow-clear />
              </a-form-item>
            </template>
          </div>
        </details>
      </div>
    </div>
  </section>
  <ProviderEditorCheckInSection v-if="checkInSupported" :draft="draft" :settings="settings" />
</template>
