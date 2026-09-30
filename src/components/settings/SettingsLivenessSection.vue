<script setup lang="ts">
import { computed } from "vue";
import { IconExperiment } from "@arco-design/web-vue/es/icon";
import CliIconSelector from "../CliIconSelector.vue";
import DurationInput from "../DurationInput.vue";
import SettingsLivenessPromptSection from "./SettingsLivenessPromptSection.vue";
import { availableCliOptions } from "../../utils/cli-environment";
import { MIN_LIVENESS_INTERVAL_SECONDS } from "../../utils/liveness-defaults";
import { livenessIntervalModeOptions } from "../../utils/liveness-options";
import { useCliRuntimeStore } from "../../stores/cli-runtime";
import type { AppSettings } from "../../stores/providers";

const props = defineProps<{
  settings: AppSettings;
  livenessModelOptions: string[];
  selectedLivenessModelProviders: { id: string; name: string }[];
}>();
const store = useCliRuntimeStore();
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
<section class="settings-card settings-liveness-card">
  <header class="settings-card-header">
    <span class="settings-card-icon settings-card-icon-green"><IconExperiment /></span>
    <div><strong>自动测活</strong></div>
    <a-switch v-model="settings.livenessEnabled" aria-label="自动测活" />
  </header>

  <div v-if="settings.livenessEnabled" class="settings-liveness-config">
    <p class="settings-card-note">定期通过 Agent 发起真实请求，检查中转站是否可用；会消耗少量额度。</p>
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
      <a-form-item v-if="settings.livenessIntervalMode === 'fixed'" label="执行周期">
        <DurationInput v-model="settings.livenessInterval" :min="MIN_LIVENESS_INTERVAL_SECONDS" label="执行周期" />
      </a-form-item>
      <template v-else>
        <a-form-item label="最短周期">
          <DurationInput v-model="settings.livenessRandomMinInterval" :min="MIN_LIVENESS_INTERVAL_SECONDS" label="最短周期" />
        </a-form-item>
        <a-form-item label="最长周期">
          <DurationInput v-model="settings.livenessRandomMaxInterval" :min="minimumRandomMaxInterval" label="最长周期" />
        </a-form-item>
      </template>
      <a-form-item label="超时（秒）">
        <a-input-number v-model="settings.livenessTimeout" :min="10" :max="600" :step="5" />
      </a-form-item>
    </div>

    <div class="settings-liveness-prompt">
      <SettingsLivenessPromptSection :settings="settings" />
    </div>
  </div>
</section>
</template>
