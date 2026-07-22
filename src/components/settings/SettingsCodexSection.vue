<script setup lang="ts">
import { computed } from "vue";
import { IconCommand, IconDesktop, IconExperiment } from "@arco-design/web-vue/es/icon";
import SettingsCodexPromptSection from "./SettingsCodexPromptSection.vue";
import SettingsCliManager from "./SettingsCliManager.vue";
import SettingsTerminalManager from "./SettingsTerminalManager.vue";
import { MIN_LIVENESS_INTERVAL_SECONDS } from "../../utils/liveness-defaults";
import { codexIntervalModeOptions, livenessCliKindOptions } from "../../utils/liveness-options";
import type { AppSettings } from "../../stores/providers";

interface ModelProviderIndexItem {
  model: string;
  providers: { id: string; name: string }[];
}

const props = defineProps<{
  settings: AppSettings;
  expanded?: boolean;
  livenessModelOptions: string[];
  modelProviderIndex: ModelProviderIndexItem[];
}>();

const codexModelSelectOptions = computed(() =>
  Array.from(
    new Set(
      [props.settings.livenessModel.trim(), ...props.livenessModelOptions.map((model) => model.trim())].filter(
        Boolean,
      ),
    ),
  ).map((model) => ({ label: model, value: model })),
);

const selectedLivenessModelProviders = computed(() => {
  const model = props.settings.livenessModel.trim();
  if (!model) return [];
  return props.modelProviderIndex.find((item) => item.model === model)?.providers ?? [];
});

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
      <SettingsCliManager />
    </section>

    <section class="settings-card settings-terminal-card">
      <header class="settings-card-header">
        <span class="settings-card-icon settings-card-icon-amber"><IconDesktop /></span>
        <div><strong>终端</strong></div>
      </header>
      <SettingsTerminalManager :settings="settings" />
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
            <a-select v-model="settings.livenessCliKind" :options="livenessCliKindOptions" />
          </a-form-item>
          <a-form-item label="默认模型">
            <a-select
              v-model="settings.livenessModel"
              :options="codexModelSelectOptions"
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
              :options="codexIntervalModeOptions"
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
            <a-input-number v-model="settings.livenessTimeout" :min="10" :max="300" :step="5" />
          </a-form-item>
        </div>

        <div class="settings-liveness-prompt">
          <SettingsCodexPromptSection :settings="settings" />
        </div>
      </div>
    </section>
  </div>
</template>
